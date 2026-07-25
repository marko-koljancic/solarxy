//! STL loading (binary and ASCII). STL is self-contained, so the byte API
//! takes only a mesh name (STL carries none of its own).

use std::io::Cursor;

use crate::FormatsError;
use solarxy_core::{MeshTopology, RawMeshData, RawModelData};

/// Parse STL bytes. `name` becomes the single mesh's name (the path loader
/// passes the file path, preserving historical naming).
pub fn load_stl_bytes(bytes: &[u8], name: &str) -> Result<RawModelData, FormatsError> {
    let mut reader = Cursor::new(bytes);
    let indexed_mesh = stl_io::read_stl(&mut reader).map_err(FormatsError::Stl)?;

    if indexed_mesh.vertices.is_empty() || indexed_mesh.faces.is_empty() {
        return Err(FormatsError::Invalid(
            "STL file contains no geometry".to_string(),
        ));
    }

    let positions: Vec<[f32; 3]> = indexed_mesh
        .vertices
        .iter()
        .map(|v| [v[0], v[1], v[2]])
        .collect();

    let indices: Vec<u32> = indexed_mesh
        .faces
        .iter()
        .flat_map(|f| f.vertices.iter().map(|&i| i as u32))
        .collect();

    let polygon_count = indexed_mesh.faces.len();

    Ok(RawModelData {
        meshes: vec![RawMeshData {
            name: name.to_string(),
            positions,
            indices,
            normals: None,
            tex_coords: None,
            material_index: None,
            topology: MeshTopology::Triangles,
            colors: None,
        }],
        materials: Vec::new(),
        polygon_count,
    })
}

/// Load an STL from disk.
#[cfg(feature = "std-fs")]
pub fn load_stl(file_path: &str) -> Result<RawModelData, FormatsError> {
    let bytes = crate::read_file(file_path)?;
    load_stl_bytes(&bytes, file_path)
}
