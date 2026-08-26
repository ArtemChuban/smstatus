use crate::extension::is_safe_extension_name;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ReloadBatch {
    pub config: bool,
    pub wasm_kinds: Vec<String>,
    pub extension_names: Vec<String>,
}

impl ReloadBatch {
    fn push_unique(vec: &mut Vec<String>, value: String) {
        if !vec.contains(&value) {
            vec.push(value);
        }
    }

    pub(crate) fn merge_request(&mut self, request: &ReloadRequest) {
        if request.config {
            self.config = true;
        }
        for kind in request.modules.clone() {
            Self::push_unique(&mut self.wasm_kinds, kind);
        }
        for name in request.extensions.clone() {
            Self::push_unique(&mut self.extension_names, name);
        }
    }

    pub(crate) fn merge_batch(&mut self, other: ReloadBatch) {
        if other.config {
            self.config = true;
        }
        for kind in other.wasm_kinds {
            Self::push_unique(&mut self.wasm_kinds, kind);
        }
        for name in other.extension_names {
            Self::push_unique(&mut self.extension_names, name);
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ReloadRequest {
    pub config: bool,
    pub modules: Vec<String>,
    pub extensions: Vec<String>,
}

impl ReloadRequest {
    pub(crate) fn config() -> Self {
        Self {
            config: true,
            ..Self::default()
        }
    }

    pub(crate) fn module(kind: impl Into<String>) -> Self {
        Self {
            modules: vec![kind.into()],
            ..Self::default()
        }
    }

    pub(crate) fn extension(name: impl Into<String>) -> Self {
        Self {
            extensions: vec![name.into()],
            ..Self::default()
        }
    }

    pub(crate) fn command_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if self.config {
            lines.push("config".to_string());
        }
        for kind in &self.modules {
            lines.push(format!("module:{kind}"));
        }
        for name in &self.extensions {
            lines.push(format!("extension:{name}"));
        }
        lines
    }
}

pub(crate) fn reload_request_from_cli(
    config: bool,
    modules: Vec<String>,
    extensions: Vec<String>,
) -> ReloadRequest {
    let mut request = ReloadRequest {
        modules,
        extensions,
        ..ReloadRequest::default()
    };
    if config || (request.modules.is_empty() && request.extensions.is_empty()) {
        request.config = true;
    }
    request
}

#[expect(
    dead_code,
    reason = "reload-only parser kept for tests and future callers"
)]
pub(crate) fn parse_command_line(line: &str) -> Result<ReloadRequest, String> {
    match parse_control_line(line)? {
        ControlLine::Reload(request) => Ok(request),
        ControlLine::Status => Err("status is not a reload command".to_string()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ControlLine {
    Reload(ReloadRequest),
    Status,
}

pub(crate) fn parse_control_line(line: &str) -> Result<ControlLine, String> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(ControlLine::Reload(ReloadRequest::default()));
    }
    if line == "status" {
        return Ok(ControlLine::Status);
    }
    parse_reload_command_line(line).map(ControlLine::Reload)
}

fn parse_reload_command_line(line: &str) -> Result<ReloadRequest, String> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(ReloadRequest::default());
    }
    if line == "config" {
        return Ok(ReloadRequest::config());
    }
    if let Some(kind) = line.strip_prefix("module:") {
        if kind.is_empty() {
            return Err("module command requires a kind".to_string());
        }
        return Ok(ReloadRequest::module(kind));
    }
    if let Some(name) = line.strip_prefix("extension:") {
        if name.is_empty() {
            return Err("extension command requires a name".to_string());
        }
        if !is_safe_extension_name(name) {
            return Err(format!("invalid extension name `{name}`"));
        }
        return Ok(ReloadRequest::extension(name));
    }
    Err(format!("unknown reload command: {line}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reload_request_from_cli_defaults_to_config_without_flags() {
        let request = reload_request_from_cli(false, Vec::new(), Vec::new());
        assert_eq!(request.command_lines(), vec!["config".to_string()]);
    }

    #[test]
    fn reload_request_from_cli_maps_module_and_extension_flags() {
        let request =
            reload_request_from_cli(true, vec!["cpu".to_string()], vec!["echo".to_string()]);
        assert_eq!(
            request.command_lines(),
            vec![
                "config".to_string(),
                "module:cpu".to_string(),
                "extension:echo".to_string(),
            ]
        );
    }

    #[test]
    fn reload_request_from_cli_module_only_skips_config_default() {
        let request = reload_request_from_cli(false, vec!["cpu".to_string()], Vec::new());
        assert_eq!(request.command_lines(), vec!["module:cpu".to_string()]);
    }

    #[test]
    fn parse_command_line_accepts_known_commands() {
        assert_eq!(
            parse_command_line("config").unwrap(),
            ReloadRequest::config()
        );
        assert_eq!(
            parse_command_line("module:cpu").unwrap(),
            ReloadRequest::module("cpu")
        );
        assert_eq!(
            parse_command_line("extension:echo").unwrap(),
            ReloadRequest::extension("echo")
        );
    }

    #[test]
    fn parse_control_line_accepts_status_without_merging_into_reload() {
        assert_eq!(parse_control_line("status").unwrap(), ControlLine::Status);
        assert!(parse_command_line("status").is_err());
    }

    #[test]
    fn parse_command_line_rejects_unsafe_extension_names() {
        for name in ["../etc/passwd", "a/b", ".", ".."] {
            let err = parse_command_line(&format!("extension:{name}")).unwrap_err();
            assert!(
                err.contains("invalid extension name"),
                "name {name:?} err {err}"
            );
        }
    }

    #[test]
    fn reload_batch_merge_request_deduplicates() {
        let mut batch = ReloadBatch::default();
        batch.merge_request(&ReloadRequest::module("cpu"));
        batch.merge_request(&ReloadRequest::module("cpu"));
        batch.merge_request(&ReloadRequest::config());
        assert!(batch.config);
        assert_eq!(batch.wasm_kinds, vec!["cpu".to_string()]);
    }
}
