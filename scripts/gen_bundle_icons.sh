#!/usr/bin/env bash
#
# gen_bundle_icons.sh — derive every res/bundle/ icon artifact from a single
# 512x512 master PNG.
#
# Inputs
#   res/bundle/solarxy-512.png     — required, the source of truth.
#                                    Drop a higher-res master here (e.g. 1024)
#                                    if you have one and adjust SIZES below.
#
# Outputs (overwritten)
#   res/bundle/solarxy-256.png     — referenced by cargo-bundle paths
#   res/bundle/solarxy-1024.png    — referenced by cargo-bundle paths
#   res/bundle/solarxy.png         — Linux hicolor 256x256 (AppImage uses this)
#   res/bundle/solarxy.icns        — macOS multi-resolution icon
#   res/bundle/solarxy.ico         — Windows multi-size icon
#
# Dependencies
#   - ImageMagick (`magick`) — required.
#   - Python 3   — required for the .icns builder (stdlib only).
#
# Cross-platform: runs on Linux and macOS without iconutil.
#
# Re-run after replacing solarxy-512.png with new art.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUNDLE="${ROOT}/res/bundle"
MASTER="${BUNDLE}/solarxy-512.png"

if [[ ! -f "${MASTER}" ]]; then
    echo "error: ${MASTER} not found — drop a 512x512 PNG master there first." >&2
    exit 1
fi

if ! command -v magick >/dev/null 2>&1; then
    echo "error: ImageMagick (magick) not found in PATH." >&2
    exit 1
fi

cd "${BUNDLE}"

# ---- 1. Standalone PNGs at the sizes cargo-bundle expects ------------------
magick solarxy-512.png -resize 256x256  solarxy-256.png
magick solarxy-512.png -resize 1024x1024 solarxy-1024.png
cp solarxy-256.png solarxy.png   # Linux hicolor 256x256

# ---- 2. Windows multi-size .ico (16/32/48/64/128/256) ----------------------
magick solarxy-512.png \
    \( -clone 0 -resize 16x16   \) \
    \( -clone 0 -resize 32x32   \) \
    \( -clone 0 -resize 48x48   \) \
    \( -clone 0 -resize 64x64   \) \
    \( -clone 0 -resize 128x128 \) \
    \( -clone 0 -resize 256x256 \) \
    -delete 0 solarxy.ico

# ---- 3. macOS .icns (built in pure Python, no iconutil dependency) ---------
# ICNS layout: 8-byte header ("icns" + uint32 BE total size), then sections,
# each with a 4-byte type code + uint32 BE section size (incl. header) + PNG.
python3 - <<'PY'
import struct, subprocess, tempfile
from pathlib import Path

OUT = Path("solarxy.icns")
SECTIONS = [
    # (type_code, dimension) — Apple's expected variants
    (b"icp4", 16),    (b"icp5", 32),    (b"icp6", 64),
    (b"ic07", 128),   (b"ic08", 256),   (b"ic09", 512),
    (b"ic10", 1024),  (b"ic11", 32),    (b"ic12", 64),
    (b"ic13", 256),   (b"ic14", 512),
]

with tempfile.TemporaryDirectory() as td:
    body = b""
    for tag, dim in SECTIONS:
        p = Path(td) / f"{tag.decode()}_{dim}.png"
        subprocess.run(
            ["magick", "solarxy-1024.png", "-resize", f"{dim}x{dim}", str(p)],
            check=True,
        )
        png = p.read_bytes()
        body += tag + struct.pack(">I", 8 + len(png)) + png

    OUT.write_bytes(b"icns" + struct.pack(">I", 8 + len(body)) + body)
PY

# ---- 4. Summary ------------------------------------------------------------
printf '\n== Generated:\n'
ls -la solarxy*.png solarxy.ico solarxy.icns
