pub fn pretty_bytes(n: u64) -> String {
    const UNITS: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let mut val = n as f64;
    let mut idx = 0usize;
    while val >= 1024.0 && idx + 1 < UNITS.len() {
        val /= 1024.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{} {}", sep_u64(n), UNITS[idx])
    } else {
        format!("{:.1} {}", val, UNITS[idx])
    }
}

pub fn sep_u64(mut n: u64) -> String {
    // thousands separator "fine": 12 345 678
    if n < 1_000 {
        return n.to_string();
    }
    let mut parts: Vec<String> = Vec::new();
    while n >= 1_000 {
        parts.push(format!("{:03}", (n % 1_000)));
        n /= 1_000;
    }
    parts.push(n.to_string());
    parts.reverse();
    parts.join(" ") // non-breaking narrow space
}

#[allow(dead_code)]
pub fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    &s[..max]
}
