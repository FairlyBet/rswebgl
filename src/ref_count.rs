use std::cell::Cell;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub(crate) struct RefCount(Rc<Cell<usize>>);

impl RefCount {
    pub fn new() -> Self {
        Self(Rc::new(Cell::new(1)))
    }

    pub fn increment(&self) {
        self.0.set(self.0.get() + 1);
    }

    pub fn decrement(&self) -> bool {
        let c = self.0.get() - 1;
        self.0.set(c);
        c == 0
    }
}

macro_rules! ref_counted {
    ($name:ident wraps $inner:ident; drop($self:ident) $body:block) => {
        #[wasm_bindgen]
        #[derive(Debug)]
        pub struct $name {
            inner: $inner,
            rc: $crate::ref_count::RefCount,
        }

        impl Clone for $name {
            fn clone(&self) -> Self {
                self.rc.increment();
                Self {
                    inner: self.inner.clone(),
                    rc: self.rc.clone(),
                }
            }
        }

        impl Drop for $name {
            fn drop(&mut $self) {
                if $self.rc.decrement() $body
            }
        }
    };
}

pub(crate) use ref_counted;
