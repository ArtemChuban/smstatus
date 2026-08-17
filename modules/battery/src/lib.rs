wit_bindgen::generate!({
    path: "../../wit",
    world: "module",
});

use crate::bslstatus::module::host;
use exports::bslstatus::module::guest::{Guest, Output};
use serde::Deserialize;
use std::cell::RefCell;

const DEFAULT_PATH: &str = "/sys/class/power_supply/BAT0/capacity";
const DEFAULT_FORMAT: &str = "BAT {}%";

#[derive(Deserialize, Default)]
struct Config {
    path: Option<String>,
    format: Option<String>,
}

thread_local! {
    static PATH: RefCell<String> = RefCell::new(DEFAULT_PATH.to_string());
    static FORMAT: RefCell<String> = RefCell::new(DEFAULT_FORMAT.to_string());
}

struct Component;

impl Guest for Component {
    fn init(config: String) {
        if let Ok(parsed) = serde_json::from_str::<Config>(&config) {
            if let Some(path) = parsed.path {
                PATH.with(|p| *p.borrow_mut() = path);
            }
            if let Some(format) = parsed.format {
                FORMAT.with(|f| *f.borrow_mut() = format);
            }
        }
    }

    fn update() -> Output {
        let path = PATH.with(|p| p.borrow().clone());
        let text = match host::read_sysfs(&path) {
            Ok(content) => FORMAT.with(|f| f.borrow().replace("{}", content.trim())),
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
