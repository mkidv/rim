use rimfs::ntfs::types::NtfsBootSector;
use rimfs::ntfs::utils::apply_usa_fixup;
use rimfs::ntfs::view::attr_view::{AttrRef, AttrView};
use rimfs::ntfs::view::mft_view::MftRecordView;
use rimio::prelude::*;

use zerocopy::FromBytes;

pub struct NtfsMetaInfo {
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub mft_lcn: u64,
    pub mft_mirr_lcn: u64,
    pub total_sectors: u64,
    pub mft_record_size: u64,
    pub index_record_size: u64,
}

pub fn get_ntfs_meta(vbr: &NtfsBootSector) -> NtfsMetaInfo {
    let bytes_per_sector = vbr.bytes_per_sector;
    let sectors_per_cluster = vbr.sectors_per_cluster;
    let bytes_per_cluster = bytes_per_sector as u64 * sectors_per_cluster as u64;
    let mft_record_size = if vbr.clusters_per_mft_record > 0 {
        vbr.clusters_per_mft_record as u64 * bytes_per_cluster
    } else {
        1u64 << (-vbr.clusters_per_mft_record as u64)
    };
    let index_record_size = if vbr.clusters_per_index_record > 0 {
        vbr.clusters_per_index_record as u64 * bytes_per_cluster
    } else {
        1u64 << (-vbr.clusters_per_index_record as u64)
    };

    NtfsMetaInfo {
        bytes_per_sector,
        sectors_per_cluster,
        mft_lcn: vbr.mft_lcn,
        mft_mirr_lcn: vbr.mft_mirr_lcn,
        total_sectors: vbr.total_sectors,
        mft_record_size,
        index_record_size,
    }
}

pub fn analyze_ntfs(io: &mut dyn RimIO, partition_offset: u64) {
    let mut buffer = [0u8; 512];
    if io.read_at(partition_offset, &mut buffer).is_err() {
        return;
    }

    let vbr = match NtfsBootSector::read_from_bytes(&buffer) {
        Ok(vbr) => vbr,
        Err(e) => {
            println!("Failed to parse NtfsBootSector: {:?}", e);
            return;
        }
    };

    let meta = get_ntfs_meta(&vbr);
    let bytes_per_cluster = meta.bytes_per_sector as u64 * meta.sectors_per_cluster as u64;
    let mft_offset = partition_offset + (meta.mft_lcn * bytes_per_cluster);
    let mft_mirr_offset = partition_offset + (meta.mft_mirr_lcn * bytes_per_cluster);

    let hidden_sectors = vbr.hidden_sectors;
    let volume_serial = vbr.volume_serial;

    println!("\n=== NTFS BOOT SECTOR (VBR) ===");
    println!("OEM ID:           {}", String::from_utf8_lossy(&vbr.oem_id));
    println!("Bytes per Sector: {}", meta.bytes_per_sector);
    println!("Sectors/Cluster:  {}", meta.sectors_per_cluster);
    println!("Total Sectors:    {}", meta.total_sectors);
    println!("Hidden Sectors:   {}", hidden_sectors);
    println!("MFT LCN:          {}", meta.mft_lcn);
    println!("MFT Mirr LCN:     {}", meta.mft_mirr_lcn);
    println!("MFT Record Size:  {} bytes", meta.mft_record_size);
    println!("Volume Serial:    0x{:016X}", volume_serial);

    println!("\nMFT starts at offset:      0x{:016X}", mft_offset);
    println!("MFT Mirr starts at offset: 0x{:016X}", mft_mirr_offset);

    let mut mft_buf = vec![0u8; meta.mft_record_size as usize];
    if io.read_at(mft_offset, &mut mft_buf).is_ok() {
        println!("\n=== MFT RECORD 0 ($MFT) ===");
        common_analyze_mft(&mft_buf, meta.mft_record_size as usize);
    }
}

pub fn dump_mft_record(io: &mut dyn RimIO, partition_offset: u64, record_num: u64) {
    let mut buffer = [0u8; 512];
    if io.read_at(partition_offset, &mut buffer).is_err() {
        return;
    }

    let vbr = match NtfsBootSector::read_from_prefix(&buffer) {
        Ok((vbr, _)) => vbr,
        Err(e) => {
            println!(
                "Failed to parse NtfsBootSector at 0x{:X}: {:?}",
                partition_offset, e
            );
            return;
        }
    };

    let meta = get_ntfs_meta(&vbr);
    let bytes_per_cluster = meta.bytes_per_sector as u64 * meta.sectors_per_cluster as u64;
    let mft_offset = partition_offset + (meta.mft_lcn * bytes_per_cluster);

    println!("\n=== MFT RECORD {} ===", record_num);
    let mut mft_buf = vec![0u8; meta.mft_record_size as usize];
    if io
        .read_at(
            mft_offset + (record_num * meta.mft_record_size),
            &mut mft_buf,
        )
        .is_ok()
    {
        common_analyze_mft(&mft_buf, meta.mft_record_size as usize);
    }
}

pub fn common_analyze_mft(mft_buf: &[u8], mft_record_size: usize) {
    let sig = &mft_buf[0..4];
    println!("Signature: {}", String::from_utf8_lossy(sig));

    let attrs_offset = u16::from_le_bytes([mft_buf[20], mft_buf[21]]) as usize;
    let bytes_used =
        u32::from_le_bytes([mft_buf[24], mft_buf[25], mft_buf[26], mft_buf[27]]) as usize;

    println!("Attributes Offset: {}", attrs_offset);
    println!("Bytes Used: {}", bytes_used);

    let mut offset = attrs_offset;
    while offset + 8 <= bytes_used && offset + 8 <= mft_record_size {
        let attr_type = u32::from_le_bytes([
            mft_buf[offset],
            mft_buf[offset + 1],
            mft_buf[offset + 2],
            mft_buf[offset + 3],
        ]);
        if attr_type == 0xFFFFFFFF {
            println!("  [0x{:04X}] End Marker", offset);
            break;
        }
        let attr_len = u32::from_le_bytes([
            mft_buf[offset + 4],
            mft_buf[offset + 5],
            mft_buf[offset + 6],
            mft_buf[offset + 7],
        ]) as usize;
        let non_resident = mft_buf[offset + 8];

        println!(
            "  [0x{:04X}] Type: 0x{:02X} ({}), Len: {}, Resident: {}",
            offset,
            attr_type,
            get_attr_name(attr_type),
            attr_len,
            non_resident == 0
        );

        if attr_len == 0 {
            break;
        }
        offset += attr_len;
    }

    println!("\n--- Raw Hex Dump ---");
    for i in (0..mft_record_size).step_by(16) {
        print!("{:04X}: ", i);
        for j in 0..16 {
            print!("{:02X} ", mft_buf[i + j]);
        }
        print!(" | ");
        for j in 0..16 {
            let c = mft_buf[i + j];
            if (32..=126).contains(&c) {
                print!("{}", c as char);
            } else {
                print!(".");
            }
        }
        println!();
    }
}

pub fn get_attr_name(id: u32) -> &'static str {
    match id {
        0x10 => "$STANDARD_INFORMATION",
        0x20 => "$ATTRIBUTE_LIST",
        0x30 => "$FILE_NAME",
        0x40 => "$OBJECT_ID",
        0x50 => "$SECURITY_DESCRIPTOR",
        0x60 => "$VOLUME_NAME",
        0x70 => "$VOLUME_INFORMATION",
        0x80 => "$DATA",
        0x90 => "$INDEX_ROOT",
        0xA0 => "$INDEX_ALLOCATION",
        0xB0 => "$BITMAP",
        0xC0 => "$REPARSE_POINT",
        0xD0 => "$EA_INFORMATION",
        0xE0 => "$EA",
        _ => "Unknown",
    }
}

pub fn run_check_ntfs(io: &mut dyn RimIO, off: u64) {
    println!("\n=== NTFS HEALTH CHECK ===");
    let mut b = [0u8; 512];
    if io.read_at(off, &mut b).is_err() {
        println!("[!] Failed to read VBR");
        return;
    }

    let vbr = match NtfsBootSector::read_from_bytes(&b) {
        Ok(vbr) => vbr,
        Err(_) => {
            println!("[!] VBR is corrupt or invalid");
            return;
        }
    };
    println!("[✓] VBR Signature valid");

    let meta = get_ntfs_meta(&vbr);
    let bpc = meta.bytes_per_sector as u64 * meta.sectors_per_cluster as u64;

    // Check Backup VBR
    let backup_off = off + (meta.total_sectors * meta.bytes_per_sector as u64);
    let mut b_back = [0u8; 512];
    if io.read_at(backup_off, &mut b_back).is_ok() {
        if b_back[..3] == b[..3] && b_back[510..512] == [0x55, 0xAA] {
            println!("[✓] Backup VBR matches and is valid");
        } else {
            println!(
                "[!] Backup VBR mismatch or invalid signature at offset 0x{:X}",
                backup_off
            );
        }
    } else {
        println!("[!] Failed to read Backup VBR at offset 0x{:X}", backup_off);
    }

    // Check MFT Mirr
    let mft_off = off + (meta.mft_lcn * bpc);
    let mirr_off = off + (meta.mft_mirr_lcn * bpc);
    let mut mft0 = vec![0u8; meta.mft_record_size as usize];
    let mut mirr0 = vec![0u8; meta.mft_record_size as usize];

    if io.read_at(mft_off, &mut mft0).is_ok() {
        println!("[✓] $MFT Record 0 is readable");
        if &mft0[..4] == b"FILE" {
            if io.read_at(mirr_off, &mut mirr0).is_ok() {
                if mft0[..512] == mirr0[..512] {
                    println!("[✓] $MFTMirr (Sector 0) matches $MFT Record 0");
                } else {
                    println!("[!] $MFTMirr differs from $MFT Record 0!");
                }
            } else {
                println!("[!] Failed to read $MFTMirr at offset 0x{:X}", mirr_off);
            }
        }
    }

    // Check $Secure (Record 9)
    let sec_off = mft_off + (9 * meta.mft_record_size);
    let mut sec = vec![0u8; meta.mft_record_size as usize];
    if io.read_at(sec_off, &mut sec).is_ok() {
        if &sec[..4] == b"FILE" {
            println!("[✓] $Secure Record 9 is present");
            let mut found_sds = false;
            let mut offset = u16::from_le_bytes([sec[20], sec[21]]) as usize;
            let used = u32::from_le_bytes([sec[24], sec[25], sec[26], sec[27]]) as usize;
            while offset + 16 <= used && offset + 16 <= meta.mft_record_size as usize {
                let attr_type = u32::from_le_bytes([
                    sec[offset],
                    sec[offset + 1],
                    sec[offset + 2],
                    sec[offset + 3],
                ]);
                if attr_type == 0xFFFFFFFF {
                    break;
                }
                let attr_len = u32::from_le_bytes([
                    sec[offset + 4],
                    sec[offset + 5],
                    sec[offset + 6],
                    sec[offset + 7],
                ]) as usize;

                if attr_type == 0x80 {
                    let name_len = sec[offset + 9];
                    let name_off =
                        u16::from_le_bytes([sec[offset + 10], sec[offset + 11]]) as usize;
                    if name_len == 4 && offset + name_off + 8 <= sec.len() {
                        let name = &sec[offset + name_off..offset + name_off + 8];
                        if name == b"$\x00S\x00D\x00S\x00" {
                            found_sds = true;
                            let val_len = u32::from_le_bytes([
                                sec[offset + 16],
                                sec[offset + 17],
                                sec[offset + 18],
                                sec[offset + 19],
                            ]);
                            if val_len == 0 {
                                println!(
                                    "[!] WARNING: $Secure:$SDS attribute is EMPTY. Windows will likely reject this!"
                                );
                            } else {
                                println!("[✓] $Secure:$SDS attribute has data ({} bytes)", val_len);
                            }
                        }
                    }
                }
                if attr_len == 0 {
                    break;
                }
                offset += attr_len;
            }
            if !found_sds {
                println!("[!] $Secure is missing the mandatory $SDS attribute!");
            }
        } else {
            println!(
                "[!] $Secure Record 9 signature is invalid (Found: {})",
                String::from_utf8_lossy(&sec[..4])
            );
        }
    }

    println!("\nSuggested Fixes for Windows Recognition:");
    println!("1. Ensure $Secure:$SDS isn't empty (needs at least one valid SD).");
    println!("2. Verify that $LogFile (Record 2) is initialized/reset.");
    println!(
        "3. Check if all mandatory system files ($UpCase, $Volume, $AttrDef) are correctly populated."
    );
}

pub fn run_diff_ntfs_meta(io1: &mut dyn RimIO, io2: &mut dyn RimIO, off1: u64, off2: u64) {
    let mut b1 = [0u8; 512];
    let mut b2 = [0u8; 512];
    io1.read_at(off1, &mut b1).unwrap();
    io2.read_at(off2, &mut b2).unwrap();

    let vbr1 = NtfsBootSector::read_from_bytes(&b1).unwrap();
    let vbr2 = NtfsBootSector::read_from_bytes(&b2).unwrap();
    let meta1 = get_ntfs_meta(&vbr1);
    let meta2 = get_ntfs_meta(&vbr2);

    println!("\n=== NTFS METADATA COMPARISON ===");
    println!("{:<25} | {:<20} | {:<20}", "Field", "VHD 1", "VHD 2");
    println!("{:-<25}-|-{:-<20}-|-{:-<20}", "", "", "");

    let diff_mark = |a: u64, b: u64| if a != b { "[DIFF]" } else { "" };
    println!(
        "{:<25} | {:<20} | {:<20} {}",
        "MFT LCN",
        meta1.mft_lcn,
        meta2.mft_lcn,
        diff_mark(meta1.mft_lcn, meta2.mft_lcn)
    );
    println!(
        "{:<25} | {:<20} | {:<20} {}",
        "MFT Mirr LCN",
        meta1.mft_mirr_lcn,
        meta2.mft_mirr_lcn,
        diff_mark(meta1.mft_mirr_lcn, meta2.mft_mirr_lcn)
    );
    let vs1 = vbr1.volume_serial;
    let vs2 = vbr2.volume_serial;
    println!(
        "{:<25} | {:<20} | {:<20} {}",
        "Volume Serial",
        format!("0x{:X}", vs1),
        format!("0x{:X}", vs2),
        if vs1 != vs2 { "[DIFF]" } else { "" }
    );
    println!(
        "{:<25} | {:<20} | {:<20} {}",
        "Total Sectors",
        meta1.total_sectors,
        meta2.total_sectors,
        diff_mark(meta1.total_sectors, meta2.total_sectors)
    );

    println!("\n--- MFT Record 0 ($MFT) Comparison ---");
    compare_mft_record(io1, io2, off1, off2, &meta1, &meta2, 0);
    println!("\n--- MFT Record 3 ($Volume) Comparison ---");
    compare_mft_record(io1, io2, off1, off2, &meta1, &meta2, 3);
}

fn compare_mft_record(
    io1: &mut dyn RimIO,
    io2: &mut dyn RimIO,
    off1: u64,
    off2: u64,
    meta1: &NtfsMetaInfo,
    meta2: &NtfsMetaInfo,
    record_num: u64,
) {
    let bpc1 = meta1.bytes_per_sector as u64 * meta1.sectors_per_cluster as u64;
    let bpc2 = meta2.bytes_per_sector as u64 * meta2.sectors_per_cluster as u64;

    let mft_off1 = off1 + (meta1.mft_lcn * bpc1) + (record_num * meta1.mft_record_size);
    let mft_off2 = off2 + (meta2.mft_lcn * bpc2) + (record_num * meta2.mft_record_size);

    let mut buf1 = vec![0u8; meta1.mft_record_size as usize];
    let mut buf2 = vec![0u8; meta2.mft_record_size as usize];

    io1.read_at(mft_off1, &mut buf1).unwrap();
    io2.read_at(mft_off2, &mut buf2).unwrap();

    let used1 = u32::from_le_bytes([buf1[24], buf1[25], buf1[26], buf1[27]]);
    let used2 = u32::from_le_bytes([buf2[24], buf2[25], buf2[26], buf2[27]]);

    println!(
        "{:<25} | {:<20} | {:<20} {}",
        "Bytes Used",
        used1,
        used2,
        if used1 != used2 { "[DIFF]" } else { "" }
    );
    if buf1[..4] != buf2[..4] {
        println!(
            "Signature mismatch: {} vs {}",
            String::from_utf8_lossy(&buf1[..4]),
            String::from_utf8_lossy(&buf2[..4])
        );
    }
}

pub fn dump_secure_table(io: &mut dyn RimIO, partition_offset: u64) {
    let mut buffer = [0u8; 512];
    if io.read_at(partition_offset, &mut buffer).is_err() {
        return;
    }
    let (vbr, _) = NtfsBootSector::read_from_prefix(&buffer).unwrap();
    let meta = get_ntfs_meta(&vbr);
    let bytes_per_cluster = meta.bytes_per_sector as u64 * meta.sectors_per_cluster as u64;
    let mft_offset = partition_offset + (meta.mft_lcn * bytes_per_cluster);

    let mut sec_buf = vec![0u8; meta.mft_record_size as usize];
    io.read_at(mft_offset + (9 * meta.mft_record_size), &mut sec_buf)
        .unwrap();

    let view = MftRecordView::new(&sec_buf).unwrap();
    println!("\n=== $SECURE TABLE (MFT Record 9) ===\n");

    for attr_res in view.attrs() {
        let attr = attr_res.unwrap();
        let ty = attr.ty();
        let mut name = String::new();
        if attr.header.name_length > 0 {
            let n_off = attr.header.name_offset as usize;
            let n_len = attr.header.name_length as usize * 2;
            let name_u16: Vec<u16> = attr.raw[n_off..n_off + n_len]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            name = String::from_utf16_lossy(&name_u16);
        }

        println!(
            "[Attribute 0x{:X}] Type: 0x{:X} ({}) Name: '{}' Non-Resident: {}",
            (attr.raw.as_ptr() as usize - sec_buf.as_ptr() as usize),
            ty,
            get_attr_name(ty),
            name,
            !attr.is_resident()
        );

        let attr_view = attr.as_view().unwrap();
        if ty == 0x80 && name == "$SDS" {
            let data = match attr_view {
                AttrView::Resident { value, .. } => value.to_vec(),
                AttrView::NonResident { .. } => {
                    read_attr_content(io, &attr, &meta, partition_offset)
                }
            };
            dump_sds_data(&data);
        } else if ty == 0x90 {
            if let AttrView::Resident { value, .. } = attr_view {
                if name == "$SII" {
                    dump_index_root_security(value, "SecurityIdIndex (SII)");
                } else if name == "$SDH" {
                    dump_index_root_security(value, "SecurityDescriptorHash (SDH)");
                }
            }
        } else if ty == 0xA0 {
            let data = read_attr_content(io, &attr, &meta, partition_offset);
            if name == "$SII" {
                dump_index_allocation_security(&data, "SecurityIdIndex (SII) Allocation", &meta);
            } else if name == "$SDH" {
                dump_index_allocation_security(
                    &data,
                    "SecurityDescriptorHash (SDH) Allocation",
                    &meta,
                );
            }
        }
    }
}

fn read_attr_content(
    io: &mut dyn RimIO,
    attr: &AttrRef,
    meta: &NtfsMetaInfo,
    partition_offset: u64,
) -> Vec<u8> {
    let view = attr.as_view().unwrap();
    match view {
        AttrView::Resident { value, .. } => value.to_vec(),
        AttrView::NonResident {
            runlist, data_size, ..
        } => {
            let mut content = Vec::with_capacity(data_size as usize);
            let bytes_per_cluster = meta.bytes_per_sector as u64 * meta.sectors_per_cluster as u64;
            for run in runlist.iter() {
                let size = (run.len * bytes_per_cluster) as usize;
                if let Some(lcn) = run.lcn {
                    let mut buf = vec![0u8; size];
                    io.read_at(partition_offset + (lcn * bytes_per_cluster), &mut buf)
                        .ok();
                    content.extend_from_slice(&buf);
                } else {
                    content.resize(content.len() + size, 0);
                }
            }
            content.truncate(data_size as usize);
            content
        }
    }
}

fn dump_sds_data(data: &[u8]) {
    println!("--- $SDS Content ({} bytes) ---", data.len());
    let mut pos = 0;
    while pos + 20 <= data.len() {
        let hash = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        let id = u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]);
        let offset = u64::from_le_bytes([
            data[pos + 8],
            data[pos + 9],
            data[pos + 10],
            data[pos + 11],
            data[pos + 12],
            data[pos + 13],
            data[pos + 14],
            data[pos + 15],
        ]);
        let length = u32::from_le_bytes([
            data[pos + 16],
            data[pos + 17],
            data[pos + 18],
            data[pos + 19],
        ]);
        if length == 0 {
            break;
        }
        println!(
            "  Entry at SDS Offset 0x{:X}: Hash=0x{:08X}, Id={}, Offset=0x{:X}, Length={}",
            pos, hash, id, offset, length
        );
        pos = (pos + length as usize + 15) & !15;
    }
}

fn dump_index_root_security(value_buf: &[u8], label: &str) {
    println!("--- {} ---", label);
    if value_buf.len() < 32 {
        return;
    }
    let entries_offset =
        u32::from_le_bytes([value_buf[16], value_buf[17], value_buf[18], value_buf[19]]) as usize;
    let index_length =
        u32::from_le_bytes([value_buf[20], value_buf[21], value_buf[22], value_buf[23]]) as usize;
    let mut pos = 16 + entries_offset;
    let end = 16 + index_length;
    while pos + 16 <= end && pos < value_buf.len() {
        let entry_len = u16::from_le_bytes([value_buf[pos + 8], value_buf[pos + 9]]) as usize;
        let content_len = u16::from_le_bytes([value_buf[pos + 10], value_buf[pos + 11]]) as usize;
        let flags = value_buf[pos + 12];
        if flags & 0x02 != 0 {
            break;
        }
        println!(
            "  Entry: Len: {}, Content Len: {}, Flags: 0x{:02X}",
            entry_len, content_len, flags
        );
        if entry_len == 0 {
            break;
        }
        pos += entry_len;
    }
}

fn dump_index_allocation_security(data: &[u8], label: &str, meta: &NtfsMetaInfo) {
    println!("--- {} ---", label);
    for (i, block) in data
        .chunks_exact(meta.index_record_size as usize)
        .enumerate()
    {
        if &block[0..4] != b"INDX" {
            continue;
        }
        println!("\n  [Block {}]", i);
        let mut fixed_block = block.to_vec();
        apply_usa_fixup(&mut fixed_block, meta.bytes_per_sector as usize);
        let node_header_off = (u16::from_le_bytes([fixed_block[4], fixed_block[5]]) as usize
            + u16::from_le_bytes([fixed_block[6], fixed_block[7]]) as usize * 2
            + 7)
            & !7;
        let entries_offset = u32::from_le_bytes([
            fixed_block[node_header_off],
            fixed_block[node_header_off + 1],
            fixed_block[node_header_off + 2],
            fixed_block[node_header_off + 3],
        ]) as usize;
        let index_length = u32::from_le_bytes([
            fixed_block[node_header_off + 4],
            fixed_block[node_header_off + 5],
            fixed_block[node_header_off + 6],
            fixed_block[node_header_off + 7],
        ]) as usize;
        let mut pos = node_header_off + entries_offset;
        let end = node_header_off + index_length;
        while pos + 16 <= end && pos < fixed_block.len() {
            let entry_len =
                u16::from_le_bytes([fixed_block[pos + 8], fixed_block[pos + 9]]) as usize;
            let flags = fixed_block[pos + 12];
            if flags & 0x02 != 0 {
                break;
            }
            println!(
                "    Entry at 0x{:X}: Len: {}, Flags: 0x{:02X}",
                pos, entry_len, flags
            );
            if entry_len == 0 {
                break;
            }
            pos += entry_len;
        }
    }
}

pub fn dump_ntfs_layout(io: &mut dyn RimIO, partition_offset: u64) {
    let mut buffer = [0u8; 512];
    if io.read_at(partition_offset, &mut buffer).is_err() {
        return;
    }
    let vbr = NtfsBootSector::read_from_bytes(&buffer).unwrap();
    let meta = get_ntfs_meta(&vbr);
    let bytes_per_cluster = meta.bytes_per_sector as u64 * meta.sectors_per_cluster as u64;
    let mft_offset = partition_offset + (meta.mft_lcn * bytes_per_cluster);

    println!("=== NTFS FULL LAYOUT DUMP ===");
    println!("Partition Offset: 0x{:X}", partition_offset);
    println!("MFT Offset:       0x{:X}", mft_offset);

    for i in 0..12 {
        println!("\n--- SYSTEM RECORD {} ---", i);
        let mut mft_buf = vec![0u8; meta.mft_record_size as usize];
        if io
            .read_at(mft_offset + i * meta.mft_record_size, &mut mft_buf)
            .is_ok()
        {
            common_analyze_mft(&mft_buf, meta.mft_record_size as usize);
        }
    }
}
