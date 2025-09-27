// SPDX-License-Identifier: MIT

use core::fmt;

pub use rimio::errors::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl fmt::Display for FsAllocatorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.msg())?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsParsingError {
    IO(BlockIOError),
    Unsupported,
    Corrupted,
    Invalid(&'static str),
    Other(&'static str),
}

impl FsParsingError {
    pub fn msg(&self) -> &'static str {
        match self {
            FsParsingError::IO(_) => "IO error",
            FsParsingError::Unsupported => "Unsupported entry",
            FsParsingError::Corrupted => "Corrupted entry",
            FsParsingError::Invalid(msg) => msg,
            FsParsingError::Other(msg) => msg,
        }
    }

    pub fn source(&self) -> Option<FsError> {
        match self {
            FsParsingError::IO(e) => Some(FsError::IO(e.clone())),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsCursorError {
    IO(BlockIOError),
    Parsing(FsParsingError),
    InvalidCluster(u32),
    LoopDetected,
    UnsupportedEntrySize,
    Other(&'static str),
}

impl FsCursorError {
    pub fn msg(&self) -> &'static str {
        match self {
            FsCursorError::IO(_) => "IO error",
            FsCursorError::Parsing(_) => "Parsing error",
            FsCursorError::InvalidCluster(_) => "Invalid cluster in FAT chain",
            FsCursorError::LoopDetected => "Loop detected in FAT chain",
            FsCursorError::UnsupportedEntrySize => "Unsupported FAT entry size",
            FsCursorError::Other(msg) => msg,
        }
    }

    pub fn source(&self) -> Option<FsError> {
        match self {
            FsCursorError::IO(e) => Some(FsError::IO(e.clone())),
            FsCursorError::Parsing(e) => Some(FsError::Parsing(e.clone())),
            _ => None,
        }
    }
}

impl fmt::Display for FsCursorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.msg())?;
        if let FsCursorError::InvalidCluster(cluster) = self {
            write!(f, " (cluster: {})", cluster)?;
        }
        let mut current = self.source();
        while let Some(src) = current {
            write!(f, "\n  caused by: {}", src.msg())?;
            current = src.source();
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsResolverError {
    IO(BlockIOError),
    Cursor(FsCursorError),
    Parsing(FsParsingError),
    Unsupported,
    NotFound,
    Invalid(&'static str),
    Other(&'static str),
}

impl FsResolverError {
    pub fn msg(&self) -> &'static str {
        match self {
            FsResolverError::IO(_) => "IO error",
            FsResolverError::Cursor(_) => "Cursor error",
            FsResolverError::Parsing(_) => "Parsing error",
            FsResolverError::Unsupported => "Unsupported entry",
            FsResolverError::NotFound => "Path not found",
            FsResolverError::Invalid(msg) => msg,
            FsResolverError::Other(msg) => msg,
        }
    }

    pub fn source(&self) -> Option<FsError> {
        match self {
            FsResolverError::IO(e) => Some(FsError::IO(e.clone())),
            FsResolverError::Cursor(e) => Some(FsError::Cursor(e.clone())),
            _ => None,
        }
    }
}

impl fmt::Display for FsResolverError {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl fmt::Display for FsFormatterError {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl fmt::Display for FsInjectorError {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsCheckerError {
    IO(BlockIOError),
    Parsing(FsParsingError),
    Cursor(FsCursorError),
    Invalid(&'static str),
    Other(&'static str),
}

impl FsCheckerError {
    pub fn msg(&self) -> &'static str {
        match self {
            FsCheckerError::IO(_) => "IO error",
            FsCheckerError::Parsing(_) => "Parsing error",
            FsCheckerError::Cursor(_) => "Cursor error",
            FsCheckerError::Invalid(msg) => msg,
            FsCheckerError::Other(msg) => msg,
        }
    }

    pub fn source(&self) -> Option<FsError> {
        match self {
            FsCheckerError::IO(e) => Some(FsError::IO(e.clone())),
            FsCheckerError::Parsing(e) => Some(FsError::Parsing(e.clone())),
            FsCheckerError::Cursor(e) => Some(FsError::Cursor(e.clone())),
            _ => None,
        }
    }
}

impl fmt::Display for FsCheckerError {
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

/// Top-level error
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    IO(BlockIOError),
    Allocator(FsAllocatorError),
    Parsing(FsParsingError),
    Resolver(FsResolverError),
    Formatter(FsFormatterError),
    Injector(FsInjectorError),
    Checker(FsCheckerError),
    Cursor(FsCursorError),
    Other(&'static str),
}

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
            FsError::Parsing(e) => e.msg(),
            FsError::Resolver(e) => e.msg(),
            FsError::Formatter(e) => e.msg(),
            FsError::Injector(e) => e.msg(),
            FsError::Checker(e) => e.msg(),
            FsError::Cursor(e) => e.msg(),
            FsError::Other(msg) => msg,
        }
    }

    pub fn source(&self) -> Option<FsError> {
        match self {
            FsError::Parsing(e) => e.source(),
            FsError::Resolver(e) => e.source(),
            FsError::Formatter(e) => e.source(),
            FsError::Injector(e) => e.source(),
            FsError::Checker(e) => e.source(),
            FsError::Cursor(e) => e.source(),
            FsError::IO(_) => None,
            FsError::Allocator(_) => None,
            FsError::Other(_) => None,
        }
    }
}

// === type Fs*Result ===

pub type FsResult<T = ()> = Result<T, FsError>;
pub type FsAllocatorResult<T = ()> = Result<T, FsAllocatorError>;
pub type FsParsingResult<T = ()> = Result<T, FsParsingError>;
pub type FsResolverResult<T = ()> = Result<T, FsResolverError>;
pub type FsFormatterResult<T = ()> = Result<T, FsFormatterError>;
pub type FsInjectorResult<T = ()> = Result<T, FsInjectorError>;
pub type FsCheckerResult<T = ()> = Result<T, FsCheckerError>;
pub type FsCursorResult<T = ()> = Result<T, FsCursorError>;

crate::fs_error_wiring! {
    top => FsError {
        BlockIOError     : IO,
        FsAllocatorError : Allocator,
        FsParsingError    : Parsing,
        FsResolverError    : Resolver,
        FsFormatterError : Formatter,
        FsInjectorError  : Injector,
        FsCheckerError   : Checker,
        FsCursorError    : Cursor,
    },
    str_into => [
        FsAllocatorError,
        FsParsingError,
        FsResolverError,
        FsFormatterError,
        FsInjectorError,
        FsCheckerError,
        FsCursorError,
    ],
    sub => {
        BlockIOError     => [ FsResolverError::IO, FsFormatterError::IO, FsInjectorError::IO, FsCheckerError::IO, FsCursorError::IO ],
        FsAllocatorError => [ FsInjectorError::Allocator ],
        FsParsingError    => [ FsResolverError::Parsing, FsCursorError::Parsing, FsCheckerError::Parsing ],
        FsCursorError    => [ FsResolverError::Cursor, FsCheckerError::Cursor]
    },
}

#[cfg(all(test, feature = "std"))]
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
