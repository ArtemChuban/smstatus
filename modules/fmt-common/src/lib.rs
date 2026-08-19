pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0}{}", UNITS[unit])
    } else {
        format!("{value:.1}{}", UNITS[unit])
    }
}

pub fn format_usage(format: &str, total_bytes: u64, used_bytes: u64, free_bytes: u64) -> String {
    format
        .replace("{total}", &human_bytes(total_bytes))
        .replace("{used}", &human_bytes(used_bytes))
        .replace("{free}", &human_bytes(free_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_zero() {
        assert_eq!(human_bytes(0), "0B");
    }

    #[test]
    fn human_bytes_below_1k() {
        assert_eq!(human_bytes(512), "512B");
    }

    #[test]
    fn human_bytes_exact_1k_boundary() {
        assert_eq!(human_bytes(1024), "1.0K");
    }

    #[test]
    fn human_bytes_exact_1g_boundary() {
        assert_eq!(human_bytes(1024 * 1024 * 1024), "1.0G");
    }

    #[test]
    fn human_bytes_terabyte_range() {
        assert_eq!(human_bytes(2 * 1024u64.pow(4)), "2.0T");
    }

    #[test]
    fn human_bytes_caps_at_terabyte_unit() {
        assert_eq!(human_bytes(1024u64.pow(5)), "1024.0T");
    }

    #[test]
    fn format_usage_substitutes_all_placeholders() {
        assert_eq!(
            format_usage(
                "{used}/{total} used, {free} free",
                1024 * 1024 * 1024 * 10,
                1024 * 1024 * 1024 * 4,
                1024 * 1024 * 1024 * 6,
            ),
            "4.0G/10.0G used, 6.0G free"
        );
    }

    #[test]
    fn format_usage_no_placeholders_returns_unchanged() {
        assert_eq!(format_usage("static text", 100, 50, 50), "static text");
    }
}
