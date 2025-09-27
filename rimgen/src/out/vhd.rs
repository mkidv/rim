// SPDX-License-Identifier: MIT

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use time::OffsetDateTime;

use crate::layout::Layout;
use crate::out::img;
use crate::out::target::DryRunMode;

use zerocopy::byteorder::{BigEndian, U32, U64};
use zerocopy::{AsBytes, FromBytes, Immutable, KnownLayout};

#[repr(C)]
#[derive(AsBytes, FromBytes, KnownLayout, Immutable, Clone, Copy)]
pub struct VhdFooter {
    pub cookie: [u8; 8],                 // "conectix"
    pub features: U32<BigEndian>,        // 0x0000_0002
    pub file_format_ver: U32<BigEndian>, // 0x0001_0000
    pub data_offset: U64<BigEndian>,     // 0xFFFF_FFFF_FFFF_FFFF
    pub timestamp: U32<BigEndian>,       // seconds since 2000-01-01
    pub creator_app: [u8; 4],            // e.g. b"rim\0"
    pub creator_ver: U32<BigEndian>,     // e.g. 0x000A_0000
    pub creator_os: [u8; 4],             // e.g. b"Wi2k"
    pub orig_size: U64<BigEndian>,       // disk bytes (padded to 512 before footer)
    pub curr_size: U64<BigEndian>,       // same as orig_size for fixed
    pub geometry_cyls: [u8; 2],          // CHS (cylinders BE)
    pub geometry_heads: u8,              // heads
    pub geometry_sects: u8,              // sectors/track
    pub disk_type: U32<BigEndian>,       // 2 = fixed
    pub checksum: U32<BigEndian>,        // ones' complement of sum of all bytes w/ checksum=0
    pub unique_id: [u8; 16],             // UUID
    pub saved_state: u8,                 // 0
    pub reserved: [u8; 427],             // zero
}

impl VhdFooter {
    pub fn new_fixed(disk_size: u64, seconds_since_2000: u32, unique_id: [u8; 16]) -> Self {
        // CHS heuristique classique (spec) : 16 têtes, 63 secteurs/track
        let heads = 16u8;
        let spt = 63u8;
        let total_sectors = disk_size / 512;
        let cyls = (total_sectors / (heads as u64 * spt as u64)) as u16;

        let mut f = VhdFooter {
            cookie: *b"conectix",
            features: U32::new(0x0000_0002),
            file_format_ver: U32::new(0x0001_0000),
            data_offset: U64::new(0xFFFF_FFFF_FFFF_FFFF),
            timestamp: U32::new(seconds_since_2000),
            creator_app: *b"rim\0",
            creator_ver: U32::new(0x000A_0000),
            creator_os: *b"Wi2k",
            orig_size: U64::new(disk_size),
            curr_size: U64::new(disk_size),
            geometry_cyls: cyls.to_be_bytes(),
            geometry_heads: heads,
            geometry_sects: spt,
            disk_type: U32::new(2),
            checksum: U32::new(0), // mis à zéro avant calcul
            unique_id,
            saved_state: 0,
            reserved: [0u8; 427],
        };
        // injecte le checksum
        let sum = f.compute_checksum();
        f.checksum = U32::new(sum);
        f
    }

    /// Calcule le ones' complement de la somme des 512 octets avec checksum=0.
    pub fn compute_checksum(&self) -> u32 {
        let mut tmp = *self;
        tmp.checksum = U32::new(0);
        let bytes: &[u8; 512] = tmp.as_bytes().try_into().unwrap();
        let mut sum: u32 = 0;
        for &b in bytes.iter() {
            sum = sum.wrapping_add(b as u32);
        }
        !sum
    }

    /// Vérifie le cookie, la taille et le checksum.
    pub fn validate(&self) -> bool {
        if &self.cookie != b"conectix" {
            return false;
        }
        // checksum
        let mut tmp = *self;
        let expected = tmp.checksum.get();
        tmp.checksum = U32::new(0);
        let bytes: &[u8; 512] = tmp.as_bytes().try_into().unwrap();
        let mut sum: u32 = 0;
        for &b in bytes.iter() {
            sum = sum.wrapping_add(b as u32);
        }
        (!sum) == expected
    }
}

const VHD_FOOTER_SIZE: u64 = 512;

pub fn create(
    layout: &Layout,
    output: &Path,
    truncate: &bool,
    dry_mode: DryRunMode,
) -> anyhow::Result<()> {
    crate::log_verbose!("Create temp img.");
    let temp_root = tempfile::tempdir()?;
    let temp_path = temp_root.path().join("rim_temp.img");
    img::create(layout, &temp_path, truncate, dry_mode)?;
    if matches!(dry_mode, DryRunMode::Off) {
        crate::log_verbose!("Wrapping img to vhd.");
        wrap_raw_as_vhd_to(&temp_path, output)?;
        return Ok(());
    }
    crate::log_verbose!("Dry-run - Wrapping img to vhd.");
    Ok(())
}

/// Strip VHD footer and restore .img
pub fn unwrap_vhd_to_raw(vhd_path: &Path, img_path: &Path) -> anyhow::Result<()> {
    let mut file = File::open(vhd_path)?;
    let len = file.metadata()?.len();
    let raw_len = len - VHD_FOOTER_SIZE;
    let mut buffer = vec![0u8; raw_len as usize];
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut buffer)?;
    fs::write(img_path, &buffer)?;
    Ok(())
}

/// Wrap .img as a temporary .vhd (adds 512B VHD footer)
pub fn wrap_raw_as_vhd_to(img_path: &Path, vhd_path: &Path) -> anyhow::Result<()> {
    let mut reader = BufReader::new(File::open(img_path)?);
    let mut writer = BufWriter::new(File::create(vhd_path)?);

    let mut buffer = vec![0u8; 8192];
    let mut total_size = 0u64;
    loop {
        let read_bytes = reader.read(&mut buffer)?;
        if read_bytes == 0 {
            break;
        }
        writer.write_all(&buffer[..read_bytes])?;
        total_size += read_bytes as u64;
    }

    let remainder = total_size % VHD_FOOTER_SIZE;
    if remainder != 0 {
        let pad = vec![0u8; (VHD_FOOTER_SIZE - remainder) as usize];
        writer.write_all(&pad)?;
        total_size += VHD_FOOTER_SIZE - remainder;
    }

    let footer = generate_vhd_footer(total_size)?;
    writer.write_all(&footer)?;
    writer.flush()?;
    drop(writer);
    Ok(())
}

/// Generates a fixed VHD footer (512 bytes, spec compliant)
pub fn generate_vhd_footer(disk_size: u64) -> anyhow::Result<Vec<u8>> {
    let mut footer = vec![0u8; 512];

    footer[0..8].copy_from_slice(b"conectix"); // Cookie
    footer[8..12].copy_from_slice(&(2u32).to_be_bytes()); // Features
    footer[12..16].copy_from_slice(&(0x00010000u32).to_be_bytes()); // Version
    footer[16..24].copy_from_slice(&(0xFFFFFFFFFFFFFFFFu64).to_be_bytes()); // Data offset

    let epoch_2000 = OffsetDateTime::from_unix_timestamp(946684800).unwrap();
    let now = OffsetDateTime::now_utc();
    let seconds_since_2000 = (now - epoch_2000).whole_seconds() as u32;

    footer[24..28].copy_from_slice(&seconds_since_2000.to_be_bytes());

    footer[28..32].copy_from_slice(b"rim\0");
    footer[32..36].copy_from_slice(&(0x000A0000u32).to_be_bytes()); // Creator ver
    footer[36..40].copy_from_slice(b"Wi2k");

    footer[40..48].copy_from_slice(&disk_size.to_be_bytes()); // Orig size
    footer[48..56].copy_from_slice(&disk_size.to_be_bytes()); // Current size

    let total_sectors = disk_size / 512;
    let sectors_per_track = 63;
    let heads = 16;
    let cylinders = (total_sectors / (heads * sectors_per_track)) as u16;
    footer[56..58].copy_from_slice(&cylinders.to_be_bytes());
    footer[58] = heads as u8;
    footer[59] = sectors_per_track as u8;

    footer[60..64].copy_from_slice(&(2u32).to_be_bytes()); // Disk type: fixed
    footer[64..68].copy_from_slice(&[0u8; 4]); // Checksum placeholder

    let guid = uuid::Uuid::new_v4();
    footer[68..84].copy_from_slice(guid.as_bytes());
    footer[84] = 0;

    let mut temp = footer.clone();
    temp[64..68].copy_from_slice(&[0, 0, 0, 0]);

    let checksum: u32 = !temp.iter().fold(0u32, |acc, &b| acc.wrapping_add(b as u32));
    footer[64..68].copy_from_slice(&checksum.to_be_bytes());

    footer[510] = 0x00;
    footer[511] = 0x00;

    Ok(footer)
}
