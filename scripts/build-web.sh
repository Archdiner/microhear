#!/usr/bin/env bash
# Builds the wasm ear into web/pkg. Needs: rustup target add wasm32-unknown-unknown
# and cargo install wasm-bindgen-cli --version 0.2.100 (must match the crate pin).
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release --target wasm32-unknown-unknown -p microhear-wasm
wasm-bindgen --target web --no-typescript --out-dir web/pkg \
  target/wasm32-unknown-unknown/release/microhear_wasm.wasm
echo "serve it: python3 -m http.server -d web 8000"
