wasmtime::component::bindgen!({
    path: "wit",
    world: "module",
});

pub(crate) use Module as GuestModule;
pub(crate) use exports::smstatus::module::guest::ConfigParam;
pub(crate) use smstatus::module::host::{DiskUsage, Host, MemUsage, TimeState, XkbState};
