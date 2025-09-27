// examples/generate_test_data.rs
// SPDX-License-Identifier: MIT

use std::fs::{File, create_dir_all};
use std::io::Write;
use std::path::{Path, PathBuf};

fn main() {
    let test_data_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data");

    // Clean + recreate
    if test_data_dir.exists() {
        std::fs::remove_dir_all(&test_data_dir).expect("Failed to clean test_data");
    }
    create_dir_all(&test_data_dir).expect("Failed to create test_data");

    // 1. Top-level files
    create_file(
        test_data_dir.join("README.txt"),
        b"This is a test README file.\n",
    );
    create_file(test_data_dir.join("data.bin"), &vec![0xAA; 512 * 4]);
    create_file(
        test_data_dir.join("image.png"),
        &[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'],
    );

    // 2. Subdir1
    let sub1 = &test_data_dir.join("subdir1");
    create_dir_all(&sub1).expect("Failed to create subdir1");
    create_file(sub1.join("file1.txt"), b"Hello from subdir1/file1.txt\n");
    create_file(sub1.join("file2.txt"), b"Hello from subdir1/file2.txt\n");

    // 3. Subdir2/deep_dir
    let deep = &test_data_dir.join("subdir2/deep_dir");
    create_dir_all(&deep).expect("Failed to create subdir2/deep_dir");
    create_file(
        deep.join("deep_file.txt"),
        b"This is deep inside subdir2/deep_dir\n",
    );

    // 4. Long filename test
    create_file(
        test_data_dir.join("long_named_file_for_testing_long_filename_support_on_different_fs.txt"),
        b"This file has a long filename.\n",
    );

    // 5. Many small files
    let many = &test_data_dir.join("many_files");
    create_dir_all(&many).expect("Failed to create many_files");
    for i in 0..100 {
        create_file(
            many.join(format!("file_{i:03}.txt")),
            format!("File number {i}\n").as_bytes(),
        );
    }

    // 6. Big file
    create_file(
        test_data_dir.join("big_file.bin"),
        &vec![0xAB; 2 * 1024 * 1024], // 2 MB
    );

    // 7. Deep hierarchy
    let deep_path = test_data_dir.join("deep/deeper/deeper2/deeper3");
    create_dir_all(&deep_path).expect("Failed to create deep hierarchy");
    create_file(deep_path.join("deep_file.txt"), b"I'm very deep!\n");

    // 8. Special names
    create_file(test_data_dir.join("A.TXT"), b"A\n");
    create_file(test_data_dir.join("a.txt"), b"a\n");
    create_file(test_data_dir.join("caf\u{00E9}.txt"), "Café\n".as_bytes());
    create_file(test_data_dir.join("🚀.txt"), b"Rocket\n");

    println!("✅ test_data/ generated.");
}

fn create_file(path: impl AsRef<Path>, content: &[u8]) {
    let mut f = File::create(path).expect("Failed to create file");
    f.write_all(content).expect("Failed to write file");
}
