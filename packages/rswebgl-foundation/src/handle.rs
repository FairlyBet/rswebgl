//! Generational handles: a slot index plus the generation of that slot when the
//! handle was issued. Packed into a `u32` so JS sees a plain `number` (a `u64`
//! would cross wasm-bindgen as a `BigInt`).

/// Bits for the slot index: 2^20 ≈ 1M live objects per pool.
pub const INDEX_BITS: u32 = 20;
/// Bits for the generation: a slot can be reused 4095 times before it retires.
pub const GENERATION_BITS: u32 = 32 - INDEX_BITS;

pub(crate) const INDEX_MASK: u32 = (1 << INDEX_BITS) - 1;
pub(crate) const MAX_GENERATION: u32 = (1 << GENERATION_BITS) - 1;
/// Slot 0 is never handed out, so the all-zero handle means "none".
pub(crate) const MAX_INDEX: u32 = INDEX_MASK;

/// An untyped generational handle. `Handle::NULL` (0) never refers to anything.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
#[repr(transparent)]
pub struct Handle(u32);

impl Handle {
    pub const NULL: Handle = Handle(0);

    pub(crate) fn new(index: u32, generation: u32) -> Self {
        debug_assert!(index <= MAX_INDEX && generation <= MAX_GENERATION);
        Handle(generation << INDEX_BITS | index)
    }

    pub fn index(self) -> u32 {
        self.0 & INDEX_MASK
    }

    pub fn generation(self) -> u32 {
        self.0 >> INDEX_BITS
    }

    pub fn is_null(self) -> bool {
        self.0 == 0
    }

    /// The packed value, as passed to and from JS.
    pub fn to_raw(self) -> u32 {
        self.0
    }

    /// Rebuilds a handle from its packed value. Any `u32` is accepted: a value
    /// that was never issued simply fails to resolve in its pool.
    pub fn from_raw(raw: u32) -> Self {
        Handle(raw)
    }
}

impl std::fmt::Debug for Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_null() {
            write!(f, "Handle(null)")
        } else {
            write!(f, "Handle({}v{})", self.index(), self.generation())
        }
    }
}

/// Declares a typed handle: a newtype over [`Handle`] so a texture id can't be
/// passed where a buffer id is expected. Converts to/from `u32` for JS.
///
/// ```
/// rswebgl_foundation::typed_handle!(pub struct TextureId);
/// let t = TextureId::NULL;
/// assert!(t.is_null());
/// ```
#[macro_export]
macro_rules! typed_handle {
    ($(#[$meta:meta])* $vis:vis struct $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Debug)]
        #[repr(transparent)]
        $vis struct $name(pub $crate::handle::Handle);

        impl $name {
            pub const NULL: Self = Self($crate::handle::Handle::NULL);

            pub fn is_null(self) -> bool {
                self.0.is_null()
            }

            pub fn to_raw(self) -> u32 {
                self.0.to_raw()
            }

            pub fn from_raw(raw: u32) -> Self {
                Self($crate::handle::Handle::from_raw(raw))
            }
        }

        impl From<$crate::handle::Handle> for $name {
            fn from(h: $crate::handle::Handle) -> Self {
                Self(h)
            }
        }

        impl From<$name> for $crate::handle::Handle {
            fn from(h: $name) -> Self {
                h.0
            }
        }

        // Crosses wasm-bindgen as a plain `u32` (a JS `number`), not as a
        // wrapper object: nothing to `free()` on the JS side.
        impl $crate::__wasm_bindgen::describe::WasmDescribe for $name {
            fn describe() {
                <u32 as $crate::__wasm_bindgen::describe::WasmDescribe>::describe()
            }
        }

        impl $crate::__wasm_bindgen::convert::IntoWasmAbi for $name {
            type Abi = <u32 as $crate::__wasm_bindgen::convert::IntoWasmAbi>::Abi;
            fn into_abi(self) -> Self::Abi {
                $crate::__wasm_bindgen::convert::IntoWasmAbi::into_abi(self.to_raw())
            }
        }

        impl $crate::__wasm_bindgen::convert::FromWasmAbi for $name {
            type Abi = <u32 as $crate::__wasm_bindgen::convert::FromWasmAbi>::Abi;
            unsafe fn from_abi(js: Self::Abi) -> Self {
                Self::from_raw(unsafe {
                    <u32 as $crate::__wasm_bindgen::convert::FromWasmAbi>::from_abi(js)
                })
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_index_and_generation() {
        let h = Handle::new(MAX_INDEX, MAX_GENERATION);
        assert_eq!(h.index(), MAX_INDEX);
        assert_eq!(h.generation(), MAX_GENERATION);
        assert_eq!(h.to_raw(), u32::MAX);
        assert_eq!(Handle::from_raw(h.to_raw()), h);
    }

    #[test]
    fn null_is_zero() {
        assert!(Handle::NULL.is_null());
        assert!(!Handle::new(1, 0).is_null());
        assert_eq!(Handle::default(), Handle::NULL);
    }

    typed_handle!(struct Thing);

    #[test]
    fn typed_handle_round_trips() {
        let h = Handle::new(7, 3);
        let t = Thing::from(h);
        assert_eq!(Thing::from_raw(t.to_raw()), t);
        assert_eq!(Handle::from(t), h);
        assert!(Thing::NULL.is_null());
    }
}
