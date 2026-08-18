wit_bindgen::generate!({
    path: "../../wit",
    world: "module",
});

use crate::bslstatus::module::host;
use exports::bslstatus::module::guest::{Guest, Output};
use serde::Deserialize;
use std::cell::RefCell;
use time::OffsetDateTime;

const DEFAULT_FORMAT: &str = "[year]-[month]-[day] [hour]:[minute]:[second]";

#[derive(Deserialize, Default)]
struct Config {
    format: Option<String>,
}

thread_local! {
    static FORMAT: RefCell<String> = RefCell::new(DEFAULT_FORMAT.to_string());
}

struct Component;

impl Guest for Component {
    fn init(config: String) {
        if let Ok(parsed) = serde_json::from_str::<Config>(&config)
            && let Some(format) = parsed.format
        {
            FORMAT.with(|f| *f.borrow_mut() = format);
        }
    }

    fn update() -> Output {
        let ms = host::now_ms();
        let dt = OffsetDateTime::from_unix_timestamp_nanos(ms as i128 * 1_000_000)
            .unwrap_or(OffsetDateTime::UNIX_EPOCH);
        let offset_secs = host::local_offset_seconds();
        let dt = dt.to_offset(
            time::UtcOffset::from_whole_seconds(offset_secs).unwrap_or(time::UtcOffset::UTC),
        );

        let text = FORMAT.with(|f| {
            let fmt = f.borrow();
            time::format_description::parse_borrowed::<3>(&fmt)
                .ok()
                .and_then(|desc| dt.format(&desc).ok())
        });
        Output {
            text: text.unwrap_or_else(|| "time format error".to_string()),
            interval_ms: 1000,
        }
    }

    fn on_click(_button: u8) {}
}

export!(Component);
