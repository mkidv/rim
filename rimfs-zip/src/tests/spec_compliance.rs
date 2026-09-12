// SPDX-License-Identifier: MIT

//! ZIP (PKWARE APPNOTE.TXT) on-disk specification compliance tests.

use crate::types::*;

#[test]
fn test_zip_canonical_signatures_and_sizes() {
    assert_eq!(LOCAL_FILE_HEADER_SIG, 0x0403_4b50);
    assert_eq!(CENTRAL_DIR_HEADER_SIG, 0x0201_4b50);
    assert_eq!(END_OF_CENTRAL_DIR_SIG, 0x0605_4b50);

    assert_eq!(LOCAL_FILE_HEADER_FIXED_SIZE, 30);
    assert_eq!(CENTRAL_DIR_HEADER_FIXED_SIZE, 46);
    assert_eq!(END_OF_CENTRAL_DIR_FIXED_SIZE, 22);

    assert_eq!(METHOD_STORE, 0);
    assert_eq!(METHOD_DEFLATE, 8);
}
