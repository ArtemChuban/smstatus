use std::sync::Arc;

use wasmtime::{StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};
use x11rb::rust_connection::RustConnection;

use crate::bindings::{DiskUsage, Host, MemUsage, TimeState, XkbState};
use crate::{sysinfo, x11};

pub(crate) struct HostState {
    wasi_ctx: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
    connection: Arc<RustConnection>,
    http_agent: ureq::Agent,
}

impl HostState {
    pub(crate) fn new(connection: Arc<RustConnection>, http_agent: ureq::Agent) -> Self {
        Self {
            wasi_ctx: WasiCtxBuilder::new().build(),
            table: ResourceTable::new(),
            limits: StoreLimitsBuilder::new()
                .memory_size(10 * 1024 * 1024)
                .instances(3)
                .build(),
            connection,
            http_agent,
        }
    }

    pub(crate) fn limits(&mut self) -> &mut StoreLimits {
        &mut self.limits
    }
}

impl Host for HostState {
    fn read_sysfs(&mut self, path: String) -> Result<String, String> {
        sysinfo::read_sysfs(&path).map_err(|e| e.to_string())
    }

    fn read_time_state(&mut self) -> TimeState {
        sysinfo::time_state()
    }

    fn read_xkb_state(&mut self) -> Result<XkbState, String> {
        x11::read_xkb_state(&self.connection).map_err(|e| e.to_string())
    }

    fn read_disk_usage(&mut self, device: String) -> Result<DiskUsage, String> {
        sysinfo::disk_usage(&device).map_err(|e| e.to_string())
    }

    fn read_mem_usage(&mut self) -> Result<MemUsage, String> {
        sysinfo::mem_usage().map_err(|e| e.to_string())
    }

    fn read_process_running(&mut self, name: String) -> Result<bool, String> {
        sysinfo::process_running(&name).map_err(|e| e.to_string())
    }

    fn http_get(&mut self, url: String, headers: Vec<(String, String)>) -> Result<String, String> {
        let mut request = self.http_agent.get(&url);
        for (name, value) in &headers {
            request = request.header(name, value);
        }
        let mut response = request
            .call()
            .map_err(|e| format!("http request failed: {e}"))?;
        response
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("failed to read response body: {e}"))
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
