use std::cell::RefCell;
use std::collections::HashSet;

pub fn log(msg: &str) {
    web_sys::console::log_1(&msg.into());
}

pub fn warn(msg: &str) {
    remember("warn", msg);
    web_sys::console::warn_1(&msg.into());
}

pub fn error(msg: &str) {
    remember("error", msg);
    web_sys::console::error_1(&msg.into());
}

// The last warnings and errors, for reports (`trace::report` adds them):
// a phone's console is out of reach.
const KEPT: usize = 64;

thread_local! {
    static RECENT: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn remember(level: &str, msg: &str) {
    RECENT.with(|r| {
        let mut r = r.borrow_mut();
        if r.len() == KEPT {
            r.remove(0);
        }
        r.push(format!("{level}: {msg}"));
    });
}

/// The last warnings and errors the library reported (at most 64), oldest
/// first.
pub fn recent() -> Vec<String> {
    RECENT.with(|r| r.borrow().clone())
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
