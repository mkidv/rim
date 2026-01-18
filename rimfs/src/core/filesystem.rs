// SPDX-License-Identifier: MIT

use rimio::RimIO;

use crate::core::traits::{
    FsAllocator, FsChecker, FsFormatter, FsHandle, FsMeta, FsNodeInjector, FsResolver,
};
/// Unified trait representing a filesystem.
/// It encapsulates the fundamental components required for generation, injection, and verification.
pub trait FsFilesystem<'a> {
    /// Type of static metadata (e.g. `Fat32Meta`).
    type Meta: FsMeta<Self::AllocUnit> + Clone + 'a;

    /// Logical allocation unit (e.g. cluster ID, inode number...).
    type AllocUnit: Ord + Copy;

    /// Handle returned during allocations (may contain additional metadata).
    type Handle: FsHandle + Clone;

    /// Allocator used to reserve disk space.
    type Allocator: FsAllocator<Self::Handle> + 'a;

    /// Formatter responsible for writing the initial FS layout.
    type Formatter: FsFormatter + 'a;

    /// Injector responsible for recursive injection of files/directories.
    type Injector: FsNodeInjector<Self::Handle> + 'a;

    /// Checker responsible for internal structural validations of the FS.
    type Checker: FsChecker + 'a;

    /// Parser responsible for parsing the filesystem structure.
    type Parser: FsResolver + 'a;

    /// Creates a new allocator from the metadata.
    fn allocator(meta: &'a Self::Meta) -> Self::Allocator;

    /// Creates a new instance of the formatter.
    fn formatter(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Formatter;

    /// Creates a new injector instance from the allocator.
    fn injector(
        io: &'a mut (dyn RimIO + 'a),
        allocator: &'a mut Self::Allocator,
        meta: &'a Self::Meta,
    ) -> crate::core::FsInjectorResult<Self::Injector>;

    /// Creates a new checker from the metadata.
    fn checker(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Checker;

    fn parser(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Parser;

    /// Optional: FS name for dynamic identification (usable in a registry)
    fn identifier() -> &'static str {
        "UNKNOWN"
    }
}
