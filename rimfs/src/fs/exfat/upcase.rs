#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{boxed::Box, vec};

// exfat/upcase.rs
use crate::{
    core::{FsResolverResult, cursor::LinearCursor, utils::checksum_utils::accumulate_checksum},
    fs::exfat::{
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

pub struct UpcaseHandle {
    table: Box<[u16]>,
    bytes: Box<[u8]>,
    len: usize,
    checksum: u32,
}

impl UpcaseHandle {
    #[inline]
    pub fn upper(&self, cu: u16) -> u16 {
        let idx = cu as usize;
        if idx < self.table.len() {
            self.table[idx]
        } else {
            cu
        }
    }

    #[inline]
    pub fn checksum(&self) -> u32 {
        self.checksum
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn from_io<IO: RimIO + ?Sized>(io: &mut IO, meta: &ExFatMeta) -> FsResolverResult<Self> {
        let len = meta.upcase_size_bytes as usize;

        if len == 0 {
            return Err("upcase_size_zero".into());
        }
        if !len.is_multiple_of(2) {
            return Err("upcase_size_not_even".into());
        }

        // complete destination buffer
        let mut cur =
            LinearCursor::from_len_bytes(meta, meta.upcase_cluster, meta.upcase_size_bytes);

        let mut blob = vec![0u8; len].into_boxed_slice();
        cur.read_into(io, blob.len(), &mut blob)?;
        // Checksum in one pass
        let mut checksum: u32 = 0;
        accumulate_checksum(&mut checksum, &blob);

        // Build u16 LE table in one pass
        let mut table: Box<[u16]> = vec![0u16; len / 2].into_boxed_slice();
        for (i, ch) in blob.chunks_exact(2).enumerate() {
            table[i] = u16::from_le_bytes([ch[0], ch[1]]);
        }

        Ok(Self {
            table,
            bytes: blob,
            len,
            checksum,
        })
    }

    pub fn from_flavor(flavor: &UpcaseFlavor) -> Self {
        let (compressed, checksum, len): (&[u8], u32, usize) = match flavor {
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
        let mut blob = vec![0u8; len].into_boxed_slice();
        blob.copy_from_slice(compressed);

        let mut table = vec![0u16; len / 2].into_boxed_slice();
        for (i, chunk) in blob.chunks_exact(2).take(len / 2).enumerate() {
            table[i] = u16::from_le_bytes([chunk[0], chunk[1]]);
        }

        Self {
            table,
            bytes: blob,
            len,
            checksum,
        }
    }
}
