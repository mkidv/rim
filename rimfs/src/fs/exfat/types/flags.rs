// SPDX-License-Identifier: MIT

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Volume Flags pour exFAT Boot Sector
/// 
/// Ces flags indiquent l'état du volume et des paramètres de fonctionnement.
/// Ils sont stockés dans le champ volume_flags du Boot Sector (offset 106-107).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(transparent)]
pub struct VolumeFlags(u16);

#[allow(dead_code)]
impl VolumeFlags {
    /// Active FAT flag (bit 0)
    /// Indique quelle FAT est active (0 = première FAT, 1 = deuxième FAT)
    /// En pratique, exFAT n'utilise qu'une seule FAT, donc ce bit est généralement 0
    pub const ACTIVE_FAT: u16 = 0x0001;

    /// Volume Dirty flag (bit 1) 
    /// Indique que le volume n'a pas été correctement démonté
    /// Set par le pilote lors du montage, cleared lors du démontage propre
    pub const VOLUME_DIRTY: u16 = 0x0002;

    /// Media Failure flag (bit 2)
    /// Indique qu'une erreur de média a été détectée
    /// Une fois set, seul chkdsk peut le clearer
    pub const MEDIA_FAILURE: u16 = 0x0004;

    /// Clear to Zero flag (bit 3)
    /// Indique que tous les clusters non-alloués doivent être traités comme zéro
    /// Améliore les performances de lecture sur certains médias
    pub const CLEAR_TO_ZERO: u16 = 0x0008;

    /// Crée des flags vides
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Crée des flags à partir d'une valeur brute
    pub const fn from_bits(bits: u16) -> Self {
        Self(bits)
    }

    /// Retourne la valeur brute des flags
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Vérifie si un flag est présent
    pub const fn contains(self, flag: u16) -> bool {
        (self.0 & flag) == flag
    }

    /// Ajoute un flag
    pub const fn with_flag(mut self, flag: u16) -> Self {
        self.0 |= flag;
        self
    }

    /// Retire un flag
    pub const fn without_flag(mut self, flag: u16) -> Self {
        self.0 &= !flag;
        self
    }

    /// Bascule un flag
    pub const fn toggle_flag(mut self, flag: u16) -> Self {
        self.0 ^= flag;
        self
    }

    /// Crée des flags par défaut pour un nouveau volume (propre)
    pub fn new_volume() -> Self {
        Self::empty()
    }

    /// Crée des flags pour un volume monté (dirty bit set)
    pub fn mounted_volume() -> Self {
        Self::from_bits(Self::VOLUME_DIRTY)
    }

    /// Marque le volume comme sale (non-démonté proprement)
    pub fn mark_dirty(self) -> Self {
        self.with_flag(Self::VOLUME_DIRTY)
    }

    /// Marque le volume comme proprement démonté
    pub fn mark_clean(self) -> Self {
        self.without_flag(Self::VOLUME_DIRTY)
    }

    /// Marque une erreur de média
    pub fn mark_media_failure(self) -> Self {
        self.with_flag(Self::MEDIA_FAILURE)
    }

    /// Active le Clear to Zero pour optimiser les performances
    pub fn enable_clear_to_zero(self) -> Self {
        self.with_flag(Self::CLEAR_TO_ZERO)
    }

    /// Vérifie si le volume est sale
    pub fn is_dirty(self) -> bool {
        self.contains(Self::VOLUME_DIRTY)
    }

    /// Vérifie si une erreur de média est présente
    pub fn has_media_failure(self) -> bool {
        self.contains(Self::MEDIA_FAILURE)
    }

    /// Vérifie si Clear to Zero est activé
    pub fn is_clear_to_zero_enabled(self) -> bool {
        self.contains(Self::CLEAR_TO_ZERO)
    }
}

// Les traits zerocopy sont implémentés via derive

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