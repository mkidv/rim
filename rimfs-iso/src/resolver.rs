// SPDX-License-Identifier: MIT

//! ISO 9660, Joliet, and Rock Ridge directory resolver.

use crate::records::{IsoBootSectionHeader, IsoBootValidationEntry, IsoCatalogBootEntry};
use rimio::RimReadStructExt;
use zerocopy::FromBytes;

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use crate::meta::IsoMeta;
use crate::types::*;
use rimfs_core::errors::{FsResolverError, FsResolverResult};
use rimfs_core::normalize_fs_path;
use rimfs_core::resolver::{FsTreeResolver, PathIndex, attr::FileAttributes, attr::NodeKind};
use rimio::RimRead;
use rimio::prelude::*;
use time::OffsetDateTime;

/// Parsed entry from an ISO 9660 image.
#[derive(Debug, Clone)]
pub struct IsoResolvedEntry {
    pub name: String,
    pub lba: u32,
    pub size: u64,
    pub is_dir: bool,
    pub is_hidden: bool,
    pub is_symlink: bool,
    pub symlink_target: Option<String>,
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub modified: Option<OffsetDateTime>,
}

/// Describes an El Torito boot catalog entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElToritoBootEntry {
    /// 0x00 = x86/BIOS, 0x01 = PowerPC, 0x02 = Mac, 0xEF = EFI.
    pub platform_id: u8,
    /// Starting LBA of the boot image (in 2048-byte ISO sectors).
    pub lba: u32,
    /// Number of virtual 512-byte sectors (or 0 if dynamic/unspecified).
    pub sector_count_512: u16,
    /// Size in bytes.
    pub size_bytes: u64,
}

/// Filesystem resolver for ISO 9660 images (supporting Joliet and Rock Ridge extensions).
pub struct IsoResolver<'a, IO: RimRead + ?Sized> {
    io: &'a mut IO,
    _meta: &'a IsoMeta,
    index: PathIndex<IsoResolvedEntry>,
    pub is_joliet: bool,
    status: FsResolverResult<()>,
}

impl<'a, IO: RimRead + ?Sized> IsoResolver<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a IsoMeta) -> Self {
        let mut resolver = Self {
            io,
            _meta: meta,
            index: PathIndex::new(),
            is_joliet: false,
            status: Ok(()),
        };
        resolver.status = resolver.load_tree();
        resolver
    }

    pub fn try_new(io: &'a mut IO, meta: &'a IsoMeta) -> FsResolverResult<Self> {
        let mut resolver = Self {
            io,
            _meta: meta,
            index: PathIndex::new(),
            is_joliet: false,
            status: Ok(()),
        };
        resolver.load_tree()?;
        Ok(resolver)
    }

    /// Locates the EFI boot entry in the El Torito Boot Catalog if present.
    pub fn el_torito_efi_boot_entry(&mut self) -> FsResolverResult<Option<ElToritoBootEntry>> {
        let mut vd = [0u8; ISO_SECTOR_SIZE];
        let mut lba: u64 = 16;
        let mut catalog_lba = None;

        loop {
            if self
                .io
                .read_at(lba * ISO_SECTOR_SIZE as u64, &mut vd)
                .is_err()
            {
                break;
            }
            if &vd[1..6] != ISO_STANDARD_ID {
                break;
            }
            let boot = IsoBootDescriptor::ref_from_bytes(&vd)
                .map_err(|_| FsResolverError::Invalid("Invalid ISO boot descriptor"))?;
            if boot.kind == VD_BOOT_RECORD && &boot.system_id == EL_TORITO_SYS_ID {
                let cat = boot.catalog_lba.get();
                if cat > 0 {
                    catalog_lba = Some(cat);
                    break;
                }
            } else if vd[0] == VD_TERMINATOR {
                break;
            }
            lba += 1;
        }

        let Some(cat_lba) = catalog_lba else {
            return Ok(None);
        };

        let mut cat_buf = [0u8; ISO_SECTOR_SIZE];
        self.io
            .read_at(checked_iso_offset(cat_lba)?, &mut cat_buf)?;

        let validation = IsoBootValidationEntry::ref_from_bytes(&cat_buf[..32])
            .map_err(|_| FsResolverError::Invalid("Invalid boot validation entry"))?;
        if validation.header_id != 1 || validation.key != [0x55, 0xAA] {
            return Ok(None);
        }

        let val_platform = validation.platform_id;

        // 1. Check if Initial Entry (offset 32..64) is an EFI boot image
        let initial = IsoCatalogBootEntry::ref_from_bytes(&cat_buf[32..64])
            .map_err(|_| FsResolverError::Invalid("Invalid initial boot entry"))?;
        let init_bootable = initial.boot_indicator == 0x88;
        let init_sectors_512 = initial.sector_count.get();
        let init_lba = initial.image_lba.get();

        if init_bootable && init_lba > 0 && val_platform == 0xEF {
            let size = if init_sectors_512 > 0 {
                init_sectors_512 as u64 * 512
            } else {
                1440 * 1024
            };
            return Ok(Some(ElToritoBootEntry {
                platform_id: 0xEF,
                lba: init_lba,
                sector_count_512: init_sectors_512,
                size_bytes: size,
            }));
        }

        // 2. Scan section headers starting at offset 64
        let mut off = 64;
        while off + 32 <= cat_buf.len() {
            let section = IsoBootSectionHeader::ref_from_bytes(&cat_buf[off..off + 32])
                .map_err(|_| FsResolverError::Invalid("Invalid boot section header"))?;
            let header_indicator = section.indicator;
            if header_indicator != 0x90 && header_indicator != 0x91 {
                break;
            }
            let platform_id = section.platform_id;
            let entries_count = section.entry_count.get() as usize;
            off += 32;

            for _ in 0..entries_count.max(1) {
                if off + 32 > cat_buf.len() {
                    break;
                }
                let entry = IsoCatalogBootEntry::ref_from_bytes(&cat_buf[off..off + 32])
                    .map_err(|_| FsResolverError::Invalid("Invalid catalog boot entry"))?;
                let boot_ind = entry.boot_indicator;
                let sec_cnt_512 = entry.sector_count.get();
                let boot_lba = entry.image_lba.get();

                if boot_ind == 0x88 && boot_lba > 0 && platform_id == 0xEF {
                    let size = if sec_cnt_512 > 0 {
                        sec_cnt_512 as u64 * 512
                    } else {
                        1440 * 1024
                    };
                    return Ok(Some(ElToritoBootEntry {
                        platform_id,
                        lba: boot_lba,
                        sector_count_512: sec_cnt_512,
                        size_bytes: size,
                    }));
                }

                off += 32;
            }

            if header_indicator == 0x91 {
                break;
            }
        }

        // 3. Fallback: if initial entry was bootable and no explicit EFI section was found, check if initial entry works
        if init_bootable && init_lba > 0 {
            let size = if init_sectors_512 > 0 {
                init_sectors_512 as u64 * 512
            } else {
                1440 * 1024
            };
            return Ok(Some(ElToritoBootEntry {
                platform_id: val_platform,
                lba: init_lba,
                sector_count_512: init_sectors_512,
                size_bytes: size,
            }));
        }

        Ok(None)
    }

    /// Locates the EFI boot entry in the El Torito Boot Catalog if present.
    pub fn efi_boot_entry(&mut self) -> FsResolverResult<Option<ElToritoBootEntry>> {
        self.el_torito_efi_boot_entry()
    }

    /// Opens the EFI El Torito boot image as a lazy streaming `RimRead` without whole-image memory allocation.
    pub fn open_el_torito_efi_image<'b>(
        &'b mut self,
    ) -> FsResolverResult<Option<Box<dyn RimRead + 'b>>> {
        if let Some(entry) = self.el_torito_efi_boot_entry()? {
            let phys_off = checked_iso_offset(entry.lba)?;
            let mut size_bytes = entry.size_bytes;

            // Determine actual FAT image size if BPB is valid
            let mut bpb = [0u8; 512];
            if self.io.read_at(phys_off, &mut bpb).is_ok() && bpb[510] == 0x55 && bpb[511] == 0xAA {
                let (header, _) = rimfs_fat::types::FatCommonBpb::ref_from_prefix(&bpb)
                    .map_err(|_| FsResolverError::Invalid("Invalid boot image BPB"))?;
                let bps = header.bytes_per_sector.get() as u64;
                let s16 = header.total_sectors_16.get() as u64;
                let s32 = header.total_sectors_32.get() as u64;
                let total_secs = if s16 != 0 { s16 } else { s32 };
                if bps >= 512 && total_secs > 0 {
                    size_bytes = total_secs * bps;
                }
            }

            let stream = ExtentRimRead::from_contiguous(&mut *self.io, phys_off, size_bytes);
            Ok(Some(Box::new(stream)))
        } else {
            Ok(None)
        }
    }

    /// Opens a boot image from an El Torito descriptor as a lazy streaming `RimRead`.
    pub fn open_boot_image<'b>(
        &'b mut self,
        entry: ElToritoBootEntry,
    ) -> FsResolverResult<Box<dyn RimRead + 'b>> {
        let phys_off = checked_iso_offset(entry.lba)?;
        Ok(Box::new(ExtentRimRead::from_contiguous(
            &mut *self.io,
            phys_off,
            entry.size_bytes,
        )))
    }

    /// Loads the directory tree starting from PVD or Joliet SVD.
    fn load_tree(&mut self) -> FsResolverResult<()> {
        let pvd_buf: IsoVolumeDescriptor = self.io.read_struct(16 * ISO_SECTOR_SIZE as u64)?;

        if &pvd_buf.standard_id != ISO_STANDARD_ID || pvd_buf.kind != VD_PRIMARY {
            return Err(FsResolverError::Invalid("Not a valid ISO 9660 image"));
        }

        let mut svd_buf = IsoVolumeDescriptor::default();
        let mut root_lba = pvd_buf.root.header.extent_lba.get()?;
        let mut root_size = pvd_buf.root.header.data_length.get()? as u64;
        let mut use_joliet = false;

        if self
            .io
            .read_struct::<IsoVolumeDescriptor>(17 * ISO_SECTOR_SIZE as u64)
            .map(|value| svd_buf = value)
            .is_ok()
            && &svd_buf.standard_id == ISO_STANDARD_ID
            && svd_buf.kind == VD_SUPPLEMENTARY
            && &svd_buf.escape_sequences[..3] == JOLIET_ESCAPE_UCS2_LVL3
        {
            root_lba = svd_buf.root.header.extent_lba.get()?;
            root_size = svd_buf.root.header.data_length.get()? as u64;
            use_joliet = true;
        }

        self.is_joliet = use_joliet;
        self.read_directory_recursive(root_lba, root_size, "", use_joliet)?;
        Ok(())
    }

    /// Recursively reads Directory Records starting from the given LBA and size.
    fn read_directory_recursive(
        &mut self,
        lba: u32,
        size: u64,
        cur_path: &str,
        is_joliet: bool,
    ) -> FsResolverResult<()> {
        let phys_offset = checked_iso_offset(lba)?;
        let total_len = self.io.total_size().unwrap_or(0);
        let dir_len = checked_region_len(
            phys_offset,
            size,
            total_len,
            "Directory extent exceeds ISO bounds",
        )?;
        let mut dir_buf = vec![0; dir_len];
        self.io.read_at(phys_offset, &mut dir_buf)?;

        let mut offset = 0;
        while offset < dir_buf.len() {
            let sector_rem = ISO_SECTOR_SIZE - (offset % ISO_SECTOR_SIZE);
            if offset + 1 > dir_buf.len() {
                break;
            }
            let rec_len = dir_buf[offset] as usize;
            if rec_len == 0 {
                offset += sector_rem;
                continue;
            }
            if offset + rec_len > dir_buf.len() {
                break;
            }

            let entry_buf = &dir_buf[offset..offset + rec_len];
            offset += rec_len;

            if entry_buf.len() < 33 {
                continue;
            }

            let (header, tail) = IsoDirectoryHeader::ref_from_prefix(entry_buf)
                .map_err(|_| FsResolverError::Invalid("Invalid directory header"))?;
            let entry_lba = header.extent_lba.get()?;
            let entry_size = header.data_length.get()? as u64;
            let dt = parse_iso_binary_datetime(&header.recorded);
            let is_dir = (header.flags & DIR_FLAG_DIRECTORY) != 0;
            let is_hidden = (header.flags & 0x01) != 0;
            let name_len = header.name_len as usize;

            if 33 + name_len > entry_buf.len() {
                continue;
            }

            let raw_name_bytes = &tail[..name_len];
            if raw_name_bytes == [0] || raw_name_bytes == [1] {
                continue;
            }

            // Extract Name and Rock Ridge extensions
            let mut name = if is_joliet {
                decode_ucs2_be(raw_name_bytes)
            } else {
                let s = core::str::from_utf8(raw_name_bytes).unwrap_or("");
                // Strip ISO version suffix (e.g. ";1")
                s.split(';').next().unwrap_or(s).to_string()
            };

            let mut mode = None;
            let mut uid = None;
            let mut gid = None;
            let mut is_symlink = false;
            let mut symlink_target = None;

            let mut susp_offset = 33 + name_len;
            if !susp_offset.is_multiple_of(2) {
                susp_offset += 1;
            }

            while susp_offset + 4 <= entry_buf.len() {
                let sig = [entry_buf[susp_offset], entry_buf[susp_offset + 1]];
                let field_len = entry_buf[susp_offset + 2] as usize;
                if field_len < 4 || susp_offset + field_len > entry_buf.len() {
                    break;
                }
                let field_data = &entry_buf[susp_offset..susp_offset + field_len];
                susp_offset += field_len;

                match &sig {
                    b"PX" => {
                        if field_data.len() >= 36 {
                            mode = Some(get_both_u32(&field_data[4..12])?);
                            uid = Some(get_both_u32(&field_data[20..28])?);
                            gid = Some(get_both_u32(&field_data[28..36])?);
                        }
                    }
                    b"NM" => {
                        if field_data.len() >= 5
                            && let Ok(rr_name) = core::str::from_utf8(&field_data[5..])
                        {
                            name = rr_name.to_string();
                        }
                    }
                    b"SL" if field_data.len() >= 7 => {
                        let comp_len = field_data[6] as usize;
                        if 7 + comp_len <= field_data.len()
                            && let Ok(target) = core::str::from_utf8(&field_data[7..7 + comp_len])
                        {
                            is_symlink = true;
                            symlink_target = Some(target.to_string());
                        }
                    }
                    _ => {}
                }
            }

            let full_entry_path = if cur_path.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", cur_path, name)
            };

            self.index.insert_with_kind(
                &full_entry_path,
                IsoResolvedEntry {
                    name,
                    lba: entry_lba,
                    size: entry_size,
                    is_dir,
                    is_hidden,
                    is_symlink,
                    symlink_target,
                    mode,
                    uid,
                    gid,
                    modified: dt,
                },
                is_dir,
            );

            if is_dir && entry_lba != lba {
                self.read_directory_recursive(entry_lba, entry_size, &full_entry_path, is_joliet)?;
            }
        }

        Ok(())
    }
}

impl<'a, IO: RimRead + ?Sized> FsTreeResolver for IsoResolver<'a, IO> {
    fn exists(&mut self, path: &str) -> bool {
        if self.status.is_err() {
            return false;
        }
        self.index.contains_path(path)
    }

    fn read_dir(&mut self, path: &str) -> FsResolverResult<Vec<String>> {
        self.status?;
        let clean = normalize_fs_path(path);
        let trimmed = clean.trim_end_matches('/');
        if self.index.get(trimmed).is_some_and(|entry| !entry.is_dir) {
            return Err(FsResolverError::Invalid("Path is not a directory"));
        }
        if !self.index.is_dir(trimmed) {
            return Err(FsResolverError::NotFound);
        }
        Ok(self.index.children(trimmed).unwrap_or_default())
    }

    fn open_file<'b>(&'b mut self, path: &str) -> FsResolverResult<Box<dyn RimRead + 'b>> {
        self.status?;
        let clean = normalize_fs_path(path);
        let entry = self
            .index
            .get(clean)
            .cloned()
            .ok_or(FsResolverError::NotFound)?;

        if entry.is_dir {
            return Err(FsResolverError::Invalid("Path is a directory"));
        }

        let phys_offset = checked_iso_offset(entry.lba)?;
        Ok(Box::new(ExtentRimRead::from_contiguous(
            &mut *self.io,
            phys_offset,
            entry.size,
        )))
    }

    fn read_link(&mut self, path: &str) -> FsResolverResult<String> {
        self.status?;
        let clean = normalize_fs_path(path);
        let entry = self
            .index
            .get(clean)
            .cloned()
            .ok_or(FsResolverError::NotFound)?;

        if !entry.is_symlink {
            return Err(FsResolverError::Invalid("Entry is not a symbolic link"));
        }

        entry
            .symlink_target
            .ok_or(FsResolverError::Invalid("No symlink target found"))
    }

    fn read_attributes(&mut self, path: &str) -> FsResolverResult<FileAttributes> {
        self.status?;
        let clean = normalize_fs_path(path);
        let trimmed = clean.trim_end_matches('/');
        if trimmed.is_empty() {
            return Ok(FileAttributes::new_dir());
        }

        if let Some(entry) = self.index.get(trimmed) {
            let kind = if entry.is_symlink {
                NodeKind::Symlink
            } else if entry.is_dir {
                NodeKind::Directory
            } else {
                NodeKind::Regular
            };

            let mut attr = match kind {
                NodeKind::Directory => FileAttributes::new_dir(),
                NodeKind::Symlink => FileAttributes::new_symlink(),
                _ => FileAttributes::new_file(),
            };

            attr.read_only = true;
            attr.hidden = entry.is_hidden;
            attr.mode = entry.mode;
            attr.uid = entry.uid;
            attr.gid = entry.gid;
            attr.modified = entry.modified;
            return Ok(attr);
        }

        if self.index.is_dir(trimmed) {
            return Ok(FileAttributes::new_dir());
        }

        Err(FsResolverError::NotFound)
    }
}

fn checked_iso_offset(lba: u32) -> FsResolverResult<u64> {
    (lba as u64)
        .checked_mul(ISO_SECTOR_SIZE as u64)
        .ok_or(FsResolverError::Invalid("ISO LBA offset overflow"))
}

fn checked_region_len(
    offset: u64,
    len: u64,
    limit: u64,
    msg: &'static str,
) -> FsResolverResult<usize> {
    let end = offset
        .checked_add(len)
        .ok_or(FsResolverError::Invalid(msg))?;
    if end > limit {
        return Err(FsResolverError::Invalid(msg));
    }
    usize::try_from(len).map_err(|_| FsResolverError::Invalid(msg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checked_region_len_rejects_overflow() {
        assert!(checked_region_len(10, u64::MAX, 100, "overflow").is_err());
    }
}
