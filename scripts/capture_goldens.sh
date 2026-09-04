#!/usr/bin/env bash
# Captures the golden set for every model the gate covers, into <out>/<model>/.
#
# Two models, and both are load-bearing:
#
#   dragon - untextured OBJ (no mtllib, no usemtl). Covers geometry, lighting,
#            wireframe, depth, material-id and validation.
#   knot   - OBJ + MTL + map_Kd. Covers the albedo texture path, the base-colour
#            factor and texture filtering.
#
# The dragon alone is NOT enough, and this is not hypothetical: for a stretch of
# the web milestone the dragon captures were pixel-identical in all five modes
# while the textured captures differed on 55k pixels. The dragon cannot see the
# material/texture pipeline, so a suite with only the dragon reports "no change"
# through a rendering rewrite.
#
# Usage: capture_goldens.sh <out_dir>
set -euo pipefail
cd "$(dirname "$0")/.."

OUT="${1:?usage: capture_goldens.sh <out_dir>}"
mkdir -p "$OUT"

KNOT="res/models/knot/knot.obj"

cargo run --release -p solarxy-host --example golden -- \
    capture --model res/models/xyzrgb_dragon.obj --out "$OUT/dragon"

cargo run --release -p solarxy-host --example golden -- \
    capture --model "$KNOT" --out "$OUT/knot"

echo "captured goldens into $OUT (dragon, knot)"
