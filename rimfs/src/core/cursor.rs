// SPDX-License-Identifier: MIT

use crate::FsMeta;
use crate::core::FsCursorResult;
use crate::core::errors::{FsCursorError, FsResult};
use rimio::prelude::*;

/// Trait pour les systèmes de fichiers à allocation par clusters (FAT32, ExFAT, etc.)
pub trait ClusterMeta: FsMeta<u32> {
    const EOC: u32;
    const FIRST_CLUSTER: u32;
    const ENTRY_SIZE: usize;
    const ENTRY_MASK: u32; // Masque pour isoler les bits utiles

    /// Calcule l'offset d'une entrée FAT
    fn fat_entry_offset(&self, cluster: u32, fat_index: u8) -> u64;

    /// Vérifie si un cluster est End-of-Chain
    fn is_eoc(&self, cluster: u32) -> bool {
        cluster >= Self::EOC
    }
}

/// Curseur générique pour les systèmes de fichiers à allocation par cluster.
///
/// Ce curseur abstrait la logique de parcours des chaînes de clusters et optimise
/// automatiquement les I/O en groupant les runs contigus.
///
/// # Type Parameters
/// - `M`: Type implémentant `FsMeta<u32>` (Fat32Meta, ExFatMeta, etc.)
/// - `C`: Constantes spécifiques au FS (EOC, FIRST_CLUSTER, etc.)
#[derive(Debug)]
pub struct ClusterCursor<'a, M>
where
    M: ClusterMeta,
{
    meta: &'a M,
    current: Option<u32>,
    seen: usize,
    allow_system: bool,
}

impl<'a, M> ClusterCursor<'a, M>
where
    M: ClusterMeta,
{
    pub fn new_safe(meta: &'a M, start: u32) -> Self {
        Self {
            meta,
            current: Some(start),
            seen: 0,
            allow_system: false,
        }
    }

    pub fn new(meta: &'a M, start: u32) -> Self {
        Self {
            meta,
            current: Some(start),
            seen: 0,
            allow_system: true,
        }
    }

    #[inline]
    fn in_bounds(&self, c: u32) -> bool {
        let min = if self.allow_system {
            M::FIRST_CLUSTER
        } else {
            self.meta.first_data_unit()
        };
        let max = self.meta.last_data_unit();
        (min..=max).contains(&c)
    }

    /// Une étape d'itération (utilise read_fat_entry intégré)
    pub fn next_with<IO>(&mut self, io: &mut IO) -> Option<FsCursorResult<u32>>
    where
        IO: BlockIO + ?Sized,
    {
        let c = self.current?;
        self.seen += 1;
        if self.seen > self.meta.total_units() {
            self.current = None;
            return Some(Err(FsCursorError::LoopDetected));
        }

        let next = match read_fat_entry(io, self.meta, c, 0) {
            Ok(n) => n,
            Err(e) => {
                self.current = None;
                return Some(Err(e));
            }
        };

        // Check end-of-chain & bounds
        if !self.in_bounds(c) {
            self.current = None;
            return Some(Err(FsCursorError::InvalidCluster(c)));
        }
        if self.meta.is_eoc(next) {
            self.current = None;
        } else {
            if !self.in_bounds(next) {
                self.current = None;
                return Some(Err(FsCursorError::InvalidCluster(next)));
            }
            self.current = Some(next);
        }
        Some(Ok(c))
    }

    /// Itération cluster par cluster via callback
    pub fn for_each_cluster<IO, F>(&mut self, io: &mut IO, mut f: F) -> FsCursorResult<()>
    where
        IO: BlockIO + ?Sized,
        F: FnMut(&mut IO, u32) -> FsCursorResult<()>,
    {
        while let Some(res) = self.next_with(io) {
            let c = res?;
            f(io, c)?;
        }
        Ok(())
    }

    /// Itère par runs contigus (start,len) via callback
    pub fn for_each_run<IO, F>(&mut self, io: &mut IO, mut f: F) -> FsCursorResult<()>
    where
        IO: BlockIO + ?Sized,
        F: FnMut(&mut IO, u32, u32) -> FsCursorResult<()>,
    {
        let mut start: Option<u32> = None;
        let mut prev: Option<u32> = None;
        let mut len: u32 = 0;

        let mut flush = |io: &mut IO, s: Option<u32>, l: u32| -> FsCursorResult<()> {
            if let Some(s0) = s
                && l > 0
            {
                f(io, s0, l)?;
            }
            Ok(())
        };

        while let Some(res) = self.next_with(io) {
            let c = res?;
            match prev {
                Some(p) if c == p + 1 => {
                    len += 1;
                }
                Some(_) => {
                    flush(io, start, len)?;
                    start = Some(c);
                    len = 1;
                }
                None => {
                    start = Some(c);
                    len = 1;
                }
            }
            prev = Some(c);
        }
        flush(io, start, len)?;
        Ok(())
    }

    /// Fabrique un itérateur "cluster par cluster"
    pub fn iter<'b, IO>(&'b mut self, io: &'b mut IO) -> ClusterIter<'a, 'b, M, IO>
    where
        IO: BlockIO + ?Sized,
    {
        ClusterIter { cursor: self, io }
    }

    /// Fabrique un itérateur de **runs** contigus
    pub fn runs<'b, IO>(&'b mut self, io: &'b mut IO) -> RunIter<'a, 'b, M, IO>
    where
        IO: BlockIO + ?Sized,
    {
        RunIter {
            cursor: self,
            io,
            start: None,
            prev: None,
            len: 0,
            finished: false,
        }
    }
}

/// Itérateur cluster par cluster
pub struct ClusterIter<'a, 'b, M, IO: ?Sized>
where
    M: ClusterMeta,
{
    cursor: &'b mut ClusterCursor<'a, M>,
    io: &'b mut IO,
}

impl<'a, 'b, M, IO: ?Sized> Iterator for ClusterIter<'a, 'b, M, IO>
where
    M: ClusterMeta,
    IO: BlockIO,
{
    type Item = FsCursorResult<u32>;

    fn next(&mut self) -> Option<Self::Item> {
        self.cursor.next_with(self.io)
    }
}

/// Itérateur de runs contigus
pub struct RunIter<'a, 'b, M, IO: ?Sized>
where
    M: ClusterMeta,
{
    cursor: &'b mut ClusterCursor<'a, M>,
    io: &'b mut IO,
    start: Option<u32>,
    prev: Option<u32>,
    len: u32,
    finished: bool,
}

impl<'a, 'b, M, IO: ?Sized> Iterator for RunIter<'a, 'b, M, IO>
where
    M: ClusterMeta,
    IO: BlockIO,
{
    type Item = FsCursorResult<(u32, u32)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        loop {
            match self.cursor.next_with(self.io) {
                Some(Ok(c)) => {
                    match self.prev {
                        Some(p) if c == p + 1 => {
                            self.len += 1;
                            self.prev = Some(c);
                        }
                        Some(_) => {
                            // casse de run → on retourne le précédent
                            let out = (self.start.unwrap(), self.len);
                            // initialise le nouveau run avec c
                            self.start = Some(c);
                            self.prev = Some(c);
                            self.len = 1;
                            return Some(Ok(out));
                        }
                        None => {
                            self.start = Some(c);
                            self.prev = Some(c);
                            self.len = 1;
                        }
                    }
                }
                Some(Err(e)) => {
                    self.finished = true;
                    return Some(Err(e));
                }
                None => {
                    // fin : s'il reste un run incomplet, on le retourne une dernière fois
                    self.finished = true;
                    if let (Some(s), l @ 1..) = (self.start, self.len) {
                        return Some(Ok((s, l)));
                    }
                    return None;
                }
            }
        }
    }
}

/// Lecture *générique* d'une entrée FAT, sans allocation.
/// Supporte ENTRY_SIZE = 1/2/4 et la sélection de la copie (fat_index).
#[inline]
pub fn read_fat_entry<M, IO>(
    io: &mut IO,
    meta: &M,
    cluster: u32,
    fat_index: u8,
) -> FsCursorResult<u32>
where
    M: ClusterMeta,
    IO: BlockIO + ?Sized,
{
    let off = meta.fat_entry_offset(cluster, fat_index);

    // tampon stack maximal 4 octets (FAT12/16/32 couvert)
    let mut buf = [0u8; 4];
    let n = M::ENTRY_SIZE;
    debug_assert!(n <= 4 && n != 0);

    io.read_at(off, &mut buf[..n])?;

    let raw = match n {
        4 => u32::from_le_bytes(buf),
        2 => u16::from_le_bytes([buf[0], buf[1]]) as u32,
        1 => buf[0] as u32,
        _ => return Err(FsCursorError::UnsupportedEntrySize),
    };

    Ok(raw & M::ENTRY_MASK)
}

/// Curseur linéaire (contigu) : parcourt une plage [start .. start+clusters).
/// Ne consulte PAS la FAT. Idéal pour NOFATCHAIN (Upcase, Bitmap, etc.).
#[derive(Clone, Copy, Debug)]
pub struct LinearCursor<'a, M: ClusterMeta> {
    meta: &'a M,
    next: u32,
    end_excl: u32,
    allow_system: bool,
}

impl<'a, M: ClusterMeta> Iterator for LinearCursor<'a, M> {
    type Item = FsCursorResult<u32>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.end_excl {
            return None;
        }
        let c = self.next;
        self.next = self.next.saturating_add(1);
        if !self.in_bounds(c) {
            return Some(Err(FsCursorError::InvalidCluster(c)));
        }
        Some(Ok(c))
    }
}

impl<'a, M: ClusterMeta> LinearCursor<'a, M> {
    /// Construction depuis un nombre de clusters
    #[inline]
    pub fn from_clusters_safe(meta: &'a M, start: u32, clusters: u32) -> Self {
        Self {
            meta,
            next: start,
            end_excl: start.saturating_add(clusters),
            allow_system: false,
        }
    }

    /// Construction depuis une longueur logique en octets
    #[inline]
    pub fn from_len_bytes_safe(meta: &'a M, start: u32, len_bytes: u64) -> Self {
        let cs = meta.unit_size() as u64;
        let clusters = len_bytes.div_ceil(cs); // ceil
        Self::from_clusters_safe(meta, start, clusters as u32)
    }

    #[inline]
    pub fn from_clusters(meta: &'a M, start: u32, clusters: u32) -> Self {
        Self {
            meta,
            next: start,
            end_excl: start.saturating_add(clusters),
            allow_system: true,
        }
    }

    /// Construction depuis une longueur logique en octets
    #[inline]
    pub fn from_len_bytes(meta: &'a M, start: u32, len_bytes: u64) -> Self {
        let cs = meta.unit_size() as u64;
        let clusters = len_bytes.div_ceil(cs); // ceil
        Self::from_clusters(meta, start, clusters as u32)
    }

    #[inline]
    fn in_bounds(&self, c: u32) -> bool {
        let min = if self.allow_system {
            M::FIRST_CLUSTER
        } else {
            self.meta.first_data_unit()
        };
        let max = self.meta.last_data_unit();
        (min..=max).contains(&c)
    }

    /// Itère par **runs contigus** (start, len) et appelle `f(io, start, len)`.
    /// `IO` est passé pour permettre un batching I/O direct dans le callback.
    pub fn for_each_run<IO, F>(&mut self, io: &mut IO, mut f: F) -> FsCursorResult<()>
    where
        IO: BlockIO + ?Sized,
        F: FnMut(&mut IO, u32, u32) -> FsCursorResult<()>,
    {
        // Comme c’est linéaire, l’intégralité de la plage est un seul run si la taille > 0.
        if self.next < self.end_excl {
            let start = self.next;
            if !self.in_bounds(start) {
                return Err(FsCursorError::InvalidCluster(start));
            }
            let len = self.end_excl - self.next;
            // validation minimale: borne haute
            let last = start.saturating_add(len - 1);

            if !self.in_bounds(last) {
                return Err(FsCursorError::InvalidCluster(last));
            }

            // Consomme d’un coup
            self.next = self.end_excl;
            f(io, start, len)?;
        }
        Ok(())
    }

    /// Lecture directe du stream dans `dst`, en batchant par runs.
    /// `total_len` = bytes logiques à lire (tronque le dernier run si besoin).
    pub fn read_into<IO: BlockIO + ?Sized>(
        &mut self,
        io: &mut IO,
        total_len: usize,
        dst: &mut [u8],
    ) -> FsCursorResult<()> {
        assert!(dst.len() >= total_len);
        let cs = self.meta.unit_size();
        let mut written = 0usize;

        self.for_each_run(io, |io, run_start, run_len| {
            if written >= total_len {
                return Ok(());
            }
            // Taille en octets de ce run
            let run_bytes = (run_len as usize) * cs;
            let to_copy = core::cmp::min(run_bytes, total_len - written);
            if to_copy > 0 {
                let off = self.meta.unit_offset(run_start);
                io.read_at(off, &mut dst[written..written + to_copy])?;
                written += to_copy;
            }
            Ok(())
        })?;

        if written < total_len {
            return Err(FsCursorError::Other("linear_short_read"));
        }
        Ok(())
    }
}
