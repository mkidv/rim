// SPDX-License-Identifier: MIT

use byteorder::{LittleEndian, WriteBytesExt};
use crc32fast::Hasher;
use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const BLOCK_SIZE: u32 = 1024;
const INODE_SIZE: u16 = 256;
const INODES_PER_GROUP: u32 = 128;
const BLOCKS_PER_GROUP: u32 = 2048;
const EXTENTS_FL: u32 = 0x80000;
const ROOT_INODE: u32 = 2;
const LOST_FOUND_INODE: u32 = 3;

const SUPERBLOCK_OFFSET: u64 = 1024;
const BGDT_OFFSET: u64 = 2048;
const INODE_TABLE_OFFSET: u64 = 5120;
const ROOT_DIR_BLOCK_OFFSET: u64 = 21504;

fn now() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as u32
}

pub struct BlockGroup {
    pub block_bitmap: u32,
    pub inode_bitmap: u32,
    pub inode_table: u32,
    pub used_dirs_count: u16,
    pub free_blocks_count: u16,
    pub free_inodes_count: u16,
}

enum InodeType {
    File,
    Directory,
}

impl InodeType {
    fn mode(&self) -> u16 {
        match self {
            InodeType::File => 0x81A4,      // rw-r--r--
            InodeType::Directory => 0x41ED, // rwxr-xr-x
        }
    }
}

pub struct InodeAllocator {
    pub next_inode: u32,
}

impl InodeAllocator {
    pub fn new() -> Self {
        Self { next_inode: 4 } // 1 (unused), 2 (root), 3 (lost+found)
    }

    pub fn allocate(&mut self) -> u32 {
        let inode = self.next_inode;
        self.next_inode += 1;
        inode
    }
}

pub struct Ext4Formatter<'a, W: Write + Seek> {
    pub out: &'a mut W,
    pub offset: u64,
    pub size_bytes: u64,
    pub label: Option<String>,
    pub uuid: [u8; 16],
    pub inode_allocator: InodeAllocator,
    pub next_block: u32,
    pub block_groups: Vec<BlockGroup>,
    pub group_count: u32,
}

impl<'a, W: Write + Seek> Ext4Formatter<'a, W> {
    pub fn new(out: &'a mut W, offset: u64, size_bytes: u64, label: Option<String>) -> Self {
        let uuid = *Uuid::new_v4().as_bytes();
        let group_count = (size_bytes as u32 / (BLOCKS_PER_GROUP * BLOCK_SIZE)).max(1);
        Self {
            out,
            offset,
            size_bytes,
            label,
            uuid,
            inode_allocator: InodeAllocator::new(),
            next_block: 10,
            block_groups: vec![],
            group_count,
        }
    }

    fn allocate_block(&mut self) -> anyhow::Result<u32>  {
        for group in &mut self.block_groups {
            if group.free_blocks_count > 0 {
                let block = self.next_block;
                group.free_blocks_count -= 1;
                self.next_block += 1;
                return Ok(block);
            }
        }
        return Err(anyhow::anyhow!("No free blocks left!"));
    }

    fn allocate_blocks(&mut self, count: u32) -> anyhow::Result<u32>  {
        for group in &mut self.block_groups {
            if group.free_blocks_count as u32 >= count {
                let block = self.next_block;
                group.free_blocks_count -= count as u16;
                self.next_block += count;
                return Ok(block);
            }
        }
        return Err(anyhow::anyhow!("No free blocks for {} blocks", count));
    }

    fn allocate_inode(&mut self) -> anyhow::Result<u32> {
        for group in &mut self.block_groups {
            if group.free_inodes_count > 0 {
                let inode = self.inode_allocator.allocate();
                group.free_inodes_count -= 1;
                return Ok(inode);
            }
        }
        return Err(anyhow::anyhow!("No free inodes left!"));
    }

    pub fn format(&mut self) -> anyhow::Result<()> {
        self.write_superblock()?;
        for group_index in 0..self.group_count {
            self.write_block_group_descriptor(group_index)?;
        }
        self.write_bitmaps()?;
        self.write_inode_table()?;
        self.write_root_inode()?;
        self.write_lost_found()?;
        Ok(())
    }

    fn write_superblock(&mut self) -> anyhow::Result<()> {
        let sb_offset = self.offset + SUPERBLOCK_OFFSET;
        self.out.seek(SeekFrom::Start(sb_offset))?;
        let label = self.label.as_deref().unwrap_or("EXT4RIM");

        let mut sb = vec![0u8; 1024];
        {
            let mut w = &mut sb[..];
            w.write_u32::<LittleEndian>(INODES_PER_GROUP * self.group_count)?;
            w.write_u32::<LittleEndian>(BLOCKS_PER_GROUP * self.group_count)?;
            w.write_u32::<LittleEndian>((BLOCKS_PER_GROUP * self.group_count) - 10)?;
            w.write_u32::<LittleEndian>((INODES_PER_GROUP * self.group_count) - 2)?;
            w.write_u32::<LittleEndian>(1)?;
            w.write_u32::<LittleEndian>(now())?;
            w.write_u32::<LittleEndian>(now())?;
            w.write_u32::<LittleEndian>(8192)?;
            w.write_u32::<LittleEndian>(8192)?;
            w.write_u32::<LittleEndian>(INODES_PER_GROUP)?;
            w.write_u32::<LittleEndian>(0)?;
            w.write_u32::<LittleEndian>(0)?;
            w.write_u16::<LittleEndian>(0)?;
            w.write_u16::<LittleEndian>(0xffff)?;
            w.write_u16::<LittleEndian>(0xEF53)?;
            w.write_u16::<LittleEndian>(1)?;
            w.write_u32::<LittleEndian>(ROOT_INODE)?;
            w.write_u16::<LittleEndian>(INODE_SIZE)?;
            w.write_u16::<LittleEndian>(0)?;
            w.write_u32::<LittleEndian>(0x20)?;
            w.write_u32::<LittleEndian>(0x4)?;
            w.write_u32::<LittleEndian>(0x1)?;
            w.write_all(&self.uuid)?;
            w.write_u32::<LittleEndian>(1)?;
            w.write_u32::<LittleEndian>(u32::from_le_bytes(self.uuid[..4].try_into().unwrap()))?;
            w.write_u16::<LittleEndian>(0x0001)?;
            w.write_u16::<LittleEndian>(0x0001)?;

            let mut label_bytes = [0u8; 16];
            label_bytes[..label.len().min(16)]
                .copy_from_slice(&label.as_bytes()[..label.len().min(16)]);
            w.write_all(&label_bytes)?;
        }

        let mut hasher = Hasher::new();
        hasher.update(&sb);
        let checksum = hasher.finalize();
        sb[0xFC..0x100].copy_from_slice(&checksum.to_le_bytes());

        self.out.write_all(&sb)?;
        Ok(())
    }

    fn write_block_group_descriptor(&mut self, group_index: u32) -> anyhow::Result<()> {
        let offset = self.offset + BGDT_OFFSET + (group_index as u64 * 1024);
        self.out.seek(SeekFrom::Start(offset))?;
        let mut bgdt = vec![0u8; 1024];

        let block_bitmap = 3 + group_index * BLOCKS_PER_GROUP;
        let inode_bitmap = 4 + group_index * BLOCKS_PER_GROUP;
        let inode_table = 5 + group_index * BLOCKS_PER_GROUP;

        {
            let mut w = &mut bgdt[..];
            w.write_u32::<LittleEndian>(block_bitmap)?;
            w.write_u32::<LittleEndian>(inode_bitmap)?;
            w.write_u32::<LittleEndian>(inode_table)?;
            w.write_u16::<LittleEndian>((INODES_PER_GROUP - 1) as u16)?;
            w.write_u16::<LittleEndian>((BLOCKS_PER_GROUP - 10) as u16)?;
            w.write_u16::<LittleEndian>(1)?;
            w.write_all(&self.uuid)?;
        }

        let mut hasher = Hasher::new();
        hasher.update(&bgdt);
        let checksum = hasher.finalize();
        bgdt[1020..1024].copy_from_slice(&checksum.to_le_bytes());

        self.out.write_all(&bgdt)?;
        self.block_groups.push(BlockGroup {
            block_bitmap,
            inode_bitmap,
            inode_table,
            used_dirs_count: 1,
            free_blocks_count: (BLOCKS_PER_GROUP - 10) as u16,
            free_inodes_count: (INODES_PER_GROUP - 2) as u16,
        });
        Ok(())
    }

    fn write_bitmaps(&mut self) -> anyhow::Result<()> {
        for group in &self.block_groups {
            let block_bitmap_offset = self.offset + (group.block_bitmap * BLOCK_SIZE) as u64;
            self.out.seek(SeekFrom::Start(block_bitmap_offset))?;
            self.out.write_all(&[0b00000111])?;
            self.out.write_all(&vec![0u8; 1023])?;

            let inode_bitmap_offset = self.offset + (group.inode_bitmap * BLOCK_SIZE) as u64;
            self.out.seek(SeekFrom::Start(inode_bitmap_offset))?;
            self.out.write_all(&[0b00000111])?;
            self.out.write_all(&vec![0u8; 1023])?;
        }
        Ok(())
    }

    fn write_inode_table(&mut self) -> anyhow::Result<()> {
        self.out
            .seek(SeekFrom::Start(self.offset + INODE_TABLE_OFFSET))?;
        self.out.write_all(&vec![
            0u8;
            (INODES_PER_GROUP as usize) * (INODE_SIZE as usize)
        ])?;
        Ok(())
    }

    fn write_inode_metadata(&mut self, uid: u16, gid: u16) -> anyhow::Result<()> {
        let t = now();
        self.out.write_u32::<LittleEndian>(t)?;
        self.out.write_u32::<LittleEndian>(t)?;
        self.out.write_u32::<LittleEndian>(t)?;
        self.out.write_u32::<LittleEndian>(0)?;
        self.out.write_u16::<LittleEndian>(uid)?;
        self.out.write_u16::<LittleEndian>(gid)?;
        Ok(())
    }

    fn write_inode_header(
        &mut self,
        inode_number: u32,
        block: u32,
        size: u32,
        kind: InodeType,
    ) -> anyhow::Result<()> {
        let offset =
            self.offset + INODE_TABLE_OFFSET + ((inode_number - 1) * INODE_SIZE as u32) as u64;
        self.out.seek(SeekFrom::Start(offset))?;
        self.out.write_u16::<LittleEndian>(kind.mode())?;
        self.write_inode_metadata(0, 0)?;
        self.out.write_u16::<LittleEndian>(0)?;
        self.out.write_u32::<LittleEndian>(size)?;
        self.out.write_u32::<LittleEndian>(0)?;
        self.out.write_u32::<LittleEndian>(0)?;
        self.out.write_u32::<LittleEndian>(0)?;
        self.out.write_u32::<LittleEndian>(0)?;
        self.out.write_u16::<LittleEndian>(0)?;
        self.out.write_u16::<LittleEndian>(2)?;
        self.out.write_u32::<LittleEndian>(1)?;
        self.out.write_u32::<LittleEndian>(EXTENTS_FL)?;
        self.out.write_u32::<LittleEndian>(0)?;

        let mut block_data = vec![0u8; 60];
        {
            let mut b = &mut block_data[..];
            b.write_u16::<LittleEndian>(0xF30A)?;
            b.write_u16::<LittleEndian>(1)?;
            b.write_u16::<LittleEndian>(4)?;
            b.write_u16::<LittleEndian>(0)?;
            b.write_u32::<LittleEndian>(0)?;
            b.write_u32::<LittleEndian>(0)?;
            b.write_u16::<LittleEndian>(1)?;
            b.write_u16::<LittleEndian>(0)?;
            b.write_u32::<LittleEndian>(block)?;
        }
        self.out.write_all(&block_data)?;
        self.out
            .write_all(&vec![0u8; INODE_SIZE as usize - 40 - 60])?;
        Ok(())
    }

    fn write_root_inode(&mut self) -> anyhow::Result<()> {
        self.write_inode_header(ROOT_INODE, ROOT_DIR_BLOCK_OFFSET as u32 / BLOCK_SIZE, 1024, InodeType::Directory)?;
        self.out
            .seek(SeekFrom::Start(self.offset + ROOT_DIR_BLOCK_OFFSET))?;
        self.out.write_all(&[0u8; 1024])?;
        Ok(())
    }

    fn write_lost_found(&mut self) -> anyhow::Result<()> {
        let block = self.next_block;
        let inode = LOST_FOUND_INODE;
        self.next_block += 1;
        self.write_inode_header(inode, block, 1024, InodeType::Directory)?;
        self.out
            .seek(SeekFrom::Start(self.offset + (block * BLOCK_SIZE) as u64))?;
        self.out.write_all(&[0u8; 1024])?;
        Ok(())
    }

    pub fn inject_file(&mut self, data: &[u8]) -> anyhow::Result<u32> {
        let inode = self.allocate_inode()?;
        let blocks_needed = (data.len() as u32).div_ceil(BLOCK_SIZE);
        let block = self.allocate_blocks(blocks_needed)?;
        let size = data.len() as u32;
        self.next_block += size.div_ceil(BLOCK_SIZE);

        let offset = self.offset + (block * BLOCK_SIZE) as u64;
        self.out.seek(SeekFrom::Start(offset))?;
        self.out.write_all(data)?;
        self.write_inode_header(inode, block, size, InodeType::File)?;
        Ok(inode)
    }

    pub fn inject_dir(&mut self, path: &Path, parent_inode: u32) -> anyhow::Result<u32> {
        let inode = self.allocate_inode()?;
        let block = self.allocate_block()?;
        self.next_block += 1;

        let mut entries = vec![];

        entries.push((inode, ".".into(), 2));
        entries.push((parent_inode, "..".into(), 2));

        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let name = entry.file_name().into_string().unwrap_or_default();
            let entry_path = entry.path();
            let entry_inode = if entry_path.is_file() {
                let content = fs::read(&entry_path)?;
                self.inject_file(&content)?
            } else if entry_path.is_dir() {
                self.inject_dir(&entry_path, inode)?
            } else {
                continue;
            };
            entries.push((entry_inode, name, 1));
        }

        let mut dir_data = vec![];
        for (i, (inode, name, file_type)) in entries.iter().enumerate() {
            let name_bytes = name.as_bytes();
            let mut rec_len = 8 + name_bytes.len();
            if rec_len % 4 != 0 {
                rec_len += 4 - (rec_len % 4);
            }
            if i == entries.len() - 1 {
                rec_len = BLOCK_SIZE as usize - dir_data.len();
            }

            dir_data.write_u32::<LittleEndian>(*inode)?;
            dir_data.write_u16::<LittleEndian>(rec_len as u16)?;
            dir_data.write_u8(name_bytes.len() as u8)?;
            dir_data.write_u8(*file_type)?;
            dir_data.extend_from_slice(name_bytes);
            dir_data.extend(std::iter::repeat(0).take(rec_len - 8 - name_bytes.len()));
        }

        let offset = self.offset + (block * BLOCK_SIZE) as u64;
        self.out.seek(SeekFrom::Start(offset))?;
        self.out.write_all(&dir_data)?;
        self.write_inode_header(inode, block, 1024, InodeType::Directory)?;
        Ok(inode)
    }
}
