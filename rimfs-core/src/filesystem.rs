// SPDX-License-Identifier: MIT

use rimio::RimIO;

use crate::traits::*;

/// Unified trait representing a filesystem.
/// It encapsulates the fundamental components required for generation, injection, and verification.
pub trait FsFilesystem<'a> {
    /// Logical allocation unit (e.g. cluster ID, inode number...).
    type Unit: Ord + Copy;

    /// Type of static metadata (e.g. `Fat32Meta`).
    type Meta: FsMeta<Self::Unit> + Clone + 'a;

    /// Handle returned during allocations (may contain additional metadata).
    type Handle: FsHandle + Clone;

    /// Formatter responsible for writing the initial FS layout.
    type Formatter: FsFormatter + 'a;

    /// Injector responsible for recursive injection of files/directories.
    type Injector: FsTreeInjector<Self::Handle> + 'a;

    /// Checker responsible for internal structural validations of the FS.
    type Checker: FsChecker + 'a;

    /// Resolver responsible for resolving paths and ensuring filesystem consistency.
    type Resolver: FsTreeResolver + 'a;

    /// Creates a new instance of the formatter.
    fn formatter(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Formatter;

    /// Creates a new injector instance from the allocator.
    fn injector(
        io: &'a mut (dyn RimIO + 'a),
        meta: &'a Self::Meta,
    ) -> crate::FsInjectorResult<Self::Injector>;

    /// Creates a new checker from the metadata.
    fn checker(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Checker;

    fn resolver(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Resolver;

    /// Optional: FS name for dynamic identification (usable in a registry)
    fn identifier() -> &'static str {
        "UNKNOWN"
    }
}
