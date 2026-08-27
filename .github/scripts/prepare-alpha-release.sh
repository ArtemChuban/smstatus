#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: prepare-alpha-release.sh <calver>" >&2
  exit 1
fi

CALVER="$1"
TAG="v${CALVER}-alpha"

if git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null; then
  echo "error: tag ${TAG} already exists locally" >&2
  exit 1
fi

if git ls-remote --exit-code origin "refs/tags/${TAG}" >/dev/null 2>&1; then
  echo "error: tag ${TAG} already exists on origin" >&2
  exit 1
fi

cargo run -q -p release-check -- set-version --calver "${CALVER}"

git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"

if ! git diff --quiet -- Cargo.toml; then
  git add Cargo.toml
  git commit -m "Bump host CalVer to ${CALVER}"
  git push
fi
