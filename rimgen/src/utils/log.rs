#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Quiet,
    Normal,
    Verbose,
}

static mut LOG_LEVEL: LogLevel = LogLevel::Normal;

pub fn set_log_level(level: LogLevel) {
    unsafe {
        LOG_LEVEL = level;
    }
}

pub fn log_level() -> LogLevel {
    unsafe { LOG_LEVEL }
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
