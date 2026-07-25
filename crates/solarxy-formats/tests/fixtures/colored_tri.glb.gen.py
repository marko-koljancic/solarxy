#!/usr/bin/env python3
"""Regenerate colored_tri.glb - a single triangle carrying COLOR_0
(normalized unsigned-byte RGBA), the vertex-color import fixture.

Run: `python3 colored_tri.glb.gen.py` (writes colored_tri.glb beside this
script). Same GLB layout as triangle.glb.gen.py.

Geometry:
  Positions: (0,0,0), (1,0,0), (0,1,0)       - 3 * 12 B = 36 B
  COLOR_0:   red, green, half-blue (u8 RGBA) - 3 *  4 B = 12 B
  Indices:   0, 1, 2                         - 3 *  4 B = 12 B
glTF COLOR_0 is linear; the importer must NOT sRGB-decode these values.
"""
import json
import pathlib
import struct

positions = struct.pack("<9f", 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0)
colors = struct.pack("<12B", 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 128, 255)
indices = struct.pack("<3I", 0, 1, 2)
bin_payload = positions + colors + indices

gltf = {
    "asset": {"version": "2.0"},
    "scene": 0,
    "scenes": [{"nodes": [0]}],
    "nodes": [{"mesh": 0}],
    "meshes": [{"primitives": [
        {"attributes": {"POSITION": 0, "COLOR_0": 1}, "indices": 2}
    ]}],
    "accessors": [
        {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
         "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]},
        {"bufferView": 1, "componentType": 5121, "count": 3, "type": "VEC4",
         "normalized": True},
        {"bufferView": 2, "componentType": 5125, "count": 3, "type": "SCALAR"},
    ],
    "bufferViews": [
        {"buffer": 0, "byteOffset": 0, "byteLength": 36, "target": 34962},
        {"buffer": 0, "byteOffset": 36, "byteLength": 12, "target": 34962},
        {"buffer": 0, "byteOffset": 48, "byteLength": 12, "target": 34963},
    ],
    "buffers": [{"byteLength": 60}],
}
json_bytes = json.dumps(gltf, separators=(",", ":")).encode("utf-8")
json_bytes += b" " * ((-len(json_bytes)) % 4)
bin_payload += b"\x00" * ((-len(bin_payload)) % 4)

JSON_TYPE = 0x4E4F534A
BIN_TYPE = 0x004E4942
GLTF_MAGIC = 0x46546C67
json_chunk = struct.pack("<II", len(json_bytes), JSON_TYPE) + json_bytes
bin_chunk = struct.pack("<II", len(bin_payload), BIN_TYPE) + bin_payload

total_length = 12 + len(json_chunk) + len(bin_chunk)
header = struct.pack("<III", GLTF_MAGIC, 2, total_length)

out = pathlib.Path(__file__).parent / "colored_tri.glb"
out.write_bytes(header + json_chunk + bin_chunk)
print(f"wrote {out} ({total_length} bytes)")
