// SPDX-License-Identifier: MIT

use core::fmt;

/// Result type for BlockIO operations.
pub type BlockIOResult<T = ()> = core::result::Result<T, BlockIOError>;

/// Error type for BlockIO operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockIOError {
    Other(&'static str),
    OutOfBounds,
    Unsupported,
}

impl BlockIOError {
    pub fn msg(&self) -> &'static str {
        match self {
            BlockIOError::Other(msg) => msg,
            BlockIOError::OutOfBounds => "Out of bounds",
            BlockIOError::Unsupported => "Unsupported operation",
        }
    }
}

impl From<&'static str> for BlockIOError {
    #[inline]
    fn from(msg: &'static str) -> Self {
        BlockIOError::Other(msg)
    }
}

impl fmt::Display for BlockIOError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.msg())?;
        Ok(())
    }
}
