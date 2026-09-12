// SPDX-License-Identifier: MIT

//! ISO 9660 volume space layout and sector offset calculation.

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
use rimfs_core::resolver::attr::{FileAttributes, NodeKind};
use rimfs_core::resolver::node::FsNode;
use rimfs_core::resolver::{FsNodeCounts, FsTreeResolver};
use rimfs_core::{extract_name_from_path, is_wildcard, join_paths, strip_wildcard};
use time::OffsetDateTime;

/// In-memory representation of a planned file in the ISO layout.
#[derive(Debug, Clone)]
pub(crate) struct PlannedFile {
    pub(crate) path: String,
    pub(crate) source_path: Option<String>,
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) lba: u32,
    pub(crate) mode: u32,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
    pub(crate) mtime: OffsetDateTime,
    pub(crate) is_symlink: bool,
    pub(crate) symlink_target: Option<String>,
}

/// In-memory representation of a planned directory in the ISO layout.
#[derive(Debug, Clone)]
pub(crate) struct PlannedDirectory {
    pub(crate) name: String,
    pub(crate) parent_idx: usize, // 1-based index in path table
    pub(crate) dir_idx: usize,    // 1-based index in path table
    pub(crate) iso_lba: u32,
    pub(crate) iso_size: u32,
    pub(crate) joliet_lba: u32,
    pub(crate) joliet_size: u32,
    pub(crate) mode: u32,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
    pub(crate) mtime: OffsetDateTime,
    pub(crate) file_indices: Vec<usize>,
    pub(crate) subdir_indices: Vec<usize>,
}

/// Fully assigned and deterministic pre-computed layout plan for an ISO 9660 image.
#[derive(Debug, Clone)]
pub(crate) struct IsoLayoutPlan {
    pub(crate) svd_joliet_lba: Option<u32>,
    pub(crate) boot_record_lba: Option<u32>,
    pub(crate) terminator_lba: u32,

    pub(crate) pvd_path_table_l_lba: u32,
    pub(crate) pvd_path_table_m_lba: u32,
    pub(crate) path_table_size: u32,

    pub(crate) joliet_path_table_l_lba: Option<u32>,
    pub(crate) joliet_path_table_m_lba: Option<u32>,
    pub(crate) joliet_path_table_size: Option<u32>,

    pub(crate) boot_catalog_lba: Option<u32>,
    pub(crate) efi_boot_image_lba: Option<u32>,
    pub(crate) efi_boot_image_sectors: u32,
    pub(crate) efi_boot_image_data: Option<Vec<u8>>,
    pub(crate) bios_boot_image_lba: Option<u32>,
    pub(crate) bios_boot_image_sectors: u32,

    pub(crate) directories: Vec<PlannedDirectory>,
    pub(crate) files: Vec<PlannedFile>,
    pub(crate) total_sectors: u32,
}

impl IsoLayoutPlan {
    /// Builds and calculates all LBAs and sector allocations for the given tree.
    pub(crate) fn build(tree: &mut FsNode<'_>, meta: &IsoMeta) -> FsInjectorResult<Self> {
        let (mut plan, next_lba) = Self::new_with_boot_regions(meta)?;

        plan.collect_tree(tree, "", 1, 0)?;
        plan.assign_extents(meta, next_lba);
        Ok(plan)
    }

    pub(crate) fn build_from_resolver(
        resolver: &mut dyn FsTreeResolver,
        path: &str,
        meta: &IsoMeta,
    ) -> FsInjectorResult<(Self, FsNodeCounts)> {
        let (mut plan, next_lba) = Self::new_with_boot_regions(meta)?;
        let counts = if is_wildcard(path) {
            plan.collect_resolver_root(resolver, strip_wildcard(path))?
        } else {
            let attr = resolver.read_attributes(path)?;
            if attr.kind == NodeKind::Regular || attr.kind == NodeKind::Symlink {
                plan.collect_resolver_file_root(resolver, path, attr)?
            } else {
                plan.collect_resolver_dir(resolver, path, "", 1, 0, attr)?
            }
        };
        plan.assign_extents(meta, next_lba);
        Ok((plan, counts))
    }

    fn new_with_boot_regions(meta: &IsoMeta) -> FsInjectorResult<(Self, u32)> {
        let mut plan = Self {
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

        let mut cur_vd_lba = 17;

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

        if let Some(ref efi_bin) = meta.boot_efi {
            let fat_img = synthesize_efi_fat_image(efi_bin)?;
            let sectors = fat_img.len().div_ceil(ISO_SECTOR_SIZE) as u32;
            plan.efi_boot_image_lba = Some(next_lba);
            plan.efi_boot_image_sectors = sectors;
            plan.efi_boot_image_data = Some(fat_img);
            next_lba += sectors;
        }

        if let Some(ref bios_bin) = meta.boot_bios {
            let sectors = bios_bin.len().div_ceil(ISO_SECTOR_SIZE) as u32;
            plan.bios_boot_image_lba = Some(next_lba);
            plan.bios_boot_image_sectors = sectors;
            next_lba += sectors;
        }

        if is_bootable {
            plan.boot_catalog_lba = Some(next_lba);
            next_lba += 1;
        }

        Ok((plan, next_lba))
    }

    fn assign_extents(&mut self, meta: &IsoMeta, mut next_lba: u32) {
        let mut pt_size = 0u32;
        for dir in &self.directories {
            let name_len = if dir.dir_idx == 1 { 1 } else { dir.name.len() } as u32;
            let entry_len = 8 + name_len + (name_len % 2); // 8 bytes header + name + padding
            pt_size += entry_len;
        }
        self.path_table_size = pt_size;
        let pt_sectors = pt_size.div_ceil(ISO_SECTOR_SIZE as u32);

        self.pvd_path_table_l_lba = next_lba;
        next_lba += pt_sectors;
        self.pvd_path_table_m_lba = next_lba;
        next_lba += pt_sectors;

        if meta.enable_joliet {
            let mut jpt_size = 0u32;
            for dir in &self.directories {
                let name_len = if dir.dir_idx == 1 {
                    1
                } else {
                    dir.name.len() * 2
                } as u32;
                let entry_len = 8 + name_len + (name_len % 2);
                jpt_size += entry_len;
            }
            self.joliet_path_table_size = Some(jpt_size);
            let jpt_sectors = jpt_size.div_ceil(ISO_SECTOR_SIZE as u32);
            self.joliet_path_table_l_lba = Some(next_lba);
            next_lba += jpt_sectors;
            self.joliet_path_table_m_lba = Some(next_lba);
            next_lba += jpt_sectors;
        }

        for dir_idx in 0..self.directories.len() {
            let iso_size = self.calculate_dir_size(dir_idx, false, meta.enable_rock_ridge);
            let iso_sectors = iso_size.div_ceil(ISO_SECTOR_SIZE as u32);
            self.directories[dir_idx].iso_lba = next_lba;
            self.directories[dir_idx].iso_size = iso_size;
            next_lba += iso_sectors;

            if meta.enable_joliet {
                let joliet_size = self.calculate_dir_size(dir_idx, true, meta.enable_rock_ridge);
                let joliet_sectors = joliet_size.div_ceil(ISO_SECTOR_SIZE as u32);
                self.directories[dir_idx].joliet_lba = next_lba;
                self.directories[dir_idx].joliet_size = joliet_size;
                next_lba += joliet_sectors;
            }
        }

        for file_idx in 0..self.files.len() {
            self.files[file_idx].lba = next_lba;
            let sectors = if self.files[file_idx].is_symlink {
                0
            } else {
                self.files[file_idx].size.div_ceil(ISO_SECTOR_SIZE as u64) as u32
            };
            next_lba += sectors.max(1);
        }

        self.total_sectors = next_lba;
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
                        source_path: None,
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
                        source_path: None,
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

    fn collect_resolver_root(
        &mut self,
        resolver: &mut dyn FsTreeResolver,
        base_path: &str,
    ) -> FsInjectorResult<FsNodeCounts> {
        self.push_directory(String::new(), 1, FileAttributes::new_dir());
        let mut counts = FsNodeCounts::default();
        for entry in resolver.read_dir(base_path)? {
            let entry_path = join_paths(base_path, &entry);
            let attr = resolver.read_attributes(&entry_path)?;
            let child_counts = self.collect_resolver_child(resolver, &entry_path, "", 1, attr)?;
            counts.dirs += child_counts.dirs;
            counts.files += child_counts.files;
            counts.symlinks += child_counts.symlinks;
            counts.bytes += child_counts.bytes;
        }
        Ok(counts)
    }

    fn collect_resolver_file_root(
        &mut self,
        resolver: &mut dyn FsTreeResolver,
        path: &str,
        attr: FileAttributes,
    ) -> FsInjectorResult<FsNodeCounts> {
        self.push_directory(String::new(), 1, FileAttributes::new_dir());
        self.collect_resolver_file(resolver, path, "", 1, attr)
    }

    fn collect_resolver_child(
        &mut self,
        resolver: &mut dyn FsTreeResolver,
        path: &str,
        parent_path: &str,
        parent_idx: usize,
        attr: FileAttributes,
    ) -> FsInjectorResult<FsNodeCounts> {
        match attr.kind {
            NodeKind::Directory => {
                self.collect_resolver_dir(resolver, path, parent_path, parent_idx, 1, attr)
            }
            NodeKind::Regular | NodeKind::Symlink => {
                self.collect_resolver_file(resolver, path, parent_path, parent_idx, attr)
            }
            _ => Err(FsInjectorError::Unsupported(
                "Unsupported resolver entry kind",
            )),
        }
    }

    fn collect_resolver_dir(
        &mut self,
        resolver: &mut dyn FsTreeResolver,
        source_path: &str,
        parent_path: &str,
        parent_idx: usize,
        cur_depth: usize,
        attr: FileAttributes,
    ) -> FsInjectorResult<FsNodeCounts> {
        let name = if cur_depth == 0 {
            String::new()
        } else {
            extract_name_from_path(source_path).to_string()
        };
        let full_path = if parent_path.is_empty() {
            name.clone()
        } else if name.is_empty() {
            parent_path.to_string()
        } else {
            format!("{}/{}", parent_path, name)
        };
        let dir_idx = self.push_directory(name, parent_idx, attr);
        let mut counts = FsNodeCounts {
            dirs: 1,
            ..FsNodeCounts::default()
        };

        for entry in resolver.read_dir(source_path)? {
            let entry_path = join_paths(source_path, &entry);
            let attr = resolver.read_attributes(&entry_path)?;
            let child_counts =
                self.collect_resolver_child(resolver, &entry_path, &full_path, dir_idx, attr)?;
            counts.dirs += child_counts.dirs;
            counts.files += child_counts.files;
            counts.symlinks += child_counts.symlinks;
            counts.bytes += child_counts.bytes;
        }

        Ok(counts)
    }

    fn collect_resolver_file(
        &mut self,
        resolver: &mut dyn FsTreeResolver,
        source_path: &str,
        parent_path: &str,
        parent_idx: usize,
        attr: FileAttributes,
    ) -> FsInjectorResult<FsNodeCounts> {
        let name = extract_name_from_path(source_path).to_string();
        let file_path = if parent_path.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", parent_path, name)
        };

        let file_idx = self.files.len();
        let f_dt = attr.modified.unwrap_or(OffsetDateTime::UNIX_EPOCH);
        let f_uid = attr.uid.unwrap_or(0);
        let f_gid = attr.gid.unwrap_or(0);
        let (size, mode, target, counts) = if attr.kind == NodeKind::Symlink {
            let target = resolver.read_link(source_path)?;
            let size = target.len() as u64;
            (
                size,
                (attr.mode.unwrap_or(0o777) & 0o7777) | 0o120000,
                Some(target),
                FsNodeCounts {
                    symlinks: 1,
                    bytes: size,
                    ..FsNodeCounts::default()
                },
            )
        } else {
            let mut source = resolver.open_file(source_path)?;
            let size = source.total_size().map_err(FsInjectorError::IO)?;
            (
                size,
                (attr.mode.unwrap_or(0o644) & 0o7777) | 0o100000,
                None,
                FsNodeCounts {
                    files: 1,
                    bytes: size,
                    ..FsNodeCounts::default()
                },
            )
        };

        self.files.push(PlannedFile {
            path: file_path,
            source_path: Some(source_path.to_string()),
            name,
            size,
            lba: 0,
            mode,
            uid: f_uid,
            gid: f_gid,
            mtime: f_dt,
            is_symlink: attr.kind == NodeKind::Symlink,
            symlink_target: target,
        });
        self.directories[parent_idx - 1].file_indices.push(file_idx);

        Ok(counts)
    }

    fn push_directory(&mut self, name: String, parent_idx: usize, attr: FileAttributes) -> usize {
        let dir_idx = self.directories.len() + 1;
        let dt = attr.modified.unwrap_or(OffsetDateTime::UNIX_EPOCH);
        let mode = attr.mode.unwrap_or(0o755) & 0o7777;
        let uid = attr.uid.unwrap_or(0);
        let gid = attr.gid.unwrap_or(0);

        self.directories.push(PlannedDirectory {
            name,
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
        if dir_idx != parent_idx {
            self.directories[parent_idx - 1]
                .subdir_indices
                .push(dir_idx);
        }
        dir_idx
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
