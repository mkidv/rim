// SPDX-License-Identifier: MIT

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::{format, vec, vec::Vec};

use crate::layout::IsoLayoutPlan;
use crate::meta::IsoMeta;
use crate::types::*;
use rimfs_core::errors::{FsInjectorError, FsInjectorResult};
use rimfs_core::injector::FsTreeInjector;
use rimfs_core::resolver::{FsNodeCounts, FsTreeResolver, attr::FileAttributes, node::FsNode};
use rimio::{RimIO, RimRead};

/// Serializes and writes `FsNode` trees into a complete ISO 9660 / Joliet / Rock Ridge / El Torito image.
pub struct IsoInjector<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a IsoMeta,
}

impl<'a, IO: RimIO + ?Sized> IsoInjector<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a IsoMeta) -> FsInjectorResult<Self> {
        Ok(Self { io, meta })
    }

    fn write_plan(&mut self, plan: &IsoLayoutPlan) -> FsInjectorResult<()> {
        self.write_volume_descriptors(plan)?;
        self.write_path_tables(plan)?;
        self.write_directory_records(plan)?;
        self.write_boot_catalog_and_images(plan)?;
        Ok(())
    }

    /// Writes Volume Descriptors (PVD, Joliet SVD, El Torito Boot Record, and Terminator).
    fn write_volume_descriptors(&mut self, plan: &IsoLayoutPlan) -> FsInjectorResult<()> {
        let now = rimfs_core::utils::time_utils::now_utc();
        let now_text = format_iso_text_datetime(now);
        let now_bin = format_iso_binary_datetime(now);

        // 1. Primary Volume Descriptor (Sector 16)
        let mut pvd = [0u8; ISO_SECTOR_SIZE];
        pvd[0] = VD_PRIMARY;
        pvd[1..6].copy_from_slice(ISO_STANDARD_ID);
        pvd[6] = 1;

        let sys_id = b"LINUX                           ";
        pvd[8..40].copy_from_slice(sys_id);

        let mut vol_id = [b' '; 32];
        let label_bytes = self.meta.volume_id.as_bytes();
        let len = label_bytes.len().min(32);
        vol_id[..len].copy_from_slice(&label_bytes[..len]);
        pvd[40..72].copy_from_slice(&vol_id);

        put_both_u32(&mut pvd[80..88], plan.total_sectors);
        put_both_u16(&mut pvd[120..124], 1);
        put_both_u16(&mut pvd[124..128], 1);
        put_both_u16(&mut pvd[128..132], ISO_SECTOR_SIZE as u16);
        put_both_u32(&mut pvd[132..140], plan.path_table_size);

        pvd[140..144].copy_from_slice(&plan.pvd_path_table_l_lba.to_le_bytes());
        pvd[148..152].copy_from_slice(&plan.pvd_path_table_m_lba.to_be_bytes());

        // Root directory in PVD
        let root = &plan.directories[0];
        pvd[156] = 34;
        put_both_u32(&mut pvd[158..166], root.iso_lba);
        put_both_u32(&mut pvd[166..174], root.iso_size);
        pvd[174..181].copy_from_slice(&now_bin);
        pvd[181] = DIR_FLAG_DIRECTORY;
        put_both_u16(&mut pvd[184..188], 1);
        pvd[188] = 1;
        pvd[189] = 0; // \0 for root

        pvd[813..830].copy_from_slice(&now_text);
        pvd[830..847].copy_from_slice(&now_text);
        pvd[881] = 1;

        self.io.write_at(16 * ISO_SECTOR_SIZE as u64, &pvd)?;

        // 2. Supplementary Volume Descriptor (Joliet)
        if let Some(svd_lba) = plan.svd_joliet_lba {
            let mut svd = [0u8; ISO_SECTOR_SIZE];
            svd[0] = VD_SUPPLEMENTARY;
            svd[1..6].copy_from_slice(ISO_STANDARD_ID);
            svd[6] = 1;

            let ucs2_sys = encode_ucs2_be("LINUX");
            svd[8..8 + ucs2_sys.len().min(32)].copy_from_slice(&ucs2_sys[..ucs2_sys.len().min(32)]);

            let ucs2_vol = encode_ucs2_be(&self.meta.volume_id);
            svd[40..40 + ucs2_vol.len().min(32)]
                .copy_from_slice(&ucs2_vol[..ucs2_vol.len().min(32)]);

            put_both_u32(&mut svd[80..88], plan.total_sectors);
            svd[88..91].copy_from_slice(JOLIET_ESCAPE_UCS2_LVL3); // Escape sequence (%/@)
            put_both_u16(&mut svd[120..124], 1);
            put_both_u16(&mut svd[124..128], 1);
            put_both_u16(&mut svd[128..132], ISO_SECTOR_SIZE as u16);

            let jpt_size = plan.joliet_path_table_size.unwrap_or(10);
            put_both_u32(&mut svd[132..140], jpt_size);

            if let Some(jpt_l) = plan.joliet_path_table_l_lba {
                svd[140..144].copy_from_slice(&jpt_l.to_le_bytes());
            }
            if let Some(jpt_m) = plan.joliet_path_table_m_lba {
                svd[148..152].copy_from_slice(&jpt_m.to_be_bytes());
            }

            // Root directory in Joliet SVD
            svd[156] = 34;
            put_both_u32(&mut svd[158..166], root.joliet_lba);
            put_both_u32(&mut svd[166..174], root.joliet_size);
            svd[174..181].copy_from_slice(&now_bin);
            svd[181] = DIR_FLAG_DIRECTORY;
            put_both_u16(&mut svd[184..188], 1);
            svd[188] = 1;
            svd[189] = 0;

            svd[813..830].copy_from_slice(&now_text);
            svd[830..847].copy_from_slice(&now_text);
            svd[881] = 1;

            self.io
                .write_at(svd_lba as u64 * ISO_SECTOR_SIZE as u64, &svd)?;
        }

        // 3. El Torito Boot Record Volume Descriptor
        if let Some(boot_vd_lba) = plan.boot_record_lba {
            let mut bvd = [0u8; ISO_SECTOR_SIZE];
            bvd[0] = VD_BOOT_RECORD;
            bvd[1..6].copy_from_slice(ISO_STANDARD_ID);
            bvd[6] = 1;
            bvd[7..39].copy_from_slice(EL_TORITO_SYS_ID);

            if let Some(catalog_lba) = plan.boot_catalog_lba {
                bvd[71..75].copy_from_slice(&catalog_lba.to_le_bytes());
            }

            self.io
                .write_at(boot_vd_lba as u64 * ISO_SECTOR_SIZE as u64, &bvd)?;
        }

        // 4. Volume Descriptor Set Terminator
        let mut term = [0u8; ISO_SECTOR_SIZE];
        term[0] = VD_TERMINATOR;
        term[1..6].copy_from_slice(ISO_STANDARD_ID);
        term[6] = 1;
        self.io
            .write_at(plan.terminator_lba as u64 * ISO_SECTOR_SIZE as u64, &term)?;

        Ok(())
    }

    /// Writes standard and Joliet Path Tables (Type L and Type M).
    fn write_path_tables(&mut self, plan: &IsoLayoutPlan) -> FsInjectorResult<()> {
        // Standard ISO Type L (Little Endian)
        let mut pt_l = vec![
            0u8;
            (plan.path_table_size as usize + ISO_SECTOR_SIZE - 1)
                & !(ISO_SECTOR_SIZE - 1)
        ];
        let mut off_l = 0;

        for dir in &plan.directories {
            let name_bytes = if dir.dir_idx == 1 {
                vec![0]
            } else {
                dir.name.to_uppercase().into_bytes()
            };
            let len = name_bytes.len() as u8;
            pt_l[off_l] = len;
            pt_l[off_l + 1] = 0;
            pt_l[off_l + 2..off_l + 6].copy_from_slice(&dir.iso_lba.to_le_bytes());
            pt_l[off_l + 6..off_l + 8].copy_from_slice(&(dir.parent_idx as u16).to_le_bytes());
            pt_l[off_l + 8..off_l + 8 + name_bytes.len()].copy_from_slice(&name_bytes);
            off_l += 8 + name_bytes.len();
            if name_bytes.len() % 2 != 0 {
                off_l += 1;
            }
        }
        self.io.write_at(
            plan.pvd_path_table_l_lba as u64 * ISO_SECTOR_SIZE as u64,
            &pt_l,
        )?;

        // Standard ISO Type M (Big Endian)
        let mut pt_m = vec![0u8; pt_l.len()];
        let mut off_m = 0;
        for dir in &plan.directories {
            let name_bytes = if dir.dir_idx == 1 {
                vec![0]
            } else {
                dir.name.to_uppercase().into_bytes()
            };
            let len = name_bytes.len() as u8;
            pt_m[off_m] = len;
            pt_m[off_m + 1] = 0;
            pt_m[off_m + 2..off_m + 6].copy_from_slice(&dir.iso_lba.to_be_bytes());
            pt_m[off_m + 6..off_m + 8].copy_from_slice(&(dir.parent_idx as u16).to_be_bytes());
            pt_m[off_m + 8..off_m + 8 + name_bytes.len()].copy_from_slice(&name_bytes);
            off_m += 8 + name_bytes.len();
            if name_bytes.len() % 2 != 0 {
                off_m += 1;
            }
        }
        self.io.write_at(
            plan.pvd_path_table_m_lba as u64 * ISO_SECTOR_SIZE as u64,
            &pt_m,
        )?;

        // Joliet Path Tables
        if let (Some(jpt_l_lba), Some(jpt_m_lba), Some(jpt_size)) = (
            plan.joliet_path_table_l_lba,
            plan.joliet_path_table_m_lba,
            plan.joliet_path_table_size,
        ) {
            let mut jpt_l =
                vec![0u8; (jpt_size as usize + ISO_SECTOR_SIZE - 1) & !(ISO_SECTOR_SIZE - 1)];
            let mut j_off_l = 0;
            for dir in &plan.directories {
                let name_bytes = if dir.dir_idx == 1 {
                    vec![0]
                } else {
                    encode_ucs2_be(&dir.name)
                };
                let len = name_bytes.len() as u8;
                jpt_l[j_off_l] = len;
                jpt_l[j_off_l + 1] = 0;
                jpt_l[j_off_l + 2..j_off_l + 6].copy_from_slice(&dir.joliet_lba.to_le_bytes());
                jpt_l[j_off_l + 6..j_off_l + 8]
                    .copy_from_slice(&(dir.parent_idx as u16).to_le_bytes());
                jpt_l[j_off_l + 8..j_off_l + 8 + name_bytes.len()].copy_from_slice(&name_bytes);
                j_off_l += 8 + name_bytes.len();
                if name_bytes.len() % 2 != 0 {
                    j_off_l += 1;
                }
            }
            self.io
                .write_at(jpt_l_lba as u64 * ISO_SECTOR_SIZE as u64, &jpt_l)?;

            let mut jpt_m = vec![0u8; jpt_l.len()];
            let mut j_off_m = 0;
            for dir in &plan.directories {
                let name_bytes = if dir.dir_idx == 1 {
                    vec![0]
                } else {
                    encode_ucs2_be(&dir.name)
                };
                let len = name_bytes.len() as u8;
                jpt_m[j_off_m] = len;
                jpt_m[j_off_m + 1] = 0;
                jpt_m[j_off_m + 2..j_off_m + 6].copy_from_slice(&dir.joliet_lba.to_be_bytes());
                jpt_m[j_off_m + 6..j_off_m + 8]
                    .copy_from_slice(&(dir.parent_idx as u16).to_be_bytes());
                jpt_m[j_off_m + 8..j_off_m + 8 + name_bytes.len()].copy_from_slice(&name_bytes);
                j_off_m += 8 + name_bytes.len();
                if name_bytes.len() % 2 != 0 {
                    j_off_m += 1;
                }
            }
            self.io
                .write_at(jpt_m_lba as u64 * ISO_SECTOR_SIZE as u64, &jpt_m)?;
        }

        Ok(())
    }

    /// Writes directory records for standard ISO (with Rock Ridge) and Joliet.
    fn write_directory_records(&mut self, plan: &IsoLayoutPlan) -> FsInjectorResult<()> {
        for (dir_idx, dir) in plan.directories.iter().enumerate() {
            // Write standard ISO 9660 Directory Block
            let iso_block =
                self.serialize_dir_block(dir_idx, plan, false, self.meta.enable_rock_ridge);
            self.io
                .write_at(dir.iso_lba as u64 * ISO_SECTOR_SIZE as u64, &iso_block)?;

            // Write Joliet Directory Block
            if self.meta.enable_joliet {
                let joliet_block =
                    self.serialize_dir_block(dir_idx, plan, true, self.meta.enable_rock_ridge);
                self.io.write_at(
                    dir.joliet_lba as u64 * ISO_SECTOR_SIZE as u64,
                    &joliet_block,
                )?;
            }
        }
        Ok(())
    }

    /// Serializes a single directory's records into a 2048-byte sector aligned buffer.
    fn serialize_dir_block(
        &self,
        dir_idx: usize,
        plan: &IsoLayoutPlan,
        is_joliet: bool,
        is_rock_ridge: bool,
    ) -> Vec<u8> {
        let dir = &plan.directories[dir_idx];
        let parent = &plan.directories[dir.parent_idx - 1];
        let now_bin = format_iso_binary_datetime(dir.mtime);

        let mut block = vec![
            0u8;
            if is_joliet {
                dir.joliet_size as usize
            } else {
                dir.iso_size as usize
            }
        ];
        let mut cur_sector_off = 0;
        let mut block_off = 0;

        let write_rec =
            |rec: &[u8], block: &mut [u8], block_off: &mut usize, cur_sector_off: &mut usize| {
                if *cur_sector_off + rec.len() > ISO_SECTOR_SIZE {
                    let pad = ISO_SECTOR_SIZE - *cur_sector_off;
                    *block_off += pad;
                    *cur_sector_off = 0;
                }
                block[*block_off..*block_off + rec.len()].copy_from_slice(rec);
                *block_off += rec.len();
                *cur_sector_off += rec.len();
            };

        // 1. "." self record
        let self_lba = if is_joliet {
            dir.joliet_lba
        } else {
            dir.iso_lba
        };
        let self_size = if is_joliet {
            dir.joliet_size
        } else {
            dir.iso_size
        };
        let dot_rec = self.build_dir_record(
            self_lba,
            self_size,
            true,
            b"\x00",
            now_bin,
            dir.mode,
            dir.uid,
            dir.gid,
            is_rock_ridge,
            None,
            None,
        );
        write_rec(&dot_rec, &mut block, &mut block_off, &mut cur_sector_off);

        // 2. ".." parent record
        let parent_lba = if is_joliet {
            parent.joliet_lba
        } else {
            parent.iso_lba
        };
        let parent_size = if is_joliet {
            parent.joliet_size
        } else {
            parent.iso_size
        };
        let dotdot_rec = self.build_dir_record(
            parent_lba,
            parent_size,
            true,
            b"\x01",
            now_bin,
            parent.mode,
            parent.uid,
            parent.gid,
            is_rock_ridge,
            None,
            None,
        );
        write_rec(&dotdot_rec, &mut block, &mut block_off, &mut cur_sector_off);

        // 3. Subdirectories
        for &sub_idx in &dir.subdir_indices {
            let sub = &plan.directories[sub_idx - 1];
            let sub_lba = if is_joliet {
                sub.joliet_lba
            } else {
                sub.iso_lba
            };
            let sub_size = if is_joliet {
                sub.joliet_size
            } else {
                sub.iso_size
            };
            let sub_now = format_iso_binary_datetime(sub.mtime);

            let name_bytes = if is_joliet {
                encode_ucs2_be(&sub.name)
            } else {
                sub.name.to_uppercase().into_bytes()
            };

            let rr_name = if is_rock_ridge {
                Some(sub.name.as_str())
            } else {
                None
            };
            let sub_rec = self.build_dir_record(
                sub_lba,
                sub_size,
                true,
                &name_bytes,
                sub_now,
                sub.mode,
                sub.uid,
                sub.gid,
                is_rock_ridge,
                rr_name,
                None,
            );
            write_rec(&sub_rec, &mut block, &mut block_off, &mut cur_sector_off);
        }

        // 4. Files
        for &file_idx in &dir.file_indices {
            let f = &plan.files[file_idx];
            let f_now = format_iso_binary_datetime(f.mtime);

            let name_bytes = if is_joliet {
                encode_ucs2_be(&f.name)
            } else {
                let mut s = f.name.to_uppercase();
                s.push_str(";1");
                s.into_bytes()
            };

            let rr_name = if is_rock_ridge {
                Some(f.name.as_str())
            } else {
                None
            };
            let f_rec = self.build_dir_record(
                f.lba,
                f.size as u32,
                false,
                &name_bytes,
                f_now,
                f.mode,
                f.uid,
                f.gid,
                is_rock_ridge,
                rr_name,
                f.symlink_target.as_deref(),
            );
            write_rec(&f_rec, &mut block, &mut block_off, &mut cur_sector_off);
        }

        block
    }

    /// Constructs an individual ISO Directory Record with optional Rock Ridge SUSP fields.
    #[allow(clippy::too_many_arguments)]
    fn build_dir_record(
        &self,
        lba: u32,
        data_len: u32,
        is_dir: bool,
        name_bytes: &[u8],
        datetime_bin: [u8; 7],
        mode: u32,
        uid: u32,
        gid: u32,
        is_rock_ridge: bool,
        rock_ridge_name: Option<&str>,
        symlink_target: Option<&str>,
    ) -> Vec<u8> {
        let mut rec = Vec::with_capacity(64 + name_bytes.len());
        rec.push(0); // placeholder for total record length
        rec.push(0); // ext attribute record length
        let mut lba_buf = [0u8; 8];
        put_both_u32(&mut lba_buf, lba);
        rec.extend_from_slice(&lba_buf);

        let mut len_buf = [0u8; 8];
        put_both_u32(&mut len_buf, data_len);
        rec.extend_from_slice(&len_buf);

        rec.extend_from_slice(&datetime_bin);
        rec.push(if is_dir {
            DIR_FLAG_DIRECTORY
        } else {
            DIR_FLAG_FILE
        });
        rec.push(0); // file unit size
        rec.push(0); // interleave gap

        let mut vol_seq = [0u8; 4];
        put_both_u16(&mut vol_seq, 1);
        rec.extend_from_slice(&vol_seq);

        rec.push(name_bytes.len() as u8);
        rec.extend_from_slice(name_bytes);

        if name_bytes.len().is_multiple_of(2) {
            rec.push(0); // padding byte
        }

        // Rock Ridge Extensions (SUSP)
        if is_rock_ridge {
            // SP record on root / self or regular entries
            if name_bytes == b"\x00" {
                rec.extend_from_slice(&[b'S', b'P', 7, 1, 0xBE, 0xEF, 0]);
                rec.extend_from_slice(&[b'R', b'R', 5, 1, 0]);
            }

            // PX (POSIX Attributes: mode, nlinks, uid, gid, serial)
            let mut px = [0u8; 36];
            px[0] = b'P';
            px[1] = b'X';
            px[2] = 36; // len
            px[3] = 1; // version
            put_both_u32(&mut px[4..12], mode);
            put_both_u32(&mut px[12..20], 1);
            put_both_u32(&mut px[20..28], uid);
            put_both_u32(&mut px[28..36], gid);
            rec.extend_from_slice(&px);

            // NM (Alternate Name)
            if let Some(orig_name) = rock_ridge_name {
                let name_b = orig_name.as_bytes();
                let nm_len = 5 + name_b.len() as u8;
                rec.push(b'N');
                rec.push(b'M');
                rec.push(nm_len);
                rec.push(1);
                rec.push(0); // flags = 0
                rec.extend_from_slice(name_b);
            }

            // SL (Symlink Component)
            if let Some(target) = symlink_target {
                let target_b = target.as_bytes();
                let sl_len = 7 + target_b.len() as u8;
                rec.push(b'S');
                rec.push(b'L');
                rec.push(sl_len);
                rec.push(1);
                rec.push(0); // flags
                rec.push(0); // component flags
                rec.push(target_b.len() as u8);
                rec.extend_from_slice(target_b);
            }

            if rec.len() % 2 != 0 {
                rec.push(0); // padding
            }
        }

        rec[0] = rec.len() as u8;
        rec
    }

    /// Writes El Torito Boot Catalog and writes synthesized EFI FAT / BIOS boot images.
    fn write_boot_catalog_and_images(&mut self, plan: &IsoLayoutPlan) -> FsInjectorResult<()> {
        if let Some(catalog_lba) = plan.boot_catalog_lba {
            let mut catalog = [0u8; ISO_SECTOR_SIZE];

            // 1. Validation Entry (offset 0..32)
            catalog[0] = 0x01; // Header ID
            catalog[1] = 0x00; // Platform x86
            catalog[4..12].copy_from_slice(b"RIM_BOOT");
            catalog[30] = 0x55;
            catalog[31] = 0xAA;

            // Checksum validation entry (sum of words == 0)
            let mut sum: u16 = 0;
            for i in 0..15 {
                let word = u16::from_le_bytes([catalog[i * 2], catalog[i * 2 + 1]]);
                sum = sum.wrapping_add(word);
            }
            let word_31 = u16::from_le_bytes([catalog[30], catalog[31]]);
            sum = sum.wrapping_add(word_31);
            let chk = (0u16).wrapping_sub(sum);
            catalog[28..30].copy_from_slice(&chk.to_le_bytes());

            // 2. Initial / Default Boot Entry (offset 32..64)
            if let Some(bios_lba) = plan.bios_boot_image_lba {
                catalog[32] = 0x88; // Bootable
                catalog[33] = 0x00; // No emulation
                let sector_count = (plan.bios_boot_image_sectors * 4)
                    .min(u16::MAX as u32)
                    .max(1) as u16; // In 512B sectors
                catalog[38..40].copy_from_slice(&sector_count.to_le_bytes());
                catalog[40..44].copy_from_slice(&bios_lba.to_le_bytes());
            } else if let Some(efi_lba) = plan.efi_boot_image_lba {
                catalog[32] = 0x88;
                catalog[33] = 0x00;
                let sector_count = (plan.efi_boot_image_sectors * 4)
                    .min(u16::MAX as u32)
                    .max(1) as u16;
                catalog[38..40].copy_from_slice(&sector_count.to_le_bytes());
                catalog[40..44].copy_from_slice(&efi_lba.to_le_bytes());
            }

            // 3. Section Header for EFI Boot Entry (offset 64..96)
            if let Some(efi_lba) = plan.efi_boot_image_lba
                && plan.bios_boot_image_lba.is_some()
            {
                catalog[64] = 0x91; // Final section header
                catalog[65] = 0xEF; // EFI Platform ID
                catalog[66..68].copy_from_slice(&1u16.to_le_bytes()); // 1 entry

                // Section Entry (offset 96..128)
                catalog[96] = 0x88; // Bootable
                catalog[97] = 0x00; // No emulation
                let sector_count = (plan.efi_boot_image_sectors * 4)
                    .min(u16::MAX as u32)
                    .max(1) as u16;
                catalog[102..104].copy_from_slice(&sector_count.to_le_bytes());
                catalog[104..108].copy_from_slice(&efi_lba.to_le_bytes());
            }

            self.io
                .write_at(catalog_lba as u64 * ISO_SECTOR_SIZE as u64, &catalog)?;
        }

        // Write EFI FAT Boot Image
        if let (Some(efi_lba), Some(efi_data)) =
            (plan.efi_boot_image_lba, plan.efi_boot_image_data.as_ref())
        {
            self.io
                .write_at(efi_lba as u64 * ISO_SECTOR_SIZE as u64, efi_data)?;
        }

        // Write BIOS Boot Image
        if let (Some(bios_lba), Some(bios_data)) =
            (plan.bios_boot_image_lba, self.meta.boot_bios.as_ref())
        {
            self.io
                .write_at(bios_lba as u64 * ISO_SECTOR_SIZE as u64, bios_data)?;
        }

        Ok(())
    }

    /// Recursively streams file payloads to their pre-allocated LBAs.
    fn stream_file_payloads(
        &mut self,
        node: &mut FsNode<'_>,
        plan: &IsoLayoutPlan,
        cur_path: &str,
    ) -> FsInjectorResult<()> {
        match node {
            FsNode::File { name, source, .. } => {
                let full_path = if cur_path.is_empty() {
                    name.clone()
                } else {
                    format!("{}/{}", cur_path, name)
                };

                if let Some(planned_file) = plan
                    .files
                    .iter()
                    .find(|f| f.path == full_path && !f.is_symlink)
                {
                    let mut buf = [0u8; 8192];
                    let mut remaining = planned_file.size;
                    let mut src_off = 0;
                    let mut dst_off = planned_file.lba as u64 * ISO_SECTOR_SIZE as u64;

                    while remaining > 0 {
                        let to_read = remaining.min(buf.len() as u64) as usize;
                        source.read_at(src_off, &mut buf[..to_read])?;
                        self.io.write_at(dst_off, &buf[..to_read])?;
                        dst_off += to_read as u64;
                        src_off += to_read as u64;
                        remaining -= to_read as u64;
                    }
                }
            }
            FsNode::Dir { name, children, .. } => {
                let sub_path = if cur_path.is_empty() {
                    name.clone()
                } else {
                    format!("{}/{}", cur_path, name)
                };
                for child in children {
                    self.stream_file_payloads(child, plan, &sub_path)?;
                }
            }
            FsNode::Container { children, .. } => {
                for child in children {
                    self.stream_file_payloads(child, plan, cur_path)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn stream_resolver_file_payloads(
        &mut self,
        resolver: &mut dyn FsTreeResolver,
        plan: &IsoLayoutPlan,
    ) -> FsInjectorResult<()> {
        for planned_file in plan.files.iter().filter(|file| !file.is_symlink) {
            let source_path =
                planned_file
                    .source_path
                    .as_deref()
                    .ok_or(FsInjectorError::Invalid(
                        "Missing resolver source path for planned ISO file",
                    ))?;
            let mut source = resolver.open_file(source_path)?;
            let mut buf = [0u8; 8192];
            let mut remaining = planned_file.size;
            let mut src_off = 0;
            let mut dst_off = planned_file.lba as u64 * ISO_SECTOR_SIZE as u64;

            while remaining > 0 {
                let to_read = remaining.min(buf.len() as u64) as usize;
                source.read_at(src_off, &mut buf[..to_read])?;
                self.io.write_at(dst_off, &buf[..to_read])?;
                dst_off += to_read as u64;
                src_off += to_read as u64;
                remaining -= to_read as u64;
            }
        }
        Ok(())
    }
}

impl<'a, IO: RimIO + ?Sized> FsTreeInjector<IsoHandle> for IsoInjector<'a, IO> {
    fn write_dir(&mut self, _name: &str, _attr: &FileAttributes) -> FsInjectorResult {
        Err(FsInjectorError::Unsupported(
            "ISO 9660 does not support individual incremental directory mutations; use inject_tree or inject_tree_from_resolver",
        ))
    }

    fn write_file(
        &mut self,
        _name: &str,
        _source: &mut dyn RimRead,
        _size: u64,
        _attr: &FileAttributes,
    ) -> FsInjectorResult {
        Err(FsInjectorError::Unsupported(
            "ISO 9660 does not support individual incremental file mutations; use inject_tree or inject_tree_from_resolver",
        ))
    }

    fn set_root_context(&mut self, _attr: &FileAttributes) -> FsInjectorResult {
        Err(FsInjectorError::Unsupported(
            "ISO 9660 does not support individual incremental root mutations; use inject_tree or inject_tree_from_resolver",
        ))
    }

    fn inject_tree(&mut self, node: &mut FsNode<'_>) -> FsInjectorResult {
        let plan = IsoLayoutPlan::build(node, self.meta)?;
        self.write_plan(&plan)?;
        self.stream_file_payloads(node, &plan, "")?;
        self.io.flush()?;
        Ok(())
    }

    fn inject_tree_from_resolver(
        &mut self,
        resolver: &mut dyn FsTreeResolver,
        path: &str,
    ) -> FsInjectorResult<FsNodeCounts> {
        let (plan, counts) = IsoLayoutPlan::build_from_resolver(resolver, path, self.meta)?;
        self.write_plan(&plan)?;
        self.stream_resolver_file_payloads(resolver, &plan)?;
        self.io.flush()?;
        Ok(counts)
    }

    fn inject_entry_from_resolver(
        &mut self,
        resolver: &mut dyn FsTreeResolver,
        path: &str,
    ) -> FsInjectorResult<FsNodeCounts> {
        self.inject_tree_from_resolver(resolver, path)
    }

    fn flush(&mut self) -> FsInjectorResult {
        self.io.flush()?;
        Ok(())
    }
}
