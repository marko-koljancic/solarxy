use std::path::Path;

use solarxy_formats::{
    AssetResolver, DirResolver, NoAssets, RawModelData, gltf, load_model, load_model_bytes, obj,
    ply, stl,
};

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
        .to_string_lossy()
        .to_string()
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).unwrap()
}

/// Byte-vs-path equivalence: geometry, materials count, and polygon count
/// must match exactly (mesh names may differ — byte mode has no path).
fn assert_geometry_eq(a: &RawModelData, b: &RawModelData) {
    assert_eq!(a.polygon_count, b.polygon_count, "polygon_count");
    assert_eq!(a.materials.len(), b.materials.len(), "materials.len");
    assert_eq!(a.meshes.len(), b.meshes.len(), "meshes.len");
    for (ma, mb) in a.meshes.iter().zip(&b.meshes) {
        assert_eq!(ma.positions, mb.positions, "positions");
        assert_eq!(ma.indices, mb.indices, "indices");
        assert_eq!(ma.normals, mb.normals, "normals");
        assert_eq!(ma.tex_coords, mb.tex_coords, "tex_coords");
        assert_eq!(ma.material_index, mb.material_index, "material_index");
    }
}

#[test]
fn load_obj_triangle() {
    let raw = obj::load_obj(&fixture("triangle.obj")).unwrap();
    assert_eq!(raw.meshes.len(), 1);
    assert_eq!(raw.meshes[0].positions.len(), 3);
    assert_eq!(raw.meshes[0].indices.len(), 3);
    assert_eq!(raw.polygon_count, 1);
}

#[test]
fn load_stl_triangle() {
    let raw = stl::load_stl(&fixture("triangle.stl")).unwrap();
    assert_eq!(raw.meshes.len(), 1);
    assert_eq!(raw.meshes[0].positions.len(), 3);
    assert_eq!(raw.meshes[0].indices.len(), 3);
}

#[test]
fn load_ply_triangle() {
    let raw = ply::load_ply(&fixture("triangle.ply")).unwrap();
    assert_eq!(raw.meshes.len(), 1);
    assert_eq!(raw.meshes[0].positions.len(), 3);
    assert_eq!(raw.meshes[0].indices.len(), 3);
}

#[test]
fn load_obj_nonexistent() {
    assert!(obj::load_obj("/nonexistent/model.obj").is_err());
}

#[test]
fn load_stl_nonexistent() {
    assert!(stl::load_stl("/nonexistent/model.stl").is_err());
}

#[test]
fn load_ply_nonexistent() {
    assert!(ply::load_ply("/nonexistent/model.ply").is_err());
}

#[test]
fn obj_triangle_position_values() {
    let raw = obj::load_obj(&fixture("triangle.obj")).unwrap();
    let pos = &raw.meshes[0].positions;
    assert_eq!(pos.len(), 3);
    assert_eq!(pos[0], [0.0, 0.0, 0.0]);
    assert_eq!(pos[1], [1.0, 0.0, 0.0]);
    assert_eq!(pos[2], [0.0, 1.0, 0.0]);
}

#[test]
fn stl_triangle_normals_none() {
    let raw = stl::load_stl(&fixture("triangle.stl")).unwrap();
    assert!(
        raw.meshes[0].normals.is_none(),
        "STL raw data should not include normals"
    );
}

#[test]
fn ply_triangle_default_material() {
    let raw = ply::load_ply(&fixture("triangle.ply")).unwrap();
    assert!(
        !raw.materials.is_empty(),
        "PLY should create a default material"
    );
    assert_eq!(raw.meshes[0].material_index, Some(0));
}

#[test]
fn load_gltf_triangle() {
    let raw = gltf::load_gltf(&fixture("triangle.glb")).unwrap();
    assert_eq!(raw.meshes.len(), 1);
    assert_eq!(raw.meshes[0].positions.len(), 3);
    assert_eq!(raw.meshes[0].indices.len(), 3);
    assert_eq!(raw.polygon_count, 1);
}

#[test]
fn load_gltf_nonexistent() {
    assert!(gltf::load_gltf("/nonexistent/model.glb").is_err());
}

#[test]
fn gltf_triangle_position_values() {
    let raw = gltf::load_gltf(&fixture("triangle.glb")).unwrap();
    let pos = &raw.meshes[0].positions;
    assert_eq!(pos.len(), 3);
    assert_eq!(pos[0], [0.0, 0.0, 0.0]);
    assert_eq!(pos[1], [1.0, 0.0, 0.0]);
    assert_eq!(pos[2], [0.0, 1.0, 0.0]);
}

// ---- Byte-first API: byte-vs-path equivalence over the same fixtures ----

#[test]
fn obj_bytes_matches_path() {
    let from_path = obj::load_obj(&fixture("triangle.obj")).unwrap();
    let mut resolver = DirResolver::new(Path::new(&fixture("triangle.obj")).parent().unwrap());
    let from_bytes = obj::load_obj_bytes(&fixture_bytes("triangle.obj"), &mut resolver).unwrap();
    assert_geometry_eq(&from_path, &from_bytes);
}

#[test]
fn stl_bytes_matches_path() {
    let from_path = stl::load_stl(&fixture("triangle.stl")).unwrap();
    let from_bytes = stl::load_stl_bytes(&fixture_bytes("triangle.stl"), "triangle.stl").unwrap();
    assert_geometry_eq(&from_path, &from_bytes);
}

#[test]
fn ply_bytes_matches_path() {
    let from_path = ply::load_ply(&fixture("triangle.ply")).unwrap();
    let from_bytes = ply::load_ply_bytes(&fixture_bytes("triangle.ply"), "triangle.ply").unwrap();
    assert_geometry_eq(&from_path, &from_bytes);
}

#[test]
fn glb_bytes_matches_path() {
    let from_path = gltf::load_gltf(&fixture("triangle.glb")).unwrap();
    let from_bytes = gltf::load_gltf_bytes(&fixture_bytes("triangle.glb"), &mut NoAssets).unwrap();
    assert_geometry_eq(&from_path, &from_bytes);
}

#[test]
fn load_model_bytes_dispatches_like_load_model() {
    for name in [
        "triangle.obj",
        "triangle.stl",
        "triangle.ply",
        "triangle.glb",
    ] {
        let from_path = load_model(&fixture(name)).unwrap();
        let ext = Path::new(name).extension().unwrap().to_str().unwrap();
        let mut resolver = DirResolver::new(Path::new(&fixture(name)).parent().unwrap());
        let from_bytes = load_model_bytes(&fixture_bytes(name), ext, name, &mut resolver).unwrap();
        assert_geometry_eq(&from_path, &from_bytes);
    }
}

#[test]
fn stl_bytes_names_mesh_from_caller() {
    let raw = stl::load_stl_bytes(&fixture_bytes("triangle.stl"), "delivered.stl").unwrap();
    assert_eq!(raw.meshes[0].name, "delivered.stl");
}

#[test]
fn bytes_garbage_input_errors_not_panics() {
    let garbage = b"not a model at all";
    assert!(stl::load_stl_bytes(garbage, "g").is_err());
    assert!(ply::load_ply_bytes(garbage, "g").is_err());
    assert!(gltf::load_gltf_bytes(garbage, &mut NoAssets).is_err());
    // tobj treats unknown lines as noise; an "OBJ" of garbage parses to
    // zero meshes rather than an error — assert emptiness instead.
    let obj_raw = obj::load_obj_bytes(garbage, &mut NoAssets).unwrap();
    assert!(obj_raw.meshes.is_empty());
}

/// The resolver contract: a missing MTL degrades to no materials (tobj
/// swallows MTL load failures into `None` materials), never a panic.
#[test]
fn obj_bytes_missing_mtl_degrades() {
    let obj_with_mtl = b"mtllib missing.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
    let raw = obj::load_obj_bytes(obj_with_mtl, &mut NoAssets).unwrap();
    assert_eq!(raw.meshes.len(), 1);
    assert_eq!(raw.meshes[0].indices.len(), 3);
}

/// glTF external-buffer resolution goes through the AssetResolver; a
/// resolver without the buffer yields MissingAsset, not a panic.
#[test]
fn gltf_bytes_external_buffer_via_resolver() {
    // Minimal .gltf JSON referencing an external buffer holding one
    // 3-float position accessor (12 bytes padded to 12; 36 bytes total).
    let positions: Vec<u8> = [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect();
    let json = format!(
        r#"{{"asset":{{"version":"2.0"}},
            "buffers":[{{"uri":"tri.bin","byteLength":{len}}}],
            "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":{len}}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}}}}]}}],
            "nodes":[{{"mesh":0}}],
            "scenes":[{{"nodes":[0]}}]}}"#,
        len = positions.len()
    );

    struct OneBuffer(Vec<u8>);
    impl AssetResolver for OneBuffer {
        fn read(&mut self, rel_path: &str) -> Option<Vec<u8>> {
            (rel_path == "tri.bin").then(|| self.0.clone())
        }
    }

    let raw = gltf::load_gltf_bytes(json.as_bytes(), &mut OneBuffer(positions)).unwrap();
    assert_eq!(raw.meshes.len(), 1);
    assert_eq!(raw.meshes[0].positions.len(), 3);

    match gltf::load_gltf_bytes(json.as_bytes(), &mut NoAssets) {
        Ok(_) => panic!("expected MissingAsset error without a resolver"),
        Err(err) => assert!(matches!(
            err,
            solarxy_formats::FormatsError::MissingAsset(ref p) if p == "tri.bin"
        )),
    }
}

// ---- texture matrix: decoded pixels through every delivery path ----

/// The known texels of `texel.png` (2x2 RGBA: red, green / blue, white),
/// exactly as every loader must decode them.
const TEXEL_PIXELS: [u8; 16] = [
    255, 0, 0, 255, //
    0, 255, 0, 255, //
    0, 0, 255, 255, //
    255, 255, 255, 255,
];

/// Assert the model carries the quad plus a base-color texture decoded to
/// the exact texel bytes. The single assertion every matrix case shares.
fn assert_textured_quad(raw: &RawModelData) {
    assert_eq!(raw.meshes.len(), 1, "meshes");
    assert_eq!(raw.meshes[0].positions.len(), 4, "positions");
    assert_eq!(raw.meshes[0].indices.len(), 6, "indices");
    assert!(raw.meshes[0].tex_coords.is_some(), "tex_coords");
    assert_eq!(raw.meshes[0].material_index, Some(0), "material_index");
    assert_eq!(raw.materials.len(), 1, "materials");
    let data = raw.materials[0]
        .diffuse_texture_data
        .as_ref()
        .expect("base color texture must decode");
    assert_eq!((data.width, data.height), (2, 2), "texture dimensions");
    assert_eq!(data.pixels, TEXEL_PIXELS, "texture pixels");
}

#[test]
fn gltf_embedded_image_decodes_in_byte_mode() {
    let raw =
        gltf::load_gltf_bytes(&fixture_bytes("textured_embedded.gltf"), &mut NoAssets).unwrap();
    assert_textured_quad(&raw);
}

#[test]
fn glb_buffer_view_image_decodes_in_byte_mode() {
    let raw = gltf::load_gltf_bytes(&fixture_bytes("textured.glb"), &mut NoAssets).unwrap();
    assert_textured_quad(&raw);
}

#[test]
fn gltf_external_image_decodes_through_resolver() {
    let mut resolver = DirResolver::new(
        Path::new(&fixture("textured_external.gltf"))
            .parent()
            .unwrap(),
    );
    let raw =
        gltf::load_gltf_bytes(&fixture_bytes("textured_external.gltf"), &mut resolver).unwrap();
    assert_textured_quad(&raw);
}

/// A missing external image degrades to an empty texture slot with the URI
/// still recorded as the texture path; it never fails the model. (The
/// external BUFFER is still required, so the resolver serves only that.)
#[test]
fn gltf_missing_external_image_degrades() {
    struct BufferOnly(Vec<u8>);
    impl AssetResolver for BufferOnly {
        fn read(&mut self, rel_path: &str) -> Option<Vec<u8>> {
            (rel_path == "textured_external.bin").then(|| self.0.clone())
        }
    }
    let mut resolver = BufferOnly(fixture_bytes("textured_external.bin"));
    let raw =
        gltf::load_gltf_bytes(&fixture_bytes("textured_external.gltf"), &mut resolver).unwrap();
    assert_eq!(raw.meshes.len(), 1);
    let mat = &raw.materials[0];
    assert!(mat.diffuse_texture_data.is_none(), "no decoded pixels");
    assert_eq!(
        mat.diffuse_texture_path.as_deref(),
        Some(Path::new("texel.png")),
        "URI still recorded as the texture path"
    );
}

#[test]
fn gltf_external_bytes_matches_path_including_texture() {
    let from_path = gltf::load_gltf(&fixture("textured_external.gltf")).unwrap();
    let mut resolver = DirResolver::new(
        Path::new(&fixture("textured_external.gltf"))
            .parent()
            .unwrap(),
    );
    let from_bytes =
        gltf::load_gltf_bytes(&fixture_bytes("textured_external.gltf"), &mut resolver).unwrap();
    assert_geometry_eq(&from_path, &from_bytes);
    assert_textured_quad(&from_path);
    assert_textured_quad(&from_bytes);
}

#[test]
fn obj_map_kd_decodes_through_resolver() {
    let mut resolver = DirResolver::new(Path::new(&fixture("textured.obj")).parent().unwrap());
    let raw = obj::load_obj_bytes(&fixture_bytes("textured.obj"), &mut resolver).unwrap();
    assert_textured_quad(&raw);
}

/// The `DirResolver` must refuse to escape its base directory, or a malicious
/// model's `mtllib`/`map_Kd`/glTF `uri` becomes an arbitrary local-file read
/// (and, for the server-side `solarxy-validate` library, an LFI oracle).
/// A symlink pointing out of the base is refused, which is the escape the
/// component check alone does not catch: the path has no `..`, no root and no
/// prefix, so only the canonicalize-and-contain step can see it.
///
/// Unix only, because creating a symlink on Windows needs a privilege the
/// test runner is not guaranteed to hold.
#[cfg(unix)]
#[test]
fn dir_resolver_rejects_a_symlink_that_leaves_the_base() {
    let base = std::env::temp_dir().join("solarxy-formats-symlink-escape");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("base directory");

    let outside = std::env::temp_dir().join("solarxy-formats-symlink-target.txt");
    std::fs::write(&outside, b"not yours").expect("target file");

    // The link sits inside the base and its name is an ordinary one.
    let link = base.join("innocent.png");
    std::os::unix::fs::symlink(&outside, &link).expect("symlink");

    let mut resolver = DirResolver::new(&base);
    assert!(
        resolver.read("innocent.png").is_none(),
        "a symlink resolving outside the base must be refused after canonicalization"
    );

    // The target really is readable, so the refusal is the rule acting rather
    // than the file being absent.
    assert!(std::fs::read(&outside).is_ok());
}

#[test]
fn dir_resolver_rejects_path_traversal() {
    let dir = Path::new(&fixture("textured.obj"))
        .parent()
        .unwrap()
        .to_path_buf();
    let mut resolver = DirResolver::new(&dir);

    // A legitimate sibling still resolves.
    assert!(
        resolver.read("texel.png").is_some(),
        "a normal companion beside the model must still load"
    );

    // Every escape shape is refused.
    for evil in [
        "../../../../etc/passwd",
        "../Cargo.toml",
        "/etc/hosts",
        "..\\..\\windows\\win.ini",
        "sub/../../Cargo.toml",
    ] {
        assert!(
            resolver.read(evil).is_none(),
            "traversal must be rejected: {evil:?}"
        );
    }

    // And a real file just outside the base, reached by name, is not readable
    // even though it exists (the crate's own Cargo.toml sits two levels up).
    assert!(resolver.read("../../Cargo.toml").is_none());
}

// ---- 0.8.0 vertex-color and point-cloud fixtures ----

/// uchar 128 through the sRGB decode: 128/255 = 0.50196 encodes to linear
/// ~0.2158; the loader stores linear.
const LINEAR_128: f32 = 0.215_86;

#[test]
fn ply_colored_triangle_parses_srgb_colors_to_linear() {
    let raw = ply::load_ply_bytes(&fixture_bytes("colored_tri.ply"), "colored_tri.ply").unwrap();
    let mesh = &raw.meshes[0];
    assert_eq!(mesh.topology, solarxy_core::MeshTopology::Triangles);
    let colors = mesh.colors.as_ref().expect("colors parsed");
    assert_eq!(colors.len(), 3);
    assert!((colors[0][0] - 1.0).abs() < 1e-5, "255 -> 1.0: {colors:?}");
    assert!(colors[0][1].abs() < 1e-6 && colors[0][2].abs() < 1e-6);
    assert!(
        (colors[0][3] - 1.0).abs() < 1e-6,
        "no alpha property -> 1.0"
    );
    assert!((colors[1][1] - 1.0).abs() < 1e-5, "green vertex");
    assert!(
        (colors[2][2] - LINEAR_128).abs() < 1e-3,
        "uchar 128 decodes sRGB-to-linear, got {}",
        colors[2][2]
    );
}

#[test]
fn ply_faceless_file_loads_as_point_cloud() {
    let raw = ply::load_ply_bytes(&fixture_bytes("cloud.ply"), "cloud.ply").unwrap();
    assert_eq!(raw.polygon_count, 0);
    let mesh = &raw.meshes[0];
    assert_eq!(mesh.topology, solarxy_core::MeshTopology::Points);
    assert!(mesh.indices.is_empty());
    assert_eq!(mesh.positions.len(), 4);
    let colors = mesh.colors.as_ref().expect("cloud colors parsed");
    assert_eq!(colors.len(), 4);
    assert!((colors[0][0] - 1.0).abs() < 1e-5, "white point");
    assert!(
        (colors[3][3] - 128.0 / 255.0).abs() < 1e-3,
        "alpha stays normalized, not sRGB-decoded: {}",
        colors[3][3]
    );
}

#[test]
fn ply_binary_colors_parse_like_ascii() {
    // A binary_little_endian PLY built in-test: 2 colored points, no faces.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(
        b"ply\nformat binary_little_endian 1.0\nelement vertex 2\n\
property float x\nproperty float y\nproperty float z\n\
property uchar red\nproperty uchar green\nproperty uchar blue\n\
end_header\n",
    );
    for (pos, rgb) in [
        ([0.0f32, 0.0, 0.0], [255u8, 0, 0]),
        ([1.0f32, 2.0, 3.0], [0u8, 128, 255]),
    ] {
        for c in pos {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
        bytes.extend_from_slice(&rgb);
    }
    let raw = ply::load_ply_bytes(&bytes, "binary_cloud").unwrap();
    let mesh = &raw.meshes[0];
    assert_eq!(mesh.topology, solarxy_core::MeshTopology::Points);
    let colors = mesh.colors.as_ref().expect("binary colors parsed");
    assert!((colors[0][0] - 1.0).abs() < 1e-5);
    assert!((colors[1][1] - LINEAR_128).abs() < 1e-3);
    assert!((colors[1][2] - 1.0).abs() < 1e-5);
    assert_eq!(mesh.positions[1], [1.0, 2.0, 3.0]);
}

#[test]
fn glb_color0_parses_as_linear_rgba_without_srgb_decode() {
    let raw = gltf::load_gltf_bytes(&fixture_bytes("colored_tri.glb"), &mut NoAssets).unwrap();
    let mesh = &raw.meshes[0];
    let colors = mesh.colors.as_ref().expect("COLOR_0 parsed");
    assert_eq!(colors.len(), 3);
    assert!(
        (colors[0][0] - 1.0).abs() < 1e-5,
        "u8 255 normalizes to 1.0"
    );
    // glTF colors are already linear: u8 128 must stay 128/255, NOT be
    // sRGB-decoded like PLY.
    assert!(
        (colors[2][2] - 128.0 / 255.0).abs() < 1e-3,
        "got {}",
        colors[2][2]
    );
    assert!((colors[2][3] - 1.0).abs() < 1e-5);
}

/// The float 0.5 through the same sRGB-to-linear decode PLY uses.
const LINEAR_HALF: f32 = 0.214_04;

#[test]
fn obj_parses_vertex_colors_from_the_extended_position_form() {
    // `v x y z r g b` is unofficial but universal in scan and photogrammetry
    // output. Decoded sRGB-to-linear, matching PLY's ratified stance so the
    // same scan exported to either format imports identically.
    let raw = obj::load_obj(&fixture("colored_tri.obj")).unwrap();
    let mesh = &raw.meshes[0];
    assert_eq!(mesh.positions.len(), 3, "positions still parse");
    let colors = mesh.colors.as_ref().expect("colors parsed");
    assert_eq!(colors.len(), 3, "one colour per vertex");

    assert!((colors[0][0] - 1.0).abs() < 1e-5, "red vertex: {colors:?}");
    assert!(colors[0][1].abs() < 1e-6 && colors[0][2].abs() < 1e-6);
    assert!(
        (colors[0][3] - 1.0).abs() < 1e-6,
        "OBJ has no alpha; opaque"
    );
    assert!((colors[1][1] - 1.0).abs() < 1e-5, "green vertex");
    assert!(
        (colors[2][0] - LINEAR_HALF).abs() < 1e-3,
        "0.5 decodes sRGB-to-linear, got {}",
        colors[2][0]
    );
}

#[test]
fn a_plain_obj_still_reports_no_colors() {
    // The overwhelmingly common case: three components per `v` and no
    // colour channel at all. It must stay `None` rather than becoming an
    // all-black lane that vertex-colour display would then honour.
    let raw = obj::load_obj(&fixture("triangle.obj")).unwrap();
    assert!(raw.meshes[0].colors.is_none());
}

#[test]
fn obj_colors_survive_the_byte_api_identically() {
    let path = obj::load_obj(&fixture("colored_tri.obj")).unwrap();
    let mut resolver = NoAssets;
    let bytes = obj::load_obj_bytes(&fixture_bytes("colored_tri.obj"), &mut resolver).unwrap();
    assert_eq!(path.meshes[0].colors, bytes.meshes[0].colors);
}

// ---- Principled surface extensions ----
//
// The fixture is hand-authored against the KHR specifications rather than
// produced by our own exporter, so these tests prove we read the JSON shape
// the specifications define and not merely that we agree with ourselves.
// The exporter's own round trip is covered in `export.rs`.

#[test]
fn gltf_principled_extensions_import_with_values_intact() {
    let raw = gltf::load_gltf(&fixture("principled_extensions.gltf")).unwrap();
    let mat = raw
        .materials
        .iter()
        .find(|m| m.name == "principled_full")
        .expect("the full material");

    // The five the crate exposes through typed accessors.
    assert!((mat.ior - 1.7).abs() < 1e-6, "ior");
    assert!((mat.transmission - 0.9).abs() < 1e-6, "transmission");
    assert!((mat.thickness - 2.5).abs() < 1e-6, "thickness");
    assert!((mat.attenuation_distance - 3.0).abs() < 1e-6, "attenuation");
    assert_eq!(mat.attenuation_color, [0.8, 0.2, 0.1]);
    assert!((mat.specular_intensity - 0.6).abs() < 1e-6, "specular");
    assert_eq!(mat.specular_color, [0.9, 0.8, 0.7]);
    assert!((mat.emissive_strength - 4.0).abs() < 1e-6, "emissive");

    // The four read through the raw extension map.
    assert!((mat.clearcoat - 0.75).abs() < 1e-6, "clearcoat");
    assert!(
        (mat.clearcoat_roughness - 0.25).abs() < 1e-6,
        "clearcoat roughness"
    );
    assert_eq!(mat.sheen_color, [0.4, 0.5, 0.6]);
    assert!((mat.sheen_roughness - 0.35).abs() < 1e-6, "sheen roughness");
    assert!((mat.iridescence - 0.5).abs() < 1e-6, "iridescence");
    assert!((mat.iridescence_ior - 1.8).abs() < 1e-6, "iridescence ior");
    assert!(
        (mat.iridescence_thickness_min - 200.0).abs() < 1e-6,
        "iridescence thickness min"
    );
    assert!(
        (mat.iridescence_thickness_max - 600.0).abs() < 1e-6,
        "iridescence thickness max"
    );
    assert!((mat.anisotropy - 0.65).abs() < 1e-6, "anisotropy");
    assert!(
        (mat.anisotropy_rotation - 1.2).abs() < 1e-6,
        "anisotropy rotation"
    );

    // Extension texture references resolve through the same path as the
    // five original slots, one from a typed accessor and one from the raw
    // map, and the two indices are distinct images.
    let transmission = mat
        .transmission_texture_data
        .as_ref()
        .expect("transmission map");
    let thickness = mat.thickness_texture_data.as_ref().expect("thickness map");
    assert_ne!(transmission.hash, thickness.hash, "distinct images");
    assert!(mat.clearcoat_texture_data.is_some(), "clearcoat map");
}

#[test]
fn gltf_malformed_extensions_degrade_without_failing_the_import() {
    // Every malformed payload in the broken material must be ignored with a
    // diagnostic, leaving the extension's default in place and the ordinary
    // metallic-roughness data untouched. Nothing here may panic, because a
    // vendor file is untrusted input and these four extensions carry no
    // crate-level validation at all.
    let raw = gltf::load_gltf(&fixture("principled_extensions.gltf")).unwrap();
    let mat = raw
        .materials
        .iter()
        .find(|m| m.name == "principled_broken")
        .expect("the broken material");

    // A factor that is a string keeps the extension default...
    assert!((mat.clearcoat - 0.0).abs() < 1e-6, "string factor ignored");
    // ...while its well-formed neighbour in the same block still lands.
    assert!(
        (mat.clearcoat_roughness - 0.5).abs() < 1e-6,
        "neighbour survives"
    );
    // An out-of-range texture index yields no texture, and the factor
    // beside it in the same block still reads.
    assert!(
        mat.clearcoat_normal_texture_data.is_none(),
        "out-of-range index"
    );
    // A two-component colour is malformed, not a partial value.
    assert_eq!(mat.sheen_color, [0.0, 0.0, 0.0]);
    // A texture reference carrying no index at all.
    assert!(mat.iridescence_texture_data.is_none(), "index-less texture");
    assert!((mat.iridescence - 0.5).abs() < 1e-6, "factor beside it");
    // An extension whose value is an array is skipped whole.
    assert!((mat.anisotropy - 0.0).abs() < 1e-6, "array extension");

    // And the material is otherwise entirely intact.
    assert_eq!(mat.base_color_factor, [0.25, 0.5, 0.75, 1.0]);
    assert!((mat.roughness_factor - 0.6).abs() < 1e-6, "roughness");
}

#[test]
fn a_file_without_extensions_imports_exactly_as_before() {
    // The regression that matters most: every material default must be the
    // identity of its effect, so an ordinary file is unchanged by the
    // fields existing at all.
    let raw = gltf::load_gltf(&fixture("textured_embedded.gltf")).unwrap();
    let mat = &raw.materials[0];
    assert!((mat.ior - 1.5).abs() < 1e-6);
    assert!((mat.specular_intensity - 1.0).abs() < 1e-6);
    assert_eq!(mat.specular_color, [1.0, 1.0, 1.0]);
    assert!((mat.emissive_strength - 1.0).abs() < 1e-6);
    assert_eq!(mat.attenuation_color, [1.0, 1.0, 1.0]);
    assert!((mat.transmission - 0.0).abs() < 1e-6);
    assert!((mat.clearcoat - 0.0).abs() < 1e-6);
    assert_eq!(mat.sheen_color, [0.0, 0.0, 0.0]);
    assert!((mat.iridescence - 0.0).abs() < 1e-6);
    assert!((mat.anisotropy - 0.0).abs() < 1e-6);
}
