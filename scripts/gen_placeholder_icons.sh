#!/usr/bin/env bash
#
# gen_placeholder_icons.sh — regenerate v0.5.0 placeholder icons.
#
# Produces a flat navy tile (#1a2a4a) with a centered off-white disc (#e0e8ff)
# at the following paths:
#
#   res/bundle/solarxy-{256,512,1024}.png   — sizes referenced by cargo-bundle
#   res/bundle/solarxy.png                  — 256x256 hicolor icon for cargo-deb
#   res/bundle/solarxy.icns                 — macOS icon (built via iconutil)
#   res/bundle/solarxy.ico                  — Windows multi-size (16/32/48/64/128/256)
#
# Dependencies: python3 (stdlib only), sips (macOS), iconutil (macOS).
# Runs in ~2 s on a modern Mac. Safe to re-run — all outputs are overwritten.
#
# When real icon art lands (0.6.0-09), replace the master generator below with
# a one-line `cp real-source.png master.png` and rerun this script.
#
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUNDLE="${ROOT}/res/bundle"
mkdir -p "${BUNDLE}"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "${TMPDIR}"' EXIT

MASTER="${TMPDIR}/solarxy-1024.png"

# ---- 1. Render 1024×1024 master PNG via Python stdlib ---------------------
python3 - "${MASTER}" <<'PY'
import struct, sys, zlib

SIZE = 1024
BG   = (0x1a, 0x2a, 0x4a, 0xff)     # navy — matches viewport bg
FG   = (0xe0, 0xe8, 0xff, 0xff)     # off-white

def png_chunk(tag: bytes, data: bytes) -> bytes:
    body = tag + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

def render(path: str) -> None:
    cx = cy = SIZE / 2
    r2 = (SIZE * 0.35) ** 2
    bg_row = bytes(BG) * SIZE
    # Precompute the disc mask as an XBM-style bitmap for speed.
    rows = []
    for y in range(SIZE):
        rows.append(b"\x00")  # PNG filter byte per scanline
        dy2 = (y - cy) ** 2
        if dy2 > r2:
            rows.append(bg_row)
            continue
        # Inside disc's bounding band — compute per-pixel.
        row = bytearray(SIZE * 4)
        for x in range(SIZE):
            dx2 = (x - cx) ** 2
            c = FG if dx2 + dy2 <= r2 else BG
            row[x * 4 : x * 4 + 4] = bytes(c)
        rows.append(bytes(row))
    raw = b"".join(rows)
    ihdr = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)
    data = b"\x89PNG\r\n\x1a\n"
    data += png_chunk(b"IHDR", ihdr)
    data += png_chunk(b"IDAT", zlib.compress(raw, 9))
    data += png_chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(data)

render(sys.argv[1])
PY

# ---- 2. Copy master and derive 256/512 via sips ----------------------------
cp "${MASTER}" "${BUNDLE}/solarxy-1024.png"
sips -z 512 512 "${MASTER}" --out "${BUNDLE}/solarxy-512.png" >/dev/null
sips -z 256 256 "${MASTER}" --out "${BUNDLE}/solarxy-256.png" >/dev/null
cp "${BUNDLE}/solarxy-256.png" "${BUNDLE}/solarxy.png"   # Linux hicolor 256x256

# ---- 3. Build .icns via iconutil -------------------------------------------
ICONSET="${TMPDIR}/solarxy.iconset"
mkdir -p "${ICONSET}"
# iconutil expects specific filenames for 16/32/128/256/512 at 1x and 2x scales.
sips -z 16   16   "${MASTER}" --out "${ICONSET}/icon_16x16.png"       >/dev/null
sips -z 32   32   "${MASTER}" --out "${ICONSET}/icon_16x16@2x.png"    >/dev/null
sips -z 32   32   "${MASTER}" --out "${ICONSET}/icon_32x32.png"       >/dev/null
sips -z 64   64   "${MASTER}" --out "${ICONSET}/icon_32x32@2x.png"    >/dev/null
sips -z 128  128  "${MASTER}" --out "${ICONSET}/icon_128x128.png"     >/dev/null
sips -z 256  256  "${MASTER}" --out "${ICONSET}/icon_128x128@2x.png"  >/dev/null
sips -z 256  256  "${MASTER}" --out "${ICONSET}/icon_256x256.png"     >/dev/null
sips -z 512  512  "${MASTER}" --out "${ICONSET}/icon_256x256@2x.png"  >/dev/null
sips -z 512  512  "${MASTER}" --out "${ICONSET}/icon_512x512.png"     >/dev/null
cp   "${MASTER}"                  "${ICONSET}/icon_512x512@2x.png"
iconutil -c icns "${ICONSET}" -o "${BUNDLE}/solarxy.icns"

# ---- 4. Build multi-size .ico via Python stdlib ----------------------------
python3 - "${BUNDLE}/solarxy.ico" "${MASTER}" <<'PY'
import struct, subprocess, sys, tempfile
from pathlib import Path

out_path, master = sys.argv[1], sys.argv[2]
sizes = [16, 32, 48, 64, 128, 256]

with tempfile.TemporaryDirectory() as td:
    pngs = []
    for s in sizes:
        p = Path(td) / f"{s}.png"
        subprocess.run(
            ["sips", "-z", str(s), str(s), master, "--out", str(p)],
            check=True, stdout=subprocess.DEVNULL,
        )
        pngs.append((s, p.read_bytes()))

    # ICONDIR (6 bytes): reserved=0, type=1, count=N
    header = struct.pack("<HHH", 0, 1, len(pngs))
    # Each ICONDIRENTRY is 16 bytes; PNG data starts right after the last entry.
    data_offset = len(header) + 16 * len(pngs)

    entries = b""
    blobs = b""
    for s, png in pngs:
        # width/height byte = 0 means 256 for ICO.
        w = h = 0 if s == 256 else s
        entries += struct.pack(
            "<BBBBHHII",
            w, h,
            0,              # palette (0 = none)
            0,              # reserved
            1,              # color planes
            32,             # bits per pixel
            len(png),       # data size
            data_offset,    # data offset
        )
        blobs += png
        data_offset += len(png)

    Path(out_path).write_bytes(header + entries + blobs)
PY

# ---- 5. Quick checksums so you can see if anything changed -----------------
cd "${BUNDLE}"
printf '\n== Generated:\n'
ls -la solarxy*.png solarxy.ico solarxy.icns
