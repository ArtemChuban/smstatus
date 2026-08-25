use crate::error::Result;

pub(crate) const HOST_API_VERSION: (u32, u32, u32) = (2, 0, 0);

mod logic {
    pub(super) fn is_compatible(host: (u32, u32, u32), required: (u32, u32, u32)) -> bool {
        host.0 == required.0 && host.1 >= required.1
    }
}

pub(crate) fn check_compatible(module_name: &str, required: (u32, u32, u32)) -> Result<()> {
    if logic::is_compatible(HOST_API_VERSION, required) {
        return Ok(());
    }
    Err(format!(
        "module `{module_name}` requires host API v{}.{}.{}, but this host provides v{}.{}.{}",
        required.0,
        required.1,
        required.2,
        HOST_API_VERSION.0,
        HOST_API_VERSION.1,
        HOST_API_VERSION.2,
    )
    .into())
}

#[cfg(test)]
mod tests {
    use super::logic::is_compatible;

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
}
