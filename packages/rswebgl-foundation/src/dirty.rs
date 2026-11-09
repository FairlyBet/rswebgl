//! Change tracking at chunk granularity.
//!
//! A buffer of `len` units (bytes, elements…) is split into fixed-size chunks; a
//! write marks the chunks it touches. Uploading then walks the dirty chunks,
//! coalescing neighbours into runs: scattered writes upload only what changed
//! instead of one span from the first to the last write.

use std::ops::Range;

#[derive(Clone, Debug)]
pub struct DirtyBits {
    len: u32,
    chunk: u32,
    bits: Vec<u64>,
    any: bool,
}

impl DirtyBits {
    /// Tracks `len` units in chunks of `chunk` units. Starts clean.
    pub fn new(len: u32, chunk: u32) -> Self {
        assert!(chunk > 0, "chunk size must be positive");
        let chunks = len.div_ceil(chunk);
        Self {
            len,
            chunk,
            bits: vec![0; chunks.div_ceil(64) as usize],
            any: false,
        }
    }

    pub fn len(&self) -> u32 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Grows or shrinks the tracked length. New chunks start clean; chunks past
    /// the new end are dropped.
    pub fn resize(&mut self, len: u32) {
        let chunks = len.div_ceil(self.chunk);
        self.bits.resize(chunks.div_ceil(64) as usize, 0);
        // Clear bits beyond the last chunk in the final word.
        if !chunks.is_multiple_of(64)
            && let Some(last) = self.bits.last_mut()
        {
            *last &= (1u64 << (chunks % 64)) - 1;
        }
        self.len = len;
        self.any = self.bits.iter().any(|w| *w != 0);
    }

    /// Marks the units in `range` (clamped to the length) as changed.
    pub fn mark(&mut self, range: Range<u32>) {
        let end = range.end.min(self.len);
        if range.start >= end {
            return;
        }
        let first = range.start / self.chunk;
        let last = (end - 1) / self.chunk;
        for c in first..=last {
            self.bits[(c / 64) as usize] |= 1 << (c % 64);
        }
        self.any = true;
    }

    pub fn mark_all(&mut self) {
        self.mark(0..self.len);
    }

    pub fn is_dirty(&self) -> bool {
        self.any
    }

    pub fn clear(&mut self) {
        if self.any {
            self.bits.fill(0);
            self.any = false;
        }
    }

    /// The dirty ranges, in units, with adjacent chunks merged and the last one
    /// clamped to the length.
    pub fn runs(&self) -> Runs<'_> {
        Runs {
            d: self,
            chunk: 0,
            total: if self.any {
                self.len.div_ceil(self.chunk)
            } else {
                0
            },
        }
    }

    /// Takes the dirty ranges and clears them.
    pub fn take_runs(&mut self) -> Vec<Range<u32>> {
        let runs = self.runs().collect();
        self.clear();
        runs
    }

    /// Takes the dirty ranges, clearing them, with runs less than `max_gap`
    /// units apart merged into one (the clean units between them come
    /// along): for uploads where each call costs more than the bytes.
    pub fn take_runs_merged(&mut self, max_gap: u32) -> Vec<Range<u32>> {
        let mut out: Vec<Range<u32>> = Vec::new();
        for run in self.runs() {
            match out.last_mut() {
                Some(last) if run.start - last.end < max_gap => last.end = run.end,
                _ => out.push(run),
            }
        }
        self.clear();
        out
    }

    fn is_set(&self, c: u32) -> bool {
        self.bits[(c / 64) as usize] & (1 << (c % 64)) != 0
    }
}

pub struct Runs<'a> {
    d: &'a DirtyBits,
    chunk: u32,
    total: u32,
}

impl Iterator for Runs<'_> {
    type Item = Range<u32>;

    fn next(&mut self) -> Option<Range<u32>> {
        let d = self.d;
        while self.chunk < self.total {
            // Skip whole clean words quickly.
            let word = d.bits[(self.chunk / 64) as usize] >> (self.chunk % 64);
            if word == 0 {
                self.chunk = (self.chunk / 64 + 1) * 64;
                continue;
            }
            self.chunk += word.trailing_zeros();
            if self.chunk >= self.total {
                break;
            }
            let start = self.chunk;
            while self.chunk < self.total && d.is_set(self.chunk) {
                self.chunk += 1;
            }
            let end = (self.chunk * d.chunk).min(d.len);
            return Some(start * d.chunk..end);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_clean() {
        let d = DirtyBits::new(100, 16);
        assert!(!d.is_dirty());
        assert_eq!(d.runs().count(), 0);
    }

    #[test]
    fn scattered_writes_stay_separate() {
        // The old single-range tracking would upload 0..1004 here.
        let mut d = DirtyBits::new(1024, 16);
        d.mark(0..4);
        d.mark(1000..1004);
        assert_eq!(d.runs().collect::<Vec<_>>(), vec![0..16, 992..1008]);
    }

    #[test]
    fn adjacent_chunks_coalesce() {
        let mut d = DirtyBits::new(256, 16);
        d.mark(10..20); // chunks 0 and 1
        d.mark(32..33); // chunk 2
        assert_eq!(d.runs().collect::<Vec<_>>(), vec![0..48]);
    }

    #[test]
    fn last_run_is_clamped_to_len() {
        let mut d = DirtyBits::new(40, 16);
        d.mark(35..100);
        assert_eq!(d.runs().collect::<Vec<_>>(), vec![32..40]);
    }

    #[test]
    fn runs_cross_word_boundaries() {
        // 200 chunks of 1: spans several u64 words.
        let mut d = DirtyBits::new(200, 1);
        d.mark(60..70);
        d.mark(130..131);
        d.mark(199..200);
        assert_eq!(
            d.runs().collect::<Vec<_>>(),
            vec![60..70, 130..131, 199..200]
        );
    }

    #[test]
    fn take_runs_clears() {
        let mut d = DirtyBits::new(64, 16);
        d.mark_all();
        assert_eq!(d.take_runs(), vec![0..64]);
        assert!(!d.is_dirty());
        assert!(d.take_runs().is_empty());
    }

    #[test]
    fn close_runs_merge_far_ones_dont() {
        let mut d = DirtyBits::new(4096, 16);
        d.mark(0..16);
        d.mark(48..64); // 32 apart: merged
        d.mark(2048..2064); // far: separate
        assert_eq!(d.take_runs_merged(64), vec![0..64, 2048..2064]);
        assert!(!d.is_dirty());
        d.mark(0..16);
        d.mark(48..64);
        assert_eq!(d.take_runs_merged(32), vec![0..16, 48..64]); // gap 32 is not < 32
    }

    #[test]
    fn empty_and_out_of_range_marks_are_ignored() {
        let mut d = DirtyBits::new(32, 16);
        d.mark(5..5);
        d.mark(40..50);
        assert!(!d.is_dirty());
    }

    #[test]
    fn resize_drops_and_adds_clean_chunks() {
        let mut d = DirtyBits::new(64, 1);
        d.mark(60..64);
        d.resize(62);
        assert_eq!(d.runs().collect::<Vec<_>>(), vec![60..62]);
        d.resize(128);
        assert_eq!(d.runs().collect::<Vec<_>>(), vec![60..62]);
        d.resize(10);
        assert!(!d.is_dirty());
    }
}
