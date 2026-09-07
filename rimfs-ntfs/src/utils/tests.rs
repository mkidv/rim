use super::*;
use crate::types::NtfsFileNameNamespace;
use crate::upcase::UpcaseHandle;
use core::cmp::Ordering;

#[test]
fn test_data_run_encoding() {
    // Simple run: 10 clusters starting at LCN 100
    let (run, len) = encode_data_run(100, 10);
    assert!(len > 0);
    // First byte is header
    assert_eq!(run[0] & 0x0F, 1); // Length needs 1 byte

    // Length 128 (0x80) has high bit set, must encode as 2 bytes to stay positive
    let (run128, len128) = encode_data_run(4101, 128);
    assert_eq!(
        run128[0] & 0x0F,
        2,
        "Length 128 must use 2 bytes (0x80, 0x00)"
    );
    assert_eq!(run128[1], 0x80);
    let mut full_runs = run128[..len128].to_vec();
    full_runs.push(0); // terminator
    let runs: Vec<_> = crate::view::runlist::NtfsRunList::new(&full_runs)
        .iter()
        .collect();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].len, 128);
    assert_eq!(runs[0].lcn, Some(4101));
}

#[test]
fn test_mft_reference() {
    let record_num = 12345u64;
    let seq = 7u16;

    let reference = build_mft_reference(record_num, seq);
    let (parsed_num, parsed_seq) = parse_mft_reference(reference);

    assert_eq!(parsed_num, record_num);
    assert_eq!(parsed_seq, seq);
}

#[test]
fn test_usa_size() {
    // 1024-byte record with 512-byte sectors = 2 sectors + check = 3 words
    assert_eq!(calculate_usa_size(1024, 512), 3);

    // 4096-byte record with 512-byte sectors = 8 sectors + check = 9 words
    assert_eq!(calculate_usa_size(4096, 512), 9);
}

#[test]
fn test_compare_names_upcase() {
    use crate::upcase::UpcaseFlavor;
    let upcase = UpcaseHandle::from_flavor(&UpcaseFlavor::Windows);

    let name1: Vec<u16> = "filename.txt".encode_utf16().collect();
    let name2: Vec<u16> = "FILENAME.TXT".encode_utf16().collect();
    let name3: Vec<u16> = "filename.tyt".encode_utf16().collect();

    assert_eq!(
        compare_names_upcase(&name1, &name2, &upcase),
        Ordering::Equal
    );
    assert_eq!(
        compare_names_upcase(&name1, &name3, &upcase),
        Ordering::Less
    );

    // Unicode check: Cyrillic 'a' (U+0430) and 'A' (U+0410)
    let cyr_a_lower: Vec<u16> = vec![0x0430];
    let cyr_a_upper: Vec<u16> = vec![0x0410];
    assert_eq!(
        compare_names_upcase(&cyr_a_lower, &cyr_a_upper, &upcase),
        Ordering::Equal
    );

    // Standard ASCII order check
    assert!(
        compare_names_upcase(
            &"a".encode_utf16().collect::<Vec<_>>(),
            &"B".encode_utf16().collect::<Vec<_>>(),
            &upcase
        ) == Ordering::Less
    );
}

#[test]
fn test_dos_8_3_and_namespace() {
    assert!(is_valid_dos_8_3("FILE.TXT"));
    assert!(is_valid_dos_8_3("test_win.txt"));
    assert!(!is_valid_dos_8_3("win_payload"));
    assert!(!is_valid_dos_8_3("long_filename.extension"));

    assert_eq!(
        determine_file_name_namespace("FILE.TXT"),
        NtfsFileNameNamespace::Win32AndDos
    );
    assert_eq!(
        determine_file_name_namespace("win_payload"),
        NtfsFileNameNamespace::Win32
    );

    assert_eq!(generate_dos_8_3_name("win_payload"), "WIN_PA~1");
    assert_eq!(generate_dos_8_3_name("from_windows"), "FROM_W~1");
    assert_eq!(
        generate_dos_8_3_name("long_filename.extension"),
        "LONG_F~1.EXT"
    );
    assert_eq!(generate_dos_8_3_name("document.tar.gz"), "DOCUME~1.GZ");
    assert_eq!(generate_dos_8_3_name("a+b=c[1].txt"), "ABC1~1.TXT");
}
