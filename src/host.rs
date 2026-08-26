use std::sync::Arc;
use std::time::SystemTime;

use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use crate::bindings::{GuestModule, Host};
use crate::extension::{
    ExtensionCallAudit, ExtensionCallOutcome, ExtensionCallRecord, ExtensionRegistry,
    redact_error_message, redact_payload,
};

pub(crate) fn build_engine_and_linker() -> crate::error::Result<(Engine, Linker<HostState>)> {
    let mut wasm_config = Config::new();
    wasm_config.wasm_component_model(true);
    wasm_config.consume_fuel(true);
    let engine = Engine::new(&wasm_config)?;

    let mut linker = Linker::new(&engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
    GuestModule::add_to_linker::<_, wasmtime::component::HasSelf<_>>(
        &mut linker,
        |state: &mut HostState| state,
    )?;
    Ok((engine, linker))
}

pub(crate) fn instantiate_component(
    engine: &Engine,
    linker: &Linker<HostState>,
    component: &Component,
    extensions: Arc<ExtensionRegistry>,
    permissions: Arc<[extension_protocol::PermissionEntry]>,
    audit: Arc<ExtensionCallAudit>,
    fuel: u64,
) -> crate::error::Result<(Store<HostState>, GuestModule)> {
    let state = HostState::new(extensions, permissions, audit);
    let mut store = Store::new(engine, state);
    store.limiter(|state| state.limits());
    store.set_fuel(fuel)?;
    let module = GuestModule::instantiate(&mut store, component, linker)?;
    Ok((store, module))
}

pub(crate) struct HostState {
    wasi_ctx: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
    extensions: Arc<ExtensionRegistry>,
    permissions: Arc<[extension_protocol::PermissionEntry]>,
    audit: Arc<ExtensionCallAudit>,
    caller_module_kind: Option<String>,
}

impl HostState {
    pub(crate) fn new(
        extensions: Arc<ExtensionRegistry>,
        permissions: Arc<[extension_protocol::PermissionEntry]>,
        audit: Arc<ExtensionCallAudit>,
    ) -> Self {
        Self {
            wasi_ctx: WasiCtxBuilder::new().build(),
            table: ResourceTable::new(),
            limits: StoreLimitsBuilder::new()
                .memory_size(10 * 1024 * 1024)
                .instances(3)
                .build(),
            extensions,
            permissions,
            audit,
            caller_module_kind: None,
        }
    }

    pub(crate) fn set_caller_module_kind(&mut self, kind: Option<String>) {
        self.caller_module_kind = kind;
    }

    pub(crate) fn limits(&mut self) -> &mut StoreLimits {
        &mut self.limits
    }
}

impl Host for HostState {
    fn call_extension(
        &mut self,
        extension: String,
        method: String,
        payload: String,
    ) -> Result<String, String> {
        let payload_preview = redact_payload(&method, &payload);
        let module_kind = self.caller_module_kind.clone();

        let push_audit = |outcome: ExtensionCallOutcome| {
            log::debug!(
                "extension call audit: extension=`{extension}` method=`{method}` outcome={outcome:?} preview={payload_preview}"
            );
            self.audit.push(ExtensionCallRecord {
                at: SystemTime::now(),
                module_kind: module_kind.clone(),
                extension: extension.clone(),
                method: method.clone(),
                payload_preview: payload_preview.clone(),
                outcome,
            });
        };

        if extension_protocol::is_reserved_method(&method) {
            push_audit(ExtensionCallOutcome::Denied);
            return Err(format!("permission denied: method `{method}` is reserved"));
        }

        if !self
            .permissions
            .iter()
            .any(|perm| perm.extension == extension && perm.method == method)
        {
            push_audit(ExtensionCallOutcome::Denied);
            return Err(format!(
                "permission denied: module has no permission for extension `{extension}` method `{method}`"
            ));
        }

        let check_payload = match extension_protocol::encode_check_payload(
            self.permissions.iter().cloned(),
            method.clone(),
            payload.clone(),
        ) {
            Ok(check_payload) => check_payload,
            Err(err) => {
                push_audit(ExtensionCallOutcome::Err(redact_error_message(&err)));
                return Err(err);
            }
        };
        if let Err(err) =
            self.extensions
                .call(&extension, extension_protocol::CHECK_METHOD, &check_payload)
        {
            push_audit(ExtensionCallOutcome::Err(redact_error_message(&err)));
            return Err(err);
        }

        match self.extensions.call(&extension, &method, &payload) {
            Ok(value) => {
                push_audit(ExtensionCallOutcome::Ok);
                Ok(value)
            }
            Err(err) => {
                push_audit(ExtensionCallOutcome::Err(redact_error_message(&err)));
                Err(err)
            }
        }
    }
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi_ctx,
            table: &mut self.table,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;

    use super::*;
    use crate::bindings::Host;
    use crate::extension::{ExtensionCallAudit, ExtensionCallOutcome};

    fn echo_extension_path() -> PathBuf {
        static ECHO: OnceLock<PathBuf> = OnceLock::new();
        ECHO.get_or_init(|| {
            if let Ok(path) = std::env::var("CARGO_BIN_EXE_echo") {
                let path = PathBuf::from(path);
                if path.exists() {
                    return path;
                }
            }

            let mut dir = std::env::current_exe().unwrap();
            dir.pop();
            if dir.ends_with("deps") {
                dir.pop();
            }
            let bin = dir.join("echo");
            if bin.exists() {
                return bin;
            }

            let target_dir = dir.parent().expect("debug dir has a target-dir parent");
            let status = std::process::Command::new(env!("CARGO"))
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .args(["build", "-p", "echo", "--target-dir"])
                .arg(target_dir)
                .status()
                .expect("failed to spawn cargo build -p echo");
            assert!(
                status.success() && bin.exists(),
                "echo fixture missing at {}; build with `cargo build -p echo --target-dir {}`",
                bin.display(),
                target_dir.display()
            );
            bin
        })
        .clone()
    }

    fn install_echo(extensions_dir: &Path) {
        let pkg = extensions_dir.join("echo");
        std::fs::create_dir_all(&pkg).unwrap();
        symlink(echo_extension_path(), pkg.join("extension")).unwrap();
        std::fs::write(
            pkg.join("manifest.toml"),
            "name = \"echo\"\nversion = \"0.1.0\"\nauthor = \"ArtemChuban\"\nextensions-api = { major = 0, minor = 1 }\n",
        )
        .unwrap();
    }

    fn registry_with_echo() -> Arc<ExtensionRegistry> {
        let dir = crate::extension::test_temp_dir("host");
        let extensions_dir = dir.join("extensions");
        install_echo(&extensions_dir);
        Arc::new(ExtensionRegistry::new(extensions_dir, dir.join("sockets")))
    }

    fn echo_ping_permission() -> extension_protocol::PermissionEntry {
        extension_protocol::PermissionEntry {
            extension: "echo".to_string(),
            method: "ping".to_string(),
            constraints: BTreeMap::new(),
        }
    }

    fn fresh_audit() -> Arc<ExtensionCallAudit> {
        Arc::new(ExtensionCallAudit::new())
    }

    #[test]
    fn call_extension_errors_when_not_installed() {
        let dir = crate::extension::test_temp_dir("host-missing");
        let registry = Arc::new(ExtensionRegistry::new(
            dir.join("extensions"),
            dir.join("sockets"),
        ));
        let permissions = Arc::<[extension_protocol::PermissionEntry]>::from(vec![
            extension_protocol::PermissionEntry {
                extension: "missing".to_string(),
                method: "ping".to_string(),
                constraints: BTreeMap::new(),
            },
        ]);
        let mut state = HostState::new(registry, permissions, fresh_audit());
        let err = state
            .call_extension("missing".to_string(), "ping".to_string(), String::new())
            .unwrap_err();
        assert!(err.contains("not installed"));
    }

    #[test]
    fn call_extension_succeeds_with_matching_permission() {
        let mut state = HostState::new(
            registry_with_echo(),
            Arc::from(vec![echo_ping_permission()]),
            fresh_audit(),
        );
        let result = state
            .call_extension("echo".to_string(), "ping".to_string(), "hello".to_string())
            .unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn call_extension_denies_without_matching_permission() {
        let mut state = HostState::new(registry_with_echo(), Arc::from([]), fresh_audit());
        let err = state
            .call_extension("echo".to_string(), "ping".to_string(), "hello".to_string())
            .unwrap_err();
        assert!(err.contains("permission denied"));
    }

    #[test]
    fn call_extension_denies_unpermitted_method_before_real_rpc() {
        let mut state = HostState::new(
            registry_with_echo(),
            Arc::from(vec![echo_ping_permission()]),
            fresh_audit(),
        );
        let err = state
            .call_extension("echo".to_string(), "pong".to_string(), "hello".to_string())
            .unwrap_err();
        assert!(err.contains("permission denied"));
    }

    #[test]
    fn call_extension_denies_reserved_check_method() {
        let mut state = HostState::new(
            registry_with_echo(),
            Arc::from(vec![echo_ping_permission()]),
            fresh_audit(),
        );
        let err = state
            .call_extension(
                "echo".to_string(),
                extension_protocol::CHECK_METHOD.to_string(),
                String::new(),
            )
            .unwrap_err();
        assert!(err.contains("reserved"));
    }

    #[test]
    fn frozen_permissions_still_enforce_on_new_host_state() {
        let permissions = Arc::from(vec![echo_ping_permission()]);
        let registry = registry_with_echo();
        let audit = fresh_audit();
        let mut first = HostState::new(
            Arc::clone(&registry),
            Arc::clone(&permissions),
            Arc::clone(&audit),
        );
        assert_eq!(
            first
                .call_extension("echo".to_string(), "ping".to_string(), "one".to_string())
                .unwrap(),
            "one"
        );

        let mut second = HostState::new(registry, permissions, audit);
        assert_eq!(
            second
                .call_extension("echo".to_string(), "ping".to_string(), "two".to_string())
                .unwrap(),
            "two"
        );
        let err = second
            .call_extension("echo".to_string(), "other".to_string(), String::new())
            .unwrap_err();
        assert!(err.contains("permission denied"));
    }

    #[test]
    fn call_extension_records_ok_in_audit() {
        let audit = fresh_audit();
        let mut state = HostState::new(
            registry_with_echo(),
            Arc::from(vec![echo_ping_permission()]),
            Arc::clone(&audit),
        );
        state
            .call_extension("echo".to_string(), "ping".to_string(), "hello".to_string())
            .unwrap();
        let records = audit.recent(10);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].outcome, ExtensionCallOutcome::Ok);
        assert_eq!(records[0].module_kind, None);
    }

    #[test]
    fn call_extension_records_module_kind_when_set() {
        let audit = fresh_audit();
        let mut state = HostState::new(
            registry_with_echo(),
            Arc::from(vec![echo_ping_permission()]),
            Arc::clone(&audit),
        );
        state.set_caller_module_kind(Some("cpu".to_string()));
        state
            .call_extension("echo".to_string(), "ping".to_string(), "hello".to_string())
            .unwrap();
        let records = audit.recent(1);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].module_kind.as_deref(), Some("cpu"));
    }

    #[test]
    fn call_extension_records_check_failure_in_audit() {
        let dir = crate::extension::test_temp_dir("host-check-fail");
        let registry = Arc::new(ExtensionRegistry::new(
            dir.join("extensions"),
            dir.join("sockets"),
        ));
        let audit = fresh_audit();
        let mut state = HostState::new(
            registry,
            Arc::from(vec![echo_ping_permission()]),
            Arc::clone(&audit),
        );
        let err = state
            .call_extension("echo".to_string(), "ping".to_string(), "hello".to_string())
            .unwrap_err();
        assert!(err.contains("not installed"));
        let records = audit.recent(1);
        assert_eq!(records.len(), 1);
        assert!(matches!(records[0].outcome, ExtensionCallOutcome::Err(_)));
    }

    #[test]
    fn call_extension_records_denied_without_registry_call() {
        let audit = fresh_audit();
        let mut state = HostState::new(registry_with_echo(), Arc::from([]), Arc::clone(&audit));
        let err = state
            .call_extension("echo".to_string(), "ping".to_string(), "hello".to_string())
            .unwrap_err();
        assert!(err.contains("permission denied"));
        let records = audit.recent(10);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].outcome, ExtensionCallOutcome::Denied);
    }
}
