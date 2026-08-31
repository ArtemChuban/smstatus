#!/usr/bin/env bash
# Generate catalog/v1 index JSON from packed release archives + on-disk manifests.
#
# Inputs: dist/release/*.tar.gz with sibling *.sha256 sidecars (from pack-release-archives.sh).
# Display fields come from modules/<name>/manifest.toml and extensions/<bin>/manifest.toml.
#
# Usage:
#   generate-catalog-index.sh --base-url <https://origin> \
#     [--release-dir dist/release] \
#     [--out dist/release/catalog-v1-index.json] \
#     [--merge-from <existing-index.json>]
#
# Object URLs match publish-archives.yml key layout:
#   modules/<name>/<version>/module-<name>-<version>.tar.gz
#   extensions/<name>/<version>/linux-x86_64/extension-<name>-<version>-linux-x86_64.tar.gz
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

RELEASE_DIR="dist/release"
OUT="dist/release/catalog-v1-index.json"
BASE_URL=""
MERGE_FROM=""
ARCH="linux-x86_64"

die() {
  echo "error: $*" >&2
  exit 1
}

toml_string() {
  local file="$1" key="$2"
  local line
  line="$(grep -E "^${key}[[:space:]]*=" "$file" | head -n1)" || true
  [ -n "$line" ] || die "missing ${key} in ${file}"
  local value="${line#*=}"
  value="${value%%#*}"
  value="$(printf '%s' "$value" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' -e 's/^"//' -e 's/"$//')"
  [ -n "$value" ] || die "empty ${key} in ${file}"
  printf '%s' "$value"
}

toml_string_optional() {
  local file="$1" key="$2"
  local line
  line="$(grep -E "^${key}[[:space:]]*=" "$file" | head -n1)" || true
  [ -n "$line" ] || return 0
  local value="${line#*=}"
  value="${value%%#*}"
  value="$(printf '%s' "$value" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' -e 's/^"//' -e 's/"$//')"
  printf '%s' "$value"
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --base-url)
      [ "$#" -ge 2 ] || die "usage: --base-url <https://origin>"
      BASE_URL="$2"
      shift 2
      ;;
    --release-dir)
      [ "$#" -ge 2 ] || die "usage: --release-dir <dir>"
      RELEASE_DIR="$2"
      shift 2
      ;;
    --out)
      [ "$#" -ge 2 ] || die "usage: --out <path>"
      OUT="$2"
      shift 2
      ;;
    --merge-from)
      [ "$#" -ge 2 ] || die "usage: --merge-from <index.json>"
      MERGE_FROM="$2"
      shift 2
      ;;
    -h | --help)
      sed -n '2,20p' "$0"
      exit 0
      ;;
    *)
      die "unknown arg '$1' (want --base-url, --release-dir, --out, --merge-from)"
      ;;
  esac
done

[ -n "${BASE_URL}" ] || die "SMSTATUS_ARCHIVE_PUBLIC_BASE_URL / --base-url required (absolute HTTPS origin, no trailing slash)"
case "${BASE_URL}" in
  https://*)
    ;;
  *)
    die "--base-url must be an https:// origin, got '${BASE_URL}'"
    ;;
esac
BASE_URL="${BASE_URL%/}"

command -v jq >/dev/null 2>&1 || die "jq is required to generate catalog index"
[ -d "${RELEASE_DIR}" ] || die "missing release dir ${RELEASE_DIR}"

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/smstatus-catalog-XXXXXX")"
trap 'rm -rf "${TMP_DIR}"' EXIT

MODULES_JSON="${TMP_DIR}/modules.json"
EXTENSIONS_JSON="${TMP_DIR}/extensions.json"
printf '[]\n' >"${MODULES_JSON}"
printf '[]\n' >"${EXTENSIONS_JSON}"

if [ -n "${MERGE_FROM}" ]; then
  [ -f "${MERGE_FROM}" ] || die "merge-from file not found: ${MERGE_FROM}"
  jq -e '.schema_version == 1' "${MERGE_FROM}" >/dev/null \
    || die "merge-from catalog must have schema_version 1"
  jq -c '.modules // []' "${MERGE_FROM}" >"${MODULES_JSON}"
  jq -c '.extensions // []' "${MERGE_FROM}" >"${EXTENSIONS_JSON}"
fi

upsert_module() {
  local name="$1" version="$2" display_name="$3" url="$4" sha256="$5"
  local entry
  entry="$(jq -nc \
    --arg name "${name}" \
    --arg version "${version}" \
    --arg display_name "${display_name}" \
    --arg url "${url}" \
    --arg sha256 "${sha256}" \
    '{
      name: $name,
      version: $version,
      display_name: $display_name,
      url: $url,
      sha256: $sha256,
      official: true
    }')"
  local next
  next="$(jq -c --argjson entry "${entry}" \
    'map(select(.name != $entry.name or .version != $entry.version)) + [$entry]' \
    "${MODULES_JSON}")"
  printf '%s\n' "${next}" >"${MODULES_JSON}"
}

upsert_extension() {
  local name="$1" version="$2" url="$3" sha256="$4" arch="$5"
  local entry
  entry="$(jq -nc \
    --arg name "${name}" \
    --arg version "${version}" \
    --arg url "${url}" \
    --arg sha256 "${sha256}" \
    --arg arch "${arch}" \
    '{
      name: $name,
      version: $version,
      url: $url,
      sha256: $sha256,
      arch: $arch,
      official: true
    }')"
  local next
  next="$(jq -c --argjson entry "${entry}" \
    'map(select(.name != $entry.name or .version != $entry.version or .arch != $entry.arch)) + [$entry]' \
    "${EXTENSIONS_JSON}")"
  printf '%s\n' "${next}" >"${EXTENSIONS_JSON}"
}

shopt -s nullglob
archives=("${RELEASE_DIR}"/*.tar.gz)
if [ "${#archives[@]}" -eq 0 ]; then
  die "no archives in ${RELEASE_DIR}"
fi

for archive in "${archives[@]}"; do
  base="$(basename "${archive}")"
  sidecar="${archive}.sha256"
  [ -f "${sidecar}" ] || die "missing sidecar ${sidecar}"
  sha256="$(head -n1 "${sidecar}" | awk '{print $1}' | tr '[:upper:]' '[:lower:]')"
  [[ "${sha256}" =~ ^[0-9a-f]{64}$ ]] || die "bad sha256 in ${sidecar}"

  case "${base}" in
    module-*-*.tar.gz)
      rest="${base#module-}"
      rest="${rest%.tar.gz}"
      version="${rest##*-}"
      name="${rest%-"${version}"}"
      [ -n "${name}" ] && [ -n "${version}" ] && [ "${name}" != "${rest}" ] \
        || die "bad module archive name ${base}"
      manifest="modules/${name}/manifest.toml"
      [ -f "${manifest}" ] || die "missing ${manifest} for ${base}"
      pkg_name="$(toml_string "${manifest}" name)"
      [ "${pkg_name}" = "${name}" ] || die "manifest name '${pkg_name}' != archive name '${name}'"
      manifest_version="$(toml_string "${manifest}" version)"
      [ "${manifest_version}" = "${version}" ] || die "manifest version '${manifest_version}' != archive version '${version}' for ${name}"
      display_name="$(toml_string_optional "${manifest}" display_name)"
      [ -n "${display_name}" ] || display_name="${name}"
      key="modules/${name}/${version}/${base}"
      url="${BASE_URL}/${key}"
      upsert_module "${name}" "${version}" "${display_name}" "${url}" "${sha256}"
      ;;
    extension-*-*-linux-x86_64.tar.gz)
      rest="${base#extension-}"
      rest="${rest%-linux-x86_64.tar.gz}"
      version="${rest##*-}"
      name="${rest%-"${version}"}"
      [ -n "${name}" ] && [ -n "${version}" ] && [ "${name}" != "${rest}" ] \
        || die "bad extension archive name ${base}"
      manifest="extensions/${name}/manifest.toml"
      [ -f "${manifest}" ] || die "missing ${manifest} for ${base}"
      pkg_name="$(toml_string "${manifest}" name)"
      [ "${pkg_name}" = "${name}" ] || die "manifest name '${pkg_name}' != archive name '${name}'"
      manifest_version="$(toml_string "${manifest}" version)"
      [ "${manifest_version}" = "${version}" ] || die "manifest version '${manifest_version}' != archive version '${version}' for ${name}"
      key="extensions/${name}/${version}/${ARCH}/${base}"
      url="${BASE_URL}/${key}"
      upsert_extension "${name}" "${version}" "${url}" "${sha256}" "${ARCH}"
      ;;
    *)
      die "unrecognized archive basename ${base}"
      ;;
  esac
done

mkdir -p "$(dirname "${OUT}")"
jq -n \
  --slurpfile modules "${MODULES_JSON}" \
  --slurpfile extensions "${EXTENSIONS_JSON}" \
  '{
    schema_version: 1,
    modules: $modules[0] | sort_by(.name, .version),
    extensions: $extensions[0] | sort_by(.name, .version, .arch)
  }' >"${OUT}"

echo "wrote ${OUT}"
