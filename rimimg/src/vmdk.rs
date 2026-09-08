// SPDX-License-Identifier: MIT

//! VMDK (VMware Virtual Machine Disk) monolithic flat format support.
//!
//! Implements monolithicFlat format: text descriptor at the start (padded to 512B),
//! followed by raw disk data.

#[cfg(feature = "alloc")]
use alloc::format;
#[cfg(feature = "alloc")]
use alloc::string::ToString;
#[cfg(feature = "alloc")]
use alloc::vec::Vec;
use rimio::prelude::*;

use crate::errors::{RimImgError, RimImgResult};
#[cfg(feature = "alloc")]
use crate::options::ImageOptions;

#[cfg(feature = "alloc")]
const DESCRIPTOR_TEMPLATE: &str = r#"# Disk DescriptorFile
version=1
encoding="UTF-8"
CID={cid}
parentCID=ffffffff
createType="monolithicFlat"

# Extent description
RW {sectors} FLAT "{filename}" {offset}

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

#[cfg(feature = "alloc")]
fn generate_descriptor(disk_size: u64, filename: &str, cid: u32) -> Vec<u8> {
    let sectors = disk_size / SECTOR_SIZE;
    let cylinders = sectors / (16 * 63);
    let cid = format!("{cid:08x}");

    let descriptor = DESCRIPTOR_TEMPLATE
        .replace("{cid}", &cid)
        .replace("{sectors}", &sectors.to_string())
        .replace("{filename}", filename)
        .replace("{offset}", &DESCRIPTOR_SECTORS.to_string())
        .replace("{cylinders}", &cylinders.to_string());

    let mut bytes = descriptor.into_bytes();
    let target_size = (DESCRIPTOR_SECTORS * SECTOR_SIZE) as usize;
    bytes.resize(target_size, 0);
    bytes
}

#[cfg(feature = "alloc")]
pub fn wrap_raw_as_vmdk_io_with_progress<F: FnMut(u64, u64)>(
    src: &mut dyn RimRead,
    dst: &mut dyn RimIO,
    img_len: u64,
    options: ImageOptions,
    on_progress: F,
) -> RimImgResult {
    init_vmdk_io(dst, img_len, options)?;
    let data_offset = DESCRIPTOR_SECTORS * SECTOR_SIZE;
    dst.copy_from_with_progress(src, 0, data_offset, img_len, on_progress)?;
    dst.flush()?;

    Ok(())
}

#[cfg(feature = "alloc")]
pub fn init_vmdk_io(dst: &mut dyn RimIO, img_len: u64, options: ImageOptions) -> RimImgResult {
    let descriptor = generate_descriptor(img_len, "disk.vmdk", options.vmdk_cid);
    dst.write_at(0, &descriptor)?;

    Ok(())
}

pub fn unwrap_vmdk_io_with_progress<F: FnMut(u64, u64)>(
    src: &mut dyn RimIO,
    dst: &mut dyn RimWrite,
    on_progress: F,
) -> RimImgResult {
    let vmdk_len = src.total_size()?;
    let data_offset = DESCRIPTOR_SECTORS * SECTOR_SIZE;

    if vmdk_len < data_offset {
        return Err(RimImgError::InvalidHeader(
            "VMDK file too small (header truncated)",
        ));
    }
    let mut header = [0u8; 22];
    src.read_at(0, &mut header)?;
    if !header.starts_with(b"# Disk DescriptorFile") {
        return Err(RimImgError::InvalidHeader("Invalid VMDK descriptor"));
    }
    let raw_len = vmdk_len - data_offset;

    dst.copy_from_with_progress(src, data_offset, 0, raw_len, on_progress)?;
    dst.flush()?;

    Ok(())
}

pub fn unwrap_vmdk_io(src: &mut dyn RimIO, dst: &mut dyn RimWrite) -> RimImgResult {
    unwrap_vmdk_io_with_progress(src, dst, |_, _| {})
}
