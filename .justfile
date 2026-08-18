wasm-target := "wasm32-wasip2"

build-battery:
    cargo build -p battery --target {{wasm-target}}

build-datetime:
    cargo build -p datetime --target {{wasm-target}}

build-modules: build-battery build-datetime

build-app:
    cargo build -p smstatus

build-all: build-modules build-app

test-battery:
    cargo test -p battery

test-datetime:
    cargo test -p datetime

test-modules: test-battery test-datetime

test-app:
    cargo test -p smstatus

test-all: test-modules test-app

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

clippy:
    cargo clippy -p battery -p datetime --target {{wasm-target}} -- -D warnings
    cargo clippy -p smstatus -- -D warnings
