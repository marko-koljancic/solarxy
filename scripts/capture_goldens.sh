#!/usr/bin/env bash
# Captures the golden set for every model the gate covers, into <out>/<model>/.
#
# Two models, and both are load-bearing:
#
#   dragon - untextured OBJ (no mtllib, no usemtl). Covers geometry, lighting,
#            wireframe, depth, material-id and validation.
#   frog   - OBJ + MTL + map_Kd. Covers the albedo texture path, the base-colour
#            factor and texture filtering.
#
# The dragon alone is NOT enough, and this is not hypothetical: between Phase 8
# and Phase 15 the dragon captures were pixel-identical in all five modes while
# the frog captures differed on 55k pixels. The dragon literally cannot see the
# material/texture pipeline, so a suite with only the dragon reports "no change"
# through a rendering rewrite.
#
# Usage: capture_goldens.sh <out_dir>
set -euo pipefail
cd "$(dirname "$0")/.."

OUT="${1:?usage: capture_goldens.sh <out_dir>}"
mkdir -p "$OUT"

FROG="res/models/frog/ooz3d-export-model-20260329-181053.obj"

cargo run --release -p solarxy-renderer --example golden -- \
    capture --model res/models/xyzrgb_dragon.obj --out "$OUT/dragon"

cargo run --release -p solarxy-renderer --example golden -- \
    capture --model "$FROG" --out "$OUT/frog"

echo "captured goldens into $OUT (dragon, frog)"
