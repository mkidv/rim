// SPDX-License-Identifier: MIT

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use crate::meta::IsoMeta;
use crate::types::*;
use rimfs_core::errors::{FsInjectorError, FsInjectorResult};
use rimfs_core::resolver::attr::FileAttributes;
use rimfs_core::resolver::node::FsNode;
use time::OffsetDateTime;

/// In-memory representation of a planned file in the ISO layout.
#[derive(Debug, Clone)]
pub struct PlannedFile {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub lba: u32,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub mtime: OffsetDateTime,
    pub is_symlink: bool,
    pub symlink_target: Option<String>,
}

/// In-memory representation of a planned directory in the ISO layout.
#[derive(Debug, Clone)]
pub struct PlannedDirectory {
    pub path: String,
    pub name: String,
    pub parent_idx: usize, // 1-based index in path table
    pub dir_idx: usize,    // 1-based index in path table
    pub iso_lba: u32,
    pub iso_size: u32,
    pub joliet_lba: u32,
    pub joliet_size: u32,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub mtime: OffsetDateTime,
    pub file_indices: Vec<usize>,
    pub subdir_indices: Vec<usize>,
}

/// Fully assigned and deterministic pre-computed layout plan for an ISO 9660 image.
#[derive(Debug, Clone)]
pub struct IsoLayoutPlan {
    pub pvd_lba: u32,
    pub svd_joliet_lba: Option<u32>,
    pub boot_record_lba: Option<u32>,
    pub terminator_lba: u32,

    pub pvd_path_table_l_lba: u32,
    pub pvd_path_table_m_lba: u32,
    pub path_table_size: u32,

    pub joliet_path_table_l_lba: Option<u32>,
    pub joliet_path_table_m_lba: Option<u32>,
    pub joliet_path_table_size: Option<u32>,

    pub boot_catalog_lba: Option<u32>,
    pub efi_boot_image_lba: Option<u32>,
    pub efi_boot_image_sectors: u32,
    pub efi_boot_image_data: Option<Vec<u8>>,
    pub bios_boot_image_lba: Option<u32>,
    pub bios_boot_image_sectors: u32,

    pub directories: Vec<PlannedDirectory>,
    pub files: Vec<PlannedFile>,
    pub total_sectors: u32,
}

impl IsoLayoutPlan {
    /// Builds and calculates all LBAs and sector allocations for the given tree.
    pub fn build(tree: &mut FsNode<'_>, meta: &IsoMeta) -> FsInjectorResult<Self> {
        let mut plan = Self {
            pvd_lba: 16,
            svd_joliet_lba: None,
            boot_record_lba: None,
            terminator_lba: 0,
            pvd_path_table_l_lba: 0,
            pvd_path_table_m_lba: 0,
            path_table_size: 0,
            joliet_path_table_l_lba: None,
            joliet_path_table_m_lba: None,
            joliet_path_table_size: None,
            boot_catalog_lba: None,
            efi_boot_image_lba: None,
            efi_boot_image_sectors: 0,
            efi_boot_image_data: None,
            bios_boot_image_lba: None,
            bios_boot_image_sectors: 0,
            directories: Vec::new(),
            files: Vec::new(),
            total_sectors: 0,
        };

        // 1. Assign Volume Descriptors
        let mut cur_vd_lba = 16; // PVD is always at sector 16
        cur_vd_lba += 1;

        if meta.enable_joliet {
            plan.svd_joliet_lba = Some(cur_vd_lba);
            cur_vd_lba += 1;
        }

        let is_bootable = meta.boot_efi.is_some() || meta.boot_bios.is_some();
        if is_bootable {
            plan.boot_record_lba = Some(cur_vd_lba);
            cur_vd_lba += 1;
        }

        plan.terminator_lba = cur_vd_lba;
        cur_vd_lba += 1;

        let mut next_lba = cur_vd_lba;

        // 2. Synthesize EFI FAT boot image if requested
        if let Some(ref efi_bin) = meta.boot_efi {
            let fat_img = synthesize_efi_fat_image(efi_bin)?;
            let sectors = fat_img.len().div_ceil(ISO_SECTOR_SIZE) as u32;
            plan.efi_boot_image_lba = Some(next_lba);
            plan.efi_boot_image_sectors = sectors;
            plan.efi_boot_image_data = Some(fat_img);
            next_lba += sectors;
        }

        // 3. Synthesize BIOS boot image if requested
        if let Some(ref bios_bin) = meta.boot_bios {
            let sectors = bios_bin.len().div_ceil(ISO_SECTOR_SIZE) as u32;
            plan.bios_boot_image_lba = Some(next_lba);
            plan.bios_boot_image_sectors = sectors;
            next_lba += sectors;
        }

        // 4. Allocate 1 sector for El Torito Boot Catalog if bootable
        if is_bootable {
            plan.boot_catalog_lba = Some(next_lba);
            next_lba += 1;
        }

        // 5. Flatten the directory and file tree
        plan.collect_tree(tree, "", 1, 0)?;

        // 6. Calculate Path Table sizes
        let mut pt_size = 0u32;
        for dir in &plan.directories {
            let name_len = if dir.dir_idx == 1 { 1 } else { dir.name.len() } as u32;
            let entry_len = 8 + name_len + (name_len % 2); // 8 bytes header + name + padding
            pt_size += entry_len;
        }
        plan.path_table_size = pt_size;
        let pt_sectors = pt_size.div_ceil(ISO_SECTOR_SIZE as u32);

        plan.pvd_path_table_l_lba = next_lba;
        next_lba += pt_sectors;
        plan.pvd_path_table_m_lba = next_lba;
        next_lba += pt_sectors;

        if meta.enable_joliet {
            let mut jpt_size = 0u32;
            for dir in &plan.directories {
                let name_len = if dir.dir_idx == 1 {
                    1
                } else {
                    dir.name.len() * 2
                } as u32;
                let entry_len = 8 + name_len + (name_len % 2);
                jpt_size += entry_len;
            }
            plan.joliet_path_table_size = Some(jpt_size);
            let jpt_sectors = jpt_size.div_ceil(ISO_SECTOR_SIZE as u32);
            plan.joliet_path_table_l_lba = Some(next_lba);
            next_lba += jpt_sectors;
            plan.joliet_path_table_m_lba = Some(next_lba);
            next_lba += jpt_sectors;
        }

        // 7. Calculate and assign directory extents
        for dir_idx in 0..plan.directories.len() {
            let iso_size = plan.calculate_dir_size(dir_idx, false, meta.enable_rock_ridge);
            let iso_sectors = iso_size.div_ceil(ISO_SECTOR_SIZE as u32);
            plan.directories[dir_idx].iso_lba = next_lba;
            plan.directories[dir_idx].iso_size = iso_size;
            next_lba += iso_sectors;

            if meta.enable_joliet {
                let joliet_size = plan.calculate_dir_size(dir_idx, true, meta.enable_rock_ridge);
                let joliet_sectors = joliet_size.div_ceil(ISO_SECTOR_SIZE as u32);
                plan.directories[dir_idx].joliet_lba = next_lba;
                plan.directories[dir_idx].joliet_size = joliet_size;
                next_lba += joliet_sectors;
            }
        }

        // 8. Assign file data extents
        for file_idx in 0..plan.files.len() {
            plan.files[file_idx].lba = next_lba;
            let sectors = if plan.files[file_idx].is_symlink {
                0
            } else {
                plan.files[file_idx].size.div_ceil(ISO_SECTOR_SIZE as u64) as u32
            };
            next_lba += sectors.max(1);
        }

        plan.total_sectors = next_lba;
        Ok(plan)
    }

    /// Recursively flattens `FsNode` trees into `directories` and `files` lists.
    fn collect_tree(
        &mut self,
        node: &mut FsNode<'_>,
        parent_path: &str,
        parent_idx: usize,
        cur_depth: usize,
    ) -> FsInjectorResult<usize> {
        let dir_idx = self.directories.len() + 1;
        let dir_name = if cur_depth == 0 {
            String::new()
        } else {
            match node {
                FsNode::Dir { name, .. } => name.clone(),
                _ => String::new(),
            }
        };

        let full_dir_path = if parent_path.is_empty() {
            dir_name.clone()
        } else if dir_name.is_empty() {
            parent_path.to_string()
        } else {
            format!("{}/{}", parent_path, dir_name)
        };

        let attr = match node {
            FsNode::Dir { attr, .. } => attr.clone(),
            FsNode::Container { attr, .. } => attr.clone(),
            _ => FileAttributes::new_dir(),
        };

        let dt = attr.modified.unwrap_or(OffsetDateTime::UNIX_EPOCH);
        let mode = attr.mode.unwrap_or(0o755) & 0o7777;
        let uid = attr.uid.unwrap_or(0);
        let gid = attr.gid.unwrap_or(0);

        self.directories.push(PlannedDirectory {
            path: full_dir_path.clone(),
            name: dir_name,
            parent_idx,
            dir_idx,
            iso_lba: 0,
            iso_size: 0,
            joliet_lba: 0,
            joliet_size: 0,
            mode: mode | 0o040000,
            uid,
            gid,
            mtime: dt,
            file_indices: Vec::new(),
            subdir_indices: Vec::new(),
        });

        let children = match node {
            FsNode::Dir { children, .. } => children.as_mut_slice(),
            FsNode::Container { children, .. } => children.as_mut_slice(),
            _ => &mut [],
        };

        for child in children {
            match child {
                FsNode::File { name, source, attr } => {
                    let file_size = source.total_size().map_err(FsInjectorError::IO)?;
                    let file_idx = self.files.len();
                    let f_dt = attr.modified.unwrap_or(OffsetDateTime::UNIX_EPOCH);
                    let f_mode = (attr.mode.unwrap_or(0o644) & 0o7777) | 0o100000;
                    let f_uid = attr.uid.unwrap_or(0);
                    let f_gid = attr.gid.unwrap_or(0);

                    let file_path = if full_dir_path.is_empty() {
                        name.clone()
                    } else {
                        format!("{}/{}", full_dir_path, name)
                    };

                    self.files.push(PlannedFile {
                        path: file_path,
                        name: name.clone(),
                        size: file_size,
                        lba: 0,
                        mode: f_mode,
                        uid: f_uid,
                        gid: f_gid,
                        mtime: f_dt,
                        is_symlink: false,
                        symlink_target: None,
                    });
                    self.directories[dir_idx - 1].file_indices.push(file_idx);
                }
                FsNode::Symlink { name, target, attr } => {
                    let file_idx = self.files.len();
                    let f_dt = attr.modified.unwrap_or(OffsetDateTime::UNIX_EPOCH);
                    let f_mode = (attr.mode.unwrap_or(0o777) & 0o7777) | 0o120000;
                    let f_uid = attr.uid.unwrap_or(0);
                    let f_gid = attr.gid.unwrap_or(0);

                    let file_path = if full_dir_path.is_empty() {
                        name.clone()
                    } else {
                        format!("{}/{}", full_dir_path, name)
                    };

                    self.files.push(PlannedFile {
                        path: file_path,
                        name: name.clone(),
                        size: target.len() as u64,
                        lba: 0,
                        mode: f_mode,
                        uid: f_uid,
                        gid: f_gid,
                        mtime: f_dt,
                        is_symlink: true,
                        symlink_target: Some(target.clone()),
                    });
                    self.directories[dir_idx - 1].file_indices.push(file_idx);
                }
                FsNode::Dir { .. } => {
                    let sub_idx =
                        self.collect_tree(child, &full_dir_path, dir_idx, cur_depth + 1)?;
                    self.directories[dir_idx - 1].subdir_indices.push(sub_idx);
                }
                FsNode::Container { .. } => {
                    self.collect_tree(child, &full_dir_path, dir_idx, cur_depth + 1)?;
                }
            }
        }

        Ok(dir_idx)
    }

    /// Calculates the size in bytes required for a directory's records.
    fn calculate_dir_size(&self, dir_idx: usize, is_joliet: bool, is_rock_ridge: bool) -> u32 {
        let mut total_size = 0u32;
        let mut current_sector_used = 0u32;

        let add_record = |rec_size: u32, total_size: &mut u32, current_sector_used: &mut u32| {
            if *current_sector_used + rec_size > ISO_SECTOR_SIZE as u32 {
                *total_size += ISO_SECTOR_SIZE as u32 - *current_sector_used;
                *current_sector_used = 0;
            }
            *total_size += rec_size;
            *current_sector_used += rec_size;
        };

        // Self record (0x00)
        let dot_size = if is_rock_ridge { 34 + 14 } else { 34 };
        add_record(dot_size, &mut total_size, &mut current_sector_used);

        // Parent record (0x01)
        let dotdot_size = if is_rock_ridge { 34 + 14 } else { 34 };
        add_record(dotdot_size, &mut total_size, &mut current_sector_used);

        // Subdirectories
        for &sub_idx in &self.directories[dir_idx].subdir_indices {
            let sub = &self.directories[sub_idx - 1];
            let name_len = if is_joliet {
                sub.name.len() * 2
            } else {
                sub.name.len()
            };
            let mut rec_size = 33 + name_len as u32;
            if !rec_size.is_multiple_of(2) {
                rec_size += 1;
            }
            if is_rock_ridge {
                rec_size += 36 + 6 + sub.name.len() as u32; // PX + NM
                if !rec_size.is_multiple_of(2) {
                    rec_size += 1;
                }
            }
            add_record(rec_size, &mut total_size, &mut current_sector_used);
        }

        // Files
        for &file_idx in &self.directories[dir_idx].file_indices {
            let f = &self.files[file_idx];
            let name_len = if is_joliet {
                f.name.len() * 2
            } else {
                f.name.len() + 2 // ";1" version string for ISO 9660 level 1
            };
            let mut rec_size = 33 + name_len as u32;
            if !rec_size.is_multiple_of(2) {
                rec_size += 1;
            }
            if is_rock_ridge {
                rec_size += 36 + 6 + f.name.len() as u32; // PX + NM
                if f.is_symlink
                    && let Some(ref target) = f.symlink_target
                {
                    rec_size += 8 + target.len() as u32; // SL
                }
                if !rec_size.is_multiple_of(2) {
                    rec_size += 1;
                }
            }
            add_record(rec_size, &mut total_size, &mut current_sector_used);
        }

        // Align directory record block to 2048-byte sector boundary
        (total_size + ISO_SECTOR_SIZE as u32 - 1) & !(ISO_SECTOR_SIZE as u32 - 1)
    }
}

/// Synthesizes an in-memory FAT12/16 EFI System Partition image containing `/EFI/BOOT/BOOTX64.EFI`.
fn synthesize_efi_fat_image(efi_binary: &[u8]) -> FsInjectorResult<Vec<u8>> {
    use rimfs_core::formatter::FsFormatter;
    use rimfs_core::injector::FsTreeInjector;
    use rimfs_fat::prelude::*;
    use rimio::MemRimIO;

    // Allocate 1.44 MiB or larger if binary is large
    let fat_size = ((efi_binary.len() as u64 + 512 * 1024).max(1440 * 1024) + 511) & !511;
    let mut buf = vec![0u8; fat_size as usize];
    let mut io = MemRimIO::new(&mut buf);

    let meta = FatMeta::new_fat12(fat_size, Some("EFIBOOT")).map_err(|_| {
        FsInjectorError::Invalid("Failed to initialize FAT metadata for EFI boot image")
    })?;

    let mut formatter = FatFormatter::new(&mut io, &meta);
    formatter.format(false).map_err(|_| {
        FsInjectorError::Invalid("Failed to format FAT filesystem for EFI boot image")
    })?;

    let mut injector = FatInjector::new(&mut io, &meta)?;
    let mut tree = FsNode::Container {
        attr: FileAttributes::new_dir(),
        children: vec![FsNode::Dir {
            name: "EFI".to_string(),
            attr: FileAttributes::new_dir(),
            children: vec![FsNode::Dir {
                name: "BOOT".to_string(),
                attr: FileAttributes::new_dir(),
                children: vec![FsNode::new_file_from_source(
                    "BOOTX64.EFI",
                    alloc::boxed::Box::new(rimio::prelude::VecRimIO::new(efi_binary.to_vec())),
                    FileAttributes::new_file(),
                )],
            }],
        }],
    };

    injector.inject_tree(&mut tree)?;
    injector.flush()?;

    Ok(buf)
}
