pub use crate::core::traits::*;

use rimio::BlockIO;

use crate::fs::fat32::traits::*;

pub struct Fat32;

impl<'a> FsFilesystem<'a> for Fat32 {
    type Meta = Fat32Meta;
    type AllocUnit = u32;
    type Handle = Fat32Handle;
    type Allocator = Fat32Allocator<'a>;
    type Formatter = Fat32Formatter<'a, dyn BlockIO + 'a>;
    type Injector = Fat32Injector<'a, dyn BlockIO + 'a>;
    type Checker = Fat32Checker<'a, dyn BlockIO + 'a>;
    type Parser = Fat32Parser<'a, dyn BlockIO + 'a>;

    fn allocator(meta: &'a Self::Meta) -> Self::Allocator {
        Fat32Allocator::new(meta)
    }

    fn formatter(io: &'a mut (dyn BlockIO + 'a), meta: &'a Self::Meta) -> Self::Formatter {
        Fat32Formatter::new(io, meta)
    }

    fn injector(
        io: &'a mut (dyn BlockIO + 'a),
        allocator: &'a mut Self::Allocator,
        meta: &'a Self::Meta,
    ) -> Self::Injector {
        Fat32Injector::new(io, allocator, meta)
    }

    fn checker(io: &'a mut (dyn BlockIO + 'a), meta: &'a Self::Meta) -> Self::Checker {
        Fat32Checker::new(io, meta)
    }

    fn parser(io: &'a mut (dyn BlockIO + 'a), meta: &'a Self::Meta) -> Self::Parser {
        Fat32Parser::new(io, meta)
    }

    fn identifier() -> &'static str {
        "fat32"
    }
}
