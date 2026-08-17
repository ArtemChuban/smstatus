wit_bindgen::generate!({
    path: "wit",
    world: "module",
});

use exports::bslstatus::module::guest::{Guest, Output};

struct Component;

impl Guest for Component {
    fn metadata() -> String {
        r#"{"name":"example","version":"0.1.0","capabilities":[]}"#.to_string()
    }

    fn init() {}

    fn update() -> Output {
        Output {
            text: "hello wasm".to_string(),
            interval_ms: 1000,
        }
    }

    fn on_click(_button: u8) {}
}

export!(Component);
