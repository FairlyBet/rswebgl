pub fn log(msg: &str) {
    web_sys::console::log_1(&msg.into());
}

pub fn warn(msg: &str) {
    web_sys::console::warn_1(&msg.into());
}

pub fn error(msg: &str) {
    web_sys::console::error_1(&msg.into());
}
