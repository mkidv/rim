// SPDX-License-Identifier: MIT

use core::fmt;

pub use rimio::error::*;

#[derive(Debug, Clone)]
pub enum FsAllocatorError {
    OutOfBlocks,
    Other(&'static str),
}

impl FsAllocatorError {
    pub fn msg(&self) -> &'static str {
        match self {
            FsAllocatorError::OutOfBlocks => "Out of blocks",
            FsAllocatorError::Other(msg) => msg,
        }
    }
}

#[derive(Debug, Clone)]
pub enum FsParserError {
    IO(BlockIOError),
    Unsupported,
    Invalid(&'static str),
    Other(&'static str),
}

impl FsParserError {
    pub fn msg(&self) -> &'static str {
        match self {
            FsParserError::IO(_) => "IO error",
            FsParserError::Unsupported => "Unsupported entry",
            FsParserError::Invalid(msg) => msg,
            FsParserError::Other(msg) => msg,
        }
    }

    pub fn source(&self) -> Option<FsError> {
        match self {
            FsParserError::IO(e) => Some(FsError::IO(e.clone())),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum FsFormatterError {
    IO(BlockIOError),
    Invalid(&'static str),
    Other(&'static str),
}

impl FsFormatterError {
    pub fn msg(&self) -> &'static str {
        match self {
            FsFormatterError::IO(_) => "IO error",
            FsFormatterError::Invalid(msg) => msg,
            FsFormatterError::Other(msg) => msg,
        }
    }

    pub fn source(&self) -> Option<FsError> {
        match self {
            FsFormatterError::IO(e) => Some(FsError::IO(e.clone())),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum FsInjectorError {
    IO(BlockIOError),
    Allocator(FsAllocatorError),
    StackUnderflow,
    Invalid(&'static str),
    Other(&'static str),
}

impl FsInjectorError {
    pub fn msg(&self) -> &'static str {
        match self {
            FsInjectorError::IO(_) => "IO error",
            FsInjectorError::Allocator(_) => "Allocator error",
            FsInjectorError::StackUnderflow => "Stack underflow",
            FsInjectorError::Invalid(msg) => msg,
            FsInjectorError::Other(msg) => msg,
        }
    }

    pub fn source(&self) -> Option<FsError> {
        match self {
            FsInjectorError::IO(e) => Some(FsError::IO(e.clone())),
            FsInjectorError::Allocator(e) => Some(FsError::Allocator(e.clone())),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum FsCheckerError {
    IO(BlockIOError),
    Parser(FsParserError),
    Invalid(&'static str),
    Other(&'static str),
}

impl FsCheckerError {
    pub fn msg(&self) -> &'static str {
        match self {
            FsCheckerError::IO(_) => "IO error",
            FsCheckerError::Parser(_) => "Parser error",
            FsCheckerError::Invalid(msg) => msg,
            FsCheckerError::Other(msg) => msg,
        }
    }

    pub fn source(&self) -> Option<FsError> {
        match self {
            FsCheckerError::IO(e) => Some(FsError::IO(e.clone())),
            FsCheckerError::Parser(e) => Some(FsError::Parser(e.clone())),
            _ => None,
        }
    }
}

/// Top-level error
#[derive(Debug, Clone)]
pub enum FsError {
    IO(BlockIOError),
    Allocator(FsAllocatorError),
    Parser(FsParserError),
    Formatter(FsFormatterError),
    Injector(FsInjectorError),
    Checker(FsCheckerError),
    Other(&'static str),
}

// === impl From ===

impl From<BlockIOError> for FsParserError {
    fn from(e: BlockIOError) -> Self {
        FsParserError::IO(e)
    }
}

impl From<BlockIOError> for FsFormatterError {
    fn from(e: BlockIOError) -> Self {
        FsFormatterError::IO(e)
    }
}

impl From<BlockIOError> for FsInjectorError {
    fn from(e: BlockIOError) -> Self {
        FsInjectorError::IO(e)
    }
}

impl From<FsAllocatorError> for FsInjectorError {
    fn from(e: FsAllocatorError) -> Self {
        FsInjectorError::Allocator(e)
    }
}

impl From<BlockIOError> for FsCheckerError {
    fn from(e: BlockIOError) -> Self {
        FsCheckerError::IO(e)
    }
}

impl From<FsParserError> for FsCheckerError {
    fn from(e: FsParserError) -> Self {
        FsCheckerError::Parser(e)
    }
}

// === impl From to FsError top-level ===

impl From<BlockIOError> for FsError {
    fn from(e: BlockIOError) -> Self {
        FsError::IO(e)
    }
}

impl From<FsAllocatorError> for FsError {
    fn from(e: FsAllocatorError) -> Self {
        FsError::Allocator(e)
    }
}

impl From<FsParserError> for FsError {
    fn from(e: FsParserError) -> Self {
        FsError::Parser(e)
    }
}

impl From<FsFormatterError> for FsError {
    fn from(e: FsFormatterError) -> Self {
        FsError::Formatter(e)
    }
}

impl From<FsInjectorError> for FsError {
    fn from(e: FsInjectorError) -> Self {
        FsError::Injector(e)
    }
}

impl From<FsCheckerError> for FsError {
    fn from(e: FsCheckerError) -> Self {
        FsError::Checker(e)
    }
}

// === type Fs*Result ===

pub type FsResult<T = ()> = Result<T, FsError>;

pub type FsAllocatorResult<T = ()> = Result<T, FsAllocatorError>;
pub type FsParserResult<T = ()> = Result<T, FsParserError>;
pub type FsFormatterResult<T = ()> = Result<T, FsFormatterError>;
pub type FsInjectorResult<T = ()> = Result<T, FsInjectorError>;
pub type FsCheckerResult<T = ()> = Result<T, FsCheckerError>;

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.msg())?;
        let mut current = self.source();
        while let Some(src) = current {
            write!(f, "\n  caused by: {}", src.msg())?;
            current = src.source();
        }
        Ok(())
    }
}

impl FsError {
    pub fn msg(&self) -> &'static str {
        match self {
            FsError::IO(e) => e.msg(),
            FsError::Allocator(e) => e.msg(),
            FsError::Parser(e) => e.msg(),
            FsError::Formatter(e) => e.msg(),
            FsError::Injector(e) => e.msg(),
            FsError::Checker(e) => e.msg(),
            FsError::Other(msg) => msg,
        }
    }

    pub fn source(&self) -> Option<FsError> {
        match self {
            FsError::Parser(e) => e.source(),
            FsError::Formatter(e) => e.source(),
            FsError::Injector(e) => e.source(),
            FsError::Checker(e) => e.source(),
            FsError::IO(_) => None,
            FsError::Allocator(_) => None,
            FsError::Other(_) => None,
        }
    }
}

#[cfg(test)]
#[cfg(feature = "std")]
mod tests {
    use super::*;

    #[test]
    fn test_error_chain_display() {
        let low = BlockIOError::Unsupported;
        let inj = FsInjectorError::IO(low.clone());
        let top = FsError::Injector(inj);

        println!("{top}");
    }
}
