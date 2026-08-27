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

build-fs flags="":
    cargo build -p smstatus-fs {{flags}}

build-fs-release: (build-fs "--release")

build-mem flags="":
    cargo build -p smstatus-mem {{flags}}

build-mem-release: (build-mem "--release")

build-xkb flags="":
    cargo build -p smstatus-xkb {{flags}}

build-xkb-release: (build-xkb "--release")

build-disk-extension flags="":
    cargo build -p smstatus-disk {{flags}}

build-disk-extension-release: (build-disk-extension "--release")

build-smstatus-process flags="":
    cargo build -p smstatus-process {{flags}}

build-smstatus-process-release: (build-smstatus-process "--release")

build-http flags="":
    cargo build -p smstatus-http {{flags}}

build-http-release: (build-http "--release")

build-extensions: build-echo build-time build-fs build-mem build-xkb build-disk-extension build-smstatus-process build-http

build-extensions-release: build-echo-release build-time-release build-fs-release build-mem-release build-xkb-release build-disk-extension-release build-smstatus-process-release build-http-release

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

test-fs:
    cargo test -p smstatus-fs

test-mem:
    cargo test -p smstatus-mem

test-xkb:
    cargo test -p smstatus-xkb

test-disk-extension:
    cargo test -p smstatus-disk

test-smstatus-process:
    cargo test -p smstatus-process

test-http:
    cargo test -p smstatus-http

test-modules: test-battery test-datetime test-keyboard test-disk test-ram test-cpu test-process test-claude

test-scaffold:
    cargo test -p scaffold

test-release-check:
    cargo test -p release-check

test-packages: test-fmt-common test-extension-protocol test-scaffold test-release-check

test-extensions: test-echo test-time test-fs test-mem test-xkb test-disk-extension test-smstatus-process test-http

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

cov-fs:
    cargo llvm-cov -p smstatus-fs --summary-only

cov-mem:
    cargo llvm-cov -p smstatus-mem --summary-only

cov-xkb:
    cargo llvm-cov -p smstatus-xkb --summary-only

cov-disk-extension:
    cargo llvm-cov -p smstatus-disk --summary-only

cov-smstatus-process:
    cargo llvm-cov -p smstatus-process --summary-only

cov-http:
    cargo llvm-cov -p smstatus-http --summary-only

cov-packages: cov-fmt-common cov-extension-protocol

cov-modules:
    cargo llvm-cov -p battery -p datetime -p keyboard -p disk -p ram -p cpu -p process -p claude --summary-only

cov-extensions: cov-echo cov-time cov-fs cov-mem cov-xkb cov-disk-extension cov-smstatus-process cov-http

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
    cargo clippy -p scaffold -- -D warnings
    cargo clippy -p release-check -- -D warnings
    cargo clippy -p battery -p datetime -p keyboard -p disk -p ram -p cpu -p process -p claude --target {{wasm-target}} -- -D warnings
    cargo clippy -p echo -p smstatus-time -p smstatus-fs -p smstatus-mem -p smstatus-xkb -p smstatus-disk -p smstatus-process -p smstatus-http -- -D warnings
    cargo clippy -p smstatus -- -D warnings

profile := "debug"
target-dir := "target"
wasm-out := target-dir / wasm-target / profile
native-out := target-dir / profile
dist-dir := "dist"

new-module name:
    cargo run -q -p scaffold -- module {{name}}

release-check *args:
    cargo run -q -p release-check -- {{args}}

new-extension name:
    cargo run -q -p scaffold -- extension {{name}}

pack-module name: (build-module-by-name name)
    #!/usr/bin/env bash
    set -euo pipefail
    staging="{{dist-dir}}/staging/modules/{{name}}"
    archive="{{dist-dir}}/{{name}}.tar.gz"
    mkdir -p "$staging"
    cp "modules/{{name}}/manifest.toml" "$staging/manifest.toml"
    cp "{{wasm-out}}/{{name}}.wasm" "$staging/module.wasm"
    tar -czf "$archive" -C "$staging" manifest.toml module.wasm
    echo "packed $archive"

[private]
build-module-by-name name:
    #!/usr/bin/env bash
    set -euo pipefail
    flags=""
    if [ "{{profile}}" = "release" ]; then flags="--release"; fi
    cargo build -p "{{name}}" --target {{wasm-target}} $flags

# First arg is Cargo package name; second is extensions/<bin> directory and binary name.
# Scaffolds keep package, directory, and binary names identical.
pack-extension name bin: (build-extension-by-package name)
    #!/usr/bin/env bash
    set -euo pipefail
    staging="{{dist-dir}}/staging/extensions/{{bin}}"
    archive="{{dist-dir}}/{{bin}}.tar.gz"
    mkdir -p "$staging"
    cp "extensions/{{bin}}/manifest.toml" "$staging/manifest.toml"
    cp "{{native-out}}/{{bin}}" "$staging/extension"
    chmod +x "$staging/extension"
    tar -czf "$archive" -C "$staging" manifest.toml extension
    echo "packed $archive"

[private]
build-extension-by-package name:
    #!/usr/bin/env bash
    set -euo pipefail
    flags=""
    if [ "{{profile}}" = "release" ]; then flags="--release"; fi
    cargo build -p "{{name}}" $flags

pack-modules: (pack-module "battery") (pack-module "datetime") (pack-module "keyboard") (pack-module "disk") (pack-module "ram") (pack-module "cpu") (pack-module "process") (pack-module "claude")

pack-extensions: (pack-extension "echo" "echo") (pack-extension "smstatus-time" "time") (pack-extension "smstatus-fs" "fs") (pack-extension "smstatus-mem" "mem") (pack-extension "smstatus-xkb" "xkb") (pack-extension "smstatus-disk" "disk") (pack-extension "smstatus-process" "process") (pack-extension "smstatus-http" "http")

stage-module name dest: (pack-module name)
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p "{{dest}}/{{name}}"
    cp "modules/{{name}}/manifest.toml" "{{dest}}/{{name}}/manifest.toml"
    cp "{{wasm-out}}/{{name}}.wasm" "{{dest}}/{{name}}/module.wasm"

stage-extension package bin dest: (pack-extension package bin)
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p "{{dest}}/{{bin}}"
    cp "extensions/{{bin}}/manifest.toml" "{{dest}}/{{bin}}/manifest.toml"
    cp "{{native-out}}/{{bin}}" "{{dest}}/{{bin}}/extension"
    chmod +x "{{dest}}/{{bin}}/extension"