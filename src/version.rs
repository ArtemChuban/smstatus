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
        check_modules_api_compatible, cli_version_info,
    };

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
