// SPDX-License-Identifier: MIT

use core::fmt;

use rimio::errors::*;

/// Unified error type for partition tools (GPT, MBR, etc.)
#[derive(Debug, Clone)]
pub enum PartError {
    IO(BlockIOError),
    Unsupported,
    NotFound,
    Invalid(&'static str),
    Other(&'static str),
}

impl PartError {
    pub fn msg(&self) -> &'static str {
        match self {
            PartError::IO(e) => e.msg(),
            PartError::Unsupported => "Unsupported",
            PartError::NotFound => "No partition table found",
            PartError::Invalid(msg) => msg,
            PartError::Other(msg) => msg,
        }
    }
}

impl From<&'static str> for PartError {
    fn from(s: &'static str) -> Self {
        PartError::Other(s)
    }
}

impl From<BlockIOError> for PartError {
    fn from(e: BlockIOError) -> Self {
        PartError::IO(e)
    }
}

impl fmt::Display for PartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.msg())?;
        Ok(())
    }
}

pub type PartResult<T = ()> = Result<T, PartError>;
