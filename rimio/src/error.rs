// SPDX-License-Identifier: MIT

/// Result type for BlockIO operations.
pub type BlockIOResult<T = ()>  = core::result::Result<T, BlockIOError>;

/// Error type for BlockIO operations.
#[derive(Debug, Clone)]
pub enum BlockIOError {
    /// Underlying device I/O error.
    Error(&'static str),

    /// Attempted to read or write out of bounds.
    OutOfBounds,

    /// Unsupported
    Unsupported,
}

impl BlockIOError {
    pub fn msg(&self) -> &'static str {
        match self {
            BlockIOError::Error(msg) => msg,
            BlockIOError::OutOfBounds => "Out of bounds",
            BlockIOError::Unsupported => "Unsupported operation",
        }
    }
}
