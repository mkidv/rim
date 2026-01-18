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

#[cfg(feature = "std")]
impl std::error::Error for FsAllocatorError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsParsingError {
    IO(RimIOError),
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
            FsParsingError::IO(e) => Some(FsError::IO(*e)),
            _ => None,
        }
    }
}

impl fmt::Display for FsParsingError {
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
pub enum FsCursorError {
    IO(RimIOError),
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
            FsCursorError::IO(e) => Some(FsError::IO(*e)),
            FsCursorError::Parsing(e) => Some(FsError::Parsing(*e)),
            _ => None,
        }
    }
}

impl fmt::Display for FsCursorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.msg())?;
        if let FsCursorError::InvalidCluster(cluster) = self {
            write!(f, " (cluster: {cluster})")?;
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
    IO(RimIOError),
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
            FsResolverError::IO(e) => Some(FsError::IO(*e)),
            FsResolverError::Cursor(e) => Some(FsError::Cursor(*e)),
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
    IO(RimIOError),
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
            FsFormatterError::IO(e) => Some(FsError::IO(*e)),
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
    IO(RimIOError),
    Allocator(FsAllocatorError),
    Resolver(FsResolverError),
    StackUnderflow,
    Invalid(&'static str),
    Other(&'static str),
}

impl FsInjectorError {
    pub fn msg(&self) -> &'static str {
        match self {
            FsInjectorError::IO(_) => "IO error",
            FsInjectorError::Allocator(_) => "Allocator error",
            FsInjectorError::Resolver(_) => "Resolver error",
            FsInjectorError::StackUnderflow => "Stack underflow",
            FsInjectorError::Invalid(msg) => msg,
            FsInjectorError::Other(msg) => msg,
        }
    }

    pub fn source(&self) -> Option<FsError> {
        match self {
            FsInjectorError::IO(e) => Some(FsError::IO(*e)),
            FsInjectorError::Allocator(e) => Some(FsError::Allocator(*e)),
            FsInjectorError::Resolver(e) => Some(FsError::Resolver(*e)),
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
    IO(RimIOError),
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
            FsCheckerError::IO(e) => Some(FsError::IO(*e)),
            FsCheckerError::Parsing(e) => Some(FsError::Parsing(*e)),
            FsCheckerError::Cursor(e) => Some(FsError::Cursor(*e)),
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
    IO(RimIOError),
    Allocator(FsAllocatorError),
    Parsing(FsParsingError),
    Resolver(FsResolverError),
    Formatter(FsFormatterError),
    Injector(FsInjectorError),
    Checker(FsCheckerError),
    Cursor(FsCursorError),
    Invalid(&'static str),
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
            FsError::Invalid(msg) => msg,
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
            FsError::Invalid(_) => None,
            FsError::Other(_) => None,
        }
    }
}

// type Fs*Result

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
        RimIOError     : IO,
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
        RimIOError     => [ FsResolverError::IO, FsFormatterError::IO, FsInjectorError::IO, FsCheckerError::IO, FsCursorError::IO ],
        FsAllocatorError => [ FsInjectorError::Allocator ],
        FsParsingError    => [ FsResolverError::Parsing, FsCursorError::Parsing, FsCheckerError::Parsing ],
        FsResolverError   => [ FsInjectorError::Resolver ],
        FsCursorError    => [ FsResolverError::Cursor, FsCheckerError::Cursor]
    },
}

// std::error::Error implementations
// These are only available when the `std` feature is enabled, providing
// interoperability with the standard library error handling ecosystem.

#[cfg(feature = "std")]
mod std_error_impls {
    use super::*;

    impl std::error::Error for FsParsingError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            match self {
                FsParsingError::IO(e) => Some(e),
                _ => None,
            }
        }
    }

    impl std::error::Error for FsCursorError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            match self {
                FsCursorError::IO(e) => Some(e),
                FsCursorError::Parsing(e) => Some(e),
                _ => None,
            }
        }
    }

    impl std::error::Error for FsResolverError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            match self {
                FsResolverError::IO(e) => Some(e),
                FsResolverError::Cursor(e) => Some(e),
                FsResolverError::Parsing(e) => Some(e),
                _ => None,
            }
        }
    }

    impl std::error::Error for FsFormatterError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            match self {
                FsFormatterError::IO(e) => Some(e),
                _ => None,
            }
        }
    }

    impl std::error::Error for FsInjectorError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            match self {
                FsInjectorError::IO(e) => Some(e),
                FsInjectorError::Allocator(e) => Some(e),
                FsInjectorError::Resolver(e) => Some(e),
                _ => None,
            }
        }
    }

    impl std::error::Error for FsCheckerError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            match self {
                FsCheckerError::IO(e) => Some(e),
                FsCheckerError::Parsing(e) => Some(e),
                FsCheckerError::Cursor(e) => Some(e),
                _ => None,
            }
        }
    }

    impl std::error::Error for FsError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            match self {
                FsError::IO(e) => Some(e),
                FsError::Allocator(e) => Some(e),
                FsError::Parsing(e) => Some(e),
                FsError::Resolver(e) => Some(e),
                FsError::Formatter(e) => Some(e),
                FsError::Injector(e) => Some(e),
                FsError::Checker(e) => Some(e),
                FsError::Cursor(e) => Some(e),
                _ => None,
            }
        }
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn test_error_chain_display() {
        let low = RimIOError::Unsupported;
        let inj = FsInjectorError::IO(low);
        let top = FsError::Injector(inj);

        println!("{top}");
    }
}
