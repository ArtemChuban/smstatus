use crate::error::Result;

pub(crate) const HOST_MODULES_API: (u32, u32, u32) = (0, 1, 0);
pub(crate) const HOST_EXTENSIONS_API: (u32, u32, u32) = (0, 1, 0);

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

mod logic {
    pub(super) fn is_compatible(host: (u32, u32, u32), required: (u32, u32, u32)) -> bool {
        host.0 == required.0 && host.1 >= required.1
    }
}

pub(crate) fn parse_package_version(version: &str) -> Result<(u32, u32, u32)> {
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
        HOST_EXTENSIONS_API, HOST_MODULES_API, check_extensions_api_compatible,
        check_modules_api_compatible, cli_version_info, package_version_meets_floor,
        parse_package_version,
    };

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
}
