wit_bindgen::generate!({
    path: "wit",
    world: "module",
});

use crate::bslstatus::module::host;
use exports::bslstatus::module::guest::{Guest, Output};
use time::OffsetDateTime;
use time::macros::format_description;

struct Component;

impl Guest for Component {
    fn metadata() -> String {
        r#"{"name":"datetime","version":"0.1.0","capabilities":[]}"#.to_string()
    }

    fn init() {}

    fn update() -> Output {
        let ms = host::now_ms();
        let dt = OffsetDateTime::from_unix_timestamp_nanos(ms as i128 * 1_000_000)
            .unwrap_or(OffsetDateTime::UNIX_EPOCH);
        let format = format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
        let text = dt.format(&format).unwrap_or_default();
        Output {
            text,
            interval_ms: 1000,
        }
    }

    fn on_click(_button: u8) {}
}

export!(Component);
