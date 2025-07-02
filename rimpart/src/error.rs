// SPDX-License-Identifier: MIT

use rimio::error::*;

/// Unified error type for partition tools (GPT, MBR, etc.)
#[derive(Debug, Clone)]
pub enum PartError {
    IO(BlockIOError),
    Unsupported,
    Invalid(&'static str),
    Other(&'static str),
}

impl PartError {
    pub fn msg(&self) -> &'static str {
        match self {
            PartError::IO(e) => e.msg(),
            PartError::Unsupported => "Unsupported",
            PartError::Invalid(msg) => msg,
            PartError::Other(msg) => msg,
        }
    }
}

impl From<BlockIOError> for PartError {
    fn from(e: BlockIOError) -> Self {
        PartError::IO(e)
    }
}

pub type PartResult<T = ()> = Result<T, PartError>;
