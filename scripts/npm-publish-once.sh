#!/usr/bin/env bash
# Publishes the npm package in <dir>, unless that version is already on npm, so a
# release that stopped halfway can simply be run again. Passes $DRY_RUN through
# (empty, or --dry-run).
set -euo pipefail
dir="$1"
# A bare "npm/mcp" would be read as a GitHub repository (owner/name), not a folder.
case "$dir" in /* | ./*) ;; *) dir="./$dir" ;; esac
name=$(node -p "require('$dir/package.json').name")
version=$(node -p "require('$dir/package.json').version")
if npm view "$name@$version" version >/dev/null 2>&1; then
  echo "$name@$version is already on npm; skipping"
  exit 0
fi
# shellcheck disable=SC2086 # DRY_RUN is empty or a single flag
npm publish "$dir" --access public ${DRY_RUN:-}
