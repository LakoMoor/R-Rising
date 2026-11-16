#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build --locked --release -p rrising-web --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir web/pkg target/wasm32-unknown-unknown/release/rrising_web.wasm
mkdir -p web/branding
cp docs/branding/logo.png web/branding/logo.png
printf '%s\n' 'Built web/. Serve locally: python3 -m http.server 8080 --directory web'
