// SPDX-License-Identifier: MIT

//! Low-level FAT and exFAT forensic analysis and sector decoding.

use rimio::RimIO;
use rimfs::fat::types::FatCommonBpb;
use rimfs::exfat::types::ExFatBootSector;
use zerocopy::FromBytes;

pub fn analyze_fat(io: &mut dyn RimIO, partition_offset: u64) {
    let mut sector = [0u8; 512];
    if io.read_at(partition_offset, &mut sector).is_err() {
        println!("[-] Failed to read sector at offset 0x{:X}", partition_offset);
        return;
    }

    if let Ok((bpb, _)) = FatCommonBpb::ref_from_prefix(&sector) {
        let oem = String::from_utf8_lossy(&bpb.oem_name);
        let bps = bpb.bytes_per_sector.get();
        let spc = bpb.sectors_per_cluster;
        let rsvd = bpb.reserved_sectors.get();
        let num_fats = bpb.num_fats;
        let total_sec = if bpb.total_sectors_16.get() != 0 {
            bpb.total_sectors_16.get() as u64
        } else {
            bpb.total_sectors_32.get() as u64
        };

        println!("\n=== FAT Filesystem Forensic Overview ===");
        println!("  Offset:             0x{:X}", partition_offset);
        println!("  OEM Name:           {}", oem.trim());
        println!("  Bytes Per Sector:   {}", bps);
        println!("  Sectors Per Cluster:{}", spc);
        println!("  Cluster Size:       {} bytes", bps as u64 * spc as u64);
        println!("  Reserved Sectors:   {}", rsvd);
        println!("  Number of FATs:     {}", num_fats);
        println!("  Total Sectors:      {}", total_sec);
        println!("  Volume Size:        {} MB", (total_sec * bps as u64) / (1024 * 1024));

        let fat_start = partition_offset + (rsvd as u64 * bps as u64);
        println!("  FAT #1 Offset:      0x{:X}", fat_start);

        // Check for FAT32 FSInfo sector
        if rsvd >= 2 {
            let mut fsinfo = [0u8; 512];
            if io.read_at(partition_offset + bps as u64, &mut fsinfo).is_ok() {
                let lead_sig = u32::from_le_bytes([fsinfo[0], fsinfo[1], fsinfo[2], fsinfo[3]]);
                let struct_sig = u32::from_le_bytes([fsinfo[484], fsinfo[485], fsinfo[486], fsinfo[487]]);
                if lead_sig == 0x41615252 && struct_sig == 0x61417272 {
                    let free_clus = u32::from_le_bytes([fsinfo[488], fsinfo[489], fsinfo[490], fsinfo[491]]);
                    let next_free = u32::from_le_bytes([fsinfo[492], fsinfo[493], fsinfo[494], fsinfo[495]]);
                    println!("\n  --- FSInfo Sector ---");
                    println!("    Lead Signature:   0x{:08X} (Valid)", lead_sig);
                    println!("    Struct Signature: 0x{:08X} (Valid)", struct_sig);
                    println!("    Free Clusters:    {}", if free_clus == 0xFFFFFFFF { "Unknown (0xFFFFFFFF)".to_string() } else { free_clus.to_string() });
                    println!("    Next Free Cluster:{}", if next_free == 0xFFFFFFFF { "Unknown (0xFFFFFFFF)".to_string() } else { next_free.to_string() });
                }
            }
        }
    }
}

pub fn analyze_exfat(io: &mut dyn RimIO, partition_offset: u64) {
    let mut sector = [0u8; 512];
    if io.read_at(partition_offset, &mut sector).is_err() {
        println!("[-] Failed to read sector at offset 0x{:X}", partition_offset);
        return;
    }

    if let Ok((vbr, _)) = ExFatBootSector::ref_from_prefix(&sector) {
        let bps = 1u64 << vbr.bytes_per_sector_shift;
        let spc = 1u64 << vbr.sectors_per_cluster_shift;
        let vol_len = vbr.volume_length.get();
        let fat_off = vbr.fat_offset.get();
        let fat_len = vbr.fat_length.get();
        let heap_off = vbr.cluster_heap_offset.get();
        let clus_cnt = vbr.cluster_count.get();
        let root_dir = vbr.root_dir_cluster.get();

        println!("\n=== exFAT Filesystem Forensic Overview ===");
        println!("  Offset:             0x{:X}", partition_offset);
        println!("  FS Name:            EXFAT");
        println!("  Bytes Per Sector:   {} (shift: {})", bps, vbr.bytes_per_sector_shift);
        println!("  Sectors Per Cluster:{} (shift: {})", spc, vbr.sectors_per_cluster_shift);
        println!("  Cluster Size:       {} bytes", bps * spc);
        println!("  Volume Length:      {} sectors ({} MB)", vol_len, (vol_len * bps) / (1024 * 1024));
        println!("  FAT Offset:         0x{:X} (Sector {})", partition_offset + fat_off as u64 * bps, fat_off);
        println!("  FAT Length:         {} sectors", fat_len);
        println!("  Cluster Heap:       0x{:X} (Sector {})", partition_offset + heap_off as u64 * bps, heap_off);
        println!("  Cluster Count:      {}", clus_cnt);
        println!("  Root Dir Cluster:   {}", root_dir);
        println!("  Volume Serial:      0x{:08X}", vbr.volume_serial.get());
        println!("  FS Revision:        0x{:04X}", vbr.fs_revision.get());
    }
}
