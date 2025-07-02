// SPDX-License-Identifier: MIT
// rimgen/fs/ext4/params.rs

use crate::{
    fs::ext4::constant::*,
    core::{generate_volume_id_16, params::FsParams},
};

#[derive(Debug, Clone)]
pub struct Ext4Params {
    pub volume_id: [u8; 16],
    pub volume_label: String,
    pub size_bytes: u64,
    pub block_size: u32,
    pub block_count: u32,
    pub inode_count: u32,
    pub blocks_per_group: u32,
    pub inodes_per_group: u32,
    pub group_count: u32,
    pub first_data_block: u32,
}

impl Ext4Params {
    pub fn new(
        size_bytes: u64,
        volume_label: Option<String>,
        block_size: Option<u32>,
        inodes_per_group: Option<u32>,
    ) -> Self {
        let volume_id = generate_volume_id_16();
        let block_size = block_size.unwrap_or(EXT4_DEFAULT_BLOCK_SIZE);
        let inodes_per_group = inodes_per_group.unwrap_or(EXT4_DEFAULT_INODES_PER_GROUP);

        let block_count = (size_bytes / block_size as u64) as u32;
        let blocks_per_group = EXT4_DEFAULT_BLOCKS_PER_GROUP;

        let group_count = (block_count as u64).div_ceil(blocks_per_group as u64) as u32;

        let inode_count = group_count * inodes_per_group;

        let first_data_block = if block_size > 1024 { 0 } else { 1 };

        Self {
            volume_id,
            volume_label: volume_label.unwrap_or_else(|| "NO NAME".to_string()),
            size_bytes,
            block_size,
            block_count,
            blocks_per_group,
            group_count,
            inode_count,
            inodes_per_group,
            first_data_block,
        }
    }
}

impl FsParams for Ext4Params {}

/// Struct qui représente la disposition d'un groupe de blocs EXT4
#[derive(Debug, Clone, Copy)]
pub struct GroupLayout {
    pub group_id: u32,
    pub group_start: u32,        // Début du groupe dans l'espace de stockage
    pub block_bitmap_block: u32, // Bloc où se trouve le bitmap des blocs
    pub inode_bitmap_block: u32, // Bloc où se trouve le bitmap des inodes
    pub inode_table_block: u32,  // Bloc où commence la table des inodes
    pub inode_table_blocks: u32, // Nombre de blocs nécessaires pour la table des inodes
    pub first_data_block: u32,   // Premier bloc de données pour ce groupe
    pub reserved_blocks: u32,    // Nombre de blocs réservés (ex : pour le superblock, BGDT)
}

impl GroupLayout {
    /// Calcul et initialisation d'un `GroupLayout` pour un groupe donné
    pub fn compute(params: &Ext4Params, group_id: u32) -> Self {
        let group_start = params.first_data_block + group_id * params.blocks_per_group;

        // Fonction utilitaire : réservé pour chaque groupe
        let reserved_blocks = Self::reserved_blocks_in_group(group_id, params);

        // Calculs des blocs pour les bitmaps et table des inodes
        let block_bitmap_block = Self::block_bitmap_block(group_id, params);
        let inode_bitmap_block = Self::inode_bitmap_block(group_id, params);
        let inode_table_block = Self::inode_table_block(group_id, params);
        let inode_table_blocks =
            (params.inodes_per_group * EXT4_DEFAULT_INODE_SIZE / params.block_size).div_ceil(1);

        // Premier bloc de données
        let first_data_block = Self::first_data_block_in_group(params, group_id);

        Self {
            group_id,
            group_start,
            block_bitmap_block,
            inode_bitmap_block,
            inode_table_block,
            inode_table_blocks,
            first_data_block,
            reserved_blocks,
        }
    }
    // === Fonctions utilitaires déplacées dans GroupLayout ===

    // Calcule les blocs réservés dans le groupe (y compris les backups)
    fn reserved_blocks_in_group(group_id: u32, params: &Ext4Params) -> u32 {
        let backup_blocks =
            Self::backup_reserved_blocks_count(group_id, params.block_size, params.group_count);
        let safety_padding = 2;
        if backup_blocks > 0 {
            backup_blocks + safety_padding
        } else {
            0
        }
    }

    // Retourne le bloc où le bitmap des blocs est stocké pour ce groupe
    fn block_bitmap_block(group_id: u32, params: &Ext4Params) -> u32 {
        let group_start = params.first_data_block + group_id * params.blocks_per_group;
        group_start + 1 // Exemple, à ajuster selon le calcul réel
    }

    // Retourne le bloc où le bitmap des inodes est stocké pour ce groupe
    fn inode_bitmap_block(group_id: u32, params: &Ext4Params) -> u32 {
        let group_start = params.first_data_block + group_id * params.blocks_per_group;
        group_start + 2 // Exemple, à ajuster selon le calcul réel
    }

    // Retourne le bloc où commence la table des inodes pour ce groupe
    fn inode_table_block(group_id: u32, params: &Ext4Params) -> u32 {
        let group_start = params.first_data_block + group_id * params.blocks_per_group;
        group_start + 3 // Exemple, à ajuster selon le calcul réel
    }

    // Retourne le premier bloc de données dans le groupe
    fn first_data_block_in_group(params: &Ext4Params, group_id: u32) -> u32 {
        let inode_table_blocks =
            (params.inodes_per_group * EXT4_DEFAULT_INODE_SIZE / params.block_size).div_ceil(1);
        params.first_data_block + group_id * params.blocks_per_group + inode_table_blocks
    }

    // Fonction utilitaire pour le comptage des blocs de sauvegarde
    fn backup_reserved_blocks_count(group_id: u32, block_size: u32, group_count: u32) -> u32 {
        // Logique pour calculer les blocs de sauvegarde pour un groupe
        if group_id == 0 { 2 } else { 0 }
    }

    /// Log les informations du groupe de blocs
    pub fn log(&self) {
        println!(
            "[ext4] Group {0}: start = {1}, block_bitmap = {2}, inode_bitmap = {3}, inode_table = {4}, first_data_block = {5}",
            self.group_id,
            self.group_start,
            self.block_bitmap_block,
            self.inode_bitmap_block,
            self.inode_table_block,
            self.first_data_block
        );
    }
}
