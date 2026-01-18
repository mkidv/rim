// SPDX-License-Identifier: MIT

//! VMDK (VMware Virtual Machine Disk) monolithic flat format support.
//!
//! This implements the simplest VMDK variant: a text descriptor embedded
//! at the start of the file, followed by raw disk data.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

use crate::layout::Layout;
use crate::out::img;
use crate::out::target::DryRunMode;

/// VMDK descriptor template for monolithic flat format.
/// The descriptor is padded to 512 bytes (1 sector).
const DESCRIPTOR_TEMPLATE: &str = r#"# Disk DescriptorFile
version=1
encoding="UTF-8"
CID={cid}
parentCID=ffffffff
createType="monolithicFlat"

# Extent description
RW {sectors} FLAT "{filename}" 0

# The Disk Data Base
#DDB

ddb.virtualHWVersion = "4"
ddb.geometry.cylinders = "{cylinders}"
ddb.geometry.heads = "16"
ddb.geometry.sectors = "63"
ddb.adapterType = "ide"
"#;

/// Size of the descriptor sector (must be padded to this)
const DESCRIPTOR_SECTORS: u64 = 1;
const SECTOR_SIZE: u64 = 512;

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
        crate::log_verbose!("Wrapping img to vmdk.");
        wrap_raw_as_vmdk_to(&temp_path, output)?;
        return Ok(());
    }
    crate::log_verbose!("Dry-run - Wrapping img to vmdk.");
    Ok(())
}

/// Generate a VMDK descriptor for the given disk size
fn generate_descriptor(disk_size: u64, filename: &str) -> Vec<u8> {
    let sectors = disk_size / SECTOR_SIZE;
    let cylinders = sectors / (16 * 63);
    let cid = format!("{:08x}", rand_cid());

    let descriptor = DESCRIPTOR_TEMPLATE
        .replace("{cid}", &cid)
        .replace("{sectors}", &sectors.to_string())
        .replace("{filename}", filename)
        .replace("{cylinders}", &cylinders.to_string());

    // Pad to sector boundary
    let mut bytes = descriptor.into_bytes();
    let target_size = (DESCRIPTOR_SECTORS * SECTOR_SIZE) as usize;
    bytes.resize(target_size, 0);
    bytes
}

/// Generate a random CID (content identifier)
fn rand_cid() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    (duration.as_nanos() & 0xFFFF_FFFF) as u32
}

/// Wrap a raw .img file as a VMDK (monolithic flat)
pub fn wrap_raw_as_vmdk_to(img_path: &Path, vmdk_path: &Path) -> anyhow::Result<()> {
    let mut reader = BufReader::new(File::open(img_path)?);
    let mut writer = BufWriter::new(File::create(vmdk_path)?);

    // Get file size
    let img_size = std::fs::metadata(img_path)?.len();

    // For monolithic flat, the filename in descriptor points to the same file
    // We use offset 0 since data follows immediately (no embedded descriptor in this variant)
    let filename = vmdk_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("disk.vmdk");

    // Write descriptor
    let descriptor = generate_descriptor(img_size, filename);
    writer.write_all(&descriptor)?;

    crate::utils::progress::copy_with_progress(
        &mut reader,
        &mut writer,
        img_size,
        "Converting to VMDK",
    )?;

    writer.flush()?;
    Ok(())
}

/// Strip VMDK descriptor and restore raw .img
#[allow(dead_code)]
pub fn unwrap_vmdk_to_raw(vmdk_path: &Path, img_path: &Path) -> anyhow::Result<()> {
    let mut file = File::open(vmdk_path)?;
    let len = file.metadata()?.len();

    // Skip descriptor sector(s)
    let data_offset = DESCRIPTOR_SECTORS * SECTOR_SIZE;
    let raw_len = len - data_offset;

    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(data_offset))?;

    let mut buffer = vec![0u8; raw_len as usize];
    file.read_exact(&mut buffer)?;
    std::fs::write(img_path, &buffer)?;
    Ok(())
}
