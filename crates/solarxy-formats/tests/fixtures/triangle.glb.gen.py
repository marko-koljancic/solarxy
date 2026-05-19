#!/usr/bin/env python3
"""Regenerate triangle.glb — minimal valid glTF 2.0 binary, single triangle.

Run: `python3 triangle.glb.gen.py` (writes triangle.glb beside this script).

GLB layout:
  Header (12 B): magic 'glTF', version 2 (uint32), total length (uint32)
  JSON chunk:    4 B length, 4 B type 'JSON', JSON bytes (4-byte aligned, padded with spaces)
  BIN chunk:     4 B length, 4 B type 'BIN\\0', binary bytes (4-byte aligned, padded with zeros)

Geometry:
  Positions: (0,0,0), (1,0,0), (0,1,0) — 3 * 12 B = 36 B
  Indices:   0, 1, 2                   — 3 *  4 B = 12 B
  No materials, no UVs, no normals — minimal valid load test.
"""
import json
import pathlib
import struct

# Binary payload: positions then indices.
positions = struct.pack("<9f", 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0)
indices = struct.pack("<3I", 0, 1, 2)
bin_payload = positions + indices

# JSON metadata.
gltf = {
    "asset": {"version": "2.0"},
    "scene": 0,
    "scenes": [{"nodes": [0]}],
    "nodes": [{"mesh": 0}],
    "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]}],
    "accessors": [
        {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
         "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]},
        {"bufferView": 1, "componentType": 5125, "count": 3, "type": "SCALAR"},
    ],
    "bufferViews": [
        {"buffer": 0, "byteOffset": 0,  "byteLength": 36, "target": 34962},
        {"buffer": 0, "byteOffset": 36, "byteLength": 12, "target": 34963},
    ],
    "buffers": [{"byteLength": 48}],
}
json_bytes = json.dumps(gltf, separators=(",", ":")).encode("utf-8")
json_bytes += b" " * ((-len(json_bytes)) % 4)
bin_payload += b"\x00" * ((-len(bin_payload)) % 4)

# Chunks (length, four-CC type, payload).
JSON_TYPE = 0x4E4F534A  # 'JSON'
BIN_TYPE  = 0x004E4942  # 'BIN\0'
GLTF_MAGIC = 0x46546C67  # 'glTF'
json_chunk = struct.pack("<II", len(json_bytes),  JSON_TYPE) + json_bytes
bin_chunk  = struct.pack("<II", len(bin_payload), BIN_TYPE)  + bin_payload

total_length = 12 + len(json_chunk) + len(bin_chunk)
header = struct.pack("<III", GLTF_MAGIC, 2, total_length)

out = pathlib.Path(__file__).parent / "triangle.glb"
out.write_bytes(header + json_chunk + bin_chunk)
print(f"wrote {out} ({total_length} bytes)")
