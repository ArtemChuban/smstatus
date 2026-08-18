wasmtime::component::bindgen!({
    path: "wit",
    world: "module",
});

pub(crate) use Module as GuestModule;
pub(crate) use smstatus::module::host::{DiskUsage, Host, MemUsage, TimeState, XkbState};
