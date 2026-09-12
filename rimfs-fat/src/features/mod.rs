// SPDX-License-Identifier: MIT

//! FAT filesystem feature modules.

pub mod boot;
pub mod fsinfo;
pub mod root;
pub mod table;

pub use boot::FatBootFeature;
pub use fsinfo::FatFsInfoFeature;
pub use root::FatRootDirFeature;
pub use table::FatTableFeature;
