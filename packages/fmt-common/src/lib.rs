const COMM_MAX_LEN: usize = 15;

pub fn process_name_matches(comm_contents: &str, target_name: &str) -> bool {
    let comm = comm_contents.trim();
    if comm == target_name {
        return true;
    }
    if target_name.len() <= COMM_MAX_LEN {
        return false;
    }
    let mut boundary = COMM_MAX_LEN;
    while !target_name.is_char_boundary(boundary) {
        boundary -= 1;
    }
    Some(comm) == target_name.get(..boundary)
}

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    let unit_label = UNITS.get(unit).copied().unwrap_or("");
    if unit == 0 {
        format!("{value:.0}{unit_label}")
    } else {
        format!("{value:.1}{unit_label}")
    }
}

const MAX_PAD_WIDTH: usize = 64;

pub fn format_template(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let Some(before) = rest.get(..open) else {
            break;
        };
        out.push_str(before);
        let Some(after_open) = rest.get(open + 1..) else {
            break;
        };
        let Some(close) = after_open.find('}') else {
            if let Some(remainder) = rest.get(open..) {
                out.push_str(remainder);
            }
            return out;
        };
        let Some(body) = after_open.get(..close) else {
            break;
        };
        let Some(placeholder) = rest.get(open..open + 1 + close + 1) else {
            break;
        };
        match parse_placeholder(body) {
            Some((name, pad)) => match values.iter().find(|(k, _)| *k == name) {
                Some((_, value)) => out.push_str(&apply_pad(value, pad)),
                None => out.push_str(placeholder),
            },
            None => out.push_str(placeholder),
        }
        let Some(next_rest) = after_open.get(close + 1..) else {
            break;
        };
        rest = next_rest;
    }
    out.push_str(rest);
    out
}

#[derive(Debug, PartialEq)]
struct Pad {
    width: usize,
    zero: bool,
}

fn parse_placeholder(body: &str) -> Option<(&str, Option<Pad>)> {
    match body.split_once(':') {
        None => Some((body, None)),
        Some((name, spec)) => {
            if spec.is_empty() || !spec.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let zero = spec.starts_with('0') && spec.len() > 1;
            let width: usize = spec.parse().ok()?;
            if width > MAX_PAD_WIDTH {
                return None;
            }
            Some((name, Some(Pad { width, zero })))
        }
    }
}

fn apply_pad(value: &str, pad: Option<Pad>) -> String {
    let Some(Pad { width, zero }) = pad else {
        return value.to_string();
    };
    let value_len = value.chars().count();
    if value_len >= width {
        return value.to_string();
    }
    let fill = if zero { '0' } else { ' ' };
    let mut padded = String::with_capacity(width);
    for _ in 0..(width - value_len) {
        padded.push(fill);
    }
    padded.push_str(value);
    padded
}

#[macro_export]
macro_rules! config_schema {
    ($ty:ident $(, ($name:expr, $default:expr))* $(,)?) => {
        vec![$(
            $ty {
                name: $name.to_string(),
                param_type: "string".to_string(),
                default: $default.to_string(),
            }
        ),*]
    };
}

pub fn format_usage(format: &str, total_bytes: u64, used_bytes: u64, free_bytes: u64) -> String {
    let total = human_bytes(total_bytes);
    let used = human_bytes(used_bytes);
    let free = human_bytes(free_bytes);
    format_template(
        format,
        &[("total", &total), ("used", &used), ("free", &free)],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_zero() {
        assert_eq!(human_bytes(0), "0B");
    }

    #[test]
    fn process_name_matches_exact_comm() {
        assert!(process_name_matches("firefox", "firefox"));
    }

    #[test]
    fn process_name_matches_truncated_prefix_of_long_target() {
        assert!(process_name_matches(
            "some-very-long-",
            "some-very-long-process-name"
        ));
    }

    #[test]
    fn process_name_matches_rejects_unrelated_comm() {
        assert!(!process_name_matches(
            "other-name",
            "some-very-long-process-name"
        ));
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

    #[test]
    fn format_template_plain_replace() {
        assert_eq!(format_template("CPU {usage}%", &[("usage", "5")]), "CPU 5%");
    }

    #[test]
    fn format_template_space_pad() {
        assert_eq!(
            format_template("CPU {usage:3}%", &[("usage", "5")]),
            "CPU   5%"
        );
    }

    #[test]
    fn format_template_zero_pad() {
        assert_eq!(
            format_template("CPU {usage:03}%", &[("usage", "5")]),
            "CPU 005%"
        );
    }

    #[test]
    fn format_template_over_width_no_truncate() {
        assert_eq!(format_template("{usage:2}", &[("usage", "100")]), "100");
    }

    #[test]
    fn format_template_repeated_placeholders() {
        assert_eq!(format_template("{v}/{v:02}", &[("v", "7")]), "7/07");
    }

    #[test]
    fn format_template_unknown_name_left_intact() {
        assert_eq!(
            format_template("{usage} {other}", &[("usage", "1")]),
            "1 {other}"
        );
    }

    #[test]
    fn format_template_invalid_spec_left_intact() {
        assert_eq!(format_template("{usage:x}", &[("usage", "1")]), "{usage:x}");
    }

    #[test]
    fn format_template_anonymous_plain() {
        assert_eq!(format_template("BAT {}%", &[("", "9")]), "BAT 9%");
    }

    #[test]
    fn format_template_anonymous_zero_pad() {
        assert_eq!(format_template("BAT {:02}%", &[("", "9")]), "BAT 09%");
    }

    #[test]
    fn format_usage_supports_padding() {
        assert_eq!(
            format_usage(
                "{used:6}/{total}",
                1024 * 1024 * 1024 * 10,
                1024 * 1024 * 1024 * 4,
                0,
            ),
            "  4.0G/10.0G"
        );
    }

    #[test]
    fn format_template_unclosed_brace_left_intact() {
        assert_eq!(
            format_template("CPU {usage", &[("usage", "5")]),
            "CPU {usage"
        );
    }

    #[test]
    fn format_template_empty_spec_left_intact() {
        assert_eq!(format_template("{:}", &[("", "9")]), "{:}");
    }

    #[test]
    fn format_template_zero_width_no_pad() {
        assert_eq!(format_template("{usage:0}", &[("usage", "5")]), "5");
    }

    #[test]
    fn format_template_width_over_max_left_intact() {
        assert_eq!(
            format_template("{usage:65}", &[("usage", "5")]),
            "{usage:65}"
        );
    }

    #[derive(Debug, PartialEq)]
    struct ConfigParam {
        name: String,
        param_type: String,
        default: String,
    }

    #[test]
    fn config_schema_builds_multiple_entries() {
        let schema = config_schema![ConfigParam, ("path", "/p"), ("format", "{}")];
        assert_eq!(
            schema,
            vec![
                ConfigParam {
                    name: "path".to_string(),
                    param_type: "string".to_string(),
                    default: "/p".to_string(),
                },
                ConfigParam {
                    name: "format".to_string(),
                    param_type: "string".to_string(),
                    default: "{}".to_string(),
                },
            ]
        );
    }

    #[test]
    fn config_schema_empty_invocation_yields_empty_vec() {
        let schema: Vec<ConfigParam> = config_schema![ConfigParam];
        assert!(schema.is_empty());
    }

    #[test]
    fn config_schema_accepts_non_str_default() {
        let schema = config_schema![ConfigParam, ("interval_ms", 300_000u32)];
        assert_eq!(
            schema,
            vec![ConfigParam {
                name: "interval_ms".to_string(),
                param_type: "string".to_string(),
                default: "300000".to_string(),
            }]
        );
    }
}
