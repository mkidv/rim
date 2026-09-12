// SPDX-License-Identifier: MIT

//! Disk image container parsing and serialization errors.

use core::fmt;

use rimio::errors::RimIOError;

pub type RimImgResult<T = ()> = core::result::Result<T, RimImgError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RimImgError {
    IO(RimIOError),
    UnknownFormat,
    UnsupportedFormat,
    InvalidHeader(&'static str),
    Corrupted(&'static str),
    SizeOverflow,
}

impl RimImgError {
    pub fn msg(&self) -> &'static str {
        match self {
            RimImgError::IO(e) => e.msg(),
            RimImgError::UnknownFormat => "Unknown image format",
            RimImgError::UnsupportedFormat => "Unsupported image format",
            RimImgError::InvalidHeader(msg) => msg,
            RimImgError::Corrupted(msg) => msg,
            RimImgError::SizeOverflow => "Image size overflow",
        }
    }
}

impl From<RimIOError> for RimImgError {
    #[inline]
    fn from(e: RimIOError) -> Self {
        RimImgError::IO(e)
    }
}

impl From<&'static str> for RimImgError {
    #[inline]
    fn from(msg: &'static str) -> Self {
        RimImgError::InvalidHeader(msg)
    }
}

impl fmt::Display for RimImgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.msg())
    }
}

#[cfg(feature = "std")]
impl std::error::Error for RimImgError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RimImgError::IO(e) => Some(e),
            _ => None,
        }
    }
}
