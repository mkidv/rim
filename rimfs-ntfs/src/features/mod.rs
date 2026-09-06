// SPDX-License-Identifier: MIT
//! NTFS Modular System Features (`FsSystemFeature`)
//!
//! Each NTFS system file and disk structure is encapsulated into a decoupled
//! feature implementing `FsSystemFeature<NtfsMeta, NtfsAllocator, IO>`.

pub mod attrdef;
pub mod badclus;
pub mod boot;
pub mod extend;
pub mod logfile;
pub mod mft;
pub mod root;
pub mod secure;
pub mod upcase;
pub mod volume;

pub use attrdef::NtfsAttrDefFeature;
pub use badclus::NtfsBadClusFeature;
pub use boot::NtfsBootFeature;
pub use extend::NtfsExtendFeature;
pub use logfile::NtfsLogFileFeature;
pub use mft::NtfsMftFeature;
pub use root::NtfsRootDirFeature;
pub use secure::NtfsSecureFeature;
pub use upcase::NtfsUpCaseFeature;
pub use volume::NtfsVolumeFeature;
