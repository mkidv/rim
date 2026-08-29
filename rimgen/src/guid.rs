// SPDX-License-Identifier: MIT
//! GUID generation strategies for rimgen.

#[cfg(feature = "std")]
use uuid::Uuid;

pub trait GuidGenerator {
    fn generate_guid(&mut self) -> [u8; 16];
}

#[cfg(feature = "std")]
#[derive(Debug, Default, Clone)]
pub struct RandomGuidGenerator;

#[cfg(feature = "std")]
impl GuidGenerator for RandomGuidGenerator {
    fn generate_guid(&mut self) -> [u8; 16] {
        *Uuid::new_v4().as_bytes()
    }
}

/// Deterministic GUID generator for reproducible builds in no_std / alloc.
/// Uses a 64-bit seed (xorshift64) to generate pseudo-random GUIDs with UUID v4 format bits.
#[derive(Debug, Clone)]
pub struct SeededGuidGenerator {
    state: u64,
}

impl SeededGuidGenerator {
    pub fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0x5EED_C0DE_1234_5678
            } else {
                seed
            },
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }
}

impl GuidGenerator for SeededGuidGenerator {
    fn generate_guid(&mut self) -> [u8; 16] {
        let lo = self.next_u64();
        let hi = self.next_u64();
        let mut bytes = [0u8; 16];
        bytes[0..8].copy_from_slice(&lo.to_le_bytes());
        bytes[8..16].copy_from_slice(&hi.to_le_bytes());
        // Set UUID v4 variant and version bits
        bytes[6] = (bytes[6] & 0x0F) | 0x40; // Version 4
        bytes[8] = (bytes[8] & 0x3F) | 0x80; // Variant RFC4122
        bytes
    }
}

#[derive(Debug, Clone)]
pub struct ManualGuidGenerator {
    guids: alloc::vec::Vec<[u8; 16]>,
    index: usize,
    fallback: SeededGuidGenerator,
}

impl ManualGuidGenerator {
    pub fn new(guids: alloc::vec::Vec<[u8; 16]>) -> Self {
        Self {
            guids,
            index: 0,
            fallback: SeededGuidGenerator::new(1),
        }
    }
}

impl GuidGenerator for ManualGuidGenerator {
    fn generate_guid(&mut self) -> [u8; 16] {
        if self.index < self.guids.len() {
            let g = self.guids[self.index];
            self.index += 1;
            g
        } else {
            self.fallback.generate_guid()
        }
    }
}
