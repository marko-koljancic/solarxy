use std::io::BufReader;

use super::geometry::{RawMeshData, RawModelData};

pub fn load_stl(file_path: &str) -> anyhow::Result<RawModelData> {
    let file = std::fs::File::open(file_path)?;
    let mut reader = BufReader::new(file);
    let indexed_mesh = stl_io::read_stl(&mut reader)?;

    if indexed_mesh.vertices.is_empty() || indexed_mesh.faces.is_empty() {
        anyhow::bail!("STL file contains no geometry");
    }

    let positions: Vec<[f32; 3]> = indexed_mesh.vertices.iter().map(|v| [v[0], v[1], v[2]]).collect();

    let indices: Vec<u32> = indexed_mesh
        .faces
        .iter()
        .flat_map(|f| f.vertices.iter().map(|&i| i as u32))
        .collect();

    let polygon_count = indexed_mesh.faces.len();

    Ok(RawModelData {
        meshes: vec![RawMeshData {
            name: file_path.to_string(),
            positions,
            indices,
            normals: None,
            tex_coords: None,
            material_index: None,
        }],
        materials: Vec::new(),
        polygon_count,
    })
}
