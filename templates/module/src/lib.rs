wit_bindgen::generate!({
    path: "../../wit",
    world: "module",
});

use exports::smstatus::module::guest::{ConfigParam, Guest, Output};

struct Component;

impl Guest for Component {
    fn init(_config: String) {}

    fn update() -> Output {
        Output {
            text: String::from("{{name}}"),
            interval_ms: 1_000,
        }
    }

    fn config_schema() -> Vec<ConfigParam> {
        Vec::new()
    }
}

export!(Component);
