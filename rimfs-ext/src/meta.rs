// SPDX-License-Identifier: MIT

//! ext2/3/4 metadata descriptors and block group geometry.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::string::{String, ToString};

use crate::{
    constant::*,
    core::{
        FsError, FsResult,
        ext::{BlockMapMeta, ExtFsMeta, ExtentMeta},
        traits::FsMeta,
        utils::volume::generate_volume_id_128,
    },
    types::{
        ExtSuperblock,
        flags::{ExtCompatFeatures, ExtIncompatFeatures, ExtRoCompatFeatures},
    },
};
use rimio::prelude::*;

/// Feature configuration for the filesystem
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtFeatureSet {
    /// Has compatible features (extended attributes, dir index)
    pub has_compat: bool,
    /// Has incompatible features (filetype)
    pub has_incompat: bool,
    /// Has extents (incompatible)
    pub has_extents: bool,
    /// Has 64-bit support (incompatible)
    pub has_64bit: bool,
    /// Has high-precision timestamps, large files, etc (ro-compat)
    pub has_ro_compat: bool,
    /// Is the root directory sparse (ro-compat)
    pub has_sparse_super: bool,
    /// Has journal
    pub has_journal: bool,
}

impl ExtFeatureSet {
    /// Standard Ext2: No features
    pub const EXT2: Self = Self {
        has_compat: false,
        has_incompat: true, // only filetype
        has_extents: false,
        has_64bit: false,
        has_ro_compat: true, // sparse super only
        has_sparse_super: true,
        has_journal: false,
    };

    /// Standard Ext3: Journaling (not impl here but compatible), High-compat basic
    pub const EXT3: Self = Self {
        has_compat: true,
        has_incompat: true,
        has_extents: false,
        has_64bit: false,
        has_ro_compat: true,
        has_sparse_super: true,
        has_journal: true,
    };

    /// Standard Ext: All modern features
    pub const EXT: Self = Self {
        has_compat: true,
        has_incompat: true,
        has_extents: true,
        has_64bit: true,
        has_ro_compat: true,
        has_sparse_super: true,
        has_journal: true,
    };

    pub fn from_superblock(sb: &ExtSuperblock) -> Self {
        let compat = ExtCompatFeatures::from_bits_truncate(sb.s_feature_compat.get());
        let incompat = ExtIncompatFeatures::from_bits_truncate(sb.s_feature_incompat.get());
        let ro_compat = ExtRoCompatFeatures::from_bits_truncate(sb.s_feature_ro_compat.get());

        Self {
            has_compat: compat
                .intersects(ExtCompatFeatures::EXT_ATTR | ExtCompatFeatures::DIR_INDEX),
            has_incompat: incompat.contains(ExtIncompatFeatures::FILETYPE),
            has_extents: incompat.contains(ExtIncompatFeatures::EXTENTS),
            has_64bit: incompat.contains(ExtIncompatFeatures::_64BIT),
            has_ro_compat: ro_compat.contains(ExtRoCompatFeatures::LARGE_FILE),
            has_sparse_super: ro_compat.contains(ExtRoCompatFeatures::SPARSE_SUPER),
            has_journal: compat.contains(ExtCompatFeatures::HAS_JOURNAL),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExtMeta {
    pub features: ExtFeatureSet,
    pub volume_id: [u8; 16],
    pub volume_label: [u8; 16],
    pub volume_size_bytes: u64,
    pub block_size: u32,
    pub block_count: u64,
    pub inode_count: u64,
    pub blocks_per_group: u32,
    pub inodes_per_group: u32,
    pub group_count: u32,
    pub first_data_block: u32,
    pub inode_size: u32,
    pub bgdt_entry_size: usize,

    pub use_integrity: bool,       // RimExt: Per-block/inode integrity
    pub use_group_integrity: bool, // RimExt: Global group metadata integrity
    pub is_dirty: bool,            // RimExt: Volume was not cleanly unmounted
}

impl ExtMeta {
    pub fn new(size_bytes: u64, volume_label: Option<&str>) -> FsResult<Self> {
        let block_size = EXT_DEFAULT_BLOCK_SIZE;

        let blocks_per_group = block_size
            .checked_mul(8)
            .ok_or(FsError::Invalid("blocks_per_group overflow"))?;

        let inodes_per_group = default_inodes_per_group(block_size, blocks_per_group)?;

        Self::new_custom(
            ExtFeatureSet::EXT,
            size_bytes,
            volume_label,
            None,
            block_size,
            inodes_per_group,
        )
    }

    pub fn new_ext2(size_bytes: u64, volume_label: Option<&str>) -> FsResult<Self> {
        let block_size = EXT_DEFAULT_BLOCK_SIZE;

        let blocks_per_group = block_size
            .checked_mul(8)
            .ok_or(FsError::Invalid("blocks_per_group overflow"))?;

        let inodes_per_group = default_inodes_per_group(block_size, blocks_per_group)?;

        Self::new_custom(
            ExtFeatureSet::EXT2,
            size_bytes,
            volume_label,
            None,
            block_size,
            inodes_per_group,
        )
    }

    pub fn new_ext3(size_bytes: u64, volume_label: Option<&str>) -> FsResult<Self> {
        let block_size = EXT_DEFAULT_BLOCK_SIZE;

        let blocks_per_group = block_size
            .checked_mul(8)
            .ok_or(FsError::Invalid("blocks_per_group overflow"))?;

        let inodes_per_group = default_inodes_per_group(block_size, blocks_per_group)?;

        Self::new_custom(
            ExtFeatureSet::EXT3,
            size_bytes,
            volume_label,
            None,
            block_size,
            inodes_per_group,
        )
    }

    pub fn new_custom(
        features: ExtFeatureSet,
        volume_size_bytes: u64,
        volume_label: Option<&str>,
        volume_id: Option<[u8; 16]>,
        block_size: u32,
        inodes_per_group: u32,
    ) -> FsResult<Self> {
        crate::ensure!(
            volume_size_bytes > 0,
            FsError::Invalid("Volume size must be > 0")
        );
        crate::ensure!(
            block_size.is_power_of_two() && (1024..=65536).contains(&block_size),
            FsError::Invalid("Block size must be power of 2 between 1024 and 65536")
        );
        let block_count = volume_size_bytes / block_size as u64;
        crate::ensure!(
            block_count >= 16,
            FsError::Invalid("Volume too small for EXT filesystem")
        );
        crate::ensure!(
            inodes_per_group > 0,
            FsError::Invalid("inodes_per_group must be > 0")
        );

        let volume_id = volume_id.unwrap_or_else(|| generate_volume_id_128().to_le_bytes());

        let blocks_per_group = block_size
            .checked_mul(8)
            .ok_or(FsError::Invalid("blocks_per_group overflow"))?;

        let group_count_u64 = block_count.div_ceil(blocks_per_group as u64);
        let group_count = u32::try_from(group_count_u64)
            .map_err(|_| FsError::Invalid("EXT group count exceeds supported u32 range"))?;

        crate::ensure!(
            inodes_per_group > 0,
            FsError::Invalid("inodes_per_group must be > 0")
        );

        crate::ensure!(
            inodes_per_group <= block_size * 8,
            FsError::Invalid("inodes_per_group exceeds inode bitmap capacity")
        );

        let inode_count = group_count as u64 * inodes_per_group as u64;

        let first_data_block = if block_size > 1024 { 0 } else { 1 };

        let mut volume_label_bytes = [0u8; 16];
        if let Some(label) = volume_label {
            let bytes = label.as_bytes();
            let len = bytes.len().min(16);
            volume_label_bytes[..len].copy_from_slice(&bytes[..len]);
        }

        let inode_size = if features.has_extents || features.has_64bit {
            EXT_DEFAULT_INODE_SIZE
        } else {
            EXT2_DEFAULT_INODE_SIZE
        };

        let bgdt_entry_size = if features.has_64bit {
            EXT4_BGDT_ENTRY_SIZE
        } else {
            EXT2_BGDT_ENTRY_SIZE
        };

        Ok(Self {
            features,
            volume_id,
            volume_label: volume_label_bytes,
            volume_size_bytes,
            block_size,
            block_count,
            blocks_per_group,
            group_count,
            inode_count,
            inodes_per_group,
            first_data_block,
            inode_size,
            bgdt_entry_size,
            use_integrity: false,
            use_group_integrity: false,
            is_dirty: false,
        })
    }

    pub fn from_io<IO: rimio::RimRead + ?Sized>(io: &mut IO) -> FsResult<Self> {
        let sb: ExtSuperblock = io.read_struct(EXT_SUPERBLOCK_OFFSET)?;

        if !sb.is_valid() {
            return Err(FsError::Invalid("Invalid Ext superblock magic"));
        }

        let block_size = 1024 << sb.s_log_block_size.get();
        let features = ExtFeatureSet::from_superblock(&sb);

        let block_count = if features.has_64bit {
            (sb.s_blocks_count_lo.get() as u64) | ((sb.s_blocks_count_hi.get() as u64) << 32)
        } else {
            sb.s_blocks_count_lo.get() as u64
        };

        let inode_count = sb.s_inodes_count.get() as u64;

        let volume_size_bytes = block_count * block_size as u64;

        let inode_size = if sb.s_inode_size.get() != 0 {
            sb.s_inode_size.get() as u32
        } else if features.has_extents || features.has_64bit {
            EXT_DEFAULT_INODE_SIZE
        } else {
            EXT2_DEFAULT_INODE_SIZE
        };

        let bgdt_entry_size = if sb.s_desc_size.get() != 0 {
            sb.s_desc_size.get() as usize
        } else if features.has_64bit {
            EXT4_BGDT_ENTRY_SIZE
        } else {
            EXT2_BGDT_ENTRY_SIZE
        };

        let mut meta = Self {
            features,
            volume_id: sb.s_uuid,
            volume_label: sb.s_volume_name,
            volume_size_bytes,
            block_size,
            block_count,
            blocks_per_group: sb.s_blocks_per_group.get(),
            group_count: block_count.div_ceil(sb.s_blocks_per_group.get() as u64) as u32,
            inode_count,
            inodes_per_group: sb.s_inodes_per_group.get(),
            first_data_block: sb.s_first_data_block.get(),
            inode_size,
            bgdt_entry_size,
            use_integrity: false,
            use_group_integrity: false,
            is_dirty: sb.s_state.get() == 2, // 2 = ERROR_FS, usually indicates dirty if not explicitly 1
        };

        // Detect RimExt Creator Tag in s_reserved (at offset 0x3F0, which is 380 in s_reserved)
        if sb.s_reserved.len() >= 388 {
            let tag = &sb.s_reserved[380..388];
            if tag.starts_with(b"RIM") {
                meta.use_integrity = true;
                meta.use_group_integrity = true;
            }
        }

        Ok(meta)
    }

    pub fn group_count(&self) -> usize {
        self.block_count.div_ceil(self.blocks_per_group as u64) as usize
    }

    pub fn group_total_blocks(&self, group_index: usize) -> usize {
        let count = self.group_count();
        if group_index < count - 1 {
            self.blocks_per_group as usize
        } else {
            (self.block_count as usize) - (group_index * self.blocks_per_group as usize)
        }
    }

    pub fn group_total_inodes(&self, group_index: usize) -> usize {
        let count = self.group_count();
        if group_index < count - 1 {
            self.inodes_per_group as usize
        } else {
            (self.inode_count as usize) - (group_index * self.inodes_per_group as usize)
        }
    }
}

impl FsMeta<u32> for ExtMeta {
    fn unit_size(&self) -> u64 {
        self.block_size as u64
    }

    fn unit_offset(&self, unit: u32) -> u64 {
        unit as u64 * self.block_size as u64
    }

    fn root_unit(&self) -> u32 {
        EXT_ROOT_INODE
    }

    fn first_data_unit(&self) -> u32 {
        self.first_data_block
    }

    fn last_data_unit(&self) -> u32 {
        (self.block_count.saturating_sub(1)) as u32
    }

    fn total_units(&self) -> u64 {
        self.block_count
    }

    fn size_bytes(&self) -> u64 {
        self.volume_size_bytes
    }

    fn label(&self) -> String {
        String::from_utf8_lossy(&self.volume_label)
            .trim_matches(char::from(0))
            .to_string()
    }
}

impl ExtFsMeta for ExtMeta {
    fn block_size(&self) -> u32 {
        self.block_size
    }

    fn inodes_per_group(&self) -> u32 {
        self.inodes_per_group
    }
}

impl ExtentMeta for ExtMeta {}

impl BlockMapMeta for ExtMeta {}

fn default_inodes_per_group(block_size: u32, blocks_per_group: u32) -> FsResult<u32> {
    let group_bytes = block_size as u64 * blocks_per_group as u64;

    let desired = group_bytes.div_ceil(EXT_DEFAULT_BYTES_PER_INODE);

    let bitmap_capacity = block_size as u64 * 8;

    u32::try_from(desired.min(bitmap_capacity))
        .map_err(|_| FsError::Invalid("inodes_per_group overflow"))
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_ext4_meta_creation() {
        const SIZE_BYTES: u64 = 32 * 1024 * 1024; // 32 MB
        let meta = ExtMeta::new(SIZE_BYTES, Some("TESTEXT")).unwrap();

        assert_eq!(meta.volume_size_bytes, SIZE_BYTES, "Size mismatch");
        assert_eq!(
            meta.block_size, EXT_DEFAULT_BLOCK_SIZE,
            "Block size should be 4096"
        );
        assert_eq!(meta.label(), "TESTEXT", "Volume label mismatch");

        let expected_block_count = SIZE_BYTES / meta.block_size as u64;
        assert_eq!(
            meta.block_count, expected_block_count,
            "Block count mismatch"
        );

        #[cfg(feature = "std")]
        println!(
            "✓ EXT meta created: size={SIZE_BYTES}, block_count={}",
            meta.block_count
        );
    }

    #[test]
    fn test_ext4_group_count() {
        const SIZE_BYTES: u64 = 64 * 1024 * 1024; // 64 MB
        let meta = ExtMeta::new(SIZE_BYTES, Some("TEST")).unwrap();

        let expected_groups = meta.block_count.div_ceil(meta.blocks_per_group as u64) as u32;
        assert_eq!(meta.group_count, expected_groups, "Group count mismatch");

        #[cfg(feature = "std")]
        println!(
            "✓ EXT group count: {} groups for {} blocks",
            meta.group_count, meta.block_count
        );
    }

    #[test]
    fn test_ext4_sparse_super_groups() {
        use crate::utils::is_sparse_super_group;

        // Group 0 is always sparse super
        assert!(is_sparse_super_group(0), "Group 0 should be sparse super");
        // Group 1 is always sparse super
        assert!(is_sparse_super_group(1), "Group 1 should be sparse super");
        // Group 3 (3^1) is sparse super
        assert!(is_sparse_super_group(3), "Group 3 should be sparse super");
        // Group 5 (5^1) is sparse super
        assert!(is_sparse_super_group(5), "Group 5 should be sparse super");
        // Group 7 (7^1) is sparse super
        assert!(is_sparse_super_group(7), "Group 7 should be sparse super");
        // Group 9 (3^2) is sparse super
        assert!(is_sparse_super_group(9), "Group 9 should be sparse super");
        // Group 25 (5^2) is sparse super
        assert!(is_sparse_super_group(25), "Group 25 should be sparse super");
        // Group 49 (7^2) is sparse super
        assert!(is_sparse_super_group(49), "Group 49 should be sparse super");

        // Non-sparse groups
        assert!(
            !is_sparse_super_group(2),
            "Group 2 should NOT be sparse super"
        );
        assert!(
            !is_sparse_super_group(4),
            "Group 4 should NOT be sparse super"
        );
        assert!(
            !is_sparse_super_group(6),
            "Group 6 should NOT be sparse super"
        );
        assert!(
            !is_sparse_super_group(8),
            "Group 8 should NOT be sparse super"
        );
        assert!(
            !is_sparse_super_group(10),
            "Group 10 should NOT be sparse super"
        );

        #[cfg(feature = "std")]
        println!("✓ Sparse super group detection verified");
    }

    #[test]
    fn test_ext4_inode_allocation() {
        const SIZE_BYTES: u64 = 32 * 1024 * 1024;
        let meta = ExtMeta::new(SIZE_BYTES, Some("INODES")).unwrap();

        // Total inodes = group_count * inodes_per_group
        let expected_inodes = (meta.group_count as u64) * (meta.inodes_per_group as u64);
        assert_eq!(
            meta.inode_count, expected_inodes,
            "Total inode count mismatch"
        );

        assert!(meta.inodes_per_group > 0, "Inodes per group should be > 0");
        assert!(
            meta.inodes_per_group <= meta.blocks_per_group * 16,
            "Too many inodes per group"
        );

        #[cfg(feature = "std")]
        println!(
            "✓ EXT inode allocation: {} inodes, {} per group",
            meta.inode_count, meta.inodes_per_group
        );
    }

    #[test]
    fn test_ext4_block_count_boundary_uses_64bit_counts() {
        let max_supported = u32::MAX as u64 * EXT_DEFAULT_BLOCK_SIZE as u64;
        let meta = ExtMeta::new(max_supported, Some("MAXEXT")).unwrap();
        assert_eq!(meta.block_count, u32::MAX as u64);

        let over = ExtMeta::new(max_supported + EXT_DEFAULT_BLOCK_SIZE as u64, Some("EXT64"))
            .expect("EXT4 metadata should represent block counts above u32::MAX");
        assert_eq!(over.block_count, u32::MAX as u64 + 1);

        let sb = ExtSuperblock::from_meta(&over, 0, 0);
        let blocks_count_lo = sb.s_blocks_count_lo.get();
        let blocks_count_hi = sb.s_blocks_count_hi.get();
        assert_eq!(blocks_count_lo, 0);
        assert_eq!(blocks_count_hi, 1);
    }

    #[test]
    fn test_ext4_fs_meta_trait() {
        use crate::core::traits::FsMeta;

        const SIZE_BYTES: u64 = 32 * 1024 * 1024;
        let meta = ExtMeta::new(SIZE_BYTES, Some("FSMETA")).unwrap();

        assert_eq!(
            meta.unit_size(),
            meta.block_size as u64,
            "unit_size mismatch"
        );
        assert_eq!(
            meta.root_unit(),
            EXT_ROOT_INODE,
            "root_unit should be root inode"
        );
        assert_eq!(meta.total_units(), meta.block_count, "total_units mismatch");
        assert_eq!(meta.size_bytes(), SIZE_BYTES, "size_bytes mismatch");

        // Test unit_offset
        let offset = meta.unit_offset(0);
        assert_eq!(offset, 0, "Block 0 offset should be 0");

        let offset_1 = meta.unit_offset(1);
        assert_eq!(
            offset_1, meta.block_size as u64,
            "Block 1 offset should be block_size"
        );

        #[cfg(feature = "std")]
        println!("✓ FsMeta trait implementation verified");
    }
}
