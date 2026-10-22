#!/usr/bin/env bash
# Build the browser document engine (Rust to WebAssembly) into site/public/engine/pkg/ for the site's
# /demo and /flow pages, with a standard rustup toolchain.
set -euo pipefail
project_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
version=$(awk '
  /^\[\[package\]\]$/ { wanted = 0 }
  /^name = "wasm-bindgen"$/ { wanted = 1 }
  wanted && /^version = / { gsub(/"/, "", $3); print $3; exit }
' "$project_dir/Cargo.lock")
if [ -z "$version" ]; then
  echo 'Cannot find the wasm-bindgen version in Cargo.lock.' >&2
  exit 1
fi
if [ "${1:-}" = --print-bindgen-version ]; then
  printf '%s\n' "$version"
  exit 0
fi
if [ "$#" -ne 0 ]; then
  echo 'Usage: bash site/scripts/build-engine.sh [--print-bindgen-version]' >&2
  exit 1
fi
for tool in rustup cargo; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing $tool. Install a Rust toolchain with rustup: https://rustup.rs/" >&2
    exit 1
  fi
done
installed=$(wasm-bindgen --version 2>/dev/null || true)
if [ "$installed" != "wasm-bindgen $version" ]; then
  printf 'Required: wasm-bindgen-cli %s (from Cargo.lock). Found: %s\n' "$version" "${installed:-not installed}" >&2
  printf 'Install the matching CLI, then rerun this script:\n  cargo install wasm-bindgen-cli --version "=%s" --locked --force\n' "$version" >&2
  exit 1
fi
if ! rustup target list --installed | grep -qx wasm32-unknown-unknown; then
  echo 'Installing the missing WebAssembly target: rustup target add wasm32-unknown-unknown'
  rustup target add wasm32-unknown-unknown
fi
# Resolve relative overrides before changing directory, as Cargo does for its caller.
build_target_dir="${CARGO_TARGET_DIR:-$project_dir/target}"
case "$build_target_dir" in
  /*) ;;
  *) build_target_dir="$PWD/$build_target_dir" ;;
esac
cd "$project_dir"
cargo build --locked --release -p ox-web --target wasm32-unknown-unknown --target-dir "$build_target_dir"
wasm-bindgen --target web --out-dir site/public/engine/pkg --out-name ox_web \
  "$build_target_dir/wasm32-unknown-unknown/release/ox_web.wasm"
# Import engine bytes as a script resource, not fetch(), so connect-src can be none.
# stdin + tr works with both BSD/macOS and GNU base64 (no platform-specific flags).
{
  printf 'export default Uint8Array.from(atob("'
  base64 < site/public/engine/pkg/ox_web_bg.wasm | tr -d '\r\n'
  printf '"), c => c.charCodeAt(0));\n'
} > site/public/engine/pkg/ox_web_bytes.js
# The pages load only the bytes module; the .wasm file itself is not published.
rm site/public/engine/pkg/ox_web_bg.wasm
echo 'Engine ready in site/public/engine/pkg/. Build the site next (site/README.md).'
