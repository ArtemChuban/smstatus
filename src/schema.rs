use crate::bindings::ConfigParam;
use crate::error::Result;

const SUPPORTED_TYPE: &str = "string";

fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn is_supported_type(param_type: &str) -> bool {
    param_type == SUPPORTED_TYPE
}

pub(crate) fn validate_schema(kind: &str, name: &str, schema: &[ConfigParam]) -> Result<()> {
    let mut errors = Vec::new();
    for param in schema {
        if !is_valid_name(&param.name) {
            errors.push(format!(
                "config param name `{}` must match [A-Za-z0-9-_]+",
                param.name
            ));
        }
        if !is_supported_type(&param.param_type) {
            errors.push(format!(
                "config param `{}` has unsupported type `{}`: only `string` is supported",
                param.name, param.param_type
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "module `{name}` (kind `{kind}`) declares an invalid config schema: {}",
            errors.join("; ")
        )
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_name_with_letters_digits_dash_underscore() {
        assert!(is_valid_name("path-2_ok"));
    }

    #[test]
    fn empty_name_is_invalid() {
        assert!(!is_valid_name(""));
    }

    #[test]
    fn name_with_whitespace_is_invalid() {
        assert!(!is_valid_name("path name"));
    }

    #[test]
    fn name_with_unicode_is_invalid() {
        assert!(!is_valid_name("pathé"));
    }

    #[test]
    fn name_with_symbol_is_invalid() {
        assert!(!is_valid_name("path.name"));
    }

    #[test]
    fn string_type_is_supported() {
        assert!(is_supported_type("string"));
    }

    #[test]
    fn non_string_type_is_unsupported() {
        assert!(!is_supported_type("number"));
    }

    #[test]
    fn empty_type_is_unsupported() {
        assert!(!is_supported_type(""));
    }

    fn param(name: &str, param_type: &str) -> ConfigParam {
        ConfigParam {
            name: name.to_string(),
            param_type: param_type.to_string(),
            default: String::new(),
        }
    }

    #[test]
    fn valid_schema_passes() {
        assert!(validate_schema("cpu", "cpu", &[param("path", "string")]).is_ok());
    }

    #[test]
    fn invalid_name_is_rejected() {
        assert!(validate_schema("cpu", "cpu", &[param("bad name", "string")]).is_err());
    }

    #[test]
    fn unsupported_type_is_rejected() {
        assert!(validate_schema("cpu", "cpu", &[param("path", "number")]).is_err());
    }

    #[test]
    fn empty_schema_passes() {
        assert!(validate_schema("cpu", "cpu", &[]).is_ok());
    }
}
