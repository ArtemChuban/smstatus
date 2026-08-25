use std::sync::Arc;

use wasmtime::{StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use crate::bindings::Host;
use crate::extension::ExtensionRegistry;

pub(crate) struct HostState {
    wasi_ctx: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
    extensions: Arc<ExtensionRegistry>,
}

impl HostState {
    pub(crate) fn new(extensions: Arc<ExtensionRegistry>) -> Self {
        Self {
            wasi_ctx: WasiCtxBuilder::new().build(),
            table: ResourceTable::new(),
            limits: StoreLimitsBuilder::new()
                .memory_size(10 * 1024 * 1024)
                .instances(3)
                .build(),
            extensions,
        }
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
        self.extensions.call(&extension, &method, &payload)
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
    use super::*;
    use crate::bindings::Host;

    fn extensions() -> Arc<ExtensionRegistry> {
        let dir = crate::extension::test_temp_dir("host");
        Arc::new(ExtensionRegistry::new(
            dir.join("extensions"),
            dir.join("sockets"),
        ))
    }

    #[test]
    fn call_extension_errors_when_not_installed() {
        let mut state = HostState::new(extensions());
        let err = state
            .call_extension("missing".to_string(), "ping".to_string(), String::new())
            .unwrap_err();
        assert!(err.contains("not installed"));
    }
}
