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

build-process:
    cargo build -p process --target {{wasm-target}}

build-claude:
    cargo build -p claude --target {{wasm-target}}

build-modules: build-battery build-datetime build-keyboard build-disk build-ram build-cpu build-process build-claude

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

test-process:
    cargo test -p process

test-claude:
    cargo test -p claude

test-modules: test-battery test-datetime test-keyboard test-disk test-ram test-cpu test-process test-claude

test-app:
    cargo test -p smstatus

test-all: test-modules test-app

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

clippy:
    cargo clippy -p battery -p datetime -p keyboard -p disk -p ram -p cpu -p process -p claude --target {{wasm-target}} -- -D warnings
    cargo clippy -p smstatus -- -D warnings
