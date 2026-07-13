#!/usr/bin/env python3
"""Regenerate the textured fixtures for the Phase 13 texture matrix tests.

Run: `python3 textured.gen.py` (writes all outputs beside this script).

One 2x2 RGBA PNG (texel.png) with four known texels
  row 0: red (255,0,0,255), green (0,255,0,255)
  row 1: blue (0,0,255,255), white (255,255,255,255)
is delivered to the loaders through every texture path the matrix covers:

  textured_embedded.gltf   image as a data: URI (embedded glTF)
  textured.glb             image as a binary-chunk bufferView (GLB)
  textured_external.gltf   image as an external file URI (+ .bin buffer)
  textured_external.bin    external geometry buffer for the above
  textured.obj / .mtl      OBJ with MTL map_Kd -> texel.png

Geometry everywhere: a unit quad (4 vertices, 2 triangles) with UVs, so
tex_coords survive alongside the texture. Tests assert the decoded RGBA
bytes match PIXELS exactly.
"""
import base64
import json
import pathlib
import struct
import zlib

HERE = pathlib.Path(__file__).parent

# ---- texel.png (2x2 RGBA8) -------------------------------------------------
PIXELS = bytes(
    [255, 0, 0, 255, 0, 255, 0, 255,
     0, 0, 255, 255, 255, 255, 255, 255]
)


def png_chunk(ctype: bytes, data: bytes) -> bytes:
    return (
        struct.pack(">I", len(data))
        + ctype
        + data
        + struct.pack(">I", zlib.crc32(ctype + data) & 0xFFFFFFFF)
    )


ihdr = struct.pack(">IIBBBBB", 2, 2, 8, 6, 0, 0, 0)
raw = b"\x00" + PIXELS[0:8] + b"\x00" + PIXELS[8:16]
png = (
    b"\x89PNG\r\n\x1a\n"
    + png_chunk(b"IHDR", ihdr)
    + png_chunk(b"IDAT", zlib.compress(raw))
    + png_chunk(b"IEND", b"")
)
(HERE / "texel.png").write_bytes(png)

# ---- shared quad geometry ----------------------------------------------------
positions = struct.pack("<12f", 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 0)
uvs = struct.pack("<8f", 0, 1, 1, 1, 0, 0, 1, 0)
indices = struct.pack("<6I", 0, 1, 2, 2, 1, 3)
geo = positions + uvs + indices  # 48 + 32 + 24 = 104 bytes


def gltf_json(buffer_entry, image_entry, extra_buffer_views=None):
    views = [
        {"buffer": 0, "byteOffset": 0, "byteLength": 48, "target": 34962},
        {"buffer": 0, "byteOffset": 48, "byteLength": 32, "target": 34962},
        {"buffer": 0, "byteOffset": 80, "byteLength": 24, "target": 34963},
    ] + (extra_buffer_views or [])
    return {
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0}],
        "meshes": [{
            "name": "textured_quad",
            "primitives": [{
                "attributes": {"POSITION": 0, "TEXCOORD_0": 1},
                "indices": 2,
                "material": 0,
            }],
        }],
        "materials": [{
            "name": "texel_material",
            "pbrMetallicRoughness": {
                "baseColorTexture": {"index": 0},
                "metallicFactor": 0.0,
                "roughnessFactor": 1.0,
            },
        }],
        "textures": [{"source": 0}],
        "images": [image_entry],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
             "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]},
            {"bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC2"},
            {"bufferView": 2, "componentType": 5125, "count": 6, "type": "SCALAR"},
        ],
        "bufferViews": views,
        "buffers": [buffer_entry],
    }


# ---- textured_embedded.gltf (data-URI buffer + data-URI image) --------------
embedded = gltf_json(
    {"byteLength": len(geo),
     "uri": "data:application/octet-stream;base64," + base64.b64encode(geo).decode()},
    {"uri": "data:image/png;base64," + base64.b64encode(png).decode()},
)
(HERE / "textured_embedded.gltf").write_text(
    json.dumps(embedded, separators=(",", ":")) + "\n"
)

# ---- textured.glb (image as binary bufferView) -------------------------------
bin_payload = geo + b"\x00" * ((-len(geo)) % 4) + png
png_offset = len(geo) + ((-len(geo)) % 4)
glb_json = gltf_json(
    {"byteLength": len(bin_payload)},
    {"bufferView": 3, "mimeType": "image/png"},
    extra_buffer_views=[
        {"buffer": 0, "byteOffset": png_offset, "byteLength": len(png)},
    ],
)
json_bytes = json.dumps(glb_json, separators=(",", ":")).encode()
json_bytes += b" " * ((-len(json_bytes)) % 4)
bin_payload += b"\x00" * ((-len(bin_payload)) % 4)
json_chunk = struct.pack("<II", len(json_bytes), 0x4E4F534A) + json_bytes
bin_chunk = struct.pack("<II", len(bin_payload), 0x004E4942) + bin_payload
total = 12 + len(json_chunk) + len(bin_chunk)
header = struct.pack("<III", 0x46546C67, 2, total)
(HERE / "textured.glb").write_bytes(header + json_chunk + bin_chunk)

# ---- textured_external.gltf (+ .bin, external texel.png) --------------------
external = gltf_json(
    {"byteLength": len(geo), "uri": "textured_external.bin"},
    {"uri": "texel.png"},
)
(HERE / "textured_external.gltf").write_text(
    json.dumps(external, separators=(",", ":")) + "\n"
)
(HERE / "textured_external.bin").write_bytes(geo)

# ---- textured.obj / textured.mtl ---------------------------------------------
(HERE / "textured.obj").write_text(
    "mtllib textured.mtl\n"
    "v 0 0 0\nv 1 0 0\nv 0 1 0\nv 1 1 0\n"
    "vt 0 0\nvt 1 0\nvt 0 1\nvt 1 1\n"
    "usemtl texel\n"
    "f 1/1 2/2 3/3\nf 3/3 2/2 4/4\n"
)
(HERE / "textured.mtl").write_text(
    "newmtl texel\nKd 1 1 1\nmap_Kd texel.png\n"
)

print("wrote texel.png, textured_embedded.gltf, textured.glb, "
      "textured_external.gltf/.bin, textured.obj/.mtl")
