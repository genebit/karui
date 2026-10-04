//! Numbers as people read them.
//!
//! Decimal units, as Finder, Explorer, and every storage label use, so a file
//! karui calls 10 MB is 10 MB in the file manager next to it. Mirrored by the
//! helpers in `src/lib/utils.ts`.

pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["kB", "MB", "GB", "TB"];
    if n < 1000 {
        return format!("{n} B");
    }
    let mut value = n as f64;
    let mut unit = "B";
    for next in UNITS {
        if value < 1000.0 {
            break;
        }
        value /= 1000.0;
        unit = next;
    }
    format!("{value:.1} {unit}")
}

/// `75.4` → `1:15`, `3725` → `1:02:05`.
pub fn duration(secs: f64) -> String {
    let total = secs.max(0.0).round() as u64;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Size change as a signed percentage, e.g. `−74%`.
pub fn change(input: u64, output: u64) -> String {
    if input == 0 {
        return "n/a".into();
    }
    let pct = (output as f64 / input as f64 - 1.0) * 100.0;
    let rounded = pct.round() as i64;
    match rounded.signum() {
        -1 => format!("\u{2212}{}%", -rounded),
        1 => format!("+{rounded}%"),
        _ => "±0%".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(bytes(999), "999 B");
        assert_eq!(bytes(1_500), "1.5 kB");
        assert_eq!(bytes(120_400_000), "120.4 MB");
        assert_eq!(bytes(2_000_000_000), "2.0 GB");
        assert_eq!(duration(75.4), "1:15");
        assert_eq!(duration(3725.0), "1:02:05");
        assert_eq!(change(100, 26), "\u{2212}74%");
        assert_eq!(change(100, 112), "+12%");
        assert_eq!(change(0, 5), "n/a");
    }
}
