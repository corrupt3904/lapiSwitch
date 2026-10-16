//! Shared decimal byte units and elapsed-time formatting for operation logs.

pub fn bytes(bytes: u64) -> String {
    decimal_bytes(bytes as f64)
}

pub fn decimal_bytes(bytes: f64) -> String {
    for (scale, unit) in [(1e9, "GB"), (1e6, "MB"), (1e3, "KB")] {
        if bytes >= scale {
            return format!("{:.1} {unit}", bytes / scale);
        }
    }
    format!("{bytes:.0} B")
}

pub fn elapsed(seconds: f64) -> String {
    if seconds < 0.1 {
        "<0.1 s".to_owned()
    } else {
        format!("{seconds:.1} s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_transfers_and_short_durations_remain_readable() {
        assert_eq!(bytes(123), "123 B");
        assert_eq!(bytes(150_000), "150.0 KB");
        assert_eq!(bytes(9_300_000), "9.3 MB");
        assert_eq!(bytes(8_388_608), "8.4 MB");
        assert_eq!(elapsed(0.042), "<0.1 s");
        assert_eq!(elapsed(1.234), "1.2 s");
    }
}
