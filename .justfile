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

build-echo flags="":
    cargo build -p echo {{flags}}

build-echo-release: (build-echo "--release")

build-time flags="":
    cargo build -p smstatus-time {{flags}}

build-time-release: (build-time "--release")

build-sysfs flags="":
    cargo build -p smstatus-sysfs {{flags}}

build-sysfs-release: (build-sysfs "--release")

build-extensions: build-echo build-time build-sysfs

build-extensions-release: build-echo-release build-time-release build-sysfs-release

build-app flags="":
    cargo build -p smstatus {{flags}}

build-app-release: (build-app "--release")

build-all: build-modules build-extensions build-app

build-all-release: build-modules-release build-extensions-release build-app-release

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

test-fmt-common:
    cargo test -p fmt-common

test-extension-protocol:
    cargo test -p extension-protocol

test-echo:
    cargo test -p echo

test-time:
    cargo test -p smstatus-time

test-sysfs:
    cargo test -p smstatus-sysfs

test-modules: test-battery test-datetime test-keyboard test-disk test-ram test-cpu test-process test-claude

test-packages: test-fmt-common test-extension-protocol

test-extensions: test-echo test-time test-sysfs

# Registry integration tests need the echo binary on disk.
test-app: build-echo
    cargo test -p smstatus

test-all: test-packages test-modules test-extensions test-app

cov-app:
    cargo llvm-cov -p smstatus --summary-only

cov-fmt-common:
    cargo llvm-cov -p fmt-common --summary-only

cov-extension-protocol:
    cargo llvm-cov -p extension-protocol --summary-only

cov-echo:
    cargo llvm-cov -p echo --summary-only

cov-time:
    cargo llvm-cov -p smstatus-time --summary-only

cov-sysfs:
    cargo llvm-cov -p smstatus-sysfs --summary-only

cov-packages: cov-fmt-common cov-extension-protocol

cov-modules:
    cargo llvm-cov -p battery -p datetime -p keyboard -p disk -p ram -p cpu -p process -p claude --summary-only

cov-extensions: cov-echo cov-time cov-sysfs

cov-all:
    cargo llvm-cov --workspace --summary-only

# Pass --open to open the report in a browser after generation.
cov-html *args:
    cargo llvm-cov --workspace --html {{args}}

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

clippy:
    cargo clippy -p fmt-common -p extension-protocol -- -D warnings
    cargo clippy -p battery -p datetime -p keyboard -p disk -p ram -p cpu -p process -p claude --target {{wasm-target}} -- -D warnings
    cargo clippy -p echo -p smstatus-time -p smstatus-sysfs -- -D warnings
    cargo clippy -p smstatus -- -D warnings
