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

#[macro_export]
macro_rules! blockio_unit_test {
    ($ctor:expr) => {
        use $crate::{BlockIO, BlockIOExt, BlockIOSetLen};

        #[test]
        fn test_rw() {
            let mut io = $ctor;
            io.write_at(10, &[1, 2, 3, 4]).unwrap();

            let mut buf = [0u8; 4];
            io.read_at(10, &mut buf).unwrap();

            assert_eq!(buf, [1, 2, 3, 4]);
        }

        #[test]
        fn test_set_len_safe() {
            let mut io = $ctor;
            if let Some(io_len) = (&mut io as &mut dyn BlockIOSetLen).set_len(512).ok() {
                assert_eq!((), io_len);
            }
        }

        #[test]
        fn test_best_effort_rw_unaligned() {
            let mut io = $ctor;

            let input = [0xABu8; 17];
            let mut output = [0u8; 17];

            io.write_block_best_effort(5, &input, 8).unwrap();
            io.read_block_best_effort(5, &mut output, 8).unwrap();

            assert_eq!(input, output);
        }

        #[test]
        fn test_multi_rw() {
            let mut io = $ctor;

            let cluster_size = 8;
            let clusters = 4;
            let total_size = cluster_size * clusters;

            let input = [0xCDu8; 32];
            let mut output = [0u8; 32];

            let offsets: Vec<u64> = (0..clusters)
                .map(|i| i as u64 * cluster_size as u64)
                .collect();

            io.write_multi_at(&offsets, cluster_size, &input).unwrap();
            io.read_multi_at(&offsets, cluster_size, &mut output)
                .unwrap();

            assert_eq!(input, output);
        }

        #[test]
        fn test_streamed_bytes_rw() {
            let mut buffer = [0u8; 1024];
            let mut io = MemBlockIO::new(&mut buffer);

            // Write 10 u32 values (little endian)
            io.write_streamed_bytes::<4>(0, 10, 5, |i| (i as u32).to_le_bytes())
                .unwrap();

            // Read and verify
            let mut values = [0u32; 10];
            io.read_streamed_bytes::<4>(0, 10, 5, |i, bytes| {
                values[i] = u32::from_le_bytes(*bytes);
            })
            .unwrap();

            for (i, val) in values.iter().enumerate() {
                assert_eq!(*val, i as u32);
            }
        }

        #[test]
        fn test_zero_fill() {
            let mut io = $ctor;

            io.write_at(42, &[0xFF; 8]).unwrap();
            io.zero_fill(42, 8).unwrap();

            let mut buf = [0xAA; 8];
            io.read_at(42, &mut buf).unwrap();

            assert_eq!(buf, [0u8; 8]);
        }
    };
}
