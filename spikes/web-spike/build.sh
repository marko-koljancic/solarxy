#!/usr/bin/env bash
# Phase 0 spike build + size measurement. Run from anywhere.
# Produces dist/ (served by index.html) and prints the four size numbers.
set -euo pipefail
cd "$(dirname "$0")"

cargo build --target wasm32-unknown-unknown --release

RAW=target/wasm32-unknown-unknown/release/web_spike.wasm
wasm-bindgen --target web --no-typescript --out-dir dist "$RAW"

BG=dist/web_spike_bg.wasm
OPT=dist/web_spike_bg.opt.wasm
# -all: rustc 1.9x emits post-MVP features (bulk memory, sign-ext, ...) that
# binaryen must be told to accept.
wasm-opt -Oz -all -o "$OPT" "$BG"

RAW_SIZE=$(stat -f%z "$RAW")
BG_SIZE=$(stat -f%z "$BG")
JS_SIZE=$(stat -f%z dist/web_spike.js)
OPT_SIZE=$(stat -f%z "$OPT")
BR_SIZE=$(brotli -q 11 -c "$OPT" | wc -c | tr -d ' ')

# Serve the optimized artifact.
mv "$OPT" "$BG"

echo "SIZE raw=${RAW_SIZE} bindgen=${BG_SIZE} js_glue=${JS_SIZE} wasm_opt_Oz=${OPT_SIZE} brotli_q11=${BR_SIZE}"
echo "MB   raw=$(echo "scale=2; ${RAW_SIZE}/1048576" | bc) bindgen=$(echo "scale=2; ${BG_SIZE}/1048576" | bc) opt=$(echo "scale=2; ${OPT_SIZE}/1048576" | bc) brotli=$(echo "scale=2; ${BR_SIZE}/1048576" | bc)"
