use crate::{
    FsMeta,
    core::{FsResolverResult, utils::upcase::UpcaseHandle as CoreUpcase},
    {
        constant::{
            EXFAT_UPCASE_FULL, EXFAT_UPCASE_FULL_CHECKSUM, EXFAT_UPCASE_FULL_LENGTH,
            EXFAT_UPCASE_MINIMAL, EXFAT_UPCASE_MINIMAL_CHECKSUM, EXFAT_UPCASE_MINIMAL_LENGTH,
        },
        meta::ExFatMeta,
    },
};
use rimio::prelude::*;

#[derive(Debug, Clone, PartialEq)]
pub enum UpcaseFlavor {
    Minimal,
    Full,
}

pub struct UpcaseHandle(CoreUpcase);

impl UpcaseHandle {
    #[inline]
    pub fn upper(&self, cu: u16) -> u16 {
        self.0.upper(cu)
    }

    #[inline]
    pub fn checksum(&self) -> u32 {
        self.0.checksum()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    pub fn from_io<IO: RimRead + ?Sized>(io: &mut IO, meta: &ExFatMeta) -> FsResolverResult<Self> {
        let len = meta.upcase_size_bytes as usize;

        if len == 0 {
            return Err("upcase_size_zero".into());
        }

        let mut blob = vec![0u8; len];
        let offset = meta.unit_offset(meta.upcase_cluster);
        // Upcase table is always contiguous
        io.read_block_best_effort(offset, &mut blob, meta.unit_size())?;

        // Use core constructor
        let handle = CoreUpcase::from_bytes(&blob).map_err(|_| "upcase_parse_error")?;

        Ok(Self(handle))
    }

    pub fn from_flavor(flavor: &UpcaseFlavor) -> Self {
        let (compressed, _checksum, _len): (&[u8], u32, usize) = match flavor {
            UpcaseFlavor::Minimal => (
                &EXFAT_UPCASE_MINIMAL,
                EXFAT_UPCASE_MINIMAL_CHECKSUM,
                EXFAT_UPCASE_MINIMAL_LENGTH,
            ),
            UpcaseFlavor::Full => (
                &EXFAT_UPCASE_FULL,
                EXFAT_UPCASE_FULL_CHECKSUM,
                EXFAT_UPCASE_FULL_LENGTH,
            ),
        };
        // The compressed data in constant/upcase.rs needs to be expanded or used?
        // Wait, original code did:
        // blob.copy_from_slice(compressed);
        // This implies the constants are NOT compressed, just bytes.

        // Original code:
        // let mut blob = vec![0u8; len].into_boxed_slice();
        // blob.copy_from_slice(compressed);
        // Note: EXFAT_UPCASE_MINIMAL is an array reference.

        // So we can just pass the bytes.
        let handle = CoreUpcase::from_bytes(compressed).expect("Invalid static upcase table");
        Self(handle)
    }
}
