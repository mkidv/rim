// SPDX-License-Identifier: MIT

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Volume Flags for exFAT Boot Sector
///
/// These flags indicate the volume state and operating parameters.
/// They are stored in the volume_flags field of the Boot Sector (offset 106-107).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(transparent)]
pub struct VolumeFlags(u16);

#[allow(dead_code)]
impl VolumeFlags {
    /// Active FAT flag (bit 0)
    /// Indicates which FAT is active (0 = first FAT, 1 = second FAT)
    /// In practice, exFAT only uses one FAT, so this bit is usually 0
    pub const ACTIVE_FAT: u16 = 0x0001;

    /// Volume Dirty flag (bit 1)
    /// Indicates that the volume was not properly unmounted
    /// Set by the driver upon mounting, cleared upon clean unmounting
    pub const VOLUME_DIRTY: u16 = 0x0002;

    /// Media Failure flag (bit 2)
    /// Indicates that a media failure was detected
    /// Once set, only chkdsk can clear it
    pub const MEDIA_FAILURE: u16 = 0x0004;

    /// Clear to Zero flag (bit 3)
    /// Indicates that all unallocated clusters should be treated as zero
    /// Improves read performance on some media
    pub const CLEAR_TO_ZERO: u16 = 0x0008;

    /// Creates empty flags
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Creates flags from a raw value
    pub const fn from_bits(bits: u16) -> Self {
        Self(bits)
    }

    /// Returns the raw value of the flags
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Checks if a flag is present
    pub const fn contains(self, flag: u16) -> bool {
        (self.0 & flag) == flag
    }

    /// Adds a flag
    pub const fn with_flag(mut self, flag: u16) -> Self {
        self.0 |= flag;
        self
    }

    /// Removes a flag
    pub const fn without_flag(mut self, flag: u16) -> Self {
        self.0 &= !flag;
        self
    }

    /// Toggles a flag
    pub const fn toggle_flag(mut self, flag: u16) -> Self {
        self.0 ^= flag;
        self
    }

    /// Creates default flags for a new volume (clean)
    pub fn new_volume() -> Self {
        Self::empty()
    }

    /// Creates flags for a mounted volume (dirty bit set)
    pub fn mounted_volume() -> Self {
        Self::from_bits(Self::VOLUME_DIRTY)
    }

    /// Marks the volume as dirty (not cleanly unmounted)
    pub fn mark_dirty(self) -> Self {
        self.with_flag(Self::VOLUME_DIRTY)
    }

    /// Marks the volume as cleanly unmounted
    pub fn mark_clean(self) -> Self {
        self.without_flag(Self::VOLUME_DIRTY)
    }

    /// Marks a media failure
    pub fn mark_media_failure(self) -> Self {
        self.with_flag(Self::MEDIA_FAILURE)
    }

    /// Enables Clear to Zero to optimize performance
    pub fn enable_clear_to_zero(self) -> Self {
        self.with_flag(Self::CLEAR_TO_ZERO)
    }

    /// Checks if the volume is dirty
    pub fn is_dirty(self) -> bool {
        self.contains(Self::VOLUME_DIRTY)
    }

    /// Checks if a media failure is present
    pub fn has_media_failure(self) -> bool {
        self.contains(Self::MEDIA_FAILURE)
    }

    /// Checks if Clear to Zero is enabled
    pub fn is_clear_to_zero_enabled(self) -> bool {
        self.contains(Self::CLEAR_TO_ZERO)
    }
}

// zerocopy traits are implemented via derive

impl From<u16> for VolumeFlags {
    fn from(value: u16) -> Self {
        Self::from_bits(value)
    }
}

impl From<VolumeFlags> for u16 {
    fn from(flags: VolumeFlags) -> Self {
        flags.bits()
    }
}

impl Default for VolumeFlags {
    fn default() -> Self {
        Self::new_volume()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_volume_flags_basic() {
        let flags = VolumeFlags::new_volume();
        assert_eq!(flags.bits(), 0);
        assert!(!flags.is_dirty());
        assert!(!flags.has_media_failure());
    }

    #[test]
    fn test_volume_flags_dirty() {
        let mut flags = VolumeFlags::new_volume();
        flags = flags.mark_dirty();
        assert!(flags.is_dirty());
        assert_eq!(flags.bits(), 0x0002);

        flags = flags.mark_clean();
        assert!(!flags.is_dirty());
        assert_eq!(flags.bits(), 0x0000);
    }

    #[test]
    fn test_volume_flags_media_failure() {
        let flags = VolumeFlags::new_volume().mark_media_failure();
        assert!(flags.has_media_failure());
        assert_eq!(flags.bits(), 0x0004);
    }

    #[test]
    fn test_volume_flags_clear_to_zero() {
        let flags = VolumeFlags::new_volume().enable_clear_to_zero();
        assert!(flags.is_clear_to_zero_enabled());
        assert_eq!(flags.bits(), 0x0008);
    }

    #[test]
    fn test_volume_flags_combined() {
        let flags = VolumeFlags::new_volume()
            .mark_dirty()
            .enable_clear_to_zero();
        assert!(flags.is_dirty());
        assert!(flags.is_clear_to_zero_enabled());
        assert_eq!(flags.bits(), 0x000A); // 0x0002 | 0x0008
    }

    #[test]
    fn test_volume_flags_from_u16() {
        let flags = VolumeFlags::from(0x0006u16); // DIRTY | MEDIA_FAILURE
        assert!(flags.is_dirty());
        assert!(flags.has_media_failure());
        assert!(!flags.is_clear_to_zero_enabled());
    }

    #[test]
    fn test_volume_flags_to_u16() {
        let flags = VolumeFlags::new_volume()
            .with_flag(VolumeFlags::VOLUME_DIRTY)
            .with_flag(VolumeFlags::MEDIA_FAILURE);
        let value: u16 = flags.into();
        assert_eq!(value, 0x0006);
    }

    #[test]
    fn test_volume_flags_serialization_size() {
        use std::mem;
        assert_eq!(mem::size_of::<VolumeFlags>(), mem::size_of::<u16>());
    }
}
