// SPDX-License-Identifier: MIT

#[macro_export]
/// Defines a set of GPT partition types, along with associated constants, detection functions, and an enum for partition kinds.
///
/// This macro generates:
/// - A constant `[u8; 16]` for each partition type GUID.
/// - A function to detect the offset of the first partition of each type.
/// - A function to check if a partition entry matches a given type.
/// - An enum `GptPartitionKind` representing all defined partition types and an `Unknown` variant for unrecognized GUIDs.
/// - Implementations for converting between GUIDs and `GptPartitionKind`.
/// - A `Display` implementation for `GptPartitionKind`.
///
/// # Example
/// ```rust
/// use rimpart::define_partition_types;
///
/// define_partition_types! {
///     EFI => "EFI System Partition", [0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11, 0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B],
///     LINUX_FS => "Linux Filesystem", [0x0F, 0xC6, 0x69, 0xE8, 0xE8, 0x3A, 0xC1, 0x4D, 0x9A, 0x3E, 0x4B, 0xB1, 0x6E, 0xD6, 0x49, 0xFA],
/// }
/// ```
///
/// # Parameters
/// - `$name`: Identifier for the partition type (used for enum variant and function/constant names).
/// - `$desc`: Description string for the partition type.
/// - `$guid`: 16-byte array representing the partition type GUID.
///
/// # Generated Items
/// For each partition type:
/// - `pub const GPT_PARTITION_TYPE_<NAME>: [u8; 16]`
/// - `pub fn detect_<name>_partition_offset(io: &mut dyn RimIO) -> PartResult<u64>`
/// - `pub fn is_<name>_partition(entry: &GptEntry) -> bool`
///
/// Also generates:
/// - `pub enum GptPartitionKind`
/// - Implementations for `from_guid`, `as_guid`, and `Display` for `GptPartitionKind`.
///
/// # Note
/// This macro requires the `paste` crate for identifier concatenation.
macro_rules! define_partition_types {
    (
        $(
            $name:ident => $desc:expr, $guid:expr
        ),+ $(,)?
    ) => {
        paste::paste! {
            $(
                #[doc = $desc]
                pub const [<GPT_PARTITION_TYPE_ $name:upper>]: [u8; 16] = $guid;

                #[doc = concat!("Returns the offset of the first GPT partition of type: ", $desc)]
                pub fn [<detect_ $name:lower _partition_offset>](
                    io: &mut dyn rimio::prelude::RimIO,
                ) -> $crate::errors::PartResult<u64> {
                    $crate::utils::detect_partition_offset_by_type_guid(io, &[<GPT_PARTITION_TYPE_ $name:upper>])
                }

                #[doc = concat!("Checks if a GPT partition is of type: ", $desc)]
                pub fn [<is_ $name:lower _partition>](
                    entry: &$crate::gpt::GptEntry,
                ) -> bool {
                    entry.type_guid == [<GPT_PARTITION_TYPE_ $name:upper>]
                }
            )+

            #[derive(Debug, Clone, PartialEq, Eq)]
            pub enum GptPartitionKind {
                $($name,)+
                Unknown([u8; 16]),
            }

            impl GptPartitionKind {
                pub fn from_guid(guid: &[u8; 16]) -> Self {
                    match guid {
                        $(g if g == &[<GPT_PARTITION_TYPE_ $name:upper>] => Self::$name,)+
                        other => Self::Unknown(*other),
                    }
                }

                pub fn as_guid(&self) -> Option<&'static [u8; 16]> {
                    match self {
                        $(Self::$name => Some(&[<GPT_PARTITION_TYPE_ $name:upper>]),)+
                        Self::Unknown(_) => None,
                    }
                }
            }

            impl core::fmt::Display for GptPartitionKind {
                fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                    match self {
                        $(Self::$name => write!(f, $desc),)+
                        Self::Unknown(guid) => write!(f, "Unknown ({:02X?})", guid),
                    }
                }
            }
        }
    };
}
