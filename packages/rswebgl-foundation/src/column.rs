//! Dense typed arrays in wasm memory that JS can read and write directly.
//!
//! A [`Column`] stores `len` elements of `stride` scalars each (a `vec3`
//! position column has stride 3). JS gets a zero-copy typed-array view over it.
//!
//! A view is a window onto raw wasm memory, and it goes stale two ways:
//! - **the wasm memory grows**: every existing `ArrayBuffer` view detaches and
//!   reads as length 0. Tracked by [`memory_version`].
//! - **the column reallocates** (grows past its capacity): the view still reads,
//!   but from the old, now unrelated address. Tracked by
//!   [`Column::layout_version`].
//!
//! So: take a view, use it, and take a new one when either version changed — in
//! practice, once at the start of each frame. JS writes through a view bypass
//! change tracking; report them with [`Column::mark_dirty`].

use std::ops::Range;

use crate::dirty::DirtyBits;

mod sealed {
    pub trait Sealed {}
}

/// A scalar a column can hold: the numeric types JS typed arrays exist for.
pub trait Scalar: Copy + Default + PartialEq + std::fmt::Debug + 'static + sealed::Sealed {
    /// The JS typed array viewing a slice of this type.
    type JsArray;

    /// # Safety
    /// The view aliases `data` without a borrow. It's invalid once the wasm
    /// memory grows or `data` moves; the caller must not let JS use it after.
    unsafe fn js_view(data: &[Self]) -> Self::JsArray;
}

macro_rules! scalar {
    ($t:ty, $arr:ty) => {
        impl sealed::Sealed for $t {}
        impl Scalar for $t {
            type JsArray = $arr;
            unsafe fn js_view(data: &[Self]) -> $arr {
                unsafe { <$arr>::view(data) }
            }
        }
    };
}

scalar!(f32, js_sys::Float32Array);
scalar!(f64, js_sys::Float64Array);
scalar!(i32, js_sys::Int32Array);
scalar!(u32, js_sys::Uint32Array);
scalar!(i16, js_sys::Int16Array);
scalar!(u16, js_sys::Uint16Array);
scalar!(i8, js_sys::Int8Array);
scalar!(u8, js_sys::Uint8Array);

/// Current size of the wasm memory in 64 KiB pages. Memory only grows, so a
/// change means every outstanding JS view over it is detached. Always 0 off
/// wasm.
pub fn memory_version() -> u32 {
    #[cfg(target_arch = "wasm32")]
    {
        core::arch::wasm32::memory_size(0) as u32
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        0
    }
}

#[derive(Clone, Debug)]
pub struct Column<T: Scalar> {
    data: Vec<T>,
    stride: u32,
    // Element granularity.
    dirty: DirtyBits,
    layout_version: u32,
}

impl<T: Scalar> Column<T> {
    /// An empty column of `stride` scalars per element, tracking changes in
    /// chunks of `chunk` elements.
    pub fn new(stride: u32, chunk: u32) -> Self {
        assert!(stride > 0, "stride must be positive");
        Self {
            data: Vec::new(),
            stride,
            dirty: DirtyBits::new(0, chunk),
            layout_version: 0,
        }
    }

    pub fn stride(&self) -> u32 {
        self.stride
    }

    /// Number of elements.
    pub fn len(&self) -> u32 {
        self.data.len() as u32 / self.stride
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Changes the element count. New elements are zero and marked dirty.
    pub fn resize(&mut self, len: u32) {
        let old_len = self.len();
        let old_ptr = self.data.as_ptr();
        self.data.resize((len * self.stride) as usize, T::default());
        self.dirty.resize(len);
        if len > old_len {
            self.dirty.mark(old_len..len);
        }
        if self.data.as_ptr() != old_ptr || len != old_len {
            self.layout_version = self.layout_version.wrapping_add(1);
        }
    }

    /// Reserves room for `additional` more elements up front, so growing to
    /// that size later doesn't move the data (and stale JS views).
    pub fn reserve(&mut self, additional: u32) {
        let old_ptr = self.data.as_ptr();
        self.data.reserve((additional * self.stride) as usize);
        if self.data.as_ptr() != old_ptr {
            self.layout_version = self.layout_version.wrapping_add(1);
        }
    }

    pub fn get(&self, index: u32) -> &[T] {
        let s = (index * self.stride) as usize;
        &self.data[s..s + self.stride as usize]
    }

    /// Mutable access to one element; marks it dirty.
    pub fn get_mut(&mut self, index: u32) -> &mut [T] {
        self.dirty.mark(index..index + 1);
        let s = (index * self.stride) as usize;
        &mut self.data[s..s + self.stride as usize]
    }

    /// Overwrites one element and marks it dirty. Panics if `value` isn't
    /// exactly `stride` long.
    pub fn set(&mut self, index: u32, value: &[T]) {
        assert_eq!(value.len(), self.stride as usize, "element length");
        self.get_mut(index).copy_from_slice(value);
    }

    pub fn as_slice(&self) -> &[T] {
        &self.data
    }

    /// Mutable access to everything; marks everything dirty.
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        self.dirty.mark_all();
        &mut self.data
    }

    /// The raw bytes, for uploading to a GPU buffer.
    pub fn bytes(&self) -> &[u8] {
        let len = std::mem::size_of_val(self.data.as_slice());
        // SAFETY: `Scalar` is sealed to plain numeric types with no padding or
        // invalid bit patterns, so their memory is readable as bytes.
        unsafe { std::slice::from_raw_parts(self.data.as_ptr().cast::<u8>(), len) }
    }

    /// Byte size of one element.
    pub fn element_bytes(&self) -> u32 {
        self.stride * std::mem::size_of::<T>() as u32
    }

    /// Records writes made behind the column's back (through a JS view), in
    /// elements.
    pub fn mark_dirty(&mut self, elements: Range<u32>) {
        self.dirty.mark(elements);
    }

    pub fn dirty(&self) -> &DirtyBits {
        &self.dirty
    }

    /// Dirty element ranges, cleared.
    pub fn take_dirty(&mut self) -> Vec<Range<u32>> {
        self.dirty.take_runs()
    }

    /// Changes whenever the data moves or the length changes: a JS view taken
    /// before is stale. See also [`memory_version`].
    pub fn layout_version(&self) -> u32 {
        self.layout_version
    }

    /// A typed-array view over the column's data, without copying. Valid while
    /// [`memory_version`] and [`Self::layout_version`] stay unchanged.
    pub fn js_view(&self) -> T::JsArray {
        // SAFETY: the view's validity window is documented above and exposed
        // through the two version counters; wasm is single-threaded, so nothing
        // moves the data while the view is being used within one call.
        unsafe { T::js_view(&self.data) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elements_by_stride() {
        let mut c = Column::<f32>::new(3, 8);
        c.resize(2);
        c.set(1, &[1.0, 2.0, 3.0]);
        assert_eq!(c.len(), 2);
        assert_eq!(c.get(0), &[0.0, 0.0, 0.0]);
        assert_eq!(c.get(1), &[1.0, 2.0, 3.0]);
        assert_eq!(c.as_slice().len(), 6);
        assert_eq!(c.element_bytes(), 12);
        assert_eq!(c.bytes().len(), 24);
    }

    #[test]
    fn writes_are_tracked_per_element() {
        let mut c = Column::<u32>::new(1, 4);
        c.resize(100);
        c.take_dirty(); // growth marks new elements; start clean
        c.set(2, &[7]);
        c.get_mut(90)[0] = 9;
        assert_eq!(c.take_dirty(), vec![0..4, 88..92]);
        c.mark_dirty(50..51);
        assert_eq!(c.take_dirty(), vec![48..52]);
    }

    #[test]
    fn growth_marks_new_elements_dirty() {
        let mut c = Column::<f32>::new(2, 1);
        c.resize(2);
        c.take_dirty();
        c.resize(5);
        assert_eq!(c.take_dirty(), vec![2..5]);
    }

    #[test]
    fn layout_version_tracks_moves_and_length() {
        let mut c = Column::<f32>::new(1, 16);
        c.reserve(10);
        let v = c.layout_version();
        c.resize(10); // within capacity, but the length changed
        assert_ne!(c.layout_version(), v);
        let v = c.layout_version();
        c.set(3, &[1.0]); // plain writes don't move anything
        assert_eq!(c.layout_version(), v);
        c.resize(10_000); // reallocates
        assert_ne!(c.layout_version(), v);
    }

    #[test]
    #[should_panic(expected = "element length")]
    fn set_rejects_wrong_length() {
        let mut c = Column::<f32>::new(4, 1);
        c.resize(1);
        c.set(0, &[1.0]);
    }
}
