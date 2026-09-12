// SPDX-License-Identifier: MIT

//! Image generation options and format-specific configuration.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageOptions {
    pub timestamp_seconds: u32,
    pub unique_id: [u8; 16],
    pub vmdk_cid: u32,
}

impl ImageOptions {
    pub const fn deterministic(seed: u64) -> Self {
        let mut guid = guid_from_seed(seed);
        guid[6] = (guid[6] & 0x0F) | 0x40;
        guid[8] = (guid[8] & 0x3F) | 0x80;

        Self {
            timestamp_seconds: 0,
            unique_id: guid,
            vmdk_cid: (seed as u32).wrapping_add(0x5249_4D49),
        }
    }

    pub const fn with_values(timestamp_seconds: u32, unique_id: [u8; 16], vmdk_cid: u32) -> Self {
        Self {
            timestamp_seconds,
            unique_id,
            vmdk_cid,
        }
    }
}

#[cfg(feature = "std")]
impl Default for ImageOptions {
    fn default() -> Self {
        let epoch_2000 = time::OffsetDateTime::from_unix_timestamp(946684800)
            .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
        let now = time::OffsetDateTime::now_utc();
        let timestamp_seconds = (now - epoch_2000).whole_seconds().max(0) as u32;
        let unique_id = *uuid::Uuid::new_v4().as_bytes();

        Self {
            timestamp_seconds,
            unique_id,
            vmdk_cid: (now.unix_timestamp_nanos() as u64 as u32).wrapping_add(0x5249_4D49),
        }
    }
}

#[cfg(not(feature = "std"))]
impl Default for ImageOptions {
    fn default() -> Self {
        Self::deterministic(1)
    }
}

const fn guid_from_seed(seed: u64) -> [u8; 16] {
    let mut state = if seed == 0 {
        0x5EED_C0DE_1234_5678
    } else {
        seed
    };
    state = xorshift64(state);
    let lo = state.to_le_bytes();
    state = xorshift64(state);
    let hi = state.to_le_bytes();

    [
        lo[0], lo[1], lo[2], lo[3], lo[4], lo[5], lo[6], lo[7], hi[0], hi[1], hi[2], hi[3], hi[4],
        hi[5], hi[6], hi[7],
    ]
}

const fn xorshift64(mut state: u64) -> u64 {
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    state
}
