wasm-target := "wasm32-wasip2"

build-battery:
    cargo build -p battery --target {{wasm-target}}

build-datetime:
    cargo build -p datetime --target {{wasm-target}}

build-keyboard:
    cargo build -p keyboard --target {{wasm-target}}

build-disk:
    cargo build -p disk --target {{wasm-target}}

build-ram:
    cargo build -p ram --target {{wasm-target}}

build-cpu:
    cargo build -p cpu --target {{wasm-target}}

build-modules: build-battery build-datetime build-keyboard build-disk build-ram build-cpu

build-app:
    cargo build -p smstatus

build-all: build-modules build-app

test-battery:
    cargo test -p battery

test-datetime:
    cargo test -p datetime

test-keyboard:
    cargo test -p keyboard

test-disk:
    cargo test -p disk

test-ram:
    cargo test -p ram

test-cpu:
    cargo test -p cpu

test-modules: test-battery test-datetime test-keyboard test-disk test-ram test-cpu

test-app:
    cargo test -p smstatus

test-all: test-modules test-app

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

clippy:
    cargo clippy -p battery -p datetime -p keyboard -p disk -p ram -p cpu --target {{wasm-target}} -- -D warnings
    cargo clippy -p smstatus -- -D warnings
