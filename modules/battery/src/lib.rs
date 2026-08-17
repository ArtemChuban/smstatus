wit_bindgen::generate!({
    path: "../../wit",
    world: "module",
});

use crate::bslstatus::module::host;
use exports::bslstatus::module::guest::{Guest, Output};

struct Component;

impl Guest for Component {
    fn metadata() -> String {
        r#"{"name":"battery","version":"0.1.0","capabilities":["read-sysfs"]}"#.to_string()
    }

    fn init() {}

    fn update() -> Output {
        let text = match host::read_sysfs("/sys/class/power_supply/BAT1/capacity") {
            Ok(content) => format!("BAT {}%", content.trim()),
            Err(err) => format!("BAT error: {err}"),
        };
        Output {
            text,
            interval_ms: 5000,
        }
    }

    fn on_click(_button: u8) {}
}

export!(Component);
