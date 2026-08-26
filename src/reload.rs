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

pub(crate) fn parse_command_line(line: &str) -> Result<ReloadRequest, String> {
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
