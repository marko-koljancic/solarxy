#!/usr/bin/env python3
"""Regenerate principled_extensions.gltf.

The fixture is hand-authored against the KHR extension specifications rather
than produced by our own exporter, which is the whole point of it: a
round-trip through write_glb_bytes only proves we agree with ourselves. This
file proves we read the JSON shape the specifications actually define.

It carries two materials:

  0. principled_full    every one of the nine extensions, with values chosen
                        to be distinct from each other and from every
                        default, plus texture references on the slots that
                        have them.
  1. principled_broken  the same extensions with malformed payloads: a
                        factor that is a string, a colour with two
                        components instead of three, a texture index past
                        the end of the texture table, and an extension whose
                        value is an array rather than an object. Every one
                        must produce a diagnostic and leave the rest of the
                        material intact.

Usage: python3 principled_extensions.gen.py > principled_extensions.gltf
"""

import base64
import json
import struct
import sys
import zlib


def png(rgb):
    """A 2x2 PNG of one solid colour, as a data URI."""
    raw = b"".join(b"\x00" + bytes(rgb) * 2 for _ in range(2))
    def chunk(tag, data):
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))
    ihdr = struct.pack(">IIBBBBB", 2, 2, 8, 2, 0, 0, 0)
    blob = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(raw))
        + chunk(b"IEND", b"")
    )
    return "data:image/png;base64," + base64.b64encode(blob).decode("ascii")


# A single triangle: three positions and three indices.
positions = [(0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0)]
pos_bytes = b"".join(struct.pack("<fff", *p) for p in positions)
idx_bytes = struct.pack("<III", 0, 1, 2)
buffer_bytes = pos_bytes + idx_bytes

doc = {
    "asset": {"version": "2.0"},
    "scene": 0,
    "scenes": [{"nodes": [0, 1]}],
    "nodes": [{"mesh": 0}, {"mesh": 1}],
    "meshes": [
        {
            "name": "full",
            "primitives": [{"attributes": {"POSITION": 0}, "indices": 1, "material": 0}],
        },
        {
            "name": "broken",
            "primitives": [{"attributes": {"POSITION": 0}, "indices": 1, "material": 1}],
        },
    ],
    "materials": [
        {
            "name": "principled_full",
            "pbrMetallicRoughness": {
                "baseColorFactor": [1.0, 1.0, 1.0, 1.0],
                "metallicFactor": 0.0,
                "roughnessFactor": 0.4,
            },
            "extensions": {
                "KHR_materials_ior": {"ior": 1.7},
                "KHR_materials_transmission": {
                    "transmissionFactor": 0.9,
                    "transmissionTexture": {"index": 0},
                },
                "KHR_materials_volume": {
                    "thicknessFactor": 2.5,
                    "attenuationColor": [0.8, 0.2, 0.1],
                    "attenuationDistance": 3.0,
                    "thicknessTexture": {"index": 1},
                },
                "KHR_materials_specular": {
                    "specularFactor": 0.6,
                    "specularColorFactor": [0.9, 0.8, 0.7],
                },
                "KHR_materials_emissive_strength": {"emissiveStrength": 4.0},
                "KHR_materials_clearcoat": {
                    "clearcoatFactor": 0.75,
                    "clearcoatRoughnessFactor": 0.25,
                    "clearcoatTexture": {"index": 0},
                },
                "KHR_materials_sheen": {
                    "sheenColorFactor": [0.4, 0.5, 0.6],
                    "sheenRoughnessFactor": 0.35,
                },
                "KHR_materials_iridescence": {
                    "iridescenceFactor": 0.5,
                    "iridescenceIor": 1.8,
                    "iridescenceThicknessMinimum": 200.0,
                    "iridescenceThicknessMaximum": 600.0,
                },
                "KHR_materials_anisotropy": {
                    "anisotropyStrength": 0.65,
                    "anisotropyRotation": 1.2,
                },
            },
        },
        {
            "name": "principled_broken",
            "pbrMetallicRoughness": {
                "baseColorFactor": [0.25, 0.5, 0.75, 1.0],
                "metallicFactor": 0.0,
                "roughnessFactor": 0.6,
            },
            # Only the four extensions read through the raw map can be
            # malformed here. The five with typed accessors are validated by
            # the gltf crate itself, which rejects the whole document on a
            # bad index rather than reaching our code, exactly as it already
            # does for a bad baseColorTexture. That asymmetry is the point of
            # splitting the import by mechanism.
            "extensions": {
                # A factor that is a string, not a number, beside a
                # well-formed neighbour that must still land.
                "KHR_materials_clearcoat": {
                    "clearcoatFactor": "very shiny",
                    "clearcoatRoughnessFactor": 0.5,
                    # A texture index past the end of the texture table.
                    "clearcoatNormalTexture": {"index": 99},
                },
                # A colour with two components instead of three.
                "KHR_materials_sheen": {"sheenColorFactor": [0.1, 0.2]},
                # A texture reference with no index at all.
                "KHR_materials_iridescence": {
                    "iridescenceFactor": 0.5,
                    "iridescenceTexture": {"texCoord": 0},
                },
                # The extension value is an array, not an object.
                "KHR_materials_anisotropy": [1, 2, 3],
            },
        },
    ],
    "textures": [{"source": 0}, {"source": 1}],
    "images": [{"uri": png((255, 0, 0))}, {"uri": png((0, 0, 255))}],
    "accessors": [
        {
            "bufferView": 0,
            "componentType": 5126,
            "count": 3,
            "type": "VEC3",
            "min": [0.0, 0.0, 0.0],
            "max": [1.0, 1.0, 0.0],
        },
        {"bufferView": 1, "componentType": 5125, "count": 3, "type": "SCALAR"},
    ],
    "bufferViews": [
        {"buffer": 0, "byteOffset": 0, "byteLength": len(pos_bytes), "target": 34962},
        {
            "buffer": 0,
            "byteOffset": len(pos_bytes),
            "byteLength": len(idx_bytes),
            "target": 34963,
        },
    ],
    "buffers": [
        {
            "byteLength": len(buffer_bytes),
            "uri": "data:application/octet-stream;base64,"
            + base64.b64encode(buffer_bytes).decode("ascii"),
        }
    ],
    "extensionsUsed": [
        "KHR_materials_anisotropy",
        "KHR_materials_clearcoat",
        "KHR_materials_emissive_strength",
        "KHR_materials_ior",
        "KHR_materials_iridescence",
        "KHR_materials_sheen",
        "KHR_materials_specular",
        "KHR_materials_transmission",
        "KHR_materials_volume",
    ],
}

json.dump(doc, sys.stdout, indent=1)
sys.stdout.write("\n")
