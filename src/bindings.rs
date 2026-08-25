wasmtime::component::bindgen!({
    path: "wit",
    world: "module",
});

pub(crate) use self::smstatus::module::host::Host;
pub(crate) use Module as GuestModule;
pub(crate) use exports::smstatus::module::guest::ConfigParam;
