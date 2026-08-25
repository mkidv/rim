// SPDX-License-Identifier: MIT

use std::time::Duration;

/// Formats a byte size into human-readable representation (e.g. 512 MB, 1.25 GB).
pub fn pretty_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    const TB: u64 = 1024 * GB;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Formats an integer with thousands separator spaces.
pub fn sep_u64(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    let rem = s.len() % 3;
    let mut i = 0;
    if rem > 0 {
        out.push_str(&s[0..rem]);
        i = rem;
        if i < s.len() {
            out.push(' ');
        }
    }
    while i < s.len() {
        out.push_str(&s[i..i + 3]);
        i += 3;
        if i < s.len() {
            out.push(' ');
        }
    }
    out
}

/// Formats a duration nicely (e.g. 0.12s, 1.45s).
pub fn format_duration(d: Duration) -> String {
    format!("{:.2}s", d.as_secs_f64())
}
