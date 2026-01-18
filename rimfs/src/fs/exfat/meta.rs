// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::string::{String, ToString};

use rimio::errors::RimIOError;
use rimio::{RimIO, RimIOStructExt};
use zerocopy::FromBytes;

pub use crate::core::meta::*;

use crate::{
    core::{FsError, FsResult, cursor::ClusterMeta},
    fs::exfat::{constant::*, types::*, upcase::UpcaseFlavor},
};

#[derive(Debug, Clone, PartialEq)]
pub struct ExFatMeta {
    pub volume_id: u32,
    pub volume_guid: Option<[u8; 16]>,

    pub volume_label: [u16; 11],

    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u32,
    pub bytes_per_cluster: u32,

    pub volume_size_bytes: u64,
    pub volume_size_sectors: u64,

    pub num_fats: u8,
    pub fat_offset_bytes: u64,
    pub fat_size_sectors: u32,

    pub cluster_heap_offset_bytes: u64,
    pub cluster_count: u32,

    pub bitmap_cluster: u32,
    pub upcase_cluster: u32,
    pub root_cluster: u32,

    pub bitmap_size_bytes: u64,
    pub upcase_size_bytes: u64,
    pub upcase_checksum: u32,

    pub upcase_flavor: UpcaseFlavor,
}

impl ExFatMeta {
    pub fn new(size_bytes: u64, volume_label: Option<&str>) -> FsResult<Self> {
        // Use dynamic cluster size based on volume size
        let cluster_size = determine_cluster_size(size_bytes);

        Self::new_custom(
            size_bytes,
            volume_label,
            None,
            None,
            EXFAT_NUM_FATS,
            EXFAT_SECTOR_SIZE,
            cluster_size,
            UpcaseFlavor::Full,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_custom(
        volume_size_bytes: u64,
        volume_label: Option<&str>,
        volume_id: Option<u32>,
        volume_guid: Option<[u8; 16]>,
        num_fats: u8,
        bytes_per_sector: u16,
        bytes_per_cluster: u32,
        upcase_flavor: UpcaseFlavor,
    ) -> FsResult<Self> {
        let sectors_per_cluster = bytes_per_cluster
            .checked_div(bytes_per_sector as u32)
            .ok_or(FsError::Invalid(
                "cluster_size must be a multiple of sector_size",
            ))?;

        let mut volume_label_safe = [0u16; 11];
        if let Some(label) = volume_label {
            for (i, b) in label.encode_utf16().take(11).enumerate() {
                volume_label_safe[i] = b;
            }
        }

        let (guid, vol_id) = resolve_ids(
            volume_label.unwrap_or(""),
            volume_size_bytes,
            bytes_per_cluster,
            0,
            volume_id,
            volume_guid,
        );

        let volume_size_sectors = volume_size_bytes / bytes_per_sector as u64;

        let fat_offset_bytes = align_to_boundary(
            EXFAT_MIN_RESERVED_SECTORS as u64 * bytes_per_sector as u64,
            EXFAT_BOUNDARY_ALIGNMENT,
        );
        let fat_offset_sectors = fat_offset_bytes / bytes_per_sector as u64;

        let (fat_size_sectors, cluster_count, heap_offet_sectors) = converge_fat_layout_aligned(
            bytes_per_sector as u32,
            volume_size_sectors,
            fat_offset_sectors,
            EXFAT_ENTRY_SIZE as u32,
            EXFAT_FIRST_CLUSTER, // = 2
            EXFAT_NUM_FATS,      // = 1
            sectors_per_cluster,
            EXFAT_BOUNDARY_ALIGNMENT, // 1 MiB
        );

        let cluster_heap_offset_bytes = heap_offet_sectors * bytes_per_sector as u64;

        let bitmap_size_bytes = (cluster_count as u64).div_ceil(8);
        let bitmap_clusters = bitmap_size_bytes.div_ceil(bytes_per_cluster.into()) as u32;

        let (upcase_checksum, upcase_size_bytes): (u32, u64) = match upcase_flavor {
            UpcaseFlavor::Minimal => (
                EXFAT_UPCASE_MINIMAL_CHECKSUM,
                EXFAT_UPCASE_MINIMAL_LENGTH as u64,
            ),
            UpcaseFlavor::Full => (EXFAT_UPCASE_FULL_CHECKSUM, EXFAT_UPCASE_FULL_LENGTH as u64),
        };

        let upcase_clusters = upcase_size_bytes.div_ceil(bytes_per_cluster.into()) as u32;

        Ok(Self {
            volume_id: vol_id,
            volume_guid: Some(guid),
            volume_label: volume_label_safe,
            bytes_per_sector,
            sectors_per_cluster,
            bytes_per_cluster,
            volume_size_bytes,
            volume_size_sectors,
            num_fats,
            fat_offset_bytes,
            fat_size_sectors,
            cluster_heap_offset_bytes,
            cluster_count,
            bitmap_cluster: EXFAT_FIRST_CLUSTER,
            upcase_cluster: EXFAT_FIRST_CLUSTER + bitmap_clusters,
            root_cluster: EXFAT_FIRST_CLUSTER + bitmap_clusters + upcase_clusters,
            bitmap_size_bytes,
            upcase_size_bytes,
            upcase_checksum,
            upcase_flavor,
        })
    }

    pub fn from_io<IO: RimIO + ?Sized>(io: &mut IO) -> FsResult<Self> {
        let vbr: ExFatBootSector = io.read_struct(EXFAT_VBR_SECTOR)?;

        let bytes_per_sector = 1u32 << vbr.bytes_per_sector_shift;
        let sectors_per_cluster = 1u32 << vbr.sectors_per_cluster_shift;
        let bytes_per_cluster = bytes_per_sector * sectors_per_cluster;
        let fat_offset_bytes = vbr.fat_offset as u64 * bytes_per_sector as u64;
        let cluster_heap_offset = vbr.cluster_heap_offset as u64 * bytes_per_sector as u64;

        let root_cluster = vbr.root_dir_cluster;
        let mut found_bitmap: Option<ExFatBitmapEntry> = None;
        let mut found_upcase: Option<ExFatUpcaseEntry> = None;
        let mut found_label: Option<ExFatVolumeLabelEntry> = None;
        let mut found_guid: Option<ExFatGuidEntry> = None;

        let offset = cluster_heap_offset
            + ((root_cluster - EXFAT_FIRST_CLUSTER) as u64 * bytes_per_cluster as u64);
        let mut buf = vec![0u8; bytes_per_cluster as usize];
        io.read_at(offset, &mut buf)?;

        let entries = buf.chunks_exact(32);
        for entry in entries {
            let tag = entry[0];

            match tag {
                EXFAT_ENTRY_LABEL => {
                    found_label = Some(
                        ExFatVolumeLabelEntry::read_from_bytes(entry)
                            .map_err(|_| RimIOError::Other("volume_label_parse"))?,
                    );
                }
                EXFAT_ENTRY_BITMAP => {
                    found_bitmap = Some(
                        ExFatBitmapEntry::read_from_bytes(entry)
                            .map_err(|_| RimIOError::Other("bitmap_parse"))?,
                    );
                }
                EXFAT_ENTRY_UPCASE => {
                    found_upcase = Some(
                        ExFatUpcaseEntry::read_from_bytes(entry)
                            .map_err(|_| RimIOError::Other("upcase_parse"))?,
                    );
                }
                EXFAT_ENTRY_GUID => {
                    found_guid = Some(
                        ExFatGuidEntry::read_from_bytes(entry)
                            .map_err(|_| RimIOError::Other("guid_parse"))?,
                    );
                }
                EXFAT_EOD => break, // End of Directory
                _ => {}
            }
        }

        let volume_label = found_label.map(|f| f.volume_label).unwrap_or([0u16; 11]);

        let bitmap = found_bitmap.ok_or(RimIOError::Other("bitmap_cluster"))?;

        let upcase = found_upcase.ok_or(RimIOError::Other("upcase_cluster"))?;

        let guid = found_guid.map(|f| Some(f.guid)).unwrap_or(None);

        Ok(Self {
            volume_id: vbr.volume_serial,
            volume_guid: guid,
            volume_label,
            bytes_per_sector: bytes_per_sector as u16,
            sectors_per_cluster,
            bytes_per_cluster,
            volume_size_bytes: vbr.volume_length * (bytes_per_sector as u64),
            volume_size_sectors: vbr.volume_length,
            num_fats: vbr.number_of_fats,
            fat_offset_bytes,
            fat_size_sectors: vbr.fat_length,
            cluster_heap_offset_bytes: cluster_heap_offset,
            cluster_count: vbr.cluster_count,
            bitmap_cluster: bitmap.first_cluster,
            upcase_cluster: upcase.first_cluster,
            root_cluster,
            bitmap_size_bytes: bitmap.data_length,
            upcase_size_bytes: upcase.data_length,
            upcase_checksum: upcase.table_checksum,
            upcase_flavor: UpcaseFlavor::Full,
        })
    }

    pub fn bitmap_entry_offset(&self, cluster: u32) -> (usize, u8) {
        let bit = (cluster - EXFAT_FIRST_CLUSTER) as usize;
        let byte_index = bit / 8;
        let bit_mask = 1u8 << (bit % 8);
        (byte_index, bit_mask)
    }

    #[inline]
    pub fn bitmap_clusters(&self) -> u32 {
        let cs = self.bytes_per_cluster as u64;
        self.bitmap_size_bytes.div_ceil(cs) as u32
    }

    #[inline]
    pub fn upcase_clusters(&self) -> u32 {
        let cs = self.bytes_per_cluster as u64;
        self.upcase_size_bytes.div_ceil(cs) as u32
    }

    #[inline]
    pub fn root_clusters(&self) -> u32 {
        1
    }

    #[inline]
    pub fn system_used_clusters(&self) -> u32 {
        self.bitmap_clusters() + self.upcase_clusters() + self.root_clusters()
    }

    pub fn percent_in_use(&self) -> u8 {
        if self.cluster_count == 0 {
            return 0;
        }
        let p = (self.system_used_clusters() as u64 * 100) / (self.cluster_count as u64);
        p.min(100) as u8
    }
}

impl FsMeta<u32> for ExFatMeta {
    fn unit_size(&self) -> usize {
        self.bytes_per_cluster as usize
    }

    fn root_unit(&self) -> u32 {
        self.root_cluster
    }

    fn total_units(&self) -> usize {
        self.cluster_count as usize
    }

    fn size_bytes(&self) -> u64 {
        self.volume_size_bytes
    }

    fn label(&self) -> String {
        String::from_utf16_lossy(&self.volume_label)
            .trim_matches(char::from(0))
            .to_string()
    }

    fn unit_offset(&self, cluster: u32) -> u64 {
        self.cluster_heap_offset_bytes
            + ((cluster - EXFAT_FIRST_CLUSTER) as u64 * self.unit_size() as u64)
    }

    fn first_data_unit(&self) -> u32 {
        let end_bm = self.bitmap_cluster + self.bitmap_clusters(); // [start .. end)
        let end_uc = self.upcase_cluster + self.upcase_clusters();
        let end_rt = self.root_cluster + self.root_clusters();
        end_bm.max(end_uc).max(end_rt)
    }

    fn last_data_unit(&self) -> u32 {
        EXFAT_FIRST_CLUSTER + self.cluster_count - 1
    }
}

/// ClusterMeta implementation for ExFatMeta
impl ClusterMeta for ExFatMeta {
    const EOC: u32 = EXFAT_EOC;
    const FIRST_CLUSTER: u32 = EXFAT_FIRST_CLUSTER;
    const ENTRY_SIZE: usize = EXFAT_ENTRY_SIZE;
    const ENTRY_MASK: u32 = EXFAT_MASK; // ExFAT uses all 32 bits

    fn fat_entry_offset(&self, cluster: u32, fat_index: u8) -> u64 {
        self.fat_offset_bytes
            + fat_index as u64 * self.fat_size_sectors as u64 * self.bytes_per_sector as u64
            + cluster as u64 * EXFAT_ENTRY_SIZE as u64
    }

    fn num_fats(&self) -> u8 {
        self.num_fats
    }
}

/// exFAT convergence with 1MiB heap alignment taken into account.
/// - `sector_size`           : bytes/sector (e.g., 512)
/// - `total_sectors`         : volume size in sectors
/// - `fat_offset_sectors`    : FAT offset in sectors (can be pre-aligned to 1MiB)
/// - `entry_size`            : FAT entry size (exFAT = 4)
/// - `min_entries`           : reserved entries (exFAT = 2)
/// - `num_fats`              : number of FATs (exFAT = 1, except TexFAT)
/// - `sectors_per_cluster`   : SPC (2^n)
/// - `heap_align_bytes`      : heap alignment (e.g., 1_048_576)
#[allow(clippy::too_many_arguments)]
pub fn converge_fat_layout_aligned(
    sector_size: u32,
    total_sectors: u64,
    fat_offset_sectors: u64,
    entry_size: u32,
    min_entries: u32,
    num_fats: u8,
    sectors_per_cluster: u32,
    heap_align_bytes: u32,
) -> (u32, u32, u64) {
    assert!(sector_size != 0 && sectors_per_cluster != 0);

    let ss = sector_size as u64;
    let spc = sectors_per_cluster as u64;
    let fat_off_bytes = fat_offset_sectors * ss;
    let align = heap_align_bytes as u64;

    let mut cluster_count: u32 = 0;
    let mut fat_size_sectors: u32 = 0;
    let mut heap_off_aligned_bytes: u64 = 0;

    // Safety bound: convergence typically reached in a few iterations
    for _ in 0..32 {
        // FAT size (in sectors) for entries = clusters + reserved entries
        let entries = (cluster_count as u64) + (min_entries as u64);
        let fat_size_sectors_new = (entries * (entry_size as u64)).div_ceil(ss) as u32;

        // Heap offset: unaligned, then aligned (in bytes)
        let heap_off_unaligned =
            fat_off_bytes + (fat_size_sectors_new as u64) * (num_fats as u64) * ss;
        let heap_off_aligned_new = heap_off_unaligned.div_ceil(align) * align;

        // Actual data sectors after alignment
        let data_sectors = total_sectors.saturating_sub(heap_off_aligned_new / ss);

        // New cluster_count
        let cluster_count_new = (data_sectors / spc) as u32;

        // Reached stability?
        if cluster_count_new == cluster_count
            && fat_size_sectors_new == fat_size_sectors
            && heap_off_aligned_new == heap_off_aligned_bytes
        {
            break;
        }

        cluster_count = cluster_count_new;
        fat_size_sectors = fat_size_sectors_new;
        heap_off_aligned_bytes = heap_off_aligned_new;
    }

    let heap_offset_sectors_aligned = heap_off_aligned_bytes / ss;
    (fat_size_sectors, cluster_count, heap_offset_sectors_aligned)
}

/// Determines the optimal cluster size based on volume size
fn determine_cluster_size(size_bytes: u64) -> u32 {
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;

    const FIRST_BOUND: u64 = 256 * MB; // 256MB
    const SECOND_BOUND: u64 = 32 * GB; // 32GB

    match size_bytes {
        ..=FIRST_BOUND => 4 * 1024,          // ≤256MB → 4KB clusters
        n if n <= SECOND_BOUND => 32 * 1024, // 256MB-32GB → 32KB clusters
        _ => 128 * 1024,                     // >32GB → 128KB clusters
    }
}

/// Calculate aligned offset
fn align_to_boundary(offset: u64, boundary: u32) -> u64 {
    let boundary = boundary as u64;
    offset.div_ceil(boundary) * boundary
}

fn resolve_ids(
    label: &str,
    size_bytes: u64,
    cluster_size: u32,
    user_salt: u32,
    volume_id: Option<u32>,
    volume_guid: Option<[u8; 16]>,
) -> ([u8; 16], u32) {
    match (volume_id, volume_guid) {
        (Some(vid), None) => {
            let guid = guid_from_volume_id(vid);
            (guid, vid)
        }
        (None, Some(guid)) => {
            let vid = volume_id_from_guid(&guid);
            (guid, vid)
        }
        (Some(vid), Some(guid)) => {
            // Trust provided values (optional: verify consistency)
            (guid, vid)
        }
        (None, None) => derive_ids(label, size_bytes, cluster_size, user_salt),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dynamic_cluster_size() {
        // Test small volume (8MB) → should get 4KB clusters
        let small_meta = ExFatMeta::new(8 * 1024 * 1024, Some("SMALL")).unwrap();
        assert_eq!(
            small_meta.bytes_per_cluster,
            4 * 1024,
            "Small volume should use 4KB clusters"
        );
        assert_eq!(
            small_meta.sectors_per_cluster, 8,
            "4KB clusters = 8 sectors @ 512 bytes"
        );

        // Test medium volume (512MB) → should get 32KB clusters
        let medium_meta = ExFatMeta::new(512 * 1024 * 1024, Some("MEDIUM")).unwrap();
        assert_eq!(
            medium_meta.bytes_per_cluster,
            32 * 1024,
            "Medium volume should use 32KB clusters"
        );
        assert_eq!(
            medium_meta.sectors_per_cluster, 64,
            "32KB clusters = 64 sectors @ 512 bytes"
        );

        // Test large volume (64GB) → should get 128KB clusters
        let large_meta = ExFatMeta::new(64 * 1024 * 1024 * 1024, Some("LARGE")).unwrap();
        assert_eq!(
            large_meta.bytes_per_cluster,
            128 * 1024,
            "Large volume should use 128KB clusters"
        );
        assert_eq!(
            large_meta.sectors_per_cluster, 256,
            "128KB clusters = 256 sectors @ 512 bytes"
        );
        println!(
            "✓ Small volume (8MB): cluster_size={}, sectors_per_cluster={}",
            small_meta.bytes_per_cluster, small_meta.sectors_per_cluster,
        );
        println!(
            "✓ Medium volume (512MB): cluster_size={}, sectors_per_cluster={}",
            medium_meta.bytes_per_cluster, medium_meta.sectors_per_cluster,
        );
        println!(
            "✓ Large volume (64GB): cluster_size={}, sectors_per_cluster={}",
            large_meta.bytes_per_cluster, large_meta.sectors_per_cluster,
        );
    }

    #[test]
    fn test_fat_alignment() {
        // Test that FAT offset is aligned to 1MB boundary
        let meta = ExFatMeta::new(256 * 1024 * 1024, Some("ALIGNED")).unwrap();

        // FAT offset should be aligned to 1MB (1048576 bytes)
        assert_eq!(
            meta.fat_offset_bytes % (1024 * 1024),
            0,
            "FAT offset should be 1MB aligned"
        );

        // Cluster heap should also be aligned
        assert_eq!(
            meta.cluster_heap_offset_bytes % (1024 * 1024),
            0,
            "Cluster heap should be 1MB aligned"
        );

        println!(
            "✓ FAT offset: {} (aligned to {}MB)",
            meta.fat_offset_bytes,
            meta.fat_offset_bytes / (1024 * 1024)
        );
        println!(
            "✓ Cluster heap offset: {} (aligned to {}MB)",
            meta.cluster_heap_offset_bytes,
            meta.cluster_heap_offset_bytes / (1024 * 1024)
        );
    }

    #[test]
    fn test_bitmap_size_calculation() {
        let meta = ExFatMeta::new(8 * 1024 * 1024, Some("BITMAPTEST")).unwrap();

        let expected_size = meta.cluster_count.div_ceil(8) as u64;
        assert_eq!(meta.bitmap_size_bytes, expected_size);

        assert_eq!(((1 + 7) / 8), 1); // 1 cluster
        assert_eq!(((8 + 7) / 8), 1); // 8 clusters
        assert_eq!(((9 + 7) / 8), 2); // 9 clusters
        assert_eq!(((16 + 7) / 8), 2); // 16 clusters
        assert_eq!(((17 + 7) / 8), 3); // 17 clusters

        println!(
            "Cluster count: {}, Bitmap size: {} bytes",
            meta.cluster_count, meta.bitmap_size_bytes
        );
    }
}
