use std::path::{Component, Path};

/// Path-safe (same as host `is_safe_extension_name`) and a valid Cargo package name.
pub fn is_safe_name(name: &str) -> bool {
    is_safe_path_component(name) && is_valid_cargo_package_name(name)
}

fn is_safe_path_component(name: &str) -> bool {
    if name.is_empty() || name.contains('\0') {
        return false;
    }
    let path = Path::new(name);
    let mut components = path.components();
    match components.next() {
        Some(Component::Normal(part)) if part == name => components.next().is_none(),
        _ => false,
    }
}

/// ASCII subset of Cargo package name rules: letter/`_` first; then alnum/`_`/`-`.
fn is_valid_cargo_package_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_simple_names() {
        assert!(is_safe_name("cpu"));
        assert!(is_safe_name("scratch-module"));
        assert!(is_safe_name("my_thing"));
        assert!(is_safe_name("_private"));
    }

    #[test]
    fn rejects_empty_traversal_and_separators() {
        assert!(!is_safe_name(""));
        assert!(!is_safe_name("."));
        assert!(!is_safe_name(".."));
        assert!(!is_safe_name("a/b"));
        assert!(!is_safe_name("a\0b"));
    }

    #[test]
    fn rejects_cargo_invalid_package_names() {
        assert!(!is_safe_name("foo.bar"));
        assert!(!is_safe_name("123abc"));
        assert!(!is_safe_name("-foo"));
        assert!(!is_safe_name("foo@bar"));
    }
}
