wasm-target := "wasm32-wasip2"

build-battery flags="":
    cargo build -p battery --target {{wasm-target}} {{flags}}

build-battery-release: (build-battery "--release")

build-datetime flags="":
    cargo build -p datetime --target {{wasm-target}} {{flags}}

build-datetime-release: (build-datetime "--release")

build-keyboard flags="":
    cargo build -p keyboard --target {{wasm-target}} {{flags}}

build-keyboard-release: (build-keyboard "--release")

build-disk flags="":
    cargo build -p disk --target {{wasm-target}} {{flags}}

build-disk-release: (build-disk "--release")

build-ram flags="":
    cargo build -p ram --target {{wasm-target}} {{flags}}

build-ram-release: (build-ram "--release")

build-cpu flags="":
    cargo build -p cpu --target {{wasm-target}} {{flags}}

build-cpu-release: (build-cpu "--release")

build-process flags="":
    cargo build -p process --target {{wasm-target}} {{flags}}

build-process-release: (build-process "--release")

build-claude flags="":
    cargo build -p claude --target {{wasm-target}} {{flags}}

build-claude-release: (build-claude "--release")

build-modules: build-battery build-datetime build-keyboard build-disk build-ram build-cpu build-process build-claude

build-modules-release: build-battery-release build-datetime-release build-keyboard-release build-disk-release build-ram-release build-cpu-release build-process-release build-claude-release

build-app flags="":
    cargo build -p smstatus {{flags}}

build-app-release: (build-app "--release")

build-all: build-modules build-app

build-all-release: build-modules-release build-app-release

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
