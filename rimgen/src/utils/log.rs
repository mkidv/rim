#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Quiet,
    Normal,
    Verbose,
}

use std::sync::atomic::{AtomicU8, Ordering};

static LOG_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::Normal as u8);

pub fn set_log_level(level: LogLevel) {
    LOG_LEVEL.store(level as u8, Ordering::Relaxed);
}

pub fn log_level() -> LogLevel {
    match LOG_LEVEL.load(Ordering::Relaxed) {
        0 => LogLevel::Quiet,
        1 => LogLevel::Normal,
        _ => LogLevel::Verbose,
    }
}

#[macro_export]
macro_rules! log_normal {
    ($($arg:tt)*) => {
            println!("[rimgen] {}", format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        if $crate::utils::log_level() != $crate::utils::LogLevel::Quiet {
            println!("[rimgen] {}", format_args!($($arg)*));
        }
    };
}

#[macro_export]
macro_rules! log_verbose {
    ($($arg:tt)*) => {
        if $crate::utils::log_level() == $crate::utils::LogLevel::Verbose {
            println!("[rimgen] {}", format_args!($($arg)*));
        }
    };
}
