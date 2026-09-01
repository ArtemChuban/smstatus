#!/usr/bin/env bash
# Pack versioned module/extension archives for local use and CI S3 publish.
#
# Naming (stable basenames; S3 keys use the same basename under a versioned prefix):
#   modules:    module-<name>-<version>.tar.gz
#               module-<name>-<version>.tar.gz.sha256
#   extensions: extension-<name>-<version>-linux-x86_64.tar.gz
#               extension-<name>-<version>-linux-x86_64.tar.gz.sha256
#
# Arch policy: extensions publish linux-x86_64 only for now. Modules are WASM
# (no arch segment).
#
# Usage:
#   pack-release-archives.sh [all]
#   pack-release-archives.sh module:<name> [module:<name> ...]
#   pack-release-archives.sh extension:<bin> [extension:<bin> ...]
#   pack-release-archives.sh --changed-list <file>
#     (file lines: module:<name> or extension:<bin>; empty = no-op success)
#   pack-release-archives.sh --list-modules
#   pack-release-archives.sh --list-extension-bins
#   pack-release-archives.sh --list-extension-pairs
# Canonical shipped allowlist: pack-modules / pack-extensions / CI changed
# detection read these lists (do not duplicate elsewhere).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

DIST_DIR="${DIST_DIR:-dist}"
RELEASE_DIR="${DIST_DIR}/release"
ARCH="linux-x86_64"

ALL_MODULES=(battery datetime keyboard disk ram cpu process claude)
# cargo-package:bin pairs
ALL_EXTENSIONS=(
  "echo:echo"
  "smstatus-time:time"
  "smstatus-fs:fs"
  "smstatus-mem:mem"
  "smstatus-xkb:xkb"
  "smstatus-power:power"
  "smstatus-disk:disk"
  "smstatus-process:process"
  "smstatus-http:http"
)

die() {
  echo "error: $*" >&2
  exit 1
}

toml_field() {
  local file="$1" key="$2"
  cargo run -q -p release-check -- manifest-field --manifest "$file" --key "$key"
}

parse_host_api() {
  local body="$1" label="$2"
  local line ver
  line="$(printf '%s\n' "$body" | grep "^${label} " | head -n1)" || true
  [ -n "$line" ] || die "host floors missing ${label} line"
  ver="${line#"${label} "}"
  IFS=. read -r major minor _patch <<<"$ver"
  [ -n "${major:-}" ] && [ -n "${minor:-}" ] || die "bad host ${label}: ${ver}"
  printf '%s %s' "$major" "$minor"
}

api_compatible() {
  local host_major="$1" host_minor="$2" req_major="$3" req_minor="$4"
  [ "$host_major" = "$req_major" ] && [ "$host_minor" -ge "$req_minor" ]
}

extension_pkg_for_bin() {
  local bin="$1" pair pkg
  for pair in "${ALL_EXTENSIONS[@]}"; do
    pkg="${pair%%:*}"
    if [ "${pair#*:}" = "$bin" ]; then
      printf '%s' "$pkg"
      return 0
    fi
  done
  return 1
}

is_known_module() {
  local name="$1" m
  for m in "${ALL_MODULES[@]}"; do
    [ "$m" = "$name" ] && return 0
  done
  return 1
}

is_known_extension_bin() {
  local bin="$1"
  extension_pkg_for_bin "$bin" >/dev/null
}

SELECTED_MODULES=()
SELECTED_EXTENSIONS=() # bins

add_module() {
  local name="$1"
  is_known_module "$name" || die "unknown module '${name}' (not in pack-modules allowlist)"
  local m
  for m in "${SELECTED_MODULES[@]+"${SELECTED_MODULES[@]}"}"; do
    [ "$m" = "$name" ] && return 0
  done
  SELECTED_MODULES+=("$name")
}

add_extension() {
  local bin="$1"
  is_known_extension_bin "$bin" || die "unknown extension '${bin}' (not in pack-extensions allowlist)"
  local e
  for e in "${SELECTED_EXTENSIONS[@]+"${SELECTED_EXTENSIONS[@]}"}"; do
    [ "$e" = "$bin" ] && return 0
  done
  SELECTED_EXTENSIONS+=("$bin")
}

select_all() {
  local m pair
  for m in "${ALL_MODULES[@]}"; do
    SELECTED_MODULES+=("$m")
  done
  for pair in "${ALL_EXTENSIONS[@]}"; do
    SELECTED_EXTENSIONS+=("${pair#*:}")
  done
}

load_changed_list() {
  local file="$1" line kind rest
  [ -f "$file" ] || die "changed-list file not found: ${file}"
  while IFS= read -r line || [ -n "$line" ]; do
    line="${line%%#*}"
    line="$(printf '%s' "$line" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"
    [ -z "$line" ] && continue
    case "$line" in
      module:*)
        add_module "${line#module:}"
        ;;
      extension:*)
        add_extension "${line#extension:}"
        ;;
      *)
        die "bad changed-list entry '${line}' (want module:<name> or extension:<bin>)"
        ;;
    esac
  done <"$file"
}

# --- arg parse ---
if [ "$#" -eq 0 ]; then
  select_all
elif [ "$1" = "all" ] && [ "$#" -eq 1 ]; then
  select_all
elif [ "$1" = "--list-modules" ] && [ "$#" -eq 1 ]; then
  printf '%s\n' "${ALL_MODULES[@]}"
  exit 0
elif [ "$1" = "--list-extension-bins" ] && [ "$#" -eq 1 ]; then
  for pair in "${ALL_EXTENSIONS[@]}"; do
    printf '%s\n' "${pair#*:}"
  done
  exit 0
elif [ "$1" = "--list-extension-pairs" ] && [ "$#" -eq 1 ]; then
  printf '%s\n' "${ALL_EXTENSIONS[@]}"
  exit 0
elif [ "$1" = "--changed-list" ]; then
  [ "$#" -eq 2 ] || die "usage: --changed-list <file>"
  load_changed_list "$2"
else
  for arg in "$@"; do
    case "$arg" in
      all)
        die "'all' cannot be mixed with package filters"
        ;;
      module:*)
        add_module "${arg#module:}"
        ;;
      extension:*)
        add_extension "${arg#extension:}"
        ;;
      *)
        die "unknown arg '${arg}' (want all | module:<name> | extension:<bin> | --changed-list <file> | --list-modules | --list-extension-bins | --list-extension-pairs)"
        ;;
    esac
  done
fi

if [ "${#SELECTED_MODULES[@]}" -eq 0 ] && [ "${#SELECTED_EXTENSIONS[@]}" -eq 0 ]; then
  echo "no packages selected; nothing to pack"
  exit 0
fi

HOST_BODY="$(cargo run -q -p release-check -- format-release-body)"
HOST_MODULES="$(parse_host_api "$HOST_BODY" "modules-api")"
HOST_EXTENSIONS="$(parse_host_api "$HOST_BODY" "extensions-api")"
read -r HOST_MOD_MAJOR HOST_MOD_MINOR <<<"$HOST_MODULES"
read -r HOST_EXT_MAJOR HOST_EXT_MINOR <<<"$HOST_EXTENSIONS"

rm -rf "$RELEASE_DIR"
mkdir -p "$RELEASE_DIR"
SHA256SUMS="${RELEASE_DIR}/SHA256SUMS"
: >"$SHA256SUMS"

write_sha256() {
  local asset="$1"
  local base hash
  base="$(basename "$asset")"
  hash="$(sha256sum "$asset" | awk '{print $1}')"
  printf '%s\n' "$hash" >"${asset}.sha256"
  printf '%s  %s\n' "$hash" "$base" >>"$SHA256SUMS"
}

pack_module() {
  local name="$1"
  local manifest="modules/${name}/manifest.toml"
  [ -f "$manifest" ] || die "missing ${manifest}"

  local pkg_name version req_major req_minor asset_base
  pkg_name="$(toml_field "$manifest" name)"
  version="$(toml_field "$manifest" version)"
  read -r req_major req_minor <<<"$(toml_field "$manifest" "modules-api")"

  if ! api_compatible "$HOST_MOD_MAJOR" "$HOST_MOD_MINOR" "$req_major" "$req_minor"; then
    die "module '${pkg_name}' requires modules-api ${req_major}.${req_minor}, host provides ${HOST_MOD_MAJOR}.${HOST_MOD_MINOR}"
  fi

  just --set profile release pack-module "$name"

  asset_base="module-${pkg_name}-${version}.tar.gz"
  cp "${DIST_DIR}/${name}.tar.gz" "${RELEASE_DIR}/${asset_base}"
  write_sha256 "${RELEASE_DIR}/${asset_base}"
  echo "release asset ${RELEASE_DIR}/${asset_base}"
}

pack_extension() {
  local bin="$1"
  local pkg
  pkg="$(extension_pkg_for_bin "$bin")" || die "unknown extension bin '${bin}'"
  local manifest="extensions/${bin}/manifest.toml"
  [ -f "$manifest" ] || die "missing ${manifest}"

  local pkg_name version req_major req_minor asset_base
  pkg_name="$(toml_field "$manifest" name)"
  version="$(toml_field "$manifest" version)"
  read -r req_major req_minor <<<"$(toml_field "$manifest" "extensions-api")"

  if ! api_compatible "$HOST_EXT_MAJOR" "$HOST_EXT_MINOR" "$req_major" "$req_minor"; then
    die "extension '${pkg_name}' requires extensions-api ${req_major}.${req_minor}, host provides ${HOST_EXT_MAJOR}.${HOST_EXT_MINOR}"
  fi

  just --set profile release pack-extension "$pkg" "$bin"

  asset_base="extension-${pkg_name}-${version}-${ARCH}.tar.gz"
  cp "${DIST_DIR}/${bin}.tar.gz" "${RELEASE_DIR}/${asset_base}"
  write_sha256 "${RELEASE_DIR}/${asset_base}"
  echo "release asset ${RELEASE_DIR}/${asset_base}"
}

for name in "${SELECTED_MODULES[@]+"${SELECTED_MODULES[@]}"}"; do
  pack_module "$name"
done
for bin in "${SELECTED_EXTENSIONS[@]+"${SELECTED_EXTENSIONS[@]}"}"; do
  pack_extension "$bin"
done

echo "wrote ${SHA256SUMS}"
