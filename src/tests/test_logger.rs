//! Test logging macros — works in both WASM and native test environments.

#[cfg(test)]
#[macro_export]
macro_rules! test_log {
    ($($arg:tt)*) => {{
        #[cfg(target_arch = "wasm32")]
        {
            web_sys::console::log_1(&format!($($arg)*).into());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            println!($($arg)*);
        }
    }};
}
