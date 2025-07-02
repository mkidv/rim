// rimgen/platform/windows/vhd.rs

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use time::OffsetDateTime;

use rimgen_layout::Layout;
use crate::img;

const VHD_FOOTER_SIZE: u64 = 512;

pub fn create(layout: &Layout, output: &Path, truncate: &bool) -> anyhow::Result<()> {
    let temp_root = tempfile::tempdir()?;
    let temp_path = temp_root.path().join("temp.img");
    img::create(layout, &temp_path, truncate)?;
    wrap_raw_as_vhd_to(&temp_path, output)?;
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
