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
use solarxy_core::geometry::{MeshTopology, RawImageHdr, RawMaterialData, linear_to_srgb};

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
                for tri in mesh.indices.as_chunks::<3>().0 {
                    let _ = writeln!(out, "f {} {} {}", f(tri[0]), f(tri[1]), f(tri[2]));
                }
            }
            MeshTopology::Lines => {
                for pair in mesh.indices.as_chunks::<2>().0 {
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
        // MTL's Ni is an index of refraction, and two fields now describe
        // one. The principled `ior` wins when it has been moved off its
        // default, because that is a deliberate authoring act; otherwise the
        // legacy MTL value round-trips as it always has.
        let ni = if (mat.ior - 1.5).abs() > f32::EPSILON {
            Some(mat.ior)
        } else {
            mat.optical_density
        };
        if let Some(ni) = ni {
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
        for tri in mesh.indices.as_chunks::<3>().0 {
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
            for tri in mesh.indices.as_chunks::<3>().0 {
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

/// Whether a colour has been moved off the extension's default.
///
/// The question is exact equality, but comparing float arrays that way is a
/// lint, and at these magnitudes an epsilon answers the same question.
fn rgb_differs(value: [f32; 3], default: [f32; 3]) -> bool {
    value
        .iter()
        .zip(default)
        .any(|(a, b)| (a - b).abs() > f32::EPSILON)
}

/// Assemble the `extensions` object for one material.
///
/// Registers whatever textures the extensions reference, and records each
/// extension name it wrote into `used`, which becomes `extensionsUsed` at
/// the root. Every extension here is optional-fallback: a reader that
/// ignores all of them still gets a valid metallic-roughness material, so
/// `extensionsRequired` is deliberately never written.
///
/// A factor is written only when it differs from the extension's own
/// default, and an extension block is written only when it carries
/// something, so a plain material exports byte-identically to before.
fn material_extensions(
    mat: &RawMaterialData,
    table: &mut TextureTable,
    bin: &mut Vec<u8>,
    buffer_views: &mut Vec<serde_json::Value>,
    used: &mut std::collections::BTreeSet<&'static str>,
) -> Result<serde_json::Map<String, serde_json::Value>, FormatsError> {
    let mut extensions = serde_json::Map::new();

    // Registers a texture and inserts `{"index": n}` under `key`.
    macro_rules! tex {
        ($block:expr, $key:literal, $data:expr) => {
            if let Some(t) = table.texture_for(bin, buffer_views, $data.as_ref())? {
                $block.insert($key.into(), serde_json::json!({ "index": t }));
            }
        };
    }

    if mat.shading_model == solarxy_core::geometry::ShadingModel::Unlit {
        used.insert("KHR_materials_unlit");
        extensions.insert("KHR_materials_unlit".into(), serde_json::json!({}));
    }

    if (mat.ior - 1.5).abs() > f32::EPSILON {
        used.insert("KHR_materials_ior");
        extensions.insert(
            "KHR_materials_ior".into(),
            serde_json::json!({ "ior": mat.ior }),
        );
    }

    if (mat.emissive_strength - 1.0).abs() > f32::EPSILON {
        used.insert("KHR_materials_emissive_strength");
        extensions.insert(
            "KHR_materials_emissive_strength".into(),
            serde_json::json!({ "emissiveStrength": mat.emissive_strength }),
        );
    }

    let mut transmission = serde_json::Map::new();
    if mat.transmission != 0.0 {
        transmission.insert(
            "transmissionFactor".into(),
            serde_json::json!(mat.transmission),
        );
    }
    tex!(
        transmission,
        "transmissionTexture",
        mat.transmission_texture_data
    );
    if !transmission.is_empty() {
        used.insert("KHR_materials_transmission");
        extensions.insert(
            "KHR_materials_transmission".into(),
            serde_json::Value::Object(transmission),
        );
    }

    let mut volume = serde_json::Map::new();
    if mat.thickness != 0.0 {
        volume.insert("thicknessFactor".into(), serde_json::json!(mat.thickness));
    }
    if rgb_differs(mat.attenuation_color, [1.0, 1.0, 1.0]) {
        volume.insert(
            "attenuationColor".into(),
            serde_json::json!(mat.attenuation_color),
        );
    }
    // Zero is this type's "no attenuation"; the specification expresses the
    // same thing by omitting the key, whose default is infinite.
    if mat.attenuation_distance > 0.0 {
        volume.insert(
            "attenuationDistance".into(),
            serde_json::json!(mat.attenuation_distance),
        );
    }
    tex!(volume, "thicknessTexture", mat.thickness_texture_data);
    if !volume.is_empty() {
        used.insert("KHR_materials_volume");
        extensions.insert(
            "KHR_materials_volume".into(),
            serde_json::Value::Object(volume),
        );
    }

    let mut specular = serde_json::Map::new();
    if (mat.specular_intensity - 1.0).abs() > f32::EPSILON {
        specular.insert(
            "specularFactor".into(),
            serde_json::json!(mat.specular_intensity),
        );
    }
    if rgb_differs(mat.specular_color, [1.0, 1.0, 1.0]) {
        specular.insert(
            "specularColorFactor".into(),
            serde_json::json!(mat.specular_color),
        );
    }
    tex!(specular, "specularTexture", mat.specular_texture_data);
    tex!(
        specular,
        "specularColorTexture",
        mat.specular_color_texture_data
    );
    if !specular.is_empty() {
        used.insert("KHR_materials_specular");
        extensions.insert(
            "KHR_materials_specular".into(),
            serde_json::Value::Object(specular),
        );
    }

    let mut clearcoat = serde_json::Map::new();
    if mat.clearcoat != 0.0 {
        clearcoat.insert("clearcoatFactor".into(), serde_json::json!(mat.clearcoat));
    }
    if mat.clearcoat_roughness != 0.0 {
        clearcoat.insert(
            "clearcoatRoughnessFactor".into(),
            serde_json::json!(mat.clearcoat_roughness),
        );
    }
    tex!(clearcoat, "clearcoatTexture", mat.clearcoat_texture_data);
    tex!(
        clearcoat,
        "clearcoatRoughnessTexture",
        mat.clearcoat_roughness_texture_data
    );
    tex!(
        clearcoat,
        "clearcoatNormalTexture",
        mat.clearcoat_normal_texture_data
    );
    if !clearcoat.is_empty() {
        used.insert("KHR_materials_clearcoat");
        extensions.insert(
            "KHR_materials_clearcoat".into(),
            serde_json::Value::Object(clearcoat),
        );
    }

    let mut sheen = serde_json::Map::new();
    if rgb_differs(mat.sheen_color, [0.0, 0.0, 0.0]) {
        sheen.insert(
            "sheenColorFactor".into(),
            serde_json::json!(mat.sheen_color),
        );
    }
    if mat.sheen_roughness != 0.0 {
        sheen.insert(
            "sheenRoughnessFactor".into(),
            serde_json::json!(mat.sheen_roughness),
        );
    }
    tex!(sheen, "sheenColorTexture", mat.sheen_color_texture_data);
    tex!(
        sheen,
        "sheenRoughnessTexture",
        mat.sheen_roughness_texture_data
    );
    if !sheen.is_empty() {
        used.insert("KHR_materials_sheen");
        extensions.insert(
            "KHR_materials_sheen".into(),
            serde_json::Value::Object(sheen),
        );
    }

    let mut iridescence = serde_json::Map::new();
    if mat.iridescence != 0.0 {
        iridescence.insert(
            "iridescenceFactor".into(),
            serde_json::json!(mat.iridescence),
        );
    }
    if (mat.iridescence_ior - 1.3).abs() > f32::EPSILON {
        iridescence.insert(
            "iridescenceIor".into(),
            serde_json::json!(mat.iridescence_ior),
        );
    }
    if (mat.iridescence_thickness_min - 100.0).abs() > f32::EPSILON {
        iridescence.insert(
            "iridescenceThicknessMinimum".into(),
            serde_json::json!(mat.iridescence_thickness_min),
        );
    }
    if (mat.iridescence_thickness_max - 400.0).abs() > f32::EPSILON {
        iridescence.insert(
            "iridescenceThicknessMaximum".into(),
            serde_json::json!(mat.iridescence_thickness_max),
        );
    }
    tex!(
        iridescence,
        "iridescenceTexture",
        mat.iridescence_texture_data
    );
    tex!(
        iridescence,
        "iridescenceThicknessTexture",
        mat.iridescence_thickness_texture_data
    );
    if !iridescence.is_empty() {
        used.insert("KHR_materials_iridescence");
        extensions.insert(
            "KHR_materials_iridescence".into(),
            serde_json::Value::Object(iridescence),
        );
    }

    let mut anisotropy = serde_json::Map::new();
    if mat.anisotropy != 0.0 {
        anisotropy.insert(
            "anisotropyStrength".into(),
            serde_json::json!(mat.anisotropy),
        );
    }
    if mat.anisotropy_rotation != 0.0 {
        anisotropy.insert(
            "anisotropyRotation".into(),
            serde_json::json!(mat.anisotropy_rotation),
        );
    }
    tex!(anisotropy, "anisotropyTexture", mat.anisotropy_texture_data);
    if !anisotropy.is_empty() {
        used.insert("KHR_materials_anisotropy");
        extensions.insert(
            "KHR_materials_anisotropy".into(),
            serde_json::Value::Object(anisotropy),
        );
    }

    Ok(extensions)
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

    let mut extensions_used: std::collections::BTreeSet<&'static str> =
        std::collections::BTreeSet::new();
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
        let extensions = material_extensions(
            mat,
            &mut table,
            &mut bin,
            &mut buffer_views,
            &mut extensions_used,
        )?;
        if !extensions.is_empty() {
            entry.insert("extensions".into(), serde_json::Value::Object(extensions));
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
        // Sorted and deduplicated by the set, so the array is stable across
        // runs and a round-trip test can assert on it directly. Every one is
        // optional-fallback, so `extensionsRequired` stays absent.
        if !extensions_used.is_empty() {
            root.insert(
                "extensionsUsed".into(),
                serde_json::json!(extensions_used.iter().collect::<Vec<_>>()),
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

/// The encoding every EXR this writes carries: ZIP, lossless, scanline blocks.
///
/// Lossless is not negotiable for a pass a compositor does arithmetic on, and
/// ZIP over the run-length default because a render is not the single-coloured
/// matte run-length encoding is good at. Blocks are indexed and written in
/// increasing line order, which is what keeps the bytes identical between two
/// runs of the same render even though the crate compresses blocks in parallel.
const EXR_ENCODING: exr::image::Encoding = exr::image::Encoding::SMALL_LOSSLESS;

/// EXR-encodes a linear RGB float image.
///
/// Byte-first like every other writer here: the crate can write into anything
/// that is `Write + Seek`, so a caller who wants a file still owns the file.
///
/// Three channels and no alpha, matching [`RawImageHdr`] exactly. A still has
/// its background already in it, so an alpha lane would be a constant one
/// pretending to be a matte.
///
/// # Errors
/// The encode failing, which for a well-formed image it does not.
pub fn encode_exr_rgb_bytes(img: &RawImageHdr) -> Result<Vec<u8>, FormatsError> {
    let (width, height) = (img.width as usize, img.height as usize);
    let pixels = exr::image::SpecificChannels::rgb(|position: exr::math::Vec2<usize>| {
        let i = (position.1 * width + position.0) * 3;
        // Clamped rather than indexed blind: a short buffer is a caller error,
        // and returning black for it beats a panic inside a writer thread.
        match img.pixels.get(i..i + 3) {
            Some(p) => (p[0], p[1], p[2]),
            None => (0.0, 0.0, 0.0),
        }
    });
    write_exr(&exr::image::Image::from_encoded_channels(
        (width, height),
        EXR_ENCODING,
        pixels,
    ))
}

/// EXR-encodes a linear RGBA float image with a matte, **premultiplying on
/// the way out**.
///
/// `pixels` is four floats per pixel, colour unassociated and alpha a plain
/// coverage fraction; what leaves carries `rgb * a`, because premultiplied is
/// what the floating-point format's ecosystem assumes and what a compositor
/// will treat the file as whether or not anyone says so. The multiplication
/// happens here and nowhere else: doing it earlier would mean the renderer
/// knowing which file its pixels are destined for, and the eight-bit path
/// deliberately does not do it, because that format's specification says
/// straight. The two files therefore differ numerically at partially covered
/// pixels, which is the documented consequence of each format getting its own
/// convention rather than a defect.
///
/// A sibling of [`encode_exr_rgb_bytes`] rather than a flag on it: the two
/// have different callers and different channel sets, and an opaque render
/// keeps writing three channels, because its alpha would be a constant one
/// pretending to be a matte.
///
/// # Errors
/// The encode failing, which for a well-formed image it does not.
pub fn encode_exr_rgba_bytes(
    pixels: &[f32],
    width: u32,
    height: u32,
) -> Result<Vec<u8>, FormatsError> {
    let (w, h) = (width as usize, height as usize);
    let sampler = exr::image::SpecificChannels::rgba(|position: exr::math::Vec2<usize>| {
        let i = (position.1 * w + position.0) * 4;
        // Clamped rather than indexed blind, like the three-channel writer:
        // a short buffer is a caller error, and returning nothing at all for
        // it beats a panic inside a writer thread.
        match pixels.get(i..i + 4) {
            Some(p) => (p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]),
            None => (0.0, 0.0, 0.0, 0.0),
        }
    });
    write_exr(&exr::image::Image::from_encoded_channels(
        (w, h),
        EXR_ENCODING,
        sampler,
    ))
}

/// EXR-encodes a single-channel depth pass as the channel named `Z`.
///
/// `Z` is what a compositing package looks for, and one channel rather than a
/// grey triple because a depth is one number per pixel and writing it three
/// times would say otherwise.
///
/// # Errors
/// The encode failing.
pub fn encode_exr_depth_bytes(
    depth: &[f32],
    width: u32,
    height: u32,
) -> Result<Vec<u8>, FormatsError> {
    let (w, h) = (width as usize, height as usize);
    let pixels = exr::image::SpecificChannels::build()
        .with_channel("Z")
        .with_pixel_fn(|position: exr::math::Vec2<usize>| {
            (depth
                .get(position.1 * w + position.0)
                .copied()
                .unwrap_or(0.0),)
        });
    write_exr(&exr::image::Image::from_encoded_channels(
        (w, h),
        EXR_ENCODING,
        pixels,
    ))
}

/// The half both encoders share: write the image into memory.
fn write_exr<'a, C>(
    image: &'a exr::image::Image<exr::image::Layer<C>>,
) -> Result<Vec<u8>, FormatsError>
where
    exr::image::Layer<C>: exr::image::write::layers::WritableLayers<'a>,
{
    use exr::image::write::WritableImage;
    let mut out = std::io::Cursor::new(Vec::new());
    image
        .write()
        .to_buffered(&mut out)
        .map_err(|e| FormatsError::Export {
            format: "exr",
            message: e.to_string(),
        })?;
    Ok(out.into_inner())
}

/// JPEG-encodes an RGBA8 image (alpha dropped), quality 1..100.
pub fn encode_jpeg_bytes(img: &RawImageData, quality: u8) -> Result<Vec<u8>, FormatsError> {
    // JPEG has no alpha: flatten onto opaque.
    let rgb: Vec<u8> = img
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
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
    #![allow(clippy::float_cmp)] // exact factors that round-trip through export and reload

    use super::*;

    /// Owned quad buffers: positions, normals, tex coords, indices.
    type QuadData = (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<u32>);

    fn quad() -> QuadData {
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

    fn export_quad(d: &QuadData) -> ExportMesh<'_> {
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
    fn glb_principled_extensions_round_trip() {
        // Every principled property survives a write and a re-read, and the
        // extension names land in extensionsUsed. This is the export half of
        // the promise; the import half is proved against a hand-authored
        // fixture in tests/loaders.rs, because a round trip through our own
        // writer only shows we agree with ourselves.
        let material = Arc::new(RawMaterialData {
            name: "glass".to_string(),
            ior: 1.7,
            transmission: 0.9,
            thickness: 2.5,
            attenuation_color: [0.8, 0.2, 0.1],
            attenuation_distance: 3.0,
            clearcoat: 0.75,
            clearcoat_roughness: 0.25,
            sheen_color: [0.4, 0.5, 0.6],
            sheen_roughness: 0.35,
            iridescence: 0.5,
            iridescence_ior: 1.8,
            iridescence_thickness_min: 200.0,
            iridescence_thickness_max: 600.0,
            specular_intensity: 0.6,
            specular_color: [0.9, 0.8, 0.7],
            anisotropy: 0.65,
            anisotropy_rotation: 1.2,
            emissive_strength: 4.0,
            ..Default::default()
        });
        let d = quad();
        let mut mesh = export_quad(&d);
        mesh.material_index = Some(0);
        let bytes = write_glb_bytes(&[mesh], &[material]).expect("write");
        let model = crate::gltf::load_gltf_bytes(&bytes, &mut crate::NoAssets).expect("reimport");
        let m = &model.materials[0];

        assert!((m.ior - 1.7).abs() < 1e-6, "ior");
        assert!((m.transmission - 0.9).abs() < 1e-6, "transmission");
        assert!((m.thickness - 2.5).abs() < 1e-6, "thickness");
        assert_eq!(m.attenuation_color, [0.8, 0.2, 0.1]);
        assert!((m.attenuation_distance - 3.0).abs() < 1e-6, "attenuation");
        assert!((m.clearcoat - 0.75).abs() < 1e-6, "clearcoat");
        assert!(
            (m.clearcoat_roughness - 0.25).abs() < 1e-6,
            "coat roughness"
        );
        assert_eq!(m.sheen_color, [0.4, 0.5, 0.6]);
        assert!((m.sheen_roughness - 0.35).abs() < 1e-6, "sheen roughness");
        assert!((m.iridescence - 0.5).abs() < 1e-6, "iridescence");
        assert!((m.iridescence_ior - 1.8).abs() < 1e-6, "iridescence ior");
        assert!(
            (m.iridescence_thickness_min - 200.0).abs() < 1e-6,
            "thickness min"
        );
        assert!(
            (m.iridescence_thickness_max - 600.0).abs() < 1e-6,
            "thickness max"
        );
        assert!((m.specular_intensity - 0.6).abs() < 1e-6, "specular");
        assert_eq!(m.specular_color, [0.9, 0.8, 0.7]);
        assert!((m.anisotropy - 0.65).abs() < 1e-6, "anisotropy");
        assert!((m.anisotropy_rotation - 1.2).abs() < 1e-6, "rotation");
        assert!(
            (m.emissive_strength - 4.0).abs() < 1e-6,
            "emissive strength"
        );

        // The container half: read the JSON chunk back and confirm every
        // extension was declared, sorted and deduplicated.
        let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let json: serde_json::Value =
            serde_json::from_slice(&bytes[20..20 + json_len]).expect("json chunk");
        let used: Vec<&str> = json["extensionsUsed"]
            .as_array()
            .expect("extensionsUsed")
            .iter()
            .map(|v| v.as_str().expect("name"))
            .collect();
        assert_eq!(
            used,
            [
                "KHR_materials_anisotropy",
                "KHR_materials_clearcoat",
                "KHR_materials_emissive_strength",
                "KHR_materials_ior",
                "KHR_materials_iridescence",
                "KHR_materials_sheen",
                "KHR_materials_specular",
                "KHR_materials_transmission",
                "KHR_materials_volume",
            ]
        );
        // Optional-fallback, every one of them, so nothing is required.
        assert!(json.get("extensionsRequired").is_none());
    }

    #[test]
    fn a_plain_material_declares_no_extensions() {
        // The neutrality check that keeps the promise cheap: a material that
        // touches none of these properties must export exactly as it did
        // before they existed, with no extensions object and no
        // extensionsUsed array at all.
        let material = Arc::new(RawMaterialData {
            name: "plain".to_string(),
            roughness_factor: 0.5,
            ..Default::default()
        });
        let d = quad();
        let mut mesh = export_quad(&d);
        mesh.material_index = Some(0);
        let bytes = write_glb_bytes(&[mesh], &[material]).expect("write");
        let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let json: serde_json::Value =
            serde_json::from_slice(&bytes[20..20 + json_len]).expect("json chunk");
        assert!(json.get("extensionsUsed").is_none());
        assert!(json["materials"][0].get("extensions").is_none());
    }

    #[test]
    fn glb_extension_textures_round_trip_and_deduplicate() {
        // An extension texture travels out and back, and an image shared
        // between an original slot and an extension slot still embeds once,
        // because the texture table keys on content hash and knows nothing
        // about roles.
        let shared = Arc::new(RawImageData::new(vec![9, 9, 9, 255], 1, 1));
        let material = Arc::new(RawMaterialData {
            name: "coated".to_string(),
            clearcoat: 0.5,
            diffuse_texture_data: Some(Arc::clone(&shared)),
            clearcoat_texture_data: Some(Arc::clone(&shared)),
            transmission: 0.5,
            transmission_texture_data: Some(Arc::new(RawImageData::new(vec![1, 2, 3, 255], 1, 1))),
            ..Default::default()
        });
        let d = quad();
        let mut mesh = export_quad(&d);
        mesh.material_index = Some(0);
        let bytes = write_glb_bytes(&[mesh], &[material]).expect("write");
        let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let json: serde_json::Value =
            serde_json::from_slice(&bytes[20..20 + json_len]).expect("json chunk");
        // Two distinct images for three references.
        assert_eq!(json["images"].as_array().unwrap().len(), 2);

        let model = crate::gltf::load_gltf_bytes(&bytes, &mut crate::NoAssets).expect("reimport");
        let m = &model.materials[0];
        assert!(m.clearcoat_texture_data.is_some(), "clearcoat map");
        assert!(m.transmission_texture_data.is_some(), "transmission map");
        assert_eq!(
            m.clearcoat_texture_data.as_ref().unwrap().hash,
            m.diffuse_texture_data.as_ref().unwrap().hash,
            "the shared image stayed one image"
        );
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

    /// An image where each pixel encodes its own coordinates, so a transposed
    /// or sheared write is a failure rather than a picture that looks fine.
    fn coordinate_hdr(width: u32, height: u32) -> RawImageHdr {
        let mut pixels = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                pixels.extend_from_slice(&[x as f32, y as f32, 0.5]);
            }
        }
        RawImageHdr::new(pixels, width, height)
    }

    #[test]
    fn exr_round_trips_through_our_decoder() {
        let img = coordinate_hdr(7, 5);
        let bytes = encode_exr_rgb_bytes(&img).expect("exr");
        let back = crate::hdr::decode_exr_bytes(&bytes).expect("decode");
        assert_eq!((back.width, back.height), (7, 5));
        for y in 0..5 {
            for x in 0..7 {
                let i = (y * 7 + x) * 3;
                assert!((back.pixels[i] - x as f32).abs() < 1e-3, "red at {x},{y}");
                assert!(
                    (back.pixels[i + 1] - y as f32).abs() < 1e-3,
                    "green at {x},{y}"
                );
                assert!((back.pixels[i + 2] - 0.5).abs() < 1e-3, "blue at {x},{y}");
            }
        }
    }

    /// The property the seeded-render check downstream is built on.
    ///
    /// The crate compresses blocks on several threads, so "the same image
    /// encodes to the same bytes" is a claim about block indexing and line
    /// order rather than something a single-threaded reading of the code
    /// proves. Asserted here, once, at the level that can actually see it.
    #[test]
    fn the_same_image_encodes_to_the_same_bytes() {
        let img = coordinate_hdr(64, 48);
        let a = encode_exr_rgb_bytes(&img).expect("exr");
        let b = encode_exr_rgb_bytes(&img).expect("exr");
        assert_eq!(a, b, "two encodes of one image differed");

        let depth: Vec<f32> = (0..64 * 48).map(|i| i as f32 * 0.25).collect();
        let c = encode_exr_depth_bytes(&depth, 64, 48).expect("exr");
        let d = encode_exr_depth_bytes(&depth, 64, 48).expect("exr");
        assert_eq!(c, d, "two encodes of one depth pass differed");
    }

    /// A depth pass is one channel, and it is the one a compositor looks for.
    #[test]
    fn a_depth_pass_carries_a_single_channel_named_z() {
        use exr::prelude::{ReadChannels, ReadLayers};
        let depth: Vec<f32> = (0..12).map(|i| i as f32).collect();
        let bytes = encode_exr_depth_bytes(&depth, 4, 3).expect("exr");
        let image = exr::prelude::read()
            .no_deep_data()
            .largest_resolution_level()
            .all_channels()
            .first_valid_layer()
            .all_attributes()
            .from_buffered(std::io::Cursor::new(bytes))
            .expect("read back");
        let names: Vec<String> = image
            .layer_data
            .channel_data
            .list
            .iter()
            .map(|c| c.name.to_string())
            .collect();
        assert_eq!(names, vec!["Z".to_string()]);
    }

    /// The matte writer premultiplies, and only the matte writer.
    ///
    /// Unassociated colour with a fractional matte goes in; what the file
    /// holds is `rgb * a` beside the untouched `a`, read back through the exr
    /// crate itself rather than through this crate's own decoder, so a writer
    /// that quietly stopped multiplying would fail here rather than in
    /// somebody's compositor.
    #[test]
    fn the_matte_writer_premultiplies_on_the_way_out() {
        use exr::prelude::{ReadChannels, ReadLayers};
        // One opaque pixel, one half-covered, one uncovered with stray colour
        // (which premultiplication is what clips), one quarter-covered.
        let pixels: Vec<f32> = vec![
            0.8, 0.6, 0.4, 1.0, //
            0.8, 0.6, 0.4, 0.5, //
            0.9, 0.9, 0.9, 0.0, //
            0.4, 0.2, 0.1, 0.25,
        ];
        let bytes = encode_exr_rgba_bytes(&pixels, 4, 1).expect("exr");
        let image = exr::prelude::read()
            .no_deep_data()
            .largest_resolution_level()
            .rgba_channels(
                |size, _| vec![[0.0f32; 4]; size.0 * size.1],
                |px: &mut Vec<[f32; 4]>, pos, (r, g, b, a): (f32, f32, f32, f32)| {
                    px[pos.1 * 4 + pos.0] = [r, g, b, a];
                },
            )
            .first_valid_layer()
            .all_attributes()
            .from_buffered(std::io::Cursor::new(bytes))
            .expect("read back");
        let back = &image.layer_data.channel_data.pixels;
        for (i, want_a) in [1.0f32, 0.5, 0.0, 0.25].iter().enumerate() {
            let src = &pixels[i * 4..i * 4 + 4];
            let got = back[i];
            for c in 0..3 {
                assert!(
                    (got[c] - src[c] * want_a).abs() < 1e-6,
                    "channel {c} of pixel {i} is premultiplied"
                );
            }
            assert!(
                (got[3] - want_a).abs() < 1e-6,
                "the matte itself is untouched"
            );
        }
    }

    /// The matte writer is as deterministic as its siblings.
    #[test]
    fn the_matte_writer_encodes_to_the_same_bytes() {
        let pixels: Vec<f32> = (0..64 * 48 * 4).map(|i| (i % 251) as f32 / 251.0).collect();
        let a = encode_exr_rgba_bytes(&pixels, 64, 48).expect("exr");
        let b = encode_exr_rgba_bytes(&pixels, 64, 48).expect("exr");
        assert_eq!(a, b, "two encodes of one matte image differed");
    }
}
