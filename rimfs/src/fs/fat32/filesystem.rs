pub use crate::core::traits::*;

use rimio::RimIO;

use crate::fs::fat32::traits::*;

pub struct Fat32;

impl<'a> FsFilesystem<'a> for Fat32 {
    type Meta = Fat32Meta;
    type AllocUnit = u32;
    type Handle = Fat32Handle;
    type Allocator = Fat32Allocator<'a>;
    type Formatter = Fat32Formatter<'a, dyn RimIO + 'a>;
    type Injector = Fat32Injector<'a, dyn RimIO + 'a>;
    type Checker = Fat32Checker<'a, dyn RimIO + 'a>;
    type Parser = Fat32Resolver<'a, dyn RimIO + 'a>;

    fn allocator(meta: &'a Self::Meta) -> Self::Allocator {
        Fat32Allocator::new(meta)
    }

    fn formatter(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Formatter {
        Fat32Formatter::new(io, meta)
    }

    fn injector(
        io: &'a mut (dyn RimIO + 'a),
        allocator: &'a mut Self::Allocator,
        meta: &'a Self::Meta,
    ) -> crate::core::FsInjectorResult<Self::Injector> {
        Ok(Fat32Injector::new(io, allocator, meta))
    }

    fn checker(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Checker {
        Fat32Checker::new(io, meta)
    }

    fn parser(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Parser {
        Fat32Resolver::new(io, meta)
    }

    fn identifier() -> &'static str {
        "fat32"
    }
}
