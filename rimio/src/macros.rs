/// Automatically implements read/write functions for primitive types on BlockIO
#[macro_export]
macro_rules! blockio_impl_primitive_rw {
    ($($ty:ty),+ $(,)?) => {
        $(
            paste::paste! {
                #[inline(always)]
                fn [<write_ $ty _at>](&mut self, offset: u64, value: $ty) -> BlockIOResult {
                    let buf = value.to_le_bytes();
                    self.write_at(offset, &buf)
                }

                #[inline(always)]
                fn [<read_ $ty _at>](&mut self, offset: u64) -> BlockIOResult<$ty> {
                    let mut buf = [0u8; core::mem::size_of::<$ty>()];
                    self.read_at(offset, &mut buf)?;
                    Ok(<$ty>::from_le_bytes(buf))
                }
            }
        )+
    };
}
