use std::path::Path;

use solarxy_formats::{gltf, obj, ply, stl};

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
        .to_string_lossy()
        .to_string()
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
