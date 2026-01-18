// SPDX-License-Identifier: MIT

use rimio::errors::*;

/// Unified error type for partition tools (GPT, MBR, etc.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartError {
    IO(rimio::errors::RimIOError),
    Gpt(GptError),
    Mbr(MbrError),
    Unsupported,
    NotFound,
    Other(&'static str),
}
pub type PartResult<T = ()> = core::result::Result<T, PartError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GptError {
    InvalidSignature {
        expected: [u8; 8],
        found: [u8; 8],
    },
    InvalidRevision {
        expected: u32,
        found: u32,
    },
    HeaderSizeTooSmall {
        min: u32,
        got: u32,
    },
    HeaderSizeTooLarge {
        max: usize,
        got: u32,
    },
    EntrySizeInvalid {
        base: u32,
        got: u32,
    },
    EntrySizeExceedsSector {
        entry_size: u32,
        sector_size: u64,
    },
    EntrySizeTooLarge {
        max: u32,
        got: u32,
    },
    NumEntriesOutOfRange {
        min: u32,
        max: u32,
        got: u32,
    },
    CrcHeaderMismatch {
        expected: u32,
        found: u32,
    },
    CrcEntriesMismatch {
        expected: u32,
        found: u32,
    },
    LbaOverflow,
    DiskTooSmallForAlignment,
    EntryOutOfBounds {
        first_usable: u64,
        last_usable: u64,
        start: u64,
        end: u64,
    },
    EntryUnaligned {
        lba: u64,
        align: u64,
    },
    Overlap {
        a_start: u64,
        a_end: u64,
        b_start: u64,
        b_end: u64,
    },
    PrimaryGptCorrupted,
    BackupGptCorrupted,
}

impl GptError {
    pub fn msg(&self) -> &'static str {
        use GptError::*;
        match self {
            InvalidSignature { .. } => "GPT: invalid signature",
            InvalidRevision { .. } => "GPT: unsupported revision",
            HeaderSizeTooSmall { .. } => "GPT: header size too small",
            HeaderSizeTooLarge { .. } => "GPT: header size too large",
            EntrySizeInvalid { .. } => "GPT: invalid entry size",
            EntrySizeExceedsSector { .. } => "GPT: entry size exceeds sector",
            EntrySizeTooLarge { .. } => "GPT: entry size too large",
            NumEntriesOutOfRange { .. } => "GPT: num_entries out of range",
            CrcHeaderMismatch { .. } => "GPT: header CRC mismatch",
            CrcEntriesMismatch { .. } => "GPT: entries CRC mismatch",
            LbaOverflow => "GPT: LBA arithmetic overflow",
            DiskTooSmallForAlignment => "GPT: disk too small for required alignment",
            EntryOutOfBounds { .. } => "GPT: partition out of usable bounds",
            EntryUnaligned { .. } => "GPT: partition start not aligned",
            Overlap { .. } => "GPT: partition overlap detected",
            PrimaryGptCorrupted => "GPT: primary GPT is corrupted",
            BackupGptCorrupted => "GPT: backup GPT is corrupted",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MbrError {
    InvalidSignature {
        expected: [u8; 2],
        found: [u8; 2],
    },
    InvalidBootFlag {
        got: u8,
    },
    ZeroSectors,
    ProtectiveMissing,
    ProtectiveExtraEntries,
    ProtectiveSizeMismatch {
        expected: u32,
        got: u32,
        gt_2tib: bool,
    },
    UnsupportedType {
        ty: u8,
    },
    Overlap {
        a_start: u64,
        a_end: u64,
        b_start: u64,
        b_end: u64,
    },
}

impl MbrError {
    pub fn msg(&self) -> &'static str {
        use MbrError::*;
        match self {
            InvalidSignature { .. } => "MBR: invalid signature",
            InvalidBootFlag { .. } => "MBR: invalid boot flag",
            ZeroSectors => "MBR: non-empty entry has zero sectors",
            ProtectiveMissing => "MBR: no protective GPT (0xEE) in entry 0",
            ProtectiveExtraEntries => "MBR: only entry 0 must be used for protective MBR",
            ProtectiveSizeMismatch { .. } => "MBR: protective size mismatch",
            UnsupportedType { .. } => "MBR: unsupported legacy type",
            Overlap { .. } => "MBR: partition overlap detected",
        }
    }
}

impl PartError {
    pub fn msg(&self) -> &'static str {
        match self {
            PartError::IO(e) => e.msg(),
            PartError::Unsupported => "Unsupported",
            PartError::NotFound => "No partition table found",
            PartError::Other(msg) => msg,
            PartError::Gpt(e) => e.msg(),
            PartError::Mbr(e) => e.msg(),
        }
    }
}

impl From<RimIOError> for PartError {
    fn from(e: RimIOError) -> Self {
        PartError::IO(e)
    }
}

impl From<GptError> for PartError {
    fn from(e: GptError) -> Self {
        PartError::Gpt(e)
    }
}

impl From<MbrError> for PartError {
    fn from(e: MbrError) -> Self {
        PartError::Mbr(e)
    }
}

impl From<&'static str> for PartError {
    fn from(s: &'static str) -> Self {
        PartError::Other(s)
    }
}

impl core::fmt::Display for PartError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PartError::IO(e) => write!(f, "I/O error: {}", e.msg()),
            PartError::Unsupported => write!(f, "{}", self.msg()),
            PartError::NotFound => write!(f, "{}", self.msg()),
            PartError::Other(msg) => write!(f, "{msg}"),
            PartError::Gpt(e) => write!(f, "{e}"), // e implémente déjà Display
            PartError::Mbr(e) => write!(f, "{e}"), // idem
        }
    }
}

impl core::fmt::Display for GptError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use GptError::*;
        match *self {
            InvalidSignature { expected, found } => write!(
                f,
                "{} (expected {:?}, found {:?})",
                self.msg(),
                expected,
                found
            ),
            InvalidRevision { expected, found } => write!(
                f,
                "{} (expected 0x{:08X}, found 0x{:08X})",
                self.msg(),
                expected,
                found
            ),
            HeaderSizeTooSmall { min, got } => {
                write!(f, "{} (min {}, got {})", self.msg(), min, got)
            }
            HeaderSizeTooLarge { max, got } => {
                write!(f, "{} (max {}, got {})", self.msg(), max, got)
            }
            EntrySizeInvalid { base, got } => {
                write!(f, "{} (base {}, got {})", self.msg(), base, got)
            }
            EntrySizeExceedsSector {
                entry_size,
                sector_size,
            } => write!(
                f,
                "{} (entry {}, sector {})",
                self.msg(),
                entry_size,
                sector_size
            ),
            EntrySizeTooLarge { max, got } => {
                write!(f, "{} (max {}, got {})", self.msg(), max, got)
            }
            NumEntriesOutOfRange { min, max, got } => {
                write!(f, "{} (min {}, max {}, got {})", self.msg(), min, max, got)
            }
            CrcHeaderMismatch { expected, found } | CrcEntriesMismatch { expected, found } => {
                write!(
                    f,
                    "{} (expected 0x{:08X}, found 0x{:08X})",
                    self.msg(),
                    expected,
                    found
                )
            }
            EntryOutOfBounds {
                first_usable,
                last_usable,
                start,
                end,
            } => write!(
                f,
                "{} (usable {}..={}, entry {}..={})",
                self.msg(),
                first_usable,
                last_usable,
                start,
                end
            ),
            EntryUnaligned { lba, align } => {
                write!(f, "{} (lba {}, align {})", self.msg(), lba, align)
            }
            Overlap {
                a_start,
                a_end,
                b_start,
                b_end,
            } => write!(
                f,
                "{} (A {}..={}, B {}..={})",
                self.msg(),
                a_start,
                a_end,
                b_start,
                b_end
            ),
            LbaOverflow | DiskTooSmallForAlignment | PrimaryGptCorrupted | BackupGptCorrupted => {
                write!(f, "{}", self.msg())
            }
        }
    }
}

impl core::fmt::Display for MbrError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use MbrError::*;
        match *self {
            InvalidSignature { expected, found } => write!(
                f,
                "{} (expected {:02X}{:02X}, found {:02X}{:02X})",
                self.msg(),
                expected[0],
                expected[1],
                found[0],
                found[1]
            ),
            InvalidBootFlag { got } => write!(f, "{} 0x{:02X}", self.msg(), got),
            ZeroSectors => write!(f, "{}", self.msg()),
            ProtectiveMissing | ProtectiveExtraEntries => write!(f, "{}", self.msg()),
            ProtectiveSizeMismatch {
                expected,
                got,
                gt_2tib,
            } => {
                if gt_2tib {
                    write!(
                        f,
                        "{} (expected 0xFFFF_FFFF / {}, got {})",
                        self.msg(),
                        expected,
                        got
                    )
                } else {
                    write!(f, "{} (expected {}, got {})", self.msg(), expected, got)
                }
            }
            UnsupportedType { ty } => write!(f, "{} 0x{:02X}", self.msg(), ty),
            Overlap {
                a_start,
                a_end,
                b_start,
                b_end,
            } => write!(
                f,
                "{} (A {}..={}, B {}..={})",
                self.msg(),
                a_start,
                a_end,
                b_start,
                b_end
            ),
        }
    }
}
