use std::sync::Arc;

use x11rb::connection::Connection;
use x11rb::protocol::xkb::ConnectionExt as _;
use x11rb::protocol::xproto::{AtomEnum, PropMode, Window};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

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
        let root = connection
            .setup()
            .roots
            .get(screen_num)
            .ok_or_else(|| format!("X11 setup reported no screen {screen_num}"))?
            .root;
        Ok(Self { connection, root })
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
