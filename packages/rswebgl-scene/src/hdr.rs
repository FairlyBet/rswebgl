//! Radiance `.hdr` (RGBE) decoding: the usual format of HDR environment maps.

/// A decoded HDR image: linear RGB, rows top to bottom.
#[derive(Debug, Clone, PartialEq)]
pub struct HdrImage {
    pub width: u32,
    pub height: u32,
    /// `width * height` RGB triples.
    pub rgb: Vec<f32>,
}

/// Decodes a Radiance `.hdr` file (`32-bit_rle_rgbe`; new-style RLE, old-style
/// RLE or flat scanlines). Only the standard `-Y h +X w` orientation.
pub fn decode_hdr(bytes: &[u8]) -> Result<HdrImage, String> {
    let mut pos = 0;
    let mut line = || -> Result<&[u8], String> {
        let start = pos;
        let end = bytes[start..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|n| start + n)
            .ok_or("HDR: header ended early")?;
        pos = end + 1;
        Ok(&bytes[start..end])
    };
    let magic = line()?;
    if !magic.starts_with(b"#?RADIANCE") && !magic.starts_with(b"#?RGBE") {
        return Err("HDR: not a Radiance file (no #?RADIANCE header)".into());
    }
    loop {
        let l = line()?;
        if l.is_empty() {
            break;
        }
        if let Some(format) = l.strip_prefix(b"FORMAT=")
            && format != b"32-bit_rle_rgbe"
        {
            return Err(format!(
                "HDR: unsupported format {}",
                String::from_utf8_lossy(format)
            ));
        }
    }
    let size = String::from_utf8_lossy(line()?).into_owned();
    let parts: Vec<&str> = size.split_whitespace().collect();
    let (height, width) = match parts.as_slice() {
        ["-Y", h, "+X", w] => (
            h.parse::<u32>().map_err(|_| "HDR: bad height")?,
            w.parse::<u32>().map_err(|_| "HDR: bad width")?,
        ),
        _ => return Err(format!("HDR: unsupported orientation \"{size}\"")),
    };
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        return Err(format!("HDR: unsupported size {width}×{height}"));
    }

    let data = &bytes[pos..];
    let (w, h) = (width as usize, height as usize);
    let mut rgbe = vec![0u8; w * h * 4];
    let mut at = 0;
    for row in rgbe.chunks_exact_mut(w * 4) {
        at = read_scanline(data, at, row, w)?;
    }
    let rgb = rgbe
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            if p[3] == 0 {
                [0.0; 3]
            } else {
                let f = f32::powi(2.0, p[3] as i32 - 136);
                [p[0] as f32 * f, p[1] as f32 * f, p[2] as f32 * f]
            }
        })
        .collect();
    Ok(HdrImage { width, height, rgb })
}

// One scanline of `w` RGBE pixels into `out`; returns the read position after
// it.
fn read_scanline(data: &[u8], mut at: usize, out: &mut [u8], w: usize) -> Result<usize, String> {
    let short = || "HDR: pixel data ended early".to_string();
    let head = data.get(at..at + 4).ok_or_else(short)?;
    // New-style RLE: 2, 2, width (big endian), then each channel separately.
    if (8..0x8000).contains(&w) && head[0] == 2 && head[1] == 2 && head[2] & 0x80 == 0 {
        if (head[2] as usize) << 8 | head[3] as usize != w {
            return Err("HDR: scanline width mismatch".into());
        }
        at += 4;
        for channel in 0..4 {
            let mut x = 0;
            while x < w {
                let count = *data.get(at).ok_or_else(short)? as usize;
                at += 1;
                if count > 128 {
                    let run = count - 128;
                    let value = *data.get(at).ok_or_else(short)?;
                    at += 1;
                    if x + run > w {
                        return Err("HDR: run past the scanline".into());
                    }
                    for i in 0..run {
                        out[(x + i) * 4 + channel] = value;
                    }
                    x += run;
                } else {
                    if count == 0 || x + count > w {
                        return Err("HDR: bad literal run".into());
                    }
                    let src = data.get(at..at + count).ok_or_else(short)?;
                    for (i, &v) in src.iter().enumerate() {
                        out[(x + i) * 4 + channel] = v;
                    }
                    at += count;
                    x += count;
                }
            }
        }
        return Ok(at);
    }
    // Flat pixels, possibly with old-style runs (1,1,1,n repeats the last
    // pixel n << shift times).
    let mut x = 0;
    let mut shift = 0;
    while x < w {
        let p = data.get(at..at + 4).ok_or_else(short)?;
        at += 4;
        if p[0] == 1 && p[1] == 1 && p[2] == 1 {
            if x == 0 {
                return Err("HDR: run before any pixel".into());
            }
            let run = (p[3] as usize) << shift;
            if x + run > w {
                return Err("HDR: run past the scanline".into());
            }
            let last = [
                out[(x - 1) * 4],
                out[(x - 1) * 4 + 1],
                out[(x - 1) * 4 + 2],
                out[(x - 1) * 4 + 3],
            ];
            for i in 0..run {
                out[(x + i) * 4..(x + i) * 4 + 4].copy_from_slice(&last);
            }
            x += run;
            shift += 8;
        } else {
            out[x * 4..x * 4 + 4].copy_from_slice(p);
            x += 1;
            shift = 0;
        }
    }
    Ok(at)
}

/// Encodes linear RGB as a flat (uncompressed) Radiance file — for tests and
/// tools.
pub fn encode_hdr(width: u32, height: u32, rgb: &[f32]) -> Vec<u8> {
    let mut out =
        format!("#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y {height} +X {width}\n").into_bytes();
    for p in rgb.as_chunks::<3>().0 {
        let m = p[0].max(p[1]).max(p[2]);
        if m < 1e-32 {
            out.extend([0, 0, 0, 0]);
            continue;
        }
        // m = mantissa * 2^e with mantissa in [0.5, 1).
        let e = m.log2().floor() as i32 + 1;
        let scale = 256.0 / f32::powi(2.0, e);
        out.extend([
            (p[0] * scale) as u8,
            (p[1] * scale) as u8,
            (p[2] * scale) as u8,
            (e + 128) as u8,
        ]);
    }
    out
}
