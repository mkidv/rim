// SPDX-License-Identifier: MIT
//! NTFS Security-related structures
//!
//! Reference: [MS-DTYP]: Windows Data Types
//! <https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-dtyp/f993ad91-88f2-4bd5-a131-7e8c0db1683d>

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;
use zerocopy::{
    FromBytes, FromZeros, Immutable, IntoBytes, KnownLayout, byteorder::little_endian::*,
};

use super::IndexEntryHeader;
use crate::constant::*;

bitflags::bitflags! {
    /// ACE (Access Control Entry) Header Flags
    ///
    /// Reference: [MS-DTYP] 2.4.4.1 ACE_HEADER
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    #[repr(transparent)]
    pub struct AceFlags: u8 {
        /// Non-container child objects inherit the ACE
        const OBJECT_INHERIT        = 0x01;
        /// Child containers inherit the ACE
        const CONTAINER_INHERIT     = 0x02;
        /// Does not propagate to subsequent generations of containers
        const NO_PROPAGATE_INHERIT  = 0x04;
        /// Applies only to child objects, not to the object itself
        const INHERIT_ONLY          = 0x08;
        /// The ACE was inherited from a parent object
        const INHERITED             = 0x10;
        /// Generates audit messages for successful access
        const SUCCESSFUL_ACCESS     = 0x40;
        /// Generates audit messages for failed access
        const FAILED_ACCESS         = 0x80;
    }
}

bitflags::bitflags! {
    /// Windows NT File Access Rights (ACCESS_MASK)
    ///
    /// Reference: [MS-DTYP] 2.4.3 ACCESS_MASK
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    #[repr(transparent)]
    pub struct FileAccessMask: u32 {
        const GENERIC_ALL     = 0x1000_0000;
        const GENERIC_EXECUTE = 0x2000_0000;
        const GENERIC_WRITE   = 0x4000_0000;
        const GENERIC_READ    = 0x8000_0000;
        // Specific file permissions
        const FILE_READ_DATA        = 0x0000_0001; // file read / dir list
        const FILE_WRITE_DATA       = 0x0000_0002; // file write / dir create file
        const FILE_APPEND_DATA      = 0x0000_0004; // file append / dir create subdir
        const FILE_READ_EA          = 0x0000_0008;
        const FILE_WRITE_EA         = 0x0000_0010;
        const FILE_EXECUTE          = 0x0000_0020; // file execute / dir traverse
        const FILE_DELETE_CHILD     = 0x0000_0040;
        const FILE_READ_ATTRIBUTES  = 0x0000_0080;
        const FILE_WRITE_ATTRIBUTES = 0x0000_0100;

        // Standard rights
        const DELETE                = 0x0001_0000;
        const READ_CONTROL          = 0x0002_0000;
        const WRITE_DAC             = 0x0004_0000;
        const WRITE_OWNER           = 0x0008_0000;
        const SYNCHRONIZE           = 0x0010_0000;

        // Standard combinations
        const STANDARD_RIGHTS_REQUIRED = 0x000F_0000;
        const STANDARD_RIGHTS_READ     = 0x0002_0000;
        const STANDARD_RIGHTS_WRITE    = 0x0002_0000;
        const STANDARD_RIGHTS_EXECUTE  = 0x0002_0000;
        const STANDARD_RIGHTS_ALL      = 0x001F_0000;

        // Generic / Composite rights used by NTFS
        const FULL_CONTROL   = 0x001F_01FF;
        const MODIFY         = 0x0013_01BF;
        const FILE_READ = Self::READ_CONTROL.bits() | Self::SYNCHRONIZE.bits()
            | Self::FILE_READ_DATA.bits() | Self::FILE_READ_EA.bits() | Self::FILE_READ_ATTRIBUTES.bits();
        const FILE_READ_EXEC = Self::FILE_READ.bits() | Self::FILE_EXECUTE.bits();
        /// Legacy read-only alias; does not include execute/traverse.
        const READ_EXEC = Self::FILE_READ.bits();
        const READ_WRITE = 0x0012_019F;
    }
}

bitflags::bitflags! {
    /// Security Descriptor Control Flags
    ///
    /// Reference: [MS-DTYP] 2.4.6 SECURITY_DESCRIPTOR
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    #[repr(transparent)]
    pub struct SecurityDescriptorControl: u16 {
        const OWNER_DEFAULTED       = 0x0001;
        const GROUP_DEFAULTED       = 0x0002;
        const DACL_PRESENT          = 0x0004;
        const DACL_DEFAULTED        = 0x0008;
        const SACL_PRESENT          = 0x0010;
        const SACL_DEFAULTED        = 0x0020;
        const DACL_AUTO_INHERIT_REQ = 0x0100;
        const SACL_AUTO_INHERIT_REQ = 0x0200;
        const DACL_AUTO_INHERITED   = 0x0400;
        const SACL_AUTO_INHERITED   = 0x0800;
        const DACL_PROTECTED        = 0x1000;
        const SACL_PROTECTED        = 0x2000;
        const RM_CONTROL_VALID      = 0x4000;
        const SELF_RELATIVE         = 0x8000;
    }
}

/// SID Header
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct SidHeader {
    pub revision: u8,
    pub sub_authority_count: u8,
    pub identifier_authority: [u8; 6],
}

/// Header for an entry in $SDS, also used as the $SII/$SDH index payload.
///
/// This header precedes the actual self-relative security descriptor in the $SDS stream.
/// It MUST be 16-byte aligned within the stream.
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct SecurityDescriptorHeader {
    /// Hash of the security descriptor
    pub hash: U32,
    /// Unique Security ID
    pub security_id: U32,
    /// Offset of this entry in the $SDS stream
    pub offset: U64,
    /// Size of this entry (header + descriptor, excluding alignment padding)
    pub length: U32,
}

/// Complete fixed on-disk SII entry (the end marker is a separate record).
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct Sii {
    pub header: super::IndexDataEntryHeader,
    pub security_id: U32,
    pub data: SecurityDescriptorHeader,
}

impl Sii {
    pub fn new(data: SecurityDescriptorHeader) -> Self {
        Self {
            header: super::IndexDataEntryHeader {
                data_offset: 20.into(),
                data_length: 20.into(),
                entry_length: 40.into(),
                key_length: 4.into(),
                ..Default::default()
            },
            security_id: data.security_id,
            data,
        }
    }
}

/// Complete fixed on-disk SDH entry, including the canonical padding.
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct Sdh {
    pub header: super::IndexDataEntryHeader,
    pub hash: U32,
    pub security_id: U32,
    pub data: SecurityDescriptorHeader,
    pub padding: [u8; 4],
}

impl Sdh {
    pub fn new(data: SecurityDescriptorHeader) -> Self {
        Self {
            header: super::IndexDataEntryHeader {
                data_offset: 24.into(),
                data_length: 20.into(),
                entry_length: 48.into(),
                key_length: 8.into(),
                ..Default::default()
            },
            hash: data.hash,
            security_id: data.security_id,
            data,
            padding: *b"I\0I\0",
        }
    }
}

/// Complete SDS entry: its fixed header and borrowed typed descriptor.
/// Fields are private so the hash and length cannot diverge from the payload.
#[derive(Debug, Clone)]
pub struct Sds<'a> {
    header: SecurityDescriptorHeader,
    descriptor: SecurityDescriptor<'a>,
}

impl<'a> Sds<'a> {
    pub fn new(
        security_id: u32,
        descriptor: &SecurityDescriptor<'a>,
        offset: u64,
    ) -> Result<Self, SecurityDescriptorError> {
        if !offset.is_multiple_of(16) {
            return Err(SecurityDescriptorError::UnalignedSdsOffset);
        }
        let hash = descriptor.hash()?;
        let descriptor = *descriptor;
        let header = SecurityDescriptorHeader {
            hash: hash.into(),
            security_id: security_id.into(),
            offset: offset.into(),
            length: ((core::mem::size_of::<SecurityDescriptorHeader>() + descriptor.encoded_len())
                as u32)
                .into(),
        };
        Ok(Self { header, descriptor })
    }

    pub fn header(&self) -> &SecurityDescriptorHeader {
        &self.header
    }

    pub fn descriptor(&self) -> &SecurityDescriptor<'a> {
        &self.descriptor
    }

    pub fn encoded_len(&self) -> usize {
        (self.header.length.get() as usize + 15) & !15
    }

    /// Append this record and its alignment padding. The header offset identifies
    /// its primary stream position, including when serializing a mirror copy.
    pub fn to_raw_buffer(&self, out: &mut Vec<u8>) {
        let end = out.len() + self.encoded_len();
        out.reserve(self.encoded_len());
        out.extend_from_slice(self.header.as_bytes());
        self.descriptor
            .encode_into(out)
            .expect("Sds holds a validated immutable descriptor");
        out.resize(end, 0);
    }
}

/// Calculates the hash for a security descriptor as expected by NTFS.
///
/// The algorithm is a rotating sum of 32-bit words.
pub fn calculate_security_hash(data: &[u8]) -> u32 {
    update_security_hash(0, data)
}

fn update_security_hash(mut hash: u32, data: &[u8]) -> u32 {
    for i in (0..data.len()).step_by(4) {
        let mut word = 0u32;
        for j in 0..4 {
            if i + j < data.len() {
                word |= (data[i + j] as u32) << (j * 8);
            }
        }
        hash = hash.rotate_left(3).wrapping_add(word);
    }
    hash
}

const _: () = {
    assert!(core::mem::size_of::<Sii>() == 40);
    assert!(core::mem::offset_of!(Sii, security_id) == 16);
    assert!(core::mem::offset_of!(Sii, data) == 20);
    assert!(core::mem::size_of::<Sdh>() == 48);
    assert!(core::mem::offset_of!(Sdh, hash) == 16);
    assert!(core::mem::offset_of!(Sdh, security_id) == 20);
    assert!(core::mem::offset_of!(Sdh, data) == 24);
    assert!(core::mem::offset_of!(Sdh, padding) == 44);
};

pub const ACCESS_ALLOWED_ACE_TYPE: u8 = 0x00;
pub const ACE_INHERIT: AceFlags = AceFlags::OBJECT_INHERIT.union(AceFlags::CONTAINER_INHERIT);

pub const ACE_INHERIT_ONLY: AceFlags = AceFlags::OBJECT_INHERIT
    .union(AceFlags::CONTAINER_INHERIT)
    .union(AceFlags::INHERIT_ONLY);

/// Security Descriptor (Self-Relative)
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct SecurityDescriptorRelative {
    pub revision: u8,
    pub sbz1: u8,
    pub control: U16,
    pub owner_offset: U32,
    pub group_offset: U32,
    pub sacl_offset: U32,
    pub dacl_offset: U32,
}

/// ACL Header
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct AclHeader {
    pub revision: u8,
    pub sbz1: u8,
    pub acl_size: U16,
    pub ace_count: U16,
    pub sbz2: U16,
}

/// ACE Header
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct AceHeader {
    pub ace_type: u8,
    pub ace_flags: u8,
    pub ace_size: U16,
}

/// Access Allowed ACE
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct AccessAllowedAce {
    pub header: AceHeader,
    pub mask: U32,
    // SID follows
}

#[derive(Debug, Clone, Copy)]
pub struct Ace<'a> {
    pub ace_type: u8,
    pub flags: AceFlags,
    pub mask: FileAccessMask,
    pub sid: &'a [u8],
}

impl<'a> Ace<'a> {
    pub const fn allow(sid: &'a [u8], mask: FileAccessMask, flags: AceFlags) -> Self {
        Self {
            ace_type: ACCESS_ALLOWED_ACE_TYPE,
            flags,
            mask,
            sid,
        }
    }

    pub const fn encoded_len(&self) -> usize {
        core::mem::size_of::<AccessAllowedAce>() + self.sid.len()
    }
}

/// Invalid input to the self-relative descriptor encoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityDescriptorError {
    InvalidSid,
    UnsupportedAceType,
    AclTooLarge,
    UnalignedSdsOffset,
}

fn validate_sid(sid: &[u8]) -> Result<(), SecurityDescriptorError> {
    if sid.len() < 8 || sid[0] != 1 || sid[1] > 15 || sid.len() != 8 + 4 * usize::from(sid[1]) {
        return Err(SecurityDescriptorError::InvalidSid);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub struct SecurityDescriptor<'a> {
    pub owner: &'a [u8],
    pub group: &'a [u8],
    pub dacl: &'a [Ace<'a>],
}

impl<'a> SecurityDescriptor<'a> {
    pub const fn new(owner: &'a [u8], group: &'a [u8], dacl: &'a [Ace<'a>]) -> Self {
        Self { owner, group, dacl }
    }

    pub const fn encoded_len(&self) -> usize {
        let mut len =
            core::mem::size_of::<SecurityDescriptorRelative>() + core::mem::size_of::<AclHeader>();

        let mut i = 0;

        while i < self.dacl.len() {
            len += self.dacl[i].encoded_len();
            i += 1;
        }

        len + self.owner.len() + self.group.len()
    }

    /// Convenience wrapper for known-valid descriptors.
    /// Panics for invalid input; use `try_to_bytes` for caller-provided data.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.try_to_bytes().expect("invalid security descriptor")
    }

    pub fn try_to_bytes(&self) -> Result<Vec<u8>, SecurityDescriptorError> {
        let mut out = Vec::new();
        self.encode_into(&mut out)?;
        Ok(out)
    }

    /// Appends a descriptor, leaving `out` unchanged on validation failure.
    /// Only access-allowed ACEs are supported by this encoder.
    pub fn encode_into(&self, out: &mut Vec<u8>) -> Result<(), SecurityDescriptorError> {
        let acl_size = self.validate()?;
        out.reserve(self.encoded_len());
        self.encode_with(acl_size, |bytes| out.extend_from_slice(bytes));
        Ok(())
    }

    /// Hash the exact encoded descriptor without allocating a payload buffer.
    pub fn hash(&self) -> Result<u32, SecurityDescriptorError> {
        let acl_size = self.validate()?;
        let mut hash = 0;
        self.encode_with(acl_size, |bytes| {
            // All canonical headers and validated SIDs are a multiple of four bytes.
            debug_assert!(bytes.len().is_multiple_of(4));
            hash = update_security_hash(hash, bytes);
        });
        Ok(hash)
    }

    fn validate(&self) -> Result<usize, SecurityDescriptorError> {
        validate_sid(self.owner)?;
        validate_sid(self.group)?;
        let mut acl_size = core::mem::size_of::<AclHeader>();
        for ace in self.dacl {
            if ace.ace_type != ACCESS_ALLOWED_ACE_TYPE {
                return Err(SecurityDescriptorError::UnsupportedAceType);
            }
            validate_sid(ace.sid)?;
            // Valid SIDs are at most 68 bytes; bound each addition before continuing.
            acl_size += ace.encoded_len();
            if acl_size > u16::MAX as usize {
                return Err(SecurityDescriptorError::AclTooLarge);
            }
        }
        Ok(acl_size)
    }

    fn encode_with(&self, acl_size: usize, mut emit: impl FnMut(&[u8])) {
        let header_size = core::mem::size_of::<SecurityDescriptorRelative>();
        let dacl_offset = header_size;
        let owner_offset = dacl_offset + acl_size;
        let group_offset = owner_offset + self.owner.len();

        let descriptor = SecurityDescriptorRelative {
            revision: 1,
            sbz1: 0,
            control: (SecurityDescriptorControl::SELF_RELATIVE
                | SecurityDescriptorControl::DACL_PRESENT)
                .bits()
                .into(),
            owner_offset: (owner_offset as u32).into(),
            group_offset: (group_offset as u32).into(),
            sacl_offset: 0.into(),
            dacl_offset: (dacl_offset as u32).into(),
        };

        let acl = AclHeader {
            revision: 2,
            sbz1: 0,
            acl_size: (acl_size as u16).into(),
            ace_count: (self.dacl.len() as u16).into(),
            sbz2: 0.into(),
        };

        emit(descriptor.as_bytes());
        emit(acl.as_bytes());

        for ace in self.dacl {
            let access_ace = AccessAllowedAce {
                header: AceHeader {
                    ace_type: ace.ace_type,
                    ace_flags: ace.flags.bits(),
                    ace_size: (ace.encoded_len() as u16).into(),
                },
                mask: ace.mask.bits().into(),
            };

            emit(access_ace.as_bytes());
            emit(ace.sid);
        }

        emit(self.owner);
        emit(self.group);
    }
}

/// Security descriptor for user-visible files and directories (164 bytes).
///
/// Owner = SYSTEM (S-1-5-18)
/// Group = Administrators (S-1-5-32-544)
/// DACL  = Full Control (0x001F01FF) with Container/Object inherit to:
///         Everyone, Authenticated Users, SYSTEM, Administrators, Users
///
/// Intended SecurityId: 0x100.
pub const SECURITY_DESCRIPTOR_EVERYONE: SecurityDescriptor<'static> = SecurityDescriptor::new(
    SID_SYSTEM,
    SID_ADMINISTRATORS,
    &[
        Ace::allow(SID_EVERYONE, FileAccessMask::FULL_CONTROL, ACE_INHERIT),
        Ace::allow(
            SID_AUTHENTICATED_USERS,
            FileAccessMask::FULL_CONTROL,
            ACE_INHERIT,
        ),
        Ace::allow(SID_SYSTEM, FileAccessMask::FULL_CONTROL, ACE_INHERIT),
        Ace::allow(
            SID_ADMINISTRATORS,
            FileAccessMask::FULL_CONTROL,
            ACE_INHERIT,
        ),
        Ace::allow(SID_USERS, FileAccessMask::FULL_CONTROL, ACE_INHERIT),
    ],
);

/// Security descriptor for NTFS system files.
///
/// Owner = SYSTEM (S-1-5-18)
/// Group = Administrators (S-1-5-32-544)
/// DACL:
///   SYSTEM         : Read / Write (0x0012019F)
///   Administrators : Read / Write (0x0012019F)
///
/// Intended SecurityId: 0x101.
pub const SECURITY_DESCRIPTOR_SYSTEM: SecurityDescriptor<'static> = SecurityDescriptor::new(
    SID_SYSTEM,
    SID_ADMINISTRATORS,
    &[
        Ace::allow(SID_SYSTEM, FileAccessMask::READ_WRITE, AceFlags::empty()),
        Ace::allow(
            SID_ADMINISTRATORS,
            FileAccessMask::READ_WRITE,
            AceFlags::empty(),
        ),
    ],
);

/// Standard Windows NTFS root directory security descriptor (228 bytes).
///
/// Owner = SYSTEM (S-1-5-18)
/// Group = SYSTEM (S-1-5-18)
/// DACL:
///   Administrators      : Full Control (0x001F01FF) + Container/Object inherit
///   SYSTEM              : Full Control (0x001F01FF) + Container/Object inherit
///   Authenticated Users : Modify (0x001301BF) + Container/Object inherit
///   Users               : Read & Execute (0x001200A9) + Container/Object inherit
const AUTHENTICATED_USERS_INHERITED: FileAccessMask = FileAccessMask::GENERIC_READ
    .union(FileAccessMask::GENERIC_WRITE)
    .union(FileAccessMask::GENERIC_EXECUTE)
    .union(FileAccessMask::DELETE);

const USERS_INHERITED: FileAccessMask =
    FileAccessMask::GENERIC_READ.union(FileAccessMask::GENERIC_EXECUTE);

pub const SECURITY_DESCRIPTOR_ROOT: SecurityDescriptor<'static> = SecurityDescriptor::new(
    SID_SYSTEM,
    SID_SYSTEM,
    &[
        Ace::allow(
            SID_ADMINISTRATORS,
            FileAccessMask::FULL_CONTROL,
            AceFlags::empty(),
        ),
        Ace::allow(
            SID_ADMINISTRATORS,
            FileAccessMask::GENERIC_ALL,
            ACE_INHERIT_ONLY,
        ),
        Ace::allow(SID_SYSTEM, FileAccessMask::FULL_CONTROL, AceFlags::empty()),
        Ace::allow(SID_SYSTEM, FileAccessMask::GENERIC_ALL, ACE_INHERIT_ONLY),
        Ace::allow(
            SID_AUTHENTICATED_USERS,
            FileAccessMask::MODIFY,
            AceFlags::empty(),
        ),
        Ace::allow(
            SID_AUTHENTICATED_USERS,
            AUTHENTICATED_USERS_INHERITED,
            ACE_INHERIT_ONLY,
        ),
        Ace::allow(SID_USERS, FileAccessMask::FILE_READ_EXEC, AceFlags::empty()),
        Ace::allow(SID_USERS, USERS_INHERITED, ACE_INHERIT_ONLY),
    ],
);

const _: () = {
    assert!(core::mem::size_of::<SecurityDescriptorRelative>() == 20);
    assert!(core::mem::size_of::<AclHeader>() == 8);
    assert!(core::mem::size_of::<AceHeader>() == 4);
    assert!(core::mem::size_of::<AccessAllowedAce>() == 8);
    assert!(core::mem::size_of::<SidHeader>() == 8);
    assert!(core::mem::size_of::<SecurityDescriptorHeader>() == 20);
    assert!(core::mem::align_of::<SecurityDescriptorHeader>() == 1);
    assert!(core::mem::offset_of!(SecurityDescriptorHeader, offset) == 8);
};

#[derive(Debug, PartialEq, Eq)]
pub enum SecureContentError {
    Descriptor(SecurityDescriptorError),
    DuplicateSecurityId,
    SdsBlockFull,
}

/// Encoded bytes of the three $Secure attributes.
pub struct SecureFileContent {
    pub sds: Vec<u8>,
    pub sii_entries: Vec<u8>,
    pub sdh_entries: Vec<u8>,
}

impl SecureFileContent {
    const BLOCK_SIZE: usize = 256 * 1024;
    const DEFAULT_DESCRIPTORS: [(u32, &'static SecurityDescriptor<'static>); 2] = [
        (SECURITY_ID_EVERYONE, &SECURITY_DESCRIPTOR_EVERYONE),
        (SECURITY_ID_SYSTEM, &SECURITY_DESCRIPTOR_SYSTEM),
    ];

    /// Size the built-in stream without constructing or hashing its contents.
    pub fn default_sds_size() -> usize {
        Self::sds_size(&Self::DEFAULT_DESCRIPTORS).expect("valid default SDS size")
    }

    fn sds_size(
        descriptors: &[(u32, &SecurityDescriptor<'_>)],
    ) -> Result<usize, SecureContentError> {
        let capacity = descriptors
            .iter()
            .try_fold(0usize, |size, (_, descriptor)| {
                size.checked_add(
                    (core::mem::size_of::<SecurityDescriptorHeader>()
                        + descriptor.encoded_len()
                        + 15)
                        & !15,
                )
                .filter(|&size| size <= Self::BLOCK_SIZE)
                .ok_or(SecureContentError::SdsBlockFull)
            })?;
        Ok(Self::BLOCK_SIZE + capacity)
    }

    /// Builds one primary SDS block and its mirror, plus the two resident indexes.
    pub fn new<const N: usize>(
        descriptors: [(u32, &SecurityDescriptor<'_>); N],
    ) -> Result<Self, SecureContentError> {
        let mut sds = Vec::with_capacity(Self::sds_size(&descriptors)?);
        let mut entries = [SecurityDescriptorHeader::new_zeroed(); N];
        for ((security_id, descriptor), entry) in descriptors.into_iter().zip(&mut entries) {
            let record = Sds::new(security_id, descriptor, sds.len() as u64)
                .map_err(SecureContentError::Descriptor)?;
            *entry = *record.header();
            record.to_raw_buffer(&mut sds);
        }
        entries.sort_unstable_by_key(|entry| entry.security_id.get());
        if entries
            .windows(2)
            .any(|pair| pair[0].security_id == pair[1].security_id)
        {
            return Err(SecureContentError::DuplicateSecurityId);
        }
        let sii_entries = encode_index(entries.map(Sii::new));
        entries.sort_unstable_by_key(|entry| (entry.hash.get(), entry.security_id.get()));
        let sdh_entries = encode_index(entries.map(Sdh::new));
        let primary_len = sds.len();
        sds.resize(Self::BLOCK_SIZE, 0);
        sds.extend_from_within(..primary_len);
        Ok(Self {
            sds,
            sii_entries,
            sdh_entries,
        })
    }
}

impl Default for SecureFileContent {
    fn default() -> Self {
        Self::new(Self::DEFAULT_DESCRIPTORS).expect("valid built-in descriptors")
    }
}

fn encode_index<T: IntoBytes + zerocopy::Immutable, const N: usize>(entries: [T; N]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(core::mem::size_of_val(&entries) + 16);
    bytes.extend_from_slice(entries.as_bytes());
    bytes.extend_from_slice(IndexEntryHeader::end_marker().as_bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_descriptor_headers_are_borrowed_with_correct_offsets() {
        let bytes = SECURITY_DESCRIPTOR_ROOT.to_bytes();

        assert_eq!(bytes.len(), 228);

        let (header, _) = SecurityDescriptorRelative::ref_from_prefix(&bytes).unwrap();

        assert_eq!(header.control.get(), 0x8004);
        assert_eq!(header.owner_offset.get(), 204);

        let (acl, _) =
            AclHeader::ref_from_prefix(&bytes[header.dacl_offset.get() as usize..]).unwrap();

        assert_eq!(acl.ace_count.get(), 8);
        assert_eq!(acl.acl_size.get(), 184);
    }

    #[test]
    fn sds_entry_keeps_header_payload_and_padding_consistent() {
        let invalid = SecurityDescriptor::new(&[], SID_SYSTEM, &[]);
        assert_eq!(
            Sds::new(42, &invalid, 16).unwrap_err(),
            SecurityDescriptorError::InvalidSid
        );
        assert_eq!(
            Sds::new(42, &SECURITY_DESCRIPTOR_SYSTEM, 17).unwrap_err(),
            SecurityDescriptorError::UnalignedSdsOffset
        );
        let entry = Sds::new(42, &SECURITY_DESCRIPTOR_SYSTEM, 16).unwrap();
        assert_eq!(entry.header().offset.get(), 16);
        assert_eq!(entry.header().security_id.get(), 42);
        assert_eq!(entry.header().length.get(), 120);
        assert_eq!(
            entry.header().hash.get(),
            entry.descriptor().hash().unwrap()
        );
        let mut out = Vec::from([0xaa; 16]);
        entry.to_raw_buffer(&mut out);
        assert_eq!(out.len(), 16 + entry.encoded_len());
        assert_eq!(&out[..16], &[0xaa; 16]);
        assert_eq!(
            &out[36..136],
            SECURITY_DESCRIPTOR_SYSTEM.try_to_bytes().unwrap().as_slice()
        );
        assert_eq!(&out[136..], &[0; 8]);
    }

    #[test]
    fn invalid_descriptors_do_not_modify_destination() {
        let mut out = Vec::from([0xaa; 3]);
        let mut bad_revision = SID_SYSTEM.to_vec();
        bad_revision[0] = 2;
        let mut excessive_count = SID_SYSTEM.to_vec();
        excessive_count[1] = 16;
        for sid in [&[][..], &SID_SYSTEM[..11], &bad_revision, &excessive_count] {
            let ace = [Ace::allow(
                sid,
                FileAccessMask::FILE_READ,
                AceFlags::empty(),
            )];
            for descriptor in [
                SecurityDescriptor::new(sid, SID_SYSTEM, &[]),
                SecurityDescriptor::new(SID_SYSTEM, sid, &[]),
                SecurityDescriptor::new(SID_SYSTEM, SID_SYSTEM, &ace),
            ] {
                assert_eq!(
                    descriptor.encode_into(&mut out),
                    Err(SecurityDescriptorError::InvalidSid)
                );
                assert_eq!(out, [0xaa; 3]);
            }
        }
        let mut ace = Ace::allow(SID_SYSTEM, FileAccessMask::FILE_READ, AceFlags::empty());
        ace.ace_type = 5;
        assert_eq!(
            SecurityDescriptor::new(SID_SYSTEM, SID_SYSTEM, &[ace]).encode_into(&mut out),
            Err(SecurityDescriptorError::UnsupportedAceType)
        );
        ace.ace_type = ACCESS_ALLOWED_ACE_TYPE;
        let aces = [ace; 3277];
        assert!(
            SecurityDescriptor::new(SID_SYSTEM, SID_SYSTEM, &aces[..3276])
                .try_to_bytes()
                .is_ok()
        );
        assert_eq!(
            SecurityDescriptor::new(SID_SYSTEM, SID_SYSTEM, &aces).encode_into(&mut out),
            Err(SecurityDescriptorError::AclTooLarge)
        );
        assert_eq!(out, [0xaa; 3]);
    }

    #[test]
    fn constructor_sorts_indexes_and_rejects_invalid_sets() {
        let content = SecureFileContent::new([
            (42, &SECURITY_DESCRIPTOR_SYSTEM),
            (7, &SECURITY_DESCRIPTOR_EVERYONE),
        ])
        .ok()
        .unwrap();
        assert_eq!(&content.sii_entries[16..20], &7u32.to_le_bytes());
        assert_eq!(&content.sii_entries[56..60], &42u32.to_le_bytes());
        assert!(matches!(
            SecureFileContent::new([
                (7, &SECURITY_DESCRIPTOR_SYSTEM),
                (7, &SECURITY_DESCRIPTOR_EVERYONE),
            ]),
            Err(SecureContentError::DuplicateSecurityId)
        ));
        let invalid = SecurityDescriptor::new(&[], &[], &[]);
        assert!(matches!(
            SecureFileContent::new([(7, &invalid)]),
            Err(SecureContentError::Descriptor(_))
        ));
        assert!(matches!(
            SecureFileContent::new([(7, &SECURITY_DESCRIPTOR_SYSTEM); 2049]),
            Err(SecureContentError::SdsBlockFull)
        ));
        let empty = SecureFileContent::new([]).ok().unwrap();
        assert_eq!(empty.sii_entries.len(), 16);
        assert_eq!(empty.sdh_entries.len(), 16);
    }

    #[test]
    fn default_stream_mirror_and_indices_match_descriptors() {
        let content = SecureFileContent::default();
        assert_eq!(content.sds.len(), 262464);
        assert_eq!(content.sds.len(), SecureFileContent::default_sds_size());
        assert_eq!(&content.sds[..320], &content.sds[262144..]);
        assert!(content.sds[320..262144].iter().all(|&b| b == 0));
        assert_eq!(content.sii_entries.len(), 96);
        assert_eq!(content.sdh_entries.len(), 112);
        let d_everyone = SECURITY_DESCRIPTOR_EVERYONE.try_to_bytes().unwrap();
        let d_system = SECURITY_DESCRIPTOR_SYSTEM.try_to_bytes().unwrap();
        for (index, expected) in [d_everyone.as_slice(), d_system.as_slice()]
            .into_iter()
            .enumerate()
        {
            let offset = [0, 192][index];
            let header =
                SecurityDescriptorHeader::ref_from_bytes(&content.sds[offset..offset + 20]).unwrap();
            assert_eq!(header.offset.get(), offset as u64);
            assert_eq!(header.length.get() as usize, 20 + expected.len());
            assert_eq!(
                &content.sds[offset + 20..offset + 20 + expected.len()],
                expected
            );
            assert_eq!(header.hash.get(), calculate_security_hash(expected));
            assert_eq!(
                &content.sii_entries[index * 40 + 20..index * 40 + 40],
                header.as_bytes()
            );
            let hash_entry = content.sdh_entries[..96]
                .chunks_exact(48)
                .find(|entry| &entry[20..24] == header.security_id.as_bytes())
                .unwrap();
            assert_eq!(&hash_entry[24..44], header.as_bytes());
        }
    }

    #[test]
    fn test_sii_layout_compliance() {
        let hash = 0x12345678;
        let id = 256;
        let offset = 0x1000;
        let length = 0x40;

        let data = SecurityDescriptorHeader {
            hash: hash.into(),
            security_id: (id).into(),
            offset: offset.into(),
            length: length.into(),
        };

        let res = encode_index([Sii::new(data)]);

        assert_eq!(res.len(), 40 + 16);
        assert_eq!(&res[0..2], &0x14u16.to_le_bytes());
        assert_eq!(&res[2..4], &0x14u16.to_le_bytes());
        assert_eq!(&res[4..8], &[0; 4]);
        assert_eq!(&res[8..10], &0x28u16.to_le_bytes());
        assert_eq!(&res[10..12], &0x04u16.to_le_bytes());
        assert_eq!(&res[16..20], &id.to_le_bytes());
        assert_eq!(&res[20..24], &hash.to_le_bytes());
        assert_eq!(&res[24..28], &id.to_le_bytes());
        assert_eq!(&res[28..36], &offset.to_le_bytes());
        assert_eq!(&res[36..40], &length.to_le_bytes());
        assert_eq!(&res[40..48], &[0; 8]);
        assert_eq!(&res[48..50], &16u16.to_le_bytes());
        assert_eq!(res[52], 0x02);
    }

    #[test]
    fn test_sdh_layout_compliance() {
        let hash = 0xAABBCCDD;
        let id = 256;
        let offset = 0x2000;
        let length = 0x60;

        let data = SecurityDescriptorHeader {
            hash: hash.into(),
            security_id: (id).into(),
            offset: offset.into(),
            length: length.into(),
        };

        let res = encode_index([Sdh::new(data)]);

        assert_eq!(res.len(), 48 + 16);
        assert_eq!(&res[0..2], &0x18u16.to_le_bytes());
        assert_eq!(&res[2..4], &0x14u16.to_le_bytes());
        assert_eq!(&res[4..8], &[0; 4]);
        assert_eq!(&res[8..10], &0x30u16.to_le_bytes());
        assert_eq!(&res[10..12], &0x08u16.to_le_bytes());
        assert_eq!(&res[16..20], &hash.to_le_bytes());
        assert_eq!(&res[20..24], &id.to_le_bytes());
        assert_eq!(&res[24..28], &hash.to_le_bytes());
        assert_eq!(&res[28..32], &id.to_le_bytes());
        assert_eq!(&res[32..40], &offset.to_le_bytes());
        assert_eq!(&res[40..44], &length.to_le_bytes());
        assert_eq!(&res[44..48], b"I\0I\0");
        assert_eq!(res[60], 0x02);
    }
}
