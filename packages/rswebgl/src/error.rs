// The error type of every fallible call. In Rust it reads as a message (it
// derefs to `str`); across wasm-bindgen it becomes a JS `Error`, so JS callers
// get `instanceof Error`, a message and a stack rather than a bare string.

use std::fmt;
use std::ops::Deref;

use wasm_bindgen::JsValue;

/// What went wrong, as a human-readable message.
#[derive(Clone, PartialEq, Eq)]
pub struct Error(String);

impl Error {
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl Deref for Error {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// Debug shows the message as is, so `expect` / `unwrap` panics stay readable.
impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for Error {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl From<Error> for String {
    fn from(e: Error) -> Self {
        e.0
    }
}

impl From<Error> for JsValue {
    fn from(e: Error) -> Self {
        js_sys::Error::new(&e.0).into()
    }
}

/// `format!` into an `Error`.
macro_rules! err {
    ($($t:tt)*) => {
        $crate::error::Error::from(format!($($t)*))
    };
}
pub(crate) use err;
