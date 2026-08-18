wasm-target := "wasm32-wasip2"

build-battery:
    cargo build -p battery --target {{wasm-target}}

build-datetime:
    cargo build -p datetime --target {{wasm-target}}

build-modules: build-battery build-datetime

build-app:
    cargo build -p bslstatus

build-all: build-modules build-app

fmt:
    cargo fmt --all

clippy:
    cargo clippy -p battery -p datetime --target {{wasm-target}}
    cargo clippy -p bslstatus
