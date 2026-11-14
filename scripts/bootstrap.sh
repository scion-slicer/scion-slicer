#!/usr/bin/env bash
# Prepare vendor/DragonFruit for building:
#   1. check out the pinned DragonFruit commit and its plugin submodules;
#   2. apply our engine patches (patches/dragonfruit/*.patch, in order);
#   3. generate the engine's plugin encoder list (DragonFruit's own generator, needs Node).
#
# Safe to run again: the patch set is applied only once, and only to a clean checkout.
# To pick up changed patches, run with --reset (discards any edits under vendor/DragonFruit).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
df="$root/vendor/DragonFruit"
state="$root/.scion/patches-applied"

reset=0
[[ "${1:-}" == "--reset" ]] && reset=1

git -C "$root" submodule update --init --recursive

shopt -s nullglob
patches=("$root"/patches/dragonfruit/*.patch)
want="$(cat "${patches[@]}" /dev/null | shasum | cut -d' ' -f1)"

if [[ $reset == 1 ]]; then
  echo "resetting vendor/DragonFruit to the pinned commit"
  git -C "$df" checkout -q -- .
  git -C "$df" clean -qfd -- . ':!plugins'
  rm -f "$state"
fi

if [[ -f "$state" && "$(cat "$state")" == "$want" ]]; then
  echo "engine patches already applied (${#patches[@]})"
else
  if [[ -n "$(git -C "$df" status --porcelain --ignore-submodules=all -- . ':!rust/**/generated*' ':!src/**/generated*' ':!src-tauri/**/generated*')" ]]; then
    echo "vendor/DragonFruit has changes that aren't from this patch set." >&2
    echo "Run scripts/bootstrap.sh --reset to start from the pinned commit." >&2
    exit 1
  fi
  for p in "${patches[@]}"; do
    git -C "$df" apply "$p"
    echo "applied $(basename "$p")"
  done
  mkdir -p "$(dirname "$state")"
  echo "$want" > "$state"
fi

node "$df/scripts/generate-plugin-registry.mjs" >/dev/null
echo "generated the plugin encoder list"
