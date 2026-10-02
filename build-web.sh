#!/bin/sh
set -eu

cargo build --release --locked --target wasm32-unknown-unknown --bin refinement-microegg
mkdir -p pkg
wasm-bindgen \
  target/wasm32-unknown-unknown/release/refinement-microegg.wasm \
  --out-dir pkg \
  --out-name refinement_microegg \
  --target web \
  --no-typescript

printf '{"type":"module"}\n' > pkg/package.json
node tests/web-smoke.mjs
