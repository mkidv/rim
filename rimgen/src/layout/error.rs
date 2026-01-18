// SPDX-License-Identifier: MIT

use core::fmt;

#[allow(dead_code)]
#[derive(Debug)]
pub enum LayoutError {
    SizeTooLarge(&'static str, u64),
    SizeTooSmall(&'static str, u64),
    InvalidConfig(&'static str),
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayoutError::SizeTooLarge(fs, size) => {
                write!(f, "{fs} is not recommended beyond {size} MiB")
            }
            LayoutError::SizeTooSmall(fs, size) => {
                write!(f, "{fs} needs at least {size} MiB")
            }
            LayoutError::InvalidConfig(msg) => write!(f, "Invalid config: {msg}"),
        }
    }
}
