// // SPDX-License-Identifier: MIT

// use rimfs::{
//     Fat32Builder, Fat32Meta, Fat32Parser, FsBuilder, FsParser, MemFsIO, MemFsParser,
// };

// #[test]
// fn test_fat32_parse_and_validate() {
//     const SIZE_MB: u64 = 32;
//     const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;

//     // Load test_data dir into memory
//     let test_data_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
//         .join("test_data");
//     let test_data_path = test_data_dir.to_str().unwrap();

//     let mut mem_parser =
//         MemFsParser::load_from_disk(test_data_path).expect("failed to load test data into memory");

//     // Allocate image buffer
//     let mut buf = vec![0u8; SIZE_BYTES as usize];
//     let mut mem_io = MemFsIO::new_zero(&mut buf);
//     let meta = Fat32Meta::new(SIZE_BYTES, Some("TESTFS".to_string()));
//     let mut builder = Fat32Builder::new(&mut mem_io, &meta);

//     // Format + Inject
//     builder.format(true).expect("format failed");
//     builder
//         .inject(&mut mem_parser, test_data_path)
//         .expect("inject failed");

//     // Parse
//     let mut parser = Fat32Parser::new(&mut mem_io, &meta);
//     let root = parser.parse_tree("/").expect("parse_tree failed");

//     // Check big_file.bin exists and has expected size
//     let big_file = root
//         .find("/test_data/big_file.bin")
//         .expect("big_file.bin not found");
//     assert_eq!(big_file.size(), 2 * 1024 * 1024, "big_file.bin size mismatch");

//     // Check some LFN files exist
//     assert!(root.find("/test_data/café.txt").is_some(), "café.txt not found");
//     assert!(root
//         .find("/test_data/long_named_file_for_testing_fat32_long_filename_support.txt")
//         .is_some(), "LFN test file not found");

//     // Check UTF-8 filename with emoji
//     assert!(root.find("/test_data/🚀.txt").is_some(), "🚀.txt not found");

//     // Check one deep file
//     assert!(root
//         .find("/test_data/deep/deeper/deeper2/deeper3/deep_file.txt")
//         .is_some(), "deep_file.txt not found");

//     // Check all 100 many_files are present
//     for i in 0..100 {
//         let name = format!("/test_data/many_files/file_{:03}.txt", i);
//         assert!(
//             root.find(&name).is_some(),
//             "Missing file: {}",
//             name
//         );
//     }

//     // If we reached here, all checks passed
//     println!("✅ FAT32 image validated successfully.");
// }
