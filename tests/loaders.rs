#![cfg(feature = "analyzer")]

use std::path::Path;

use solarxy_formats::obj;
use solarxy_formats::ply;
use solarxy_formats::stl;

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

#[cfg(feature = "viewer")]
#[test]
fn format_extension_detection() {
    use solarxy::cgi::resources::is_supported_model_extension;

    for ext in &["obj", "stl", "ply", "gltf", "glb"] {
        let name = format!("model.{}", ext);
        assert!(
            is_supported_model_extension(Path::new(&name)),
            "{} should be supported",
            ext
        );
    }
    for ext in &["OBJ", "STL", "PLY", "GLTF", "GLB"] {
        let name = format!("model.{}", ext);
        assert!(
            is_supported_model_extension(Path::new(&name)),
            "{} should be supported (case-insensitive)",
            ext
        );
    }
    for ext in &["txt", "png", "rs", "json", "fbx"] {
        let name = format!("model.{}", ext);
        assert!(
            !is_supported_model_extension(Path::new(&name)),
            "{} should not be supported",
            ext
        );
    }
    assert!(!is_supported_model_extension(Path::new("no_extension")));
}
