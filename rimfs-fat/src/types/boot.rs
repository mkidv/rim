// SPDX-License-Identifier: MIT

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

use crate::{
    Validate,
    core::FsParsingError,
    {constant::*, meta::*},
};

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug, Default)]
#[repr(C, packed)]
pub struct FatCommonBpb {
    pub jump_boot: [u8; 3],
    pub oem_name: [u8; 8],
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u16,
    pub num_fats: u8,
    pub root_entry_count: u16,
    pub total_sectors_16: u16,
    pub media: u8,
    pub fat_size_16: u16,
    pub sectors_per_track: u16,
    pub num_heads: u16,
    pub hidden_sectors: u32,
    pub total_sectors_32: u32,
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Unaligned, Copy, Clone, Debug, Default)]
#[repr(C, packed)]
pub struct Fat12_16Ebpb {
    pub drive_number: u8,
    pub reserved1: u8,
    pub boot_signature: u8,
    pub volume_id: u32,
    pub volume_label: [u8; 11],
    pub fs_type: [u8; 8],
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Unaligned, Copy, Clone, Debug, Default)]
#[repr(C, packed)]
pub struct Fat32Ebpb {
    pub fat_size_32: u32,
    pub ext_flags: u16,
    pub fs_version: u16,
    pub root_cluster: u32,
    pub fs_info_sector: u16,
    pub backup_boot_sector: u16,
    pub reserved1: [u8; 12],
    pub drive_number: u8,
    pub reserved2: u8,
    pub boot_signature: u8,
    pub volume_id: u32,
    pub volume_label: [u8; 11],
    pub fs_type: [u8; 8],
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct FatVbr {
    pub bpb: FatCommonBpb,

    // This region starts at offset 36.
    // Length: 512 (sector) - 36 (header) - 2 (signature) = 474 bytes.
    pub eb_pb: [u8; 474],
    pub signature: u16,
}

impl FatVbr {
    pub fn from_meta(meta: &FatMeta) -> Self {
        let bpb = FatCommonBpb {
            jump_boot: FAT_JUMP_BOOT,
            oem_name: if meta.use_integrity {
                *FAT_OEM_NAME
            } else {
                oem_name()
            },
            bytes_per_sector: meta.bytes_per_sector,
            sectors_per_cluster: meta.sectors_per_cluster as u8,
            reserved_sectors: (meta.fat_offset_bytes / meta.bytes_per_sector as u64) as u16,
            num_fats: meta.num_fats,
            root_entry_count: if meta.bits == 32 {
                0
            } else {
                meta.root_entry_count
            },
            total_sectors_16: if meta.bits != 32 && meta.volume_size_sectors < 65536 {
                meta.volume_size_sectors as u16
            } else {
                0
            },
            media: FAT_MEDIA_DESCRIPTOR,
            fat_size_16: if meta.bits != 32 && meta.fat_size_sectors < 65536 {
                meta.fat_size_sectors as u16
            } else {
                0
            },
            sectors_per_track: FAT_SECTORS_PER_TRACK,
            num_heads: FAT_HEADS,
            hidden_sectors: 0, // Set later via with_hidden_sectors
            total_sectors_32: if meta.bits == 32 || meta.volume_size_sectors >= 65536 {
                meta.volume_size_sectors as u32
            } else {
                0
            },
        };

        let mut vbr = Self {
            bpb,
            eb_pb: [0u8; 474],
            signature: FAT_SIGNATURE,
        };

        if meta.bits != 32 {
            let ebpb = Fat12_16Ebpb {
                drive_number: FAT_DRIVE_NUMBER,
                boot_signature: FAT_BOOT_SIGNATURE,
                volume_id: meta.volume_id,
                volume_label: meta.volume_label,
                fs_type: match meta.bits {
                    12 => *FAT_FS_TYPE_12,
                    16 => *FAT_FS_TYPE_16,
                    32 => *FAT_FS_TYPE_32,
                    _ => *FAT_FS_TYPE_UNKNOWN,
                },
                ..Default::default()
            };
            *vbr.f16_mut() = ebpb;
        } else {
            let ebpb = Fat32Ebpb {
                fat_size_32: meta.fat_size_sectors,
                ext_flags: FAT_EXT_FLAGS,
                fs_version: FAT_FS_VERSION,
                root_cluster: meta.root_unit(),
                fs_info_sector: FAT_FSINFO_SECTOR as u16,
                backup_boot_sector: FAT_VBR_BACKUP_SECTOR as u16,
                drive_number: FAT_DRIVE_NUMBER,
                boot_signature: FAT_BOOT_SIGNATURE,
                volume_id: meta.volume_id,
                volume_label: meta.volume_label,
                fs_type: *FAT_FS_TYPE_32,
                ..Default::default()
            };
            *vbr.f32_mut() = ebpb;
        }

        vbr
    }

    #[inline(always)]
    pub fn is_fat32(&self) -> bool {
        self.bpb.fat_size_16 == 0
    }

    /// Access the extended BPB area as a specific type.
    ///
    /// # Safety
    /// The caller must ensure that the requested type `T` fits within the `eb_pb` area (474 bytes).
    /// Most standard EBPBs (FAT12/16: 26 bytes, FAT32: 54 bytes) fit easily.
    pub fn view_as<
        T: zerocopy::FromBytes + zerocopy::Unaligned + zerocopy::KnownLayout + zerocopy::Immutable,
    >(
        &self,
    ) -> &T {
        let size = core::mem::size_of::<T>();
        zerocopy::Ref::into_ref(zerocopy::Ref::<&[u8], T>::from_bytes(&self.eb_pb[..size]).unwrap())
    }

    /// Access the extended BPB area as a specific mutable type.
    pub fn view_mut_as<
        T: zerocopy::FromBytes
            + zerocopy::IntoBytes
            + zerocopy::Unaligned
            + zerocopy::KnownLayout
            + zerocopy::Immutable,
    >(
        &mut self,
    ) -> &mut T {
        let size = core::mem::size_of::<T>();
        zerocopy::Ref::into_mut(
            zerocopy::Ref::<&mut [u8], T>::from_bytes(&mut self.eb_pb[..size]).unwrap(),
        )
    }

    /// Shortcut for FAT12/16 layouts.
    #[inline(always)]
    pub fn f16(&self) -> &Fat12_16Ebpb {
        self.view_as::<Fat12_16Ebpb>()
    }

    /// Shortcut for FAT32 layouts.
    #[inline(always)]
    pub fn f32(&self) -> &Fat32Ebpb {
        self.view_as::<Fat32Ebpb>()
    }

    /// Mutable shortcut for FAT12/16 layouts.
    #[inline(always)]
    pub fn f16_mut(&mut self) -> &mut Fat12_16Ebpb {
        self.view_mut_as::<Fat12_16Ebpb>()
    }

    /// Mutable shortcut for FAT32 layouts.
    #[inline(always)]
    pub fn f32_mut(&mut self) -> &mut Fat32Ebpb {
        self.view_mut_as::<Fat32Ebpb>()
    }

    pub fn fat_size_sectors(&self) -> u32 {
        if self.bpb.fat_size_16 != 0 {
            self.bpb.fat_size_16 as u32
        } else {
            self.f32().fat_size_32
        }
    }

    pub fn with_boot_code(mut self, bits: u8, code: &[u8]) -> Self {
        let offset = if bits == 32 {
            core::mem::size_of::<Fat32Ebpb>()
        } else {
            core::mem::size_of::<Fat12_16Ebpb>()
        };
        let available = self.eb_pb.len() - offset;
        let len = code.len().min(available);
        self.eb_pb[offset..offset + len].copy_from_slice(&code[..len]);
        self
    }

    pub fn with_hidden_sectors(mut self, hidden: u32) -> Self {
        self.bpb.hidden_sectors = hidden;
        self
    }
}

impl Default for FatVbr {
    fn default() -> Self {
        Self {
            bpb: FatCommonBpb {
                jump_boot: FAT_JUMP_BOOT,
                oem_name: oem_name(),
                bytes_per_sector: FAT_SECTOR_SIZE,
                sectors_per_cluster: FAT_SECTORS_PER_CLUSTER,
                reserved_sectors: DEFAULT_FAT_RESERVED_SECTORS,
                num_fats: FAT_NUM_FATS,
                root_entry_count: FAT_ROOT_ENTRY_COUNT,
                total_sectors_16: FAT_TOTAL_SECTORS_16,
                media: FAT_MEDIA_DESCRIPTOR,
                fat_size_16: FAT_FAT_SIZE_16,
                sectors_per_track: FAT_SECTORS_PER_TRACK,
                num_heads: FAT_HEADS,
                hidden_sectors: FAT_HIDDEN_SECTORS,
                total_sectors_32: 0,
            },
            eb_pb: [0u8; 474],
            signature: FAT_SIGNATURE,
        }
    }
}

impl Validate<FatMeta> for FatVbr {
    type Err = FsParsingError;

    fn neutralized(&self) -> Self {
        *self
    }

    fn validate(&self, meta: &FatMeta) -> Result<(), Self::Err> {
        crate::ensure!(
            self.signature == FAT_SIGNATURE,
            FsParsingError::Invalid("VBR: missing 0x55AA")
        );
        // Sanity BPB
        let bps = self.bpb.bytes_per_sector as usize;
        let spc = self.bpb.sectors_per_cluster as usize;
        crate::ensure!(
            bps > 0 && (bps & (bps - 1)) == 0,
            FsParsingError::Invalid("BPB: BytesPerSector not pow2")
        );
        crate::ensure!(
            spc > 0 && (spc & (spc - 1)) == 0,
            FsParsingError::Invalid("BPB: SectorsPerCluster not pow2")
        );
        crate::ensure!(
            self.bpb.num_fats > 0,
            FsParsingError::Invalid("BPB: NumFATs == 0")
        );

        if meta.bits == 32 {
            // Check FAT32 specific fields
            let ebpb = self.view_as::<Fat32Ebpb>();
            crate::ensure!(
                ebpb.fat_size_32 > 0,
                FsParsingError::Invalid("BPB: FATLength == 0")
            );
            crate::ensure!(
                ebpb.root_cluster >= FAT_FIRST_CLUSTER
                    && ebpb.root_cluster <= meta.last_data_unit(),
                FsParsingError::Invalid("BPB: root_cluster out of range")
            );
        }
        Ok(())
    }
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct FatFsInfo {
    pub lead_signature: [u8; 4],
    pub reserved1: [u8; 476],
    pub fat_checksum: u32, // RimFAT: CRC32 of the active FAT table
    pub struct_signature: [u8; 4],
    pub free_cluster_count: u32,
    pub next_free_cluster: u32,
    pub reserved2: [u8; 12],
    pub trail_signature: [u8; 4],
}

impl FatFsInfo {
    pub fn from_meta(meta: &FatMeta) -> Self {
        Self {
            lead_signature: FAT_FSINFO_LEAD_SIGNATURE,
            reserved1: [0u8; 476],
            fat_checksum: 0,
            struct_signature: FAT_FSINFO_STRUCT_SIGNATURE,
            free_cluster_count: meta.cluster_count.saturating_sub(1),
            next_free_cluster: 3,
            reserved2: [0u8; 12],
            trail_signature: FAT_FSINFO_TRAIL_SIGNATURE,
        }
    }
}

impl Default for FatFsInfo {
    fn default() -> Self {
        Self {
            lead_signature: FAT_FSINFO_LEAD_SIGNATURE,
            reserved1: [0u8; 476],
            fat_checksum: 0,
            struct_signature: FAT_FSINFO_STRUCT_SIGNATURE,
            free_cluster_count: FAT_FSINFO_UNKNOWN,
            next_free_cluster: FAT_ROOT_CLUSTER + 1,
            reserved2: [0u8; 12],
            trail_signature: FAT_FSINFO_TRAIL_SIGNATURE,
        }
    }
}

impl Validate<FatMeta> for FatFsInfo {
    type Err = FsParsingError;

    fn neutralized(&self) -> Self {
        *self
    }

    fn validate(&self, meta: &FatMeta) -> Result<(), Self::Err> {
        crate::ensure!(
            self.lead_signature == FAT_FSINFO_LEAD_SIGNATURE,
            FsParsingError::Invalid("FSINFO: bad lead sig")
        );
        crate::ensure!(
            self.struct_signature == FAT_FSINFO_STRUCT_SIGNATURE,
            FsParsingError::Invalid("FSINFO: bad struct sig")
        );
        crate::ensure!(
            self.trail_signature == FAT_FSINFO_TRAIL_SIGNATURE,
            FsParsingError::Invalid("FSINFO: bad trail sig")
        );
        if self.next_free_cluster != FAT_FSINFO_UNKNOWN {
            let c = self.next_free_cluster;
            crate::ensure!(
                c >= meta.first_data_unit() && c <= meta.last_data_unit(),
                FsParsingError::Invalid("FSINFO: next_free out of range")
            );
        }
        Ok(())
    }
}
