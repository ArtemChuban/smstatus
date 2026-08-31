use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::ValueEnum;
use serde::Deserialize;
use ureq::ResponseExt;

use crate::error::Result;

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CATALOG_BYTES: u64 = 1024 * 1024;

/// Placeholder public base until publish wiring locks the real origin.
pub(crate) const DEFAULT_CATALOG_PUBLIC_BASE: &str = "https://archives.smstatus.invalid";

fn default_catalog_url() -> String {
    format!("{DEFAULT_CATALOG_PUBLIC_BASE}/catalog/v1/index.json")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum CatalogKind {
    Module,
    Extension,
}

impl CatalogKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Module => "module",
            Self::Extension => "extension",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CatalogSource {
    File(PathBuf),
    Url(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedCatalogSource {
    pub source: CatalogSource,
    pub official_eligible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CatalogEntry {
    pub kind: CatalogKind,
    pub name: String,
    pub version: String,
    pub display_name: Option<String>,
    pub url: String,
    pub sha256: String,
    pub arch: Option<String>,
    pub official: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CatalogIndex {
    pub schema_version: u32,
    pub modules: Vec<CatalogEntry>,
    pub extensions: Vec<CatalogEntry>,
}

impl CatalogIndex {
    fn entries(&self) -> impl Iterator<Item = &CatalogEntry> {
        self.modules.iter().chain(self.extensions.iter())
    }
}

#[derive(Debug, Deserialize)]
struct RawCatalogIndex {
    schema_version: u32,
    #[serde(default)]
    modules: Vec<RawModuleEntry>,
    #[serde(default)]
    extensions: Vec<RawExtensionEntry>,
}

#[derive(Debug, Deserialize)]
struct RawModuleEntry {
    name: String,
    version: String,
    #[serde(default)]
    display_name: Option<String>,
    url: String,
    sha256: String,
    #[serde(default)]
    official: bool,
}

#[derive(Debug, Deserialize)]
struct RawExtensionEntry {
    name: String,
    version: String,
    url: String,
    sha256: String,
    #[serde(default)]
    arch: Option<String>,
    #[serde(default)]
    official: bool,
}

fn require_non_empty(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(format!("catalog entry missing required field `{field}`").into());
    }
    Ok(())
}

fn validate_sha256(hash: &str) -> Result<String> {
    require_non_empty("sha256", hash)?;
    let normalized = hash.trim().to_ascii_lowercase();
    if normalized.len() != 64 || !normalized.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("catalog entry `sha256` must be 64 hexadecimal characters".into());
    }
    Ok(normalized)
}

fn module_from_raw(raw: RawModuleEntry) -> Result<CatalogEntry> {
    require_non_empty("name", &raw.name)?;
    require_non_empty("version", &raw.version)?;
    require_non_empty("url", &raw.url)?;
    let sha256 = validate_sha256(&raw.sha256)?;
    Ok(CatalogEntry {
        kind: CatalogKind::Module,
        name: raw.name,
        version: raw.version,
        display_name: raw.display_name.filter(|s| !s.trim().is_empty()),
        url: raw.url,
        sha256,
        arch: None,
        official: raw.official,
    })
}

fn extension_from_raw(raw: RawExtensionEntry) -> Result<CatalogEntry> {
    require_non_empty("name", &raw.name)?;
    require_non_empty("version", &raw.version)?;
    require_non_empty("url", &raw.url)?;
    let sha256 = validate_sha256(&raw.sha256)?;
    Ok(CatalogEntry {
        kind: CatalogKind::Extension,
        name: raw.name,
        version: raw.version,
        display_name: None,
        url: raw.url,
        sha256,
        arch: raw.arch.filter(|s| !s.trim().is_empty()),
        official: raw.official,
    })
}

pub(crate) fn parse_catalog_json(text: &str) -> Result<CatalogIndex> {
    let raw: RawCatalogIndex =
        serde_json::from_str(text).map_err(|e| format!("failed to parse catalog index: {e}"))?;
    let modules = raw
        .modules
        .into_iter()
        .map(module_from_raw)
        .collect::<Result<Vec<_>>>()?;
    let extensions = raw
        .extensions
        .into_iter()
        .map(extension_from_raw)
        .collect::<Result<Vec<_>>>()?;
    Ok(CatalogIndex {
        schema_version: raw.schema_version,
        modules,
        extensions,
    })
}

pub(crate) fn load_catalog_from_path(path: &Path) -> Result<CatalogIndex> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("failed to read catalog file `{}`: {e}", path.display()))?;
    parse_catalog_json(&text)
}

fn reject_cleartext_catalog_urls(urls: &[String]) -> Result<()> {
    for url in urls {
        if url.starts_with("http://") {
            return Err(
                "cleartext HTTP catalog downloads are disabled; use HTTPS or `--file`".into(),
            );
        }
    }
    Ok(())
}

fn collect_catalog_urls(source: &str, response: &impl ResponseExt) -> Vec<String> {
    let mut urls = vec![source.to_string()];
    if let Some(history) = response.get_redirect_history() {
        for uri in history {
            urls.push(uri.to_string());
        }
    }
    urls.push(response.get_uri().to_string());
    urls
}

fn read_limited_to_string<R: Read>(mut reader: R, max_bytes: u64) -> Result<String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut total: u64 = 0;
    loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|e| format!("failed to read catalog body: {e}"))?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > max_bytes {
            return Err(
                format!("catalog download exceeds maximum size of {max_bytes} bytes").into(),
            );
        }
        buf.extend_from_slice(&chunk[..read]);
    }
    String::from_utf8(buf).map_err(|e| format!("catalog body is not valid UTF-8: {e}").into())
}

pub(crate) fn fetch_catalog(url: &str) -> Result<CatalogIndex> {
    if url.starts_with("http://") {
        return Err("cleartext HTTP catalog downloads are disabled; use HTTPS or `--file`".into());
    }
    if !url.starts_with("https://") {
        return Err(format!("catalog URL must be HTTPS, got `{url}`").into());
    }
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(DOWNLOAD_TIMEOUT))
            .save_redirect_history(true)
            .build(),
    );
    let mut response = agent
        .get(url)
        .call()
        .map_err(|e| format!("failed to download catalog `{url}`: {e}"))?;
    reject_cleartext_catalog_urls(&collect_catalog_urls(url, &response))?;
    let reader = response.body_mut().with_config().reader();
    let text = read_limited_to_string(reader, MAX_CATALOG_BYTES)
        .map_err(|e| format!("failed to read catalog body for `{url}`: {e}"))?;
    parse_catalog_json(&text)
}

pub(crate) fn resolve_catalog_source(cli_file: Option<&Path>) -> Result<ResolvedCatalogSource> {
    if let Some(path) = cli_file {
        return Ok(ResolvedCatalogSource {
            source: CatalogSource::File(path.to_path_buf()),
            official_eligible: false,
        });
    }
    if let Some(path) = std::env::var_os("SMSTATUS_CATALOG_FILE") {
        let path = PathBuf::from(path);
        if !path.as_os_str().is_empty() {
            return Ok(ResolvedCatalogSource {
                source: CatalogSource::File(path),
                official_eligible: false,
            });
        }
    }
    if let Ok(url) = std::env::var("SMSTATUS_CATALOG_URL") {
        let url = url.trim().to_string();
        if !url.is_empty() {
            return Ok(ResolvedCatalogSource {
                source: CatalogSource::Url(url),
                official_eligible: false,
            });
        }
    }
    Ok(ResolvedCatalogSource {
        source: CatalogSource::Url(default_catalog_url()),
        official_eligible: true,
    })
}

fn display_text(entry: &CatalogEntry) -> &str {
    match entry.kind {
        CatalogKind::Module => entry
            .display_name
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(entry.name.as_str()),
        CatalogKind::Extension => entry.name.as_str(),
    }
}

fn trust_tag(entry: &CatalogEntry, official_eligible: bool) -> &'static str {
    if official_eligible && entry.official {
        "[official]"
    } else {
        "[unofficial]"
    }
}

pub(crate) fn format_list_line(entry: &CatalogEntry, official_eligible: bool) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}",
        entry.kind.as_str(),
        entry.name,
        entry.version,
        display_text(entry),
        trust_tag(entry, official_eligible),
        entry.url
    )
}

pub(crate) fn list_entries<'a>(
    index: &'a CatalogIndex,
    kind: Option<CatalogKind>,
    query: Option<&str>,
) -> Vec<&'a CatalogEntry> {
    let query = query.map(|q| q.to_ascii_lowercase());
    index
        .entries()
        .filter(|entry| kind.is_none_or(|k| entry.kind == k))
        .filter(|entry| {
            let Some(query) = query.as_deref() else {
                return true;
            };
            let name = entry.name.to_ascii_lowercase();
            let display = display_text(entry).to_ascii_lowercase();
            name.contains(query) || display.contains(query)
        })
        .collect()
}

#[allow(dead_code)]
pub(crate) fn find_entry<'a>(
    index: &'a CatalogIndex,
    name: &str,
    kind: Option<CatalogKind>,
    version: Option<&str>,
) -> Result<&'a CatalogEntry> {
    let matches: Vec<_> = index
        .entries()
        .filter(|entry| entry.name == name)
        .filter(|entry| kind.is_none_or(|k| entry.kind == k))
        .filter(|entry| version.is_none_or(|v| entry.version == v))
        .collect();

    match matches.as_slice() {
        [entry] => Ok(*entry),
        [] => {
            let kind_label = kind.map(|k| k.as_str()).unwrap_or("module/extension");
            if let Some(version) = version {
                Err(format!("catalog has no {kind_label} `{name}` version `{version}`").into())
            } else {
                Err(format!("catalog has no {kind_label} `{name}`").into())
            }
        }
        many => {
            let distinct_kinds = many.iter().any(|e| e.kind != many[0].kind);
            if kind.is_none() && distinct_kinds {
                return Err(format!(
                    "catalog entry `{name}` is ambiguous across kinds; pass `--kind module` or `--kind extension`"
                )
                .into());
            }
            Err(
                format!("catalog entry `{name}` is ambiguous; pass `--version` to select one")
                    .into(),
            )
        }
    }
}

fn load_catalog(source: &CatalogSource) -> Result<CatalogIndex> {
    match source {
        CatalogSource::File(path) => load_catalog_from_path(path),
        CatalogSource::Url(url) => fetch_catalog(url),
    }
}

pub(crate) fn cmd_list(
    kind: Option<CatalogKind>,
    query: Option<&str>,
    cli_file: Option<&Path>,
) -> Result<Vec<String>> {
    let resolved = resolve_catalog_source(cli_file)?;
    let index = load_catalog(&resolved.source)?;
    let entries = list_entries(&index, kind, query);
    Ok(entries
        .into_iter()
        .map(|entry| format_list_line(entry, resolved.official_eligible))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    const VALID_SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const OTHER_SHA: &str = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

    fn sample_json() -> String {
        format!(
            r#"{{
  "schema_version": 1,
  "modules": [
    {{
      "name": "battery",
      "version": "0.1.0",
      "display_name": "Battery",
      "url": "https://example.test/modules/battery/0.1.0/module-battery-0.1.0.tar.gz",
      "sha256": "{valid_sha}",
      "official": true
    }},
    {{
      "name": "battery",
      "version": "0.2.0",
      "display_name": "Battery",
      "url": "https://example.test/modules/battery/0.2.0/module-battery-0.2.0.tar.gz",
      "sha256": "{other_sha}",
      "official": true
    }}
  ],
  "extensions": [
    {{
      "name": "power",
      "version": "0.1.0",
      "url": "https://example.test/extensions/power/0.1.0/linux-x86_64/extension-power-0.1.0-linux-x86_64.tar.gz",
      "sha256": "{valid_sha}",
      "arch": "linux-x86_64",
      "official": true
    }},
    {{
      "name": "battery",
      "version": "0.1.0",
      "url": "https://example.test/extensions/battery/0.1.0/linux-x86_64/extension-battery-0.1.0-linux-x86_64.tar.gz",
      "sha256": "{other_sha}",
      "arch": "linux-x86_64",
      "official": false
    }}
  ]
}}"#,
            valid_sha = VALID_SHA,
            other_sha = OTHER_SHA
        )
    }

    #[test]
    fn parse_catalog_json_reads_modules_and_extensions() {
        let index = parse_catalog_json(&sample_json()).unwrap();
        assert_eq!(index.schema_version, 1);
        assert_eq!(index.modules.len(), 2);
        assert_eq!(index.extensions.len(), 2);
        assert_eq!(index.modules[0].name, "battery");
        assert_eq!(index.modules[0].display_name.as_deref(), Some("Battery"));
        assert_eq!(index.extensions[0].arch.as_deref(), Some("linux-x86_64"));
    }

    #[test]
    fn parse_catalog_json_rejects_missing_sha256() {
        let text = r#"{
  "schema_version": 1,
  "modules": [
    {
      "name": "battery",
      "version": "0.1.0",
      "url": "https://example.test/m.tar.gz"
    }
  ]
}"#;
        let err = parse_catalog_json(text).unwrap_err().to_string();
        assert!(
            err.contains("sha256") || err.contains("missing field"),
            "{err}"
        );
    }

    #[test]
    fn parse_catalog_json_rejects_missing_url() {
        let text = format!(
            r#"{{
  "schema_version": 1,
  "modules": [
    {{
      "name": "battery",
      "version": "0.1.0",
      "sha256": "{VALID_SHA}"
    }}
  ]
}}"#
        );
        let err = parse_catalog_json(&text).unwrap_err().to_string();
        assert!(
            err.contains("url") || err.contains("missing field"),
            "{err}"
        );
    }

    #[test]
    fn parse_catalog_json_rejects_empty_url() {
        let text = format!(
            r#"{{
  "schema_version": 1,
  "modules": [
    {{
      "name": "battery",
      "version": "0.1.0",
      "url": "",
      "sha256": "{VALID_SHA}"
    }}
  ]
}}"#
        );
        let err = parse_catalog_json(&text).unwrap_err().to_string();
        assert!(err.contains("url"), "{err}");
    }

    #[test]
    fn list_entries_filters_by_kind_and_query() {
        let index = parse_catalog_json(&sample_json()).unwrap();
        let modules = list_entries(&index, Some(CatalogKind::Module), None);
        assert_eq!(modules.len(), 2);
        let power = list_entries(&index, None, Some("pow"));
        assert_eq!(power.len(), 1);
        assert_eq!(power[0].name, "power");
        let battery_display = list_entries(&index, Some(CatalogKind::Module), Some("Battery"));
        assert_eq!(battery_display.len(), 2);
    }

    #[test]
    fn find_entry_errors_when_ambiguous_across_kinds() {
        let index = parse_catalog_json(&sample_json()).unwrap();
        let err = find_entry(&index, "battery", None, Some("0.1.0"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("ambiguous across kinds"), "{err}");
    }

    #[test]
    fn find_entry_selects_with_kind_and_version() {
        let index = parse_catalog_json(&sample_json()).unwrap();
        let entry =
            find_entry(&index, "battery", Some(CatalogKind::Module), Some("0.2.0")).unwrap();
        assert_eq!(entry.version, "0.2.0");
        assert_eq!(entry.kind, CatalogKind::Module);
    }

    #[test]
    fn find_entry_errors_when_version_ambiguous() {
        let index = parse_catalog_json(&sample_json()).unwrap();
        let err = find_entry(&index, "battery", Some(CatalogKind::Module), None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("ambiguous"), "{err}");
    }

    #[test]
    fn format_list_line_tags_official_only_when_eligible() {
        let index = parse_catalog_json(&sample_json()).unwrap();
        let module = &index.modules[0];
        let official = format_list_line(module, true);
        assert!(official.contains("\t[official]\t"), "{official}");
        let unofficial = format_list_line(module, false);
        assert!(unofficial.contains("\t[unofficial]\t"), "{unofficial}");
        let ext = index
            .extensions
            .iter()
            .find(|e| e.name == "battery")
            .unwrap();
        let forced = format_list_line(ext, true);
        assert!(forced.contains("\t[unofficial]\t"), "{forced}");
    }

    #[test]
    fn load_catalog_from_path_reads_fixture_file() {
        let dir =
            std::env::temp_dir().join(format!("smstatus-catalog-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("index.json");
        fs::write(&path, sample_json()).unwrap();
        let index = load_catalog_from_path(&path).unwrap();
        assert_eq!(index.modules.len(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_catalog_source_priority_and_official_flag() {
        let _guard = ENV_LOCK.lock().unwrap();
        // SAFETY: serialized by ENV_LOCK for catalog source tests.
        unsafe {
            std::env::remove_var("SMSTATUS_CATALOG_FILE");
            std::env::remove_var("SMSTATUS_CATALOG_URL");
        }

        let cli_path = PathBuf::from("/tmp/cli-catalog.json");
        let resolved = resolve_catalog_source(Some(&cli_path)).unwrap();
        assert_eq!(resolved.source, CatalogSource::File(cli_path.clone()));
        assert!(!resolved.official_eligible);

        let env_file = std::env::temp_dir().join("smstatus-catalog-env.json");
        // SAFETY: serialized by ENV_LOCK for catalog source tests.
        unsafe {
            std::env::set_var("SMSTATUS_CATALOG_FILE", &env_file);
            std::env::set_var("SMSTATUS_CATALOG_URL", "https://evil.test/index.json");
        }
        let resolved = resolve_catalog_source(None).unwrap();
        assert_eq!(resolved.source, CatalogSource::File(env_file));
        assert!(!resolved.official_eligible);

        // SAFETY: serialized by ENV_LOCK for catalog source tests.
        unsafe {
            std::env::remove_var("SMSTATUS_CATALOG_FILE");
        }
        let resolved = resolve_catalog_source(None).unwrap();
        assert_eq!(
            resolved.source,
            CatalogSource::Url("https://evil.test/index.json".into())
        );
        assert!(!resolved.official_eligible);

        // SAFETY: serialized by ENV_LOCK for catalog source tests.
        unsafe {
            std::env::remove_var("SMSTATUS_CATALOG_URL");
        }
        let resolved = resolve_catalog_source(None).unwrap();
        assert_eq!(resolved.source, CatalogSource::Url(default_catalog_url()));
        assert!(resolved.official_eligible);
    }

    #[test]
    fn fetch_catalog_rejects_cleartext_http() {
        let err = fetch_catalog("http://example.test/catalog/v1/index.json")
            .unwrap_err()
            .to_string();
        assert!(err.contains("cleartext HTTP"), "{err}");
    }

    #[test]
    fn reject_cleartext_catalog_urls_blocks_http_redirect_hop() {
        let urls = vec![
            "https://example.test/catalog/v1/index.json".to_string(),
            "http://example.test/catalog/v1/index.json".to_string(),
            "https://example.test/catalog/v1/index.json".to_string(),
        ];
        let err = reject_cleartext_catalog_urls(&urls)
            .unwrap_err()
            .to_string();
        assert!(err.contains("cleartext HTTP"), "{err}");
    }

    #[test]
    fn read_limited_to_string_rejects_oversized_body() {
        let data = vec![b'a'; 32];
        let err = read_limited_to_string(data.as_slice(), 16)
            .unwrap_err()
            .to_string();
        assert!(err.contains("exceeds maximum size"), "{err}");
    }

    #[test]
    fn read_limited_to_string_accepts_body_within_limit() {
        let text = read_limited_to_string(b"{\"schema_version\":1}".as_slice(), 64).unwrap();
        assert_eq!(text, "{\"schema_version\":1}");
    }

    #[test]
    fn cmd_list_with_file_fixture() {
        let dir =
            std::env::temp_dir().join(format!("smstatus-catalog-cmd-list-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("index.json");
        fs::write(&path, sample_json()).unwrap();
        let lines = cmd_list(Some(CatalogKind::Extension), Some("power"), Some(&path)).unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("extension\tpower\t0.1.0\tpower\t[unofficial]\t"));
        let _ = fs::remove_dir_all(&dir);
    }
}
