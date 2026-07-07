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
