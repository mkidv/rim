// SPDX-License-Identifier: MIT

//! Generic verification suite for RimIO implementations.

use crate::prelude::*;

pub fn check_basic_rw(io: &mut impl RimIO) {
    let data = [0xAA, 0xBB, 0xCC, 0xDD];
    io.write_at(0, &data).expect("write_at failed");

    let mut buf = [0u8; 4];
    io.read_at(0, &mut buf).expect("read_at failed");
    assert_eq!(buf, data, "Read data mismatch");
}

pub fn check_rw_at_offset(io: &mut impl RimIO) {
    let data = [1, 2, 3, 4, 5];
    io.write_at(100, &data).unwrap();

    let mut buf = [0u8; 5];
    io.read_at(100, &mut buf).unwrap();
    assert_eq!(buf, data);
}

pub fn check_zero_fill(io: &mut impl RimIO) {
    io.write_at(50, &[0xFF; 10]).unwrap();
    io.zero_fill(50, 10).unwrap();

    let mut buf = [0x11; 10];
    io.read_at(50, &mut buf).unwrap();
    assert_eq!(buf, [0u8; 10], "Zero fill failed");
}

pub fn check_bounds(io: &mut impl RimIO, len: u64, allow_growth: bool) {
    let mut buf = [0u8; 1];
    assert!(
        io.read_at(len, &mut buf).is_err(),
        "Read at bounds limit should fail (EOF)"
    );
    assert!(io.read_at(len + 1, &mut buf).is_err());

    if !allow_growth {
        assert!(
            io.write_at(len, &buf).is_err(),
            "Write at bounds limit should fail"
        );
        assert!(io.write_at(len + 1, &buf).is_err());
    } else {
        // If growable, writing past len should succeed (and grow file)
        io.write_at(len + 10, &buf).unwrap();
    }

    // Should succeed before len (if len > 0)
    if len > 0 {
        assert!(
            io.read_at(len - 1, &mut buf).is_ok(),
            "Should perform partial/full read at last byte"
        );
    }
}

pub fn check_set_len(io: &mut impl RimIOSetLen) {
    // Resize up
    io.set_len(2048).unwrap();
    io.write_at(2000, &[0xAA]).unwrap();
    let mut buf = [0u8; 1];
    io.read_at(2000, &mut buf).unwrap();
    assert_eq!(buf[0], 0xAA);

    // Resize down
    io.set_len(1024).unwrap();
    assert!(io.read_at(1500, &mut buf).is_err());
}
