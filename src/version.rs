use std::cmp::Ordering;

use crate::error::Result;

pub const HOST_MODULES_API: (u32, u32, u32) = (0, 1, 0);
pub const HOST_EXTENSIONS_API: (u32, u32, u32) = (0, 1, 0);

pub(crate) fn cli_version_info() -> String {
    format!(
        "{}\nmodules-api {}.{}.{}\nextensions-api {}.{}.{}",
        env!("CARGO_PKG_VERSION"),
        HOST_MODULES_API.0,
        HOST_MODULES_API.1,
        HOST_MODULES_API.2,
        HOST_EXTENSIONS_API.0,
        HOST_EXTENSIONS_API.1,
        HOST_EXTENSIONS_API.2,
    )
}

pub fn release_notes_body() -> String {
    cli_version_info()
}

pub fn format_api_version(v: (u32, u32, u32)) -> String {
    format!("{}.{}.{}", v.0, v.1, v.2)
}

pub fn format_calver((y, m, d): (u32, u32, u32)) -> String {
    format!("{y}.{m}.{d}")
}

fn calver_segment_rejects_leading_zero(part: &str, s: &str, label: &str) -> Result<()> {
    if part.len() > 1 && part.starts_with('0') {
        return Err(format!("invalid calver `{s}`: {label} must not have leading zeros").into());
    }
    Ok(())
}

pub fn parse_calver(s: &str) -> Result<(u32, u32, u32)> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty calver".into());
    }
    let parts: Vec<&str> = s.split('.').collect();
    let [year_s, month_s, day_s] = parts.as_slice() else {
        return Err(format!("invalid calver `{s}`: expected YYYY.M.D").into());
    };
    for (part, label) in [(year_s, "year"), (month_s, "month"), (day_s, "day")] {
        calver_segment_rejects_leading_zero(part, s, label)?;
    }
    let year: u32 = year_s
        .parse()
        .map_err(|_| format!("invalid calver `{s}`: bad year `{year_s}`"))?;
    let month: u32 = month_s
        .parse()
        .map_err(|_| format!("invalid calver `{s}`: bad month `{month_s}`"))?;
    let day: u32 = day_s
        .parse()
        .map_err(|_| format!("invalid calver `{s}`: bad day `{day_s}`"))?;
    if !(1..=12).contains(&month) {
        return Err(format!("invalid calver `{s}`: month out of range").into());
    }
    if !(1..=31).contains(&day) {
        return Err(format!("invalid calver `{s}`: day out of range").into());
    }
    Ok((year, month, day))
}

pub fn parse_alpha_release_tag(tag: &str) -> Result<String> {
    let tag = tag.trim();
    let rest = tag.strip_prefix('v').unwrap_or(tag);
    let Some(calver_base) = rest.strip_suffix("-alpha") else {
        return Err(format!("invalid alpha release tag `{tag}`: missing -alpha suffix").into());
    };
    if calver_base.is_empty() || calver_base.contains('-') {
        return Err(format!("invalid alpha release tag `{tag}`").into());
    }
    parse_calver(calver_base)?;
    Ok(calver_base.to_string())
}

pub fn calver_ord(a: (u32, u32, u32), b: (u32, u32, u32)) -> Ordering {
    a.cmp(&b)
}

pub fn is_legal_api_step(from: (u32, u32, u32), to: (u32, u32, u32)) -> bool {
    if from == to {
        return true;
    }
    let (fx, fy, fz) = from;
    to == (fx, fy, fz + 1) || to == (fx, fy + 1, 0) || to == (fx + 1, 0, 0)
}

mod logic {
    pub(super) fn is_compatible(host: (u32, u32, u32), required: (u32, u32, u32)) -> bool {
        host.0 == required.0 && host.1 >= required.1
    }
}

pub fn parse_package_version(version: &str) -> Result<(u32, u32, u32)> {
    let version = version.trim();
    if version.is_empty() {
        return Err("empty package version".into());
    }
    let parts: Vec<&str> = version.split('.').collect();
    let parse_part = |part: &str, label: &str| -> Result<u32> {
        if part.is_empty() {
            return Err(format!("invalid package version `{version}`: empty {label}").into());
        }
        part.parse::<u32>().map_err(|_| {
            format!("invalid package version `{version}`: bad {label} `{part}`").into()
        })
    };
    match parts.as_slice() {
        [major, minor] => Ok((parse_part(major, "major")?, parse_part(minor, "minor")?, 0)),
        [major, minor, patch] => Ok((
            parse_part(major, "major")?,
            parse_part(minor, "minor")?,
            parse_part(patch, "patch")?,
        )),
        _ => Err(format!("invalid package version `{version}`").into()),
    }
}

pub(crate) fn package_version_meets_floor(
    installed: (u32, u32, u32),
    required_major: u32,
    required_minor: u32,
) -> bool {
    logic::is_compatible(installed, (required_major, required_minor, 0))
}

fn check_api_compatible(
    kind: &str,
    name: &str,
    contract: &str,
    host: (u32, u32, u32),
    required: (u32, u32, u32),
) -> Result<()> {
    if logic::is_compatible(host, required) {
        return Ok(());
    }
    Err(format!(
        "{kind} `{name}` requires {contract} v{}.{}.{}, but this host provides v{}.{}.{}",
        required.0, required.1, required.2, host.0, host.1, host.2,
    )
    .into())
}

pub(crate) fn check_modules_api_compatible(
    module_name: &str,
    required: (u32, u32, u32),
) -> Result<()> {
    check_api_compatible(
        "module",
        module_name,
        "modules-api",
        HOST_MODULES_API,
        required,
    )
}

pub(crate) fn check_extensions_api_compatible(
    extension_name: &str,
    required: (u32, u32, u32),
) -> Result<()> {
    check_api_compatible(
        "extension",
        extension_name,
        "extensions-api",
        HOST_EXTENSIONS_API,
        required,
    )
}

#[cfg(test)]
mod tests {
    use super::logic::is_compatible;
    use super::{
        HOST_EXTENSIONS_API, HOST_MODULES_API, calver_ord, check_extensions_api_compatible,
        check_modules_api_compatible, cli_version_info, format_api_version, format_calver,
        is_legal_api_step, package_version_meets_floor, parse_alpha_release_tag, parse_calver,
        parse_package_version, release_notes_body,
    };
    use std::cmp::Ordering;

    #[test]
    fn parse_package_version_accepts_x_y_and_x_y_z() {
        assert_eq!(parse_package_version("0.1").unwrap(), (0, 1, 0));
        assert_eq!(parse_package_version("0.1.0").unwrap(), (0, 1, 0));
        assert_eq!(parse_package_version("1.2.3").unwrap(), (1, 2, 3));
    }

    #[test]
    fn parse_package_version_rejects_empty_and_garbage() {
        assert!(parse_package_version("").is_err());
        assert!(parse_package_version("  ").is_err());
        assert!(parse_package_version("1").is_err());
        assert!(parse_package_version("1.2.3.4").is_err());
        assert!(parse_package_version("a.b.c").is_err());
        assert!(parse_package_version("1..2").is_err());
    }

    #[test]
    fn package_version_floor_match_and_higher_minor_ok() {
        assert!(package_version_meets_floor((0, 1, 0), 0, 1));
        assert!(package_version_meets_floor((0, 2, 0), 0, 1));
    }

    #[test]
    fn package_version_floor_lower_minor_or_different_major_fails() {
        assert!(!package_version_meets_floor((0, 0, 9), 0, 1));
        assert!(!package_version_meets_floor((1, 9, 0), 0, 1));
    }

    #[test]
    fn package_version_floor_ignores_patch() {
        assert!(package_version_meets_floor((0, 1, 0), 0, 1));
        assert!(package_version_meets_floor((0, 1, 99), 0, 1));
    }

    #[test]
    fn exact_match_is_compatible() {
        assert!(is_compatible((1, 2, 3), (1, 2, 3)));
    }

    #[test]
    fn host_with_higher_minor_is_compatible() {
        assert!(is_compatible((1, 5, 0), (1, 2, 9)));
    }

    #[test]
    fn host_with_lower_minor_is_incompatible() {
        assert!(!is_compatible((1, 1, 0), (1, 2, 0)));
    }

    #[test]
    fn different_major_is_incompatible_even_if_minor_is_higher() {
        assert!(!is_compatible((2, 9, 0), (1, 0, 0)));
    }

    #[test]
    fn patch_is_ignored_in_both_directions() {
        assert!(is_compatible((1, 0, 5), (1, 0, 999)));
        assert!(is_compatible((1, 0, 0), (1, 0, 999)));
    }

    #[test]
    fn modules_api_mismatch_names_the_contract() {
        let err = check_modules_api_compatible("cpu", (9, 0, 0)).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("modules-api"));
        assert!(msg.contains("cpu"));
    }

    #[test]
    fn extensions_api_mismatch_names_the_contract() {
        let err = check_extensions_api_compatible("echo", (9, 0, 0)).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("extensions-api"));
        assert!(msg.contains("echo"));
    }

    #[test]
    fn cli_version_info_lists_calver_and_both_apis() {
        let info = cli_version_info();
        assert!(info.contains(env!("CARGO_PKG_VERSION")));
        assert!(info.contains(&format!(
            "modules-api {}.{}.{}",
            HOST_MODULES_API.0, HOST_MODULES_API.1, HOST_MODULES_API.2
        )));
        assert!(info.contains(&format!(
            "extensions-api {}.{}.{}",
            HOST_EXTENSIONS_API.0, HOST_EXTENSIONS_API.1, HOST_EXTENSIONS_API.2
        )));
        assert!(!info.contains("protocol"));
    }

    #[test]
    fn parse_calver_accepts_unpadded() {
        assert_eq!(parse_calver("2026.8.27").unwrap(), (2026, 8, 27));
        assert_eq!(parse_calver("2026.1.1").unwrap(), (2026, 1, 1));
    }

    #[test]
    fn parse_calver_rejects_zero_padded_segments() {
        assert!(parse_calver("2026.08.27").is_err());
        assert!(parse_calver("2026.8.07").is_err());
        assert!(parse_calver("2026.08.07").is_err());
    }

    #[test]
    fn parse_calver_rejects_empty_wrong_count_and_ranges() {
        assert!(parse_calver("").is_err());
        assert!(parse_calver("2026.8").is_err());
        assert!(parse_calver("2026.8.27.1").is_err());
        assert!(parse_calver("2026.0.27").is_err());
        assert!(parse_calver("2026.13.1").is_err());
        assert!(parse_calver("2026.8.0").is_err());
        assert!(parse_calver("2026.8.32").is_err());
        assert!(parse_calver("a.b.c").is_err());
    }

    #[test]
    fn format_calver_unpadded() {
        assert_eq!(format_calver((2026, 8, 27)), "2026.8.27");
        assert_eq!(format_calver((2026, 1, 1)), "2026.1.1");
    }

    #[test]
    fn format_and_parse_calver_round_trip() {
        let v = (2026, 8, 27);
        assert_eq!(parse_calver(&format_calver(v)).unwrap(), v);
    }

    #[test]
    fn parse_alpha_release_tag_accepts_v_prefix() {
        assert_eq!(
            parse_alpha_release_tag("v2026.8.27-alpha").unwrap(),
            "2026.8.27"
        );
        assert_eq!(
            parse_alpha_release_tag("2026.8.27-alpha").unwrap(),
            "2026.8.27"
        );
    }

    #[test]
    fn parse_alpha_release_tag_rejects_non_alpha() {
        assert!(parse_alpha_release_tag("v2026.8.27").is_err());
        assert!(parse_alpha_release_tag("v2026.8.27-beta").is_err());
        assert!(parse_alpha_release_tag("v2026.08.27-alpha").is_err());
    }

    #[test]
    fn calver_ord_lexicographic() {
        assert_eq!(calver_ord((2026, 8, 26), (2026, 8, 27)), Ordering::Less);
        assert_eq!(calver_ord((2026, 8, 27), (2026, 8, 27)), Ordering::Equal);
        assert_eq!(calver_ord((2026, 9, 1), (2026, 8, 31)), Ordering::Greater);
    }

    #[test]
    fn is_legal_api_step_matrix() {
        assert!(is_legal_api_step((1, 2, 3), (1, 2, 3)));
        assert!(is_legal_api_step((1, 2, 3), (1, 2, 4)));
        assert!(is_legal_api_step((1, 2, 3), (1, 3, 0)));
        assert!(is_legal_api_step((1, 2, 3), (2, 0, 0)));
        assert!(!is_legal_api_step((1, 2, 3), (1, 2, 5)));
        assert!(!is_legal_api_step((1, 2, 3), (1, 4, 0)));
        assert!(!is_legal_api_step((1, 2, 3), (3, 0, 0)));
        assert!(!is_legal_api_step((1, 2, 3), (1, 3, 1)));
        assert!(!is_legal_api_step((1, 2, 3), (2, 1, 0)));
        assert!(!is_legal_api_step((1, 2, 3), (1, 1, 0)));
    }

    #[test]
    fn format_api_version_x_y_z() {
        assert_eq!(format_api_version((0, 1, 0)), "0.1.0");
    }

    #[test]
    fn release_notes_body_matches_cli_version_info() {
        assert_eq!(release_notes_body(), cli_version_info());
    }
}
