// SPDX-License-Identifier: MIT

//! Error types for the rimio I/O abstraction layer.

use core::fmt;

/// Result type for RimIO operations.
pub type RimIOResult<T = ()> = core::result::Result<T, RimIOError>;

/// Error type for RimIO operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RimIOError {
    Other(&'static str),
    Invalid(&'static str),
    OutOfBounds,
    Unsupported,
    InvalidBuffer,
}

impl RimIOError {
    pub fn msg(&self) -> &'static str {
        match self {
            RimIOError::Other(msg) => msg,
            RimIOError::Invalid(msg) => msg,
            RimIOError::OutOfBounds => "Out of bounds",
            RimIOError::Unsupported => "Unsupported operation",
            RimIOError::InvalidBuffer => "Invalid or insufficient buffer",
        }
    }
}

impl From<&'static str> for RimIOError {
    #[inline]
    fn from(msg: &'static str) -> Self {
        RimIOError::Other(msg)
    }
}

impl fmt::Display for RimIOError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.msg())?;
        Ok(())
    }
}

#[cfg(feature = "std")]
impl std::error::Error for RimIOError {}
