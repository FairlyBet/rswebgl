use std::cell::RefCell;
use std::collections::HashSet;

pub fn log(msg: &str) {
    web_sys::console::log_1(&msg.into());
}

pub fn warn(msg: &str) {
    web_sys::console::warn_1(&msg.into());
}

pub fn error(msg: &str) {
    web_sys::console::error_1(&msg.into());
}

thread_local! {
    static REPORTED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

// True the first time `msg` is seen.
fn first_time(msg: &str) -> bool {
    REPORTED.with(|r| {
        let mut r = r.borrow_mut();
        !r.contains(msg) && r.insert(msg.to_string())
    })
}

/// `warn`, but only the first time this exact message comes up: for problems a
/// per-frame path (uniform setters, draws) would otherwise report every frame.
pub fn warn_once(msg: &str) {
    if first_time(msg) {
        warn(msg);
    }
}

/// `error`, deduplicated like `warn_once`.
pub fn error_once(msg: &str) {
    if first_time(msg) {
        error(msg);
    }
}
