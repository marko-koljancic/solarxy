//! Geometry and image WRITERS:
//! the first export surface in the workspace. Byte-first like the
//! loaders; the caller owns file handling.
//!
//! Every mesh carries its topology, and each writer maps it onto what its
//! format can say: GLB uses the matching primitive modes (points, lines,
//! triangles), OBJ writes `p`/`l`/`f` records, PLY exports vertices with
//! faces only from triangle meshes (an all-point export is a face-less
//! point-cloud PLY), and STL, which is facets-only by definition, skips
//! non-triangle meshes. Vertex colors export where the format carries
//! them (GLB `COLOR_0` as linear float, PLY as sRGB-encoded uchar
//! properties); GLB additionally exports the material table with
//! PNG-embedded textures deduplicated by content hash. Every format
//! round-trips through this crate's own loaders, which the tests pin.

use std::sync::Arc;

use solarxy_core::RawImageData;
use solarxy_core::geometry::{MeshTopology, RawMaterialData, linear_to_srgb};

use crate::FormatsError;

/// One mesh to write, borrowed from whatever cooked representation the
/// caller holds (the graph's `KernelMesh` maps field-for-field).
pub struct ExportMesh<'a> {
    pub name: &'a str,
    pub positions: &'a [[f32; 3]],
    pub normals: Option<&'a [[f32; 3]]>,
    pub tex_coords: Option<&'a [[f32; 2]]>,
    /// Primitive indices, read per `topology`: triples, segment pairs, or
    /// ignored for point clouds.
    pub indices: &'a [u32],
    pub topology: MeshTopology,
    /// Per-vertex linear RGBA colors (position-count when present).
    pub colors: Option<&'a [[f32; 4]]>,
    /// Index into the caller's material table (the `materials` slice the
    /// material-aware writers take).
    pub material_index: Option<usize>,
}

/// Wavefront OBJ: one `o` block per mesh, shared v/vt/vn numbering.
/// Geometry only; the material-carrying form is [`write_obj_mtl_bytes`].
#[must_use]
pub fn write_obj_bytes(meshes: &[ExportMesh<'_>]) -> Vec<u8> {
    obj_text(meshes, None).into_bytes()
}

/// The `.obj` text, optionally referencing an MTL library:
/// `(mtl filename, sanitized material name per material index)`.
fn obj_text(meshes: &[ExportMesh<'_>], mtl: Option<(&str, &[String])>) -> String {
    use std::fmt::Write as _;
    let mut out = String::from("# Exported by Solarxy\n");
    if let Some((mtl_file, _)) = mtl {
        let _ = writeln!(out, "mtllib {mtl_file}");
    }
    // OBJ indices are 1-based and global across the file.
    let (mut v_base, mut vt_base, mut vn_base) = (1u32, 1u32, 1u32);
    for (i, mesh) in meshes.iter().enumerate() {
        let name = if mesh.name.is_empty() {
            format!("mesh_{i}")
        } else {
            mesh.name.replace(char::is_whitespace, "_")
        };
        let _ = writeln!(out, "o {name}");
        if let Some((_, names)) = mtl
            && let Some(material) = mesh.material_index.and_then(|m| names.get(m))
        {
            let _ = writeln!(out, "usemtl {material}");
        }
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
        let f = |i: u32| {
            let v = v_base + i;
            match (mesh.tex_coords.is_some(), mesh.normals.is_some()) {
                (true, true) => format!("{v}/{}/{}", vt_base + i, vn_base + i),
                (true, false) => format!("{v}/{}", vt_base + i),
                (false, true) => format!("{v}//{}", vn_base + i),
                (false, false) => format!("{v}"),
            }
        };
        match mesh.topology {
            MeshTopology::Triangles => {
                for tri in mesh.indices.chunks_exact(3) {
                    let _ = writeln!(out, "f {} {} {}", f(tri[0]), f(tri[1]), f(tri[2]));
                }
            }
            MeshTopology::Lines => {
                for pair in mesh.indices.chunks_exact(2) {
                    let _ = writeln!(out, "l {} {}", v_base + pair[0], v_base + pair[1]);
                }
            }
            MeshTopology::Points => {
                for i in 0..mesh.positions.len() as u32 {
                    let _ = writeln!(out, "p {}", v_base + i);
                }
            }
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
    out
}

/// A multi-file OBJ export: the `.obj`, its `.mtl` sidecar, and the
/// referenced texture PNGs, each named as referenced from the MTL. The
/// caller owns delivery (the web shell packs the trio into a zip).
pub struct ObjMtlExport {
    pub obj: Vec<u8>,
    pub mtl: Vec<u8>,
    /// `(filename, PNG bytes)`, deduplicated by image content hash.
    pub textures: Vec<(String, Vec<u8>)>,
}

/// Wavefront OBJ with its MTL sidecar. The `.obj` references
/// `<base_name>.mtl` and tags each mesh with `usemtl`; the MTL carries
/// the classic illumination set (Ka/Kd/Ks/Ns/d/Ni, reversing the
/// importer's Kd folding into the base-color factor) plus the PBR
/// scalars the importer reads back (`Pr` roughness, `Pm` metallic) and
/// `Ke` when emissive. Diffuse and normal textures emit as PNGs
/// referenced by `map_Kd`/`map_Bump`, the two roles MTL can express and
/// the loader round-trips; the remaining texture roles are GLB-only.
pub fn write_obj_mtl_bytes(
    meshes: &[ExportMesh<'_>],
    materials: &[Arc<RawMaterialData>],
    base_name: &str,
) -> Result<ObjMtlExport, FormatsError> {
    use std::fmt::Write as _;

    // Unique whitespace-free MTL names, one per material table entry.
    let mut names: Vec<String> = Vec::with_capacity(materials.len());
    for (i, mat) in materials.iter().enumerate() {
        let mut name = mat.name.replace(char::is_whitespace, "_");
        if name.is_empty() {
            name = format!("material_{i}");
        }
        while names.contains(&name) {
            name.push('_');
        }
        names.push(name);
    }

    // Texture files, deduplicated by content hash; the first role to
    // reference an image names it.
    let mut textures: Vec<(String, Vec<u8>)> = Vec::new();
    let mut name_by_hash: Vec<(u64, String)> = Vec::new();
    let mut texture_file =
        |img: &Arc<RawImageData>, mat: &str, role: &str| -> Result<String, FormatsError> {
            if let Some((_, name)) = name_by_hash.iter().find(|(h, _)| *h == img.hash) {
                return Ok(name.clone());
            }
            let file = format!("{mat}_{role}.png");
            textures.push((file.clone(), encode_png_bytes(img)?));
            name_by_hash.push((img.hash, file.clone()));
            Ok(file)
        };

    let mut mtl = String::from("# Exported by Solarxy\n");
    for (mat, name) in materials.iter().zip(&names) {
        let _ = writeln!(mtl, "newmtl {name}");
        if let Some([r, g, b]) = mat.ambient {
            let _ = writeln!(mtl, "Ka {r} {g} {b}");
        }
        // Kd: the stored MTL diffuse when the material came from an OBJ,
        // else the base-color factor it was folded into.
        let kd = mat.diffuse.unwrap_or([
            mat.base_color_factor[0],
            mat.base_color_factor[1],
            mat.base_color_factor[2],
        ]);
        let _ = writeln!(mtl, "Kd {} {} {}", kd[0], kd[1], kd[2]);
        if let Some([r, g, b]) = mat.specular {
            let _ = writeln!(mtl, "Ks {r} {g} {b}");
        }
        if let Some(ns) = mat.shininess {
            let _ = writeln!(mtl, "Ns {ns}");
        }
        if let Some(d) = mat.dissolve {
            let _ = writeln!(mtl, "d {d}");
        }
        if let Some(ni) = mat.optical_density {
            let _ = writeln!(mtl, "Ni {ni}");
        }
        let _ = writeln!(mtl, "Pr {}", mat.roughness_factor);
        let _ = writeln!(mtl, "Pm {}", mat.metallic_factor);
        if mat.emissive_factor != [0.0, 0.0, 0.0] {
            let e = mat.emissive_factor;
            let _ = writeln!(mtl, "Ke {} {} {}", e[0], e[1], e[2]);
        }
        if let Some(img) = mat.diffuse_texture_data.as_ref() {
            let file = texture_file(img, name, "diffuse")?;
            let _ = writeln!(mtl, "map_Kd {file}");
        }
        if let Some(img) = mat.normal_texture_data.as_ref() {
            let file = texture_file(img, name, "normal")?;
            let _ = writeln!(mtl, "map_Bump {file}");
        }
        mtl.push('\n');
    }

    let mtl_file = format!("{base_name}.mtl");
    let obj = obj_text(meshes, Some((mtl_file.as_str(), &names)));
    Ok(ObjMtlExport {
        obj: obj.into_bytes(),
        mtl: mtl.into_bytes(),
        textures,
    })
}

/// Binary STL: every mesh's triangles concatenated (STL has no objects,
/// no normals-per-vertex, no UVs; facet normals are recomputed). STL is
/// facets-only by definition, so point and line meshes are skipped
/// entirely (the export node documents this).
pub fn write_stl_bytes(meshes: &[ExportMesh<'_>]) -> Result<Vec<u8>, FormatsError> {
    let mut tris = Vec::new();
    for mesh in meshes {
        if mesh.topology != MeshTopology::Triangles {
            continue;
        }
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
/// object concept). Normals, UVs, and colors export when EVERY mesh
/// carries them; colors write as sRGB-encoded uchar properties (the
/// loader's inverse, so an import/export pair round-trips up to 8-bit
/// quantization). Faces come from triangle meshes only: a topology mix
/// exports every vertex but only triangle faces, and an all-point export
/// is a face-less point-cloud PLY (`element face 0`), exactly the form
/// the loader reads back as Points topology. PLY has no standard edge
/// element, so line segments do not survive (their vertices do).
#[must_use]
pub fn write_ply_bytes(meshes: &[ExportMesh<'_>]) -> Vec<u8> {
    use std::fmt::Write as _;
    let total_verts: usize = meshes.iter().map(|m| m.positions.len()).sum();
    let total_faces: usize = meshes
        .iter()
        .filter(|m| m.topology == MeshTopology::Triangles)
        .map(|m| m.indices.len() / 3)
        .sum();
    let with_normals = !meshes.is_empty() && meshes.iter().all(|m| m.normals.is_some());
    let with_uvs = !meshes.is_empty() && meshes.iter().all(|m| m.tex_coords.is_some());
    let with_colors = !meshes.is_empty() && meshes.iter().all(|m| m.colors.is_some());

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
    if with_colors {
        out.push_str(
            "property uchar red\nproperty uchar green\nproperty uchar blue\nproperty uchar alpha\n",
        );
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
            if with_colors {
                let c = mesh.colors.unwrap()[i];
                let byte = |v: f32| (linear_to_srgb(v) * 255.0).round() as u8;
                // Alpha is coverage, not color: no transfer curve.
                let alpha = (c[3].clamp(0.0, 1.0) * 255.0).round() as u8;
                let _ = write!(
                    out,
                    " {} {} {} {}",
                    byte(c[0]),
                    byte(c[1]),
                    byte(c[2]),
                    alpha
                );
            }
            out.push('\n');
        }
    }
    let mut base = 0u32;
    for mesh in meshes {
        if mesh.topology == MeshTopology::Triangles {
            for tri in mesh.indices.chunks_exact(3) {
                let _ = writeln!(
                    out,
                    "3 {} {} {}",
                    base + tri[0],
                    base + tri[1],
                    base + tri[2]
                );
            }
        }
        base += mesh.positions.len() as u32;
    }
    out.into_bytes()
}

/// Appends `bytes` to the GLB BIN chunk 4-byte-aligned per the spec and
/// records a bufferView (with `target` for vertex/index data; image views
/// carry none).
fn push_view(
    bin: &mut Vec<u8>,
    buffer_views: &mut Vec<serde_json::Value>,
    bytes: &[u8],
    target: Option<u32>,
) -> usize {
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let offset = bin.len();
    bin.extend_from_slice(bytes);
    let mut view = serde_json::json!({
        "buffer": 0,
        "byteOffset": offset,
        "byteLength": bytes.len(),
    });
    if let (Some(target), Some(obj)) = (target, view.as_object_mut()) {
        obj.insert("target".into(), serde_json::json!(target));
    }
    buffer_views.push(view);
    buffer_views.len() - 1
}

/// The GLB texture table: every distinct image (by content hash) across
/// every material role becomes one PNG in the BIN chunk, one image entry,
/// and one texture entry.
struct TextureTable {
    index_by_hash: Vec<(u64, usize)>,
    images: Vec<serde_json::Value>,
    textures: Vec<serde_json::Value>,
}

impl TextureTable {
    fn texture_for(
        &mut self,
        bin: &mut Vec<u8>,
        buffer_views: &mut Vec<serde_json::Value>,
        img: Option<&Arc<RawImageData>>,
    ) -> Result<Option<usize>, FormatsError> {
        let Some(img) = img else { return Ok(None) };
        if let Some(&(_, index)) = self.index_by_hash.iter().find(|(h, _)| *h == img.hash) {
            return Ok(Some(index));
        }
        let png = encode_png_bytes(img)?;
        let view = push_view(bin, buffer_views, &png, None);
        self.images.push(serde_json::json!({
            "bufferView": view,
            "mimeType": "image/png",
        }));
        self.textures
            .push(serde_json::json!({ "source": self.images.len() - 1 }));
        let index = self.textures.len() - 1;
        self.index_by_hash.push((img.hash, index));
        Ok(Some(index))
    }
}

/// Binary glTF (GLB): one buffer, interleaved-free accessors, one node
/// per mesh. Hand-built JSON + BIN container; the format is small enough
/// that the typed builder buys nothing.
///
/// Materials export as pbrMetallicRoughness with every factor and texture
/// role `RawMaterialData` carries (base color, metallic-roughness, normal,
/// occlusion with strength, emissive with factor), alphaMode/alphaCutoff,
/// and the `KHR_materials_unlit` extension for the Unlit shading model,
/// mirroring what the importer reads. Textures embed in the BIN chunk as
/// PNG, deduplicated by image content hash, so five roles sharing one
/// image store it once. Vertex colors export as `COLOR_0` (linear float,
/// the glTF convention). Point and line meshes use the matching primitive
/// modes; point clouds are non-indexed.
pub fn write_glb_bytes(
    meshes: &[ExportMesh<'_>],
    materials: &[Arc<RawMaterialData>],
) -> Result<Vec<u8>, FormatsError> {
    let mut bin: Vec<u8> = Vec::new();
    let mut buffer_views = Vec::new();
    let mut accessors = Vec::new();
    let mut json_meshes = Vec::new();
    let mut nodes = Vec::new();
    let mut table = TextureTable {
        index_by_hash: Vec::new(),
        images: Vec::new(),
        textures: Vec::new(),
    };

    let mut any_unlit = false;
    let mut json_materials = Vec::new();
    for mat in materials {
        let mut pbr = serde_json::Map::new();
        pbr.insert(
            "baseColorFactor".into(),
            serde_json::json!(mat.base_color_factor),
        );
        pbr.insert(
            "metallicFactor".into(),
            serde_json::json!(mat.metallic_factor),
        );
        pbr.insert(
            "roughnessFactor".into(),
            serde_json::json!(mat.roughness_factor),
        );
        if let Some(t) = table.texture_for(
            &mut bin,
            &mut buffer_views,
            mat.diffuse_texture_data.as_ref(),
        )? {
            pbr.insert("baseColorTexture".into(), serde_json::json!({ "index": t }));
        }
        if let Some(t) = table.texture_for(
            &mut bin,
            &mut buffer_views,
            mat.metallic_roughness_texture_data.as_ref(),
        )? {
            pbr.insert(
                "metallicRoughnessTexture".into(),
                serde_json::json!({ "index": t }),
            );
        }

        let mut entry = serde_json::Map::new();
        entry.insert("name".into(), serde_json::json!(mat.name));
        entry.insert(
            "pbrMetallicRoughness".into(),
            serde_json::Value::Object(pbr),
        );
        if let Some(t) = table.texture_for(
            &mut bin,
            &mut buffer_views,
            mat.normal_texture_data.as_ref(),
        )? {
            entry.insert("normalTexture".into(), serde_json::json!({ "index": t }));
        }
        if let Some(t) = table.texture_for(
            &mut bin,
            &mut buffer_views,
            mat.occlusion_texture_data.as_ref(),
        )? {
            entry.insert(
                "occlusionTexture".into(),
                serde_json::json!({ "index": t, "strength": mat.occlusion_strength }),
            );
        }
        if let Some(t) = table.texture_for(
            &mut bin,
            &mut buffer_views,
            mat.emissive_texture_data.as_ref(),
        )? {
            entry.insert("emissiveTexture".into(), serde_json::json!({ "index": t }));
        }
        entry.insert(
            "emissiveFactor".into(),
            serde_json::json!(mat.emissive_factor),
        );
        match mat.alpha_mode {
            solarxy_core::geometry::AlphaMode::Opaque => {}
            solarxy_core::geometry::AlphaMode::Mask => {
                entry.insert("alphaMode".into(), serde_json::json!("MASK"));
                entry.insert("alphaCutoff".into(), serde_json::json!(mat.alpha_cutoff));
            }
            solarxy_core::geometry::AlphaMode::Blend => {
                entry.insert("alphaMode".into(), serde_json::json!("BLEND"));
            }
        }
        if mat.shading_model == solarxy_core::geometry::ShadingModel::Unlit {
            any_unlit = true;
            entry.insert(
                "extensions".into(),
                serde_json::json!({ "KHR_materials_unlit": {} }),
            );
        }
        json_materials.push(serde_json::Value::Object(entry));
    }

    for (i, mesh) in meshes.iter().enumerate() {
        let mut attributes = serde_json::Map::new();

        let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in mesh.positions {
            for c in 0..3 {
                min[c] = min[c].min(p[c]);
                max[c] = max[c].max(p[c]);
            }
        }
        let view = push_view(
            &mut bin,
            &mut buffer_views,
            bytemuck_cast(mesh.positions),
            Some(34962),
        );
        accessors.push(serde_json::json!({
            "bufferView": view, "componentType": 5126, "count": mesh.positions.len(),
            "type": "VEC3", "min": min, "max": max,
        }));
        attributes.insert("POSITION".into(), serde_json::json!(accessors.len() - 1));

        if let Some(normals) = mesh.normals {
            let view = push_view(
                &mut bin,
                &mut buffer_views,
                bytemuck_cast(normals),
                Some(34962),
            );
            accessors.push(serde_json::json!({
                "bufferView": view, "componentType": 5126, "count": normals.len(),
                "type": "VEC3",
            }));
            attributes.insert("NORMAL".into(), serde_json::json!(accessors.len() - 1));
        }
        if let Some(uvs) = mesh.tex_coords {
            let view = push_view(
                &mut bin,
                &mut buffer_views,
                bytemuck_cast2(uvs),
                Some(34962),
            );
            accessors.push(serde_json::json!({
                "bufferView": view, "componentType": 5126, "count": uvs.len(),
                "type": "VEC2",
            }));
            attributes.insert("TEXCOORD_0".into(), serde_json::json!(accessors.len() - 1));
        }
        if let Some(colors) = mesh.colors {
            let view = push_view(
                &mut bin,
                &mut buffer_views,
                bytemuck_cast4(colors),
                Some(34962),
            );
            accessors.push(serde_json::json!({
                "bufferView": view, "componentType": 5126, "count": colors.len(),
                "type": "VEC4",
            }));
            attributes.insert("COLOR_0".into(), serde_json::json!(accessors.len() - 1));
        }

        let mut primitive = serde_json::Map::new();
        primitive.insert("attributes".into(), serde_json::Value::Object(attributes));
        let mode = match mesh.topology {
            MeshTopology::Triangles => 4,
            MeshTopology::Lines => 1,
            MeshTopology::Points => 0,
        };
        primitive.insert("mode".into(), serde_json::json!(mode));
        // Point clouds are non-indexed: the count comes from POSITION.
        if mesh.topology != MeshTopology::Points {
            let index_bytes: Vec<u8> = mesh.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
            let view = push_view(&mut bin, &mut buffer_views, &index_bytes, Some(34963));
            accessors.push(serde_json::json!({
                "bufferView": view, "componentType": 5125, "count": mesh.indices.len(),
                "type": "SCALAR",
            }));
            primitive.insert("indices".into(), serde_json::json!(accessors.len() - 1));
        }
        if let Some(material) = mesh.material_index.filter(|&m| m < materials.len()) {
            primitive.insert("material".into(), serde_json::json!(material));
        }

        json_meshes.push(serde_json::json!({
            "name": if mesh.name.is_empty() { format!("mesh_{i}") } else { mesh.name.to_string() },
            "primitives": [serde_json::Value::Object(primitive)],
        }));
        nodes.push(serde_json::json!({ "mesh": i }));
    }

    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let mut json = serde_json::json!({
        "asset": { "version": "2.0", "generator": "Solarxy" },
        "scene": 0,
        "scenes": [{ "nodes": (0..nodes.len()).collect::<Vec<_>>() }],
        "nodes": nodes,
        "meshes": json_meshes,
        "buffers": [{ "byteLength": bin.len() }],
        "bufferViews": buffer_views,
        "accessors": accessors,
    });
    if let Some(root) = json.as_object_mut() {
        if !json_materials.is_empty() {
            root.insert("materials".into(), serde_json::Value::Array(json_materials));
        }
        if !table.images.is_empty() {
            root.insert("images".into(), serde_json::Value::Array(table.images));
            root.insert("textures".into(), serde_json::Value::Array(table.textures));
        }
        if any_unlit {
            root.insert(
                "extensionsUsed".into(),
                serde_json::json!(["KHR_materials_unlit"]),
            );
        }
    }
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

fn bytemuck_cast4(v: &[[f32; 4]]) -> &[u8] {
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
            topology: MeshTopology::Triangles,
            colors: None,
            material_index: None,
        }
    }

    fn points_mesh<'a>(
        positions: &'a [[f32; 3]],
        colors: Option<&'a [[f32; 4]]>,
    ) -> ExportMesh<'a> {
        ExportMesh {
            name: "cloud",
            positions,
            normals: None,
            tex_coords: None,
            indices: &[],
            topology: MeshTopology::Points,
            colors,
            material_index: None,
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
    fn ply_vertex_colors_round_trip_within_quantization() {
        let d = quad();
        let colors = vec![
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0, 0.5],
            [0.2, 0.4, 0.6, 1.0],
        ];
        let mut mesh = export_quad(&d);
        mesh.colors = Some(&colors);
        let bytes = write_ply_bytes(&[mesh]);
        let model = crate::ply::load_ply_bytes(&bytes, "quad.ply").expect("reimport");
        let back = model.meshes[0].colors.as_ref().expect("colors read back");
        // linear -> sRGB u8 -> linear: within one 8-bit step of the source.
        for (got, want) in back.iter().zip(&colors) {
            for c in 0..4 {
                assert!(
                    (got[c] - want[c]).abs() < 0.01,
                    "channel {c}: {got:?} vs {want:?}"
                );
            }
        }
    }

    #[test]
    fn ply_point_cloud_exports_faceless_and_reimports_as_points() {
        use solarxy_core::geometry::MeshTopology;
        let positions = vec![[0.0, 0.0, 0.0], [1.0, 2.0, 3.0], [4.0, 5.0, 6.0]];
        let bytes = write_ply_bytes(&[points_mesh(&positions, None)]);
        let model = crate::ply::load_ply_bytes(&bytes, "cloud.ply").expect("reimport");
        assert_eq!(model.meshes[0].topology, MeshTopology::Points);
        assert_eq!(model.meshes[0].positions.len(), 3);
        assert!(model.meshes[0].indices.is_empty());
    }

    #[test]
    fn stl_skips_non_triangle_meshes() {
        let positions = vec![[0.0; 3]; 4];
        let bytes = write_stl_bytes(&[points_mesh(&positions, None)]).expect("write");
        // Binary STL: 80-byte header, then a u32 triangle count.
        let count = u32::from_le_bytes(bytes[80..84].try_into().unwrap());
        assert_eq!(count, 0, "no facets from a point cloud");
    }

    #[test]
    fn obj_mtl_round_trips_through_our_loader() {
        use std::collections::HashMap;

        struct MapAssets(HashMap<String, Vec<u8>>);
        impl crate::AssetResolver for MapAssets {
            fn read(&mut self, rel_path: &str) -> Option<Vec<u8>> {
                self.0.get(rel_path).cloned()
            }
        }

        let texture = Arc::new(RawImageData::new(
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 40, 40, 40, 255,
            ],
            2,
            2,
        ));
        let material = Arc::new(RawMaterialData {
            name: "painted wood".to_string(),
            base_color_factor: [0.8, 0.2, 0.1, 1.0],
            roughness_factor: 0.7,
            metallic_factor: 0.3,
            diffuse_texture_data: Some(Arc::clone(&texture)),
            ..Default::default()
        });
        let d = quad();
        let mut mesh = export_quad(&d);
        mesh.material_index = Some(0);
        let export = write_obj_mtl_bytes(&[mesh], &[material], "asset").expect("write");

        let obj_text = String::from_utf8(export.obj.clone()).unwrap();
        assert!(obj_text.contains("mtllib asset.mtl"), "{obj_text}");
        assert!(obj_text.contains("usemtl painted_wood"), "{obj_text}");

        let mut assets = MapAssets(
            std::iter::once(("asset.mtl".to_string(), export.mtl.clone()))
                .chain(export.textures.iter().cloned())
                .collect(),
        );
        let model = crate::obj::load_obj_bytes(&export.obj, &mut assets).expect("reimport");
        let mat = &model.materials[0];
        assert_eq!(mat.name, "painted_wood");
        // Kd folds back into the base-color factor on import.
        for (got, want) in mat.base_color_factor.iter().zip([0.8, 0.2, 0.1, 1.0]) {
            assert!((got - want).abs() < 1e-5, "{:?}", mat.base_color_factor);
        }
        assert!((mat.roughness_factor - 0.7).abs() < 1e-6, "Pr survives");
        assert!((mat.metallic_factor - 0.3).abs() < 1e-6, "Pm survives");
        let diffuse = mat.diffuse_texture_data.as_ref().expect("texture resolved");
        assert_eq!(diffuse.hash, texture.hash, "pixels survive the PNG trip");
    }

    #[test]
    fn obj_writes_point_and_line_records() {
        use solarxy_core::geometry::MeshTopology;
        let line_positions = vec![[0.0; 3], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0]];
        let line_indices = vec![0u32, 1, 1, 2];
        let line = ExportMesh {
            name: "wire",
            positions: &line_positions,
            normals: None,
            tex_coords: None,
            indices: &line_indices,
            topology: MeshTopology::Lines,
            colors: None,
            material_index: None,
        };
        let cloud_positions = vec![[5.0, 0.0, 0.0], [6.0, 0.0, 0.0]];
        let text = String::from_utf8(write_obj_bytes(&[
            line,
            points_mesh(&cloud_positions, None),
        ]))
        .unwrap();
        assert!(
            text.contains("\nl 1 2\n") && text.contains("\nl 2 3\n"),
            "{text}"
        );
        assert!(
            text.contains("\np 4\n") && text.contains("\np 5\n"),
            "{text}"
        );
        assert!(!text.contains("\nf "), "no fabricated faces");
    }

    #[test]
    fn glb_round_trips_through_our_loader() {
        let d = quad();
        let bytes = write_glb_bytes(&[export_quad(&d)], &[]).expect("write");
        let model = crate::gltf::load_gltf_bytes(&bytes, &mut crate::NoAssets).expect("reimport");
        let mesh = &model.meshes[0];
        assert_eq!(mesh.positions.len(), 4);
        assert_eq!(mesh.indices.len(), 6);
        assert!(mesh.normals.is_some());
        assert!(mesh.tex_coords.is_some());
    }

    #[test]
    fn glb_materials_round_trip_with_deduplicated_textures() {
        use solarxy_core::geometry::{AlphaMode, ShadingModel};

        let texture = Arc::new(RawImageData::new(
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ],
            2,
            2,
        ));
        // The same image serves two roles: the exporter must embed it ONCE.
        let material = Arc::new(RawMaterialData {
            name: "painted".to_string(),
            base_color_factor: [0.9, 0.5, 0.25, 1.0],
            metallic_factor: 0.3,
            roughness_factor: 0.7,
            occlusion_strength: 0.8,
            emissive_factor: [0.1, 0.2, 0.3],
            alpha_mode: AlphaMode::Mask,
            alpha_cutoff: 0.4,
            diffuse_texture_data: Some(Arc::clone(&texture)),
            occlusion_texture_data: Some(Arc::clone(&texture)),
            ..Default::default()
        });
        let d = quad();
        let mut mesh = export_quad(&d);
        mesh.material_index = Some(0);
        let bytes = write_glb_bytes(&[mesh], &[material]).expect("write");
        let model = crate::gltf::load_gltf_bytes(&bytes, &mut crate::NoAssets).expect("reimport");

        assert_eq!(model.meshes[0].material_index, Some(0));
        let mat = &model.materials[0];
        assert_eq!(mat.name, "painted");
        assert_eq!(mat.base_color_factor, [0.9, 0.5, 0.25, 1.0]);
        assert!((mat.metallic_factor - 0.3).abs() < 1e-6);
        assert!((mat.roughness_factor - 0.7).abs() < 1e-6);
        assert!((mat.occlusion_strength - 0.8).abs() < 1e-6);
        assert_eq!(mat.emissive_factor, [0.1, 0.2, 0.3]);
        assert_eq!(mat.alpha_mode, AlphaMode::Mask);
        assert!((mat.alpha_cutoff - 0.4).abs() < 1e-6);
        assert_eq!(mat.shading_model, ShadingModel::Pbr);

        let diffuse = mat.diffuse_texture_data.as_ref().expect("diffuse texture");
        assert_eq!((diffuse.width, diffuse.height), (2, 2));
        assert_eq!(diffuse.hash, texture.hash, "pixels survive PNG round trip");
        let occlusion = mat
            .occlusion_texture_data
            .as_ref()
            .expect("occlusion texture");
        assert_eq!(occlusion.hash, texture.hash);

        // The dedup is observable in the container: exactly one image
        // JSON entry despite two texture roles.
        let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let json: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_len]).unwrap();
        assert_eq!(json["images"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn glb_unlit_extension_round_trips() {
        use solarxy_core::geometry::ShadingModel;
        let material = Arc::new(RawMaterialData {
            name: "flat".to_string(),
            shading_model: ShadingModel::Unlit,
            ..Default::default()
        });
        let d = quad();
        let mut mesh = export_quad(&d);
        mesh.material_index = Some(0);
        let bytes = write_glb_bytes(&[mesh], &[material]).expect("write");
        let model = crate::gltf::load_gltf_bytes(&bytes, &mut crate::NoAssets).expect("reimport");
        assert_eq!(model.materials[0].shading_model, ShadingModel::Unlit);
    }

    #[test]
    fn glb_vertex_colors_round_trip_exactly() {
        let d = quad();
        let colors = vec![
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0, 1.0],
            [0.25, 0.5, 0.75, 1.0],
        ];
        let mut mesh = export_quad(&d);
        mesh.colors = Some(&colors);
        let bytes = write_glb_bytes(&[mesh], &[]).expect("write");
        let model = crate::gltf::load_gltf_bytes(&bytes, &mut crate::NoAssets).expect("reimport");
        // COLOR_0 is linear float both ways: exact.
        assert_eq!(model.meshes[0].colors.as_ref().unwrap(), &colors);
    }

    #[test]
    fn glb_point_and_line_modes_round_trip() {
        use solarxy_core::geometry::MeshTopology;
        let cloud_positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let line_positions = vec![[0.0; 3], [0.0, 2.0, 0.0]];
        let line_indices = vec![0u32, 1];
        let line = ExportMesh {
            name: "wire",
            positions: &line_positions,
            normals: None,
            tex_coords: None,
            indices: &line_indices,
            topology: MeshTopology::Lines,
            colors: None,
            material_index: None,
        };
        let bytes =
            write_glb_bytes(&[points_mesh(&cloud_positions, None), line], &[]).expect("write");
        let model = crate::gltf::load_gltf_bytes(&bytes, &mut crate::NoAssets).expect("reimport");
        assert_eq!(model.meshes.len(), 2);
        assert_eq!(model.meshes[0].topology, MeshTopology::Points);
        assert_eq!(model.meshes[0].positions.len(), 3);
        assert!(model.meshes[0].indices.is_empty());
        assert_eq!(model.meshes[1].topology, MeshTopology::Lines);
        assert_eq!(model.meshes[1].indices, vec![0, 1]);
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
