use std::sync::Arc;

use x11rb::connection::Connection;
use x11rb::protocol::xkb;
use x11rb::protocol::xkb::ConnectionExt as _;
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::protocol::xproto::{AtomEnum, PropMode, Window};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

use crate::bindings::XkbState;
use crate::error::Result;

pub(crate) struct X11Bar {
    connection: Arc<RustConnection>,
    root: Window,
}

impl X11Bar {
    pub(crate) fn connect() -> Result<Self> {
        let (connection, screen_num) = x11rb::connect(None)?;
        connection.xkb_use_extension(1, 0)?.reply()?;
        let connection = Arc::new(connection);
        let root = connection.setup().roots[screen_num].root;
        Ok(Self { connection, root })
    }

    pub(crate) fn connection(&self) -> &Arc<RustConnection> {
        &self.connection
    }

    pub(crate) fn set_status(&self, text: &str) -> Result<()> {
        self.connection.change_property8(
            PropMode::REPLACE,
            self.root,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            text.as_bytes(),
        )?;
        self.connection.flush()?;
        Ok(())
    }
}

pub(crate) fn read_xkb_state(connection: &RustConnection) -> Result<XkbState> {
    let group = connection
        .xkb_get_state(xkb::ID::USE_CORE_KBD.into())?
        .reply()?
        .group;

    let names_reply = connection
        .xkb_get_names(xkb::ID::USE_CORE_KBD.into(), xkb::NameDetail::SYMBOLS)?
        .reply()?;

    let symbols_atom = names_reply
        .value_list
        .symbols_name
        .ok_or("no symbols name reported")?;

    let symbols = connection
        .get_atom_name(symbols_atom)?
        .reply()
        .map(|r| String::from_utf8_lossy(&r.name).into_owned())?;

    Ok(XkbState {
        active_group: u8::from(group),
        symbols,
    })
}
