// SPDX-License-Identifier: MIT

//! VMDK (VMware Virtual Machine Disk) monolithic flat format support.
//!
//! Implements monolithicFlat format: text descriptor at the start (padded to 512B),
//! followed by raw disk data.

use std::fs::File;
use std::path::Path;

use anyhow::Context;
use rimio::prelude::*;

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

pub const DESCRIPTOR_SECTORS: u64 = 1;
pub const SECTOR_SIZE: u64 = 512;

fn generate_descriptor(disk_size: u64, filename: &str) -> Vec<u8> {
    let sectors = disk_size / SECTOR_SIZE;
    let cylinders = sectors / (16 * 63);
    let cid = format!("{:08x}", rand_cid());

    let descriptor = DESCRIPTOR_TEMPLATE
        .replace("{cid}", &cid)
        .replace("{sectors}", &sectors.to_string())
        .replace("{filename}", filename)
        .replace("{cylinders}", &cylinders.to_string());

    let mut bytes = descriptor.into_bytes();
    let target_size = (DESCRIPTOR_SECTORS * SECTOR_SIZE) as usize;
    bytes.resize(target_size, 0);
    bytes
}

fn rand_cid() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    (duration.as_nanos() & 0xFFFF_FFFF) as u32
}

/// Wrap a raw .img file as a VMDK (monolithic flat) with progress callback.
pub fn wrap_raw_as_vmdk_with_progress<P1: AsRef<Path>, P2: AsRef<Path>, F: FnMut(u64, u64)>(
    img_path: P1,
    vmdk_path: P2,
    on_progress: F,
) -> anyhow::Result<()> {
    let mut img_file = File::open(img_path.as_ref())
        .with_context(|| format!("Failed to open input image {}", img_path.as_ref().display()))?;
    let img_len = img_file.metadata()?.len();

    let mut img_io = StdRimIO::new(&mut img_file);

    let mut vmdk_file = File::create(vmdk_path.as_ref()).with_context(|| {
        format!(
            "Failed to create VMDK file {}",
            vmdk_path.as_ref().display()
        )
    })?;
    let mut vmdk_io = StdRimIO::new(&mut vmdk_file);

    let filename = vmdk_path
        .as_ref()
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("disk.vmdk");

    let descriptor = generate_descriptor(img_len, filename);
    vmdk_io.write_at(0, &descriptor)?;

    let data_offset = DESCRIPTOR_SECTORS * SECTOR_SIZE;
    vmdk_io.copy_from_with_progress(&mut img_io, 0, data_offset, img_len, on_progress)?;
    vmdk_io.flush()?;

    Ok(())
}

pub fn wrap_raw_as_vmdk_to<P1: AsRef<Path>, P2: AsRef<Path>>(
    img_path: P1,
    vmdk_path: P2,
) -> anyhow::Result<()> {
    wrap_raw_as_vmdk_with_progress(img_path, vmdk_path, |_, _| {})
}

/// Strip VMDK descriptor and restore raw .img with progress callback.
pub fn unwrap_vmdk_with_progress<P1: AsRef<Path>, P2: AsRef<Path>, F: FnMut(u64, u64)>(
    vmdk_path: P1,
    img_path: P2,
    on_progress: F,
) -> anyhow::Result<()> {
    let mut vmdk_file = File::open(vmdk_path.as_ref())
        .with_context(|| format!("Failed to open VMDK file {}", vmdk_path.as_ref().display()))?;
    let vmdk_len = vmdk_file.metadata()?.len();
    let data_offset = DESCRIPTOR_SECTORS * SECTOR_SIZE;

    if vmdk_len < data_offset {
        anyhow::bail!("VMDK file too small (header truncated)");
    }
    let raw_len = vmdk_len - data_offset;

    let mut vmdk_io = StdRimIO::new(&mut vmdk_file);

    let mut img_file = File::create(img_path.as_ref())
        .with_context(|| format!("Failed to create raw image {}", img_path.as_ref().display()))?;
    let mut img_io = StdRimIO::new(&mut img_file);

    img_io.copy_from_with_progress(&mut vmdk_io, data_offset, 0, raw_len, on_progress)?;
    img_io.flush()?;

    Ok(())
}

pub fn unwrap_vmdk_to_raw<P1: AsRef<Path>, P2: AsRef<Path>>(
    vmdk_path: P1,
    img_path: P2,
) -> anyhow::Result<()> {
    unwrap_vmdk_with_progress(vmdk_path, img_path, |_, _| {})
}
