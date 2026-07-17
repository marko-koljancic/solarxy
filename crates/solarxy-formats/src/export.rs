//! Geometry and image WRITERS (context-expansion phase 21, decision C-7):
//! the first export surface in the workspace. Byte-first like the
//! loaders; the caller owns file handling.
//!
//! Scope (v1): triangulated geometry with normals and UVs. Materials do
//! not export yet (a recorded follow-up); glTF without materials is valid
//! and every format here round-trips through this crate's own loaders,
//! which the tests pin.

use solarxy_core::RawImageData;

use crate::FormatsError;

/// One mesh to write, borrowed from whatever cooked representation the
/// caller holds (the graph's `KernelMesh` maps field-for-field).
pub struct ExportMesh<'a> {
    pub name: &'a str,
    pub positions: &'a [[f32; 3]],
    pub normals: Option<&'a [[f32; 3]]>,
    pub tex_coords: Option<&'a [[f32; 2]]>,
    /// Triangle list (length a multiple of 3).
    pub indices: &'a [u32],
}

/// Wavefront OBJ: one `o` block per mesh, shared v/vt/vn numbering.
#[must_use]
pub fn write_obj_bytes(meshes: &[ExportMesh<'_>]) -> Vec<u8> {
    use std::fmt::Write as _;
    let mut out = String::from("# Exported by Solarxy\n");
    // OBJ indices are 1-based and global across the file.
    let (mut v_base, mut vt_base, mut vn_base) = (1u32, 1u32, 1u32);
    for (i, mesh) in meshes.iter().enumerate() {
        let name = if mesh.name.is_empty() {
            format!("mesh_{i}")
        } else {
            mesh.name.replace(char::is_whitespace, "_")
        };
        let _ = writeln!(out, "o {name}");
        for p in mesh.positions {
            let _ = writeln!(out, "v {} {} {}", p[0], p[1], p[2]);
        }
        if let Some(uvs) = mesh.tex_coords {
            for t in uvs {
                let _ = writeln!(out, "vt {} {}", t[0], t[1]);
            }
        }
        if let Some(normals) = mesh.normals {
            for n in normals {
                let _ = writeln!(out, "vn {} {} {}", n[0], n[1], n[2]);
            }
        }
        for tri in mesh.indices.chunks_exact(3) {
            let f = |i: u32| {
                let v = v_base + i;
                match (mesh.tex_coords.is_some(), mesh.normals.is_some()) {
                    (true, true) => format!("{v}/{}/{}", vt_base + i, vn_base + i),
                    (true, false) => format!("{v}/{}", vt_base + i),
                    (false, true) => format!("{v}//{}", vn_base + i),
                    (false, false) => format!("{v}"),
                }
            };
            let _ = writeln!(out, "f {} {} {}", f(tri[0]), f(tri[1]), f(tri[2]));
        }
        let count = mesh.positions.len() as u32;
        v_base += count;
        if mesh.tex_coords.is_some() {
            vt_base += count;
        }
        if mesh.normals.is_some() {
            vn_base += count;
        }
    }
    out.into_bytes()
}

/// Binary STL: every mesh's triangles concatenated (STL has no objects,
/// no normals-per-vertex, no UVs; facet normals are recomputed).
pub fn write_stl_bytes(meshes: &[ExportMesh<'_>]) -> Result<Vec<u8>, FormatsError> {
    let mut tris = Vec::new();
    for mesh in meshes {
        for tri in mesh.indices.chunks_exact(3) {
            let p = |i: u32| mesh.positions[i as usize];
            let (a, b, c) = (p(tri[0]), p(tri[1]), p(tri[2]));
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
            tris.push(stl_io::Triangle {
                normal: stl_io::Normal::new([n[0] / len, n[1] / len, n[2] / len]),
                vertices: [
                    stl_io::Vertex::new(a),
                    stl_io::Vertex::new(b),
                    stl_io::Vertex::new(c),
                ],
            });
        }
    }
    let mut out = Vec::new();
    stl_io::write_stl(&mut out, tris.iter()).map_err(|e| FormatsError::Export {
        format: "stl",
        message: e.to_string(),
    })?;
    Ok(out)
}

/// ASCII PLY: meshes merged into one vertex/face element pair (PLY has no
/// object concept). Normals and UVs export when EVERY mesh carries them.
#[must_use]
pub fn write_ply_bytes(meshes: &[ExportMesh<'_>]) -> Vec<u8> {
    use std::fmt::Write as _;
    let total_verts: usize = meshes.iter().map(|m| m.positions.len()).sum();
    let total_faces: usize = meshes.iter().map(|m| m.indices.len() / 3).sum();
    let with_normals = !meshes.is_empty() && meshes.iter().all(|m| m.normals.is_some());
    let with_uvs = !meshes.is_empty() && meshes.iter().all(|m| m.tex_coords.is_some());

    let mut out = String::new();
    out.push_str("ply\nformat ascii 1.0\ncomment Exported by Solarxy\n");
    let _ = writeln!(out, "element vertex {total_verts}");
    out.push_str("property float x\nproperty float y\nproperty float z\n");
    if with_normals {
        out.push_str("property float nx\nproperty float ny\nproperty float nz\n");
    }
    if with_uvs {
        out.push_str("property float u\nproperty float v\n");
    }
    let _ = writeln!(out, "element face {total_faces}");
    out.push_str("property list uchar uint vertex_indices\nend_header\n");

    for mesh in meshes {
        for (i, p) in mesh.positions.iter().enumerate() {
            let _ = write!(out, "{} {} {}", p[0], p[1], p[2]);
            if with_normals {
                let n = mesh.normals.unwrap()[i];
                let _ = write!(out, " {} {} {}", n[0], n[1], n[2]);
            }
            if with_uvs {
                let t = mesh.tex_coords.unwrap()[i];
                let _ = write!(out, " {} {}", t[0], t[1]);
            }
            out.push('\n');
        }
    }
    let mut base = 0u32;
    for mesh in meshes {
        for tri in mesh.indices.chunks_exact(3) {
            let _ = writeln!(
                out,
                "3 {} {} {}",
                base + tri[0],
                base + tri[1],
                base + tri[2]
            );
        }
        base += mesh.positions.len() as u32;
    }
    out.into_bytes()
}

/// Binary glTF (GLB): one buffer, interleaved-free accessors, one node
/// per mesh, no materials (v1). Hand-built JSON + BIN container; the
/// format is small enough that the typed builder buys nothing.
pub fn write_glb_bytes(meshes: &[ExportMesh<'_>]) -> Result<Vec<u8>, FormatsError> {
    let mut bin: Vec<u8> = Vec::new();
    let mut buffer_views = Vec::new();
    let mut accessors = Vec::new();
    let mut json_meshes = Vec::new();
    let mut nodes = Vec::new();

    let mut push_view = |bin: &mut Vec<u8>, bytes: &[u8], target: u32| -> usize {
        // 4-byte alignment per the spec.
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let offset = bin.len();
        bin.extend_from_slice(bytes);
        buffer_views.push(serde_json::json!({
            "buffer": 0,
            "byteOffset": offset,
            "byteLength": bytes.len(),
            "target": target,
        }));
        buffer_views.len() - 1
    };

    for (i, mesh) in meshes.iter().enumerate() {
        let mut attributes = serde_json::Map::new();

        let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in mesh.positions {
            for c in 0..3 {
                min[c] = min[c].min(p[c]);
                max[c] = max[c].max(p[c]);
            }
        }
        let view = push_view(&mut bin, bytemuck_cast(mesh.positions), 34962);
        accessors.push(serde_json::json!({
            "bufferView": view, "componentType": 5126, "count": mesh.positions.len(),
            "type": "VEC3", "min": min, "max": max,
        }));
        attributes.insert("POSITION".into(), serde_json::json!(accessors.len() - 1));

        if let Some(normals) = mesh.normals {
            let view = push_view(&mut bin, bytemuck_cast(normals), 34962);
            accessors.push(serde_json::json!({
                "bufferView": view, "componentType": 5126, "count": normals.len(),
                "type": "VEC3",
            }));
            attributes.insert("NORMAL".into(), serde_json::json!(accessors.len() - 1));
        }
        if let Some(uvs) = mesh.tex_coords {
            let view = push_view(&mut bin, bytemuck_cast2(uvs), 34962);
            accessors.push(serde_json::json!({
                "bufferView": view, "componentType": 5126, "count": uvs.len(),
                "type": "VEC2",
            }));
            attributes.insert("TEXCOORD_0".into(), serde_json::json!(accessors.len() - 1));
        }

        let index_bytes: Vec<u8> = mesh.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let view = push_view(&mut bin, &index_bytes, 34963);
        accessors.push(serde_json::json!({
            "bufferView": view, "componentType": 5125, "count": mesh.indices.len(),
            "type": "SCALAR",
        }));

        json_meshes.push(serde_json::json!({
            "name": if mesh.name.is_empty() { format!("mesh_{i}") } else { mesh.name.to_string() },
            "primitives": [{
                "attributes": attributes,
                "indices": accessors.len() - 1,
                "mode": 4,
            }],
        }));
        nodes.push(serde_json::json!({ "mesh": i }));
    }

    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let json = serde_json::json!({
        "asset": { "version": "2.0", "generator": "Solarxy" },
        "scene": 0,
        "scenes": [{ "nodes": (0..nodes.len()).collect::<Vec<_>>() }],
        "nodes": nodes,
        "meshes": json_meshes,
        "buffers": [{ "byteLength": bin.len() }],
        "bufferViews": buffer_views,
        "accessors": accessors,
    });
    let mut json_bytes = serde_json::to_vec(&json).map_err(|e| FormatsError::Export {
        format: "glb",
        message: e.to_string(),
    })?;
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }

    // GLB container: header + JSON chunk + BIN chunk.
    let total = 12 + 8 + json_bytes.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    Ok(out)
}

fn bytemuck_cast(v: &[[f32; 3]]) -> &[u8] {
    // Plain little-endian f32 triples; safe on every target we ship
    // (wasm32 and the desktop triples are little-endian).
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}

fn bytemuck_cast2(v: &[[f32; 2]]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}

/// PNG-encodes an RGBA8 image.
pub fn encode_png_bytes(img: &RawImageData) -> Result<Vec<u8>, FormatsError> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::write_buffer_with_format(
        &mut out,
        &img.pixels,
        img.width,
        img.height,
        image::ExtendedColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|e| FormatsError::Export {
        format: "png",
        message: e.to_string(),
    })?;
    Ok(out.into_inner())
}

/// JPEG-encodes an RGBA8 image (alpha dropped), quality 1..100.
pub fn encode_jpeg_bytes(img: &RawImageData, quality: u8) -> Result<Vec<u8>, FormatsError> {
    // JPEG has no alpha: flatten onto opaque.
    let rgb: Vec<u8> = img
        .pixels
        .chunks_exact(4)
        .flat_map(|c| [c[0], c[1], c[2]])
        .collect();
    let mut out = std::io::Cursor::new(Vec::new());
    let encoder =
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100));
    image::ImageEncoder::write_image(
        encoder,
        &rgb,
        img.width,
        img.height,
        image::ExtendedColorType::Rgb8,
    )
    .map_err(|e| FormatsError::Export {
        format: "jpeg",
        message: e.to_string(),
    })?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad() -> (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<u32>) {
        (
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
            ],
            vec![[0.0, 0.0, 1.0]; 4],
            vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            vec![0, 1, 2, 0, 2, 3],
        )
    }

    fn export_quad<'a>(
        d: &'a (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<u32>),
    ) -> ExportMesh<'a> {
        ExportMesh {
            name: "quad",
            positions: &d.0,
            normals: Some(&d.1),
            tex_coords: Some(&d.2),
            indices: &d.3,
        }
    }

    #[test]
    fn obj_round_trips_through_our_loader() {
        let d = quad();
        let bytes = write_obj_bytes(&[export_quad(&d)]);
        let model = crate::obj::load_obj_bytes(&bytes, &mut crate::NoAssets).expect("reimport");
        let mesh = &model.meshes[0];
        assert_eq!(mesh.positions.len(), 4);
        assert_eq!(mesh.indices.len(), 6);
        assert!(mesh.tex_coords.is_some());
    }

    #[test]
    fn stl_round_trips_through_our_loader() {
        let d = quad();
        let bytes = write_stl_bytes(&[export_quad(&d)]).expect("write");
        let model = crate::stl::load_stl_bytes(&bytes, "quad.stl").expect("reimport");
        // STL deduplicates per-triangle vertices on load; the triangle
        // count is the invariant.
        assert_eq!(model.meshes[0].indices.len(), 6);
    }

    #[test]
    fn ply_round_trips_through_our_loader() {
        let d = quad();
        let bytes = write_ply_bytes(&[export_quad(&d)]);
        let model = crate::ply::load_ply_bytes(&bytes, "quad.ply").expect("reimport");
        assert_eq!(model.meshes[0].positions.len(), 4);
        assert_eq!(model.meshes[0].indices.len(), 6);
    }

    #[test]
    fn glb_round_trips_through_our_loader() {
        let d = quad();
        let bytes = write_glb_bytes(&[export_quad(&d)]).expect("write");
        let model = crate::gltf::load_gltf_bytes(&bytes, &mut crate::NoAssets).expect("reimport");
        let mesh = &model.meshes[0];
        assert_eq!(mesh.positions.len(), 4);
        assert_eq!(mesh.indices.len(), 6);
        assert!(mesh.normals.is_some());
        assert!(mesh.tex_coords.is_some());
    }

    #[test]
    fn png_and_jpeg_encode_and_png_redecodes() {
        let img = RawImageData::new(vec![255, 0, 0, 255, 0, 255, 0, 255], 2, 1);
        let png = encode_png_bytes(&img).expect("png");
        let back = crate::decode_image_bytes(&png).expect("decode");
        assert_eq!((back.width, back.height), (2, 1));
        assert_eq!(&back.pixels[0..4], &[255, 0, 0, 255]);
        let jpg = encode_jpeg_bytes(&img, 90).expect("jpeg");
        assert!(jpg.starts_with(&[0xFF, 0xD8]), "JPEG SOI marker");
    }
}
