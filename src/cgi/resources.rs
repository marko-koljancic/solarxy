use std::io::{BufReader, Cursor};
use tobj::LoadError;
use wgpu::util::DeviceExt;
use cgmath::InnerSpace;
use ply_rs_bw::ply::Property;
use super::{material, texture, model};

pub async fn load_model_any(
    file_path: &str,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
) -> anyhow::Result<(model::Model, model::NormalsGeometry)> {
    let ext = std::path::Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "stl" => load_stl(file_path, device, queue, layout).await,
        "ply" => load_ply(file_path, device, queue, layout).await,
        _ => load_model(file_path, device, queue, layout).await,
    }
}

pub async fn load_string(file_path: &str) -> anyhow::Result<String> {
    let txt = {
        let path = std::path::Path::new(file_path);
        std::fs::read_to_string(path)?
    };

    Ok(txt)
}

pub async fn load_binary(file_path: &str) -> anyhow::Result<Vec<u8>> {
    let data = {
        let path = std::path::Path::new(file_path);
        std::fs::read(path)?
    };

    Ok(data)
}

pub async fn load_texture(
    file_path: &str,
    is_normal_map: bool,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> anyhow::Result<texture::Texture> {
    let data = load_binary(file_path).await?;
    texture::Texture::from_bytes(device, queue, &data, file_path, is_normal_map)
}

pub async fn load_model(
    file_path: &str,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
) -> anyhow::Result<(model::Model, model::NormalsGeometry)> {
    let obj_text = load_string(file_path).await?;
    let obj_cursor = Cursor::new(obj_text);
    let mut obj_reader = BufReader::new(obj_cursor);

    let obj_dir = std::path::Path::new(file_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());

    let (models, obj_materials) = tobj::load_obj_buf(
        &mut obj_reader,
        &tobj::LoadOptions {
            triangulate: true,
            single_index: true,
            ..Default::default()
        },
        |p| {
            let mat_path = std::path::Path::new(&obj_dir).join(p);
            let mat_text = std::fs::read_to_string(&mat_path).map_err(|_| LoadError::ReadError)?;
            tobj::load_mtl_buf(&mut BufReader::new(Cursor::new(mat_text)))
        },
    )?;

    let mut materials = Vec::new();
    for m in obj_materials.unwrap_or_default() {
        let diffuse_path = m
            .diffuse_texture
            .as_deref()
            .map(|p| std::path::Path::new(&obj_dir).join(p).to_string_lossy().to_string());

        let normal_path = m.normal_texture.as_deref().map(|p| {
            let cleaned = p
                .split_whitespace()
                .filter(|s| !s.starts_with('-'))
                .collect::<Vec<_>>()
                .join(" ");
            std::path::Path::new(&obj_dir)
                .join(cleaned)
                .to_string_lossy()
                .to_string()
        });

        let diffuse_texture = match diffuse_path {
            Some(path) if !path.is_empty() => load_texture(&path, false, device, queue).await.unwrap_or_else(|e| {
                eprintln!("Warning: Failed to load diffuse texture '{}': {}", path, e);
                create_default_texture(device, queue, false)
            }),
            _ => create_default_texture(device, queue, false),
        };

        let normal_texture = match normal_path {
            Some(path) if !path.is_empty() => load_texture(&path, true, device, queue)
                .await
                .unwrap_or_else(|_| create_default_texture(device, queue, true)),
            _ => create_default_texture(device, queue, true),
        };

        materials.push(material::Material::new(
            device,
            &m.name,
            diffuse_texture,
            normal_texture,
            layout,
        ));
    }

    if materials.is_empty() {
        let diffuse = create_default_texture_colored(device, queue, [147, 132, 120, 255]);
        let normal = create_default_texture(device, queue, true);
        materials.push(material::Material::new(device, "clay_default", diffuse, normal, layout));
    }

    let mut global_min = [f32::INFINITY; 3];
    let mut global_max = [f32::NEG_INFINITY; 3];

    let mut vertex_lines: Vec<[f32; 3]> = Vec::new();
    let mut face_lines: Vec<[f32; 3]> = Vec::new();

    let mut meshes = Vec::new();
    for m in models {
        if m.mesh.positions.is_empty() || m.mesh.indices.is_empty() {
            continue;
        }
        let mut vertices = (0..m.mesh.positions.len() / 3)
            .map(|i| model::ModelVertex {
                position: [
                    m.mesh.positions[i * 3],
                    m.mesh.positions[i * 3 + 1],
                    m.mesh.positions[i * 3 + 2],
                ],
                tex_coords: if m.mesh.texcoords.is_empty() {
                    [0.0, 0.0]
                } else {
                    [m.mesh.texcoords[i * 2], 1.0 - m.mesh.texcoords[i * 2 + 1]]
                },
                normal: [0.0, 0.0, 0.0],

                tangent: [0.0; 3],
                bitangent: [0.0; 3],
            })
            .collect::<Vec<_>>();

        let indices = &m.mesh.indices;
        let mut triangles_included = vec![0; vertices.len()];

        if m.mesh.normals.is_empty() {
            for c in indices.chunks(3) {
                let p0: cgmath::Vector3<f32> = vertices[c[0] as usize].position.into();
                let p1: cgmath::Vector3<f32> = vertices[c[1] as usize].position.into();
                let p2: cgmath::Vector3<f32> = vertices[c[2] as usize].position.into();
                let face_normal = (p1 - p0).cross(p2 - p0);
                for &vi in c {
                    let n = cgmath::Vector3::from(vertices[vi as usize].normal) + face_normal;
                    vertices[vi as usize].normal = n.into();
                }
            }
            for v in &mut vertices {
                let n = cgmath::Vector3::from(v.normal);
                if n.magnitude() > 0.0 {
                    v.normal = n.normalize().into();
                }
            }
        } else {
            for (i, v) in vertices.iter_mut().enumerate() {
                v.normal = [
                    m.mesh.normals[i * 3],
                    m.mesh.normals[i * 3 + 1],
                    m.mesh.normals[i * 3 + 2],
                ];
            }
        }

        for c in indices.chunks(3) {
            let v0 = vertices[c[0] as usize];
            let v1 = vertices[c[1] as usize];
            let v2 = vertices[c[2] as usize];

            let pos0: cgmath::Vector3<_> = v0.position.into();
            let pos1: cgmath::Vector3<_> = v1.position.into();
            let pos2: cgmath::Vector3<_> = v2.position.into();

            let uv0: cgmath::Vector2<_> = v0.tex_coords.into();
            let uv1: cgmath::Vector2<_> = v1.tex_coords.into();
            let uv2: cgmath::Vector2<_> = v2.tex_coords.into();

            let delta_pos1 = pos1 - pos0;
            let delta_pos2 = pos2 - pos0;

            let delta_uv1 = uv1 - uv0;
            let delta_uv2 = uv2 - uv0;

            let r = 1.0 / (delta_uv1.x * delta_uv2.y - delta_uv1.y * delta_uv2.x);
            let tangent = (delta_pos1 * delta_uv2.y - delta_pos2 * delta_uv1.y) * r;

            let bitangent = (delta_pos2 * delta_uv1.x - delta_pos1 * delta_uv2.x) * -r;

            vertices[c[0] as usize].tangent = (tangent + cgmath::Vector3::from(vertices[c[0] as usize].tangent)).into();
            vertices[c[1] as usize].tangent = (tangent + cgmath::Vector3::from(vertices[c[1] as usize].tangent)).into();
            vertices[c[2] as usize].tangent = (tangent + cgmath::Vector3::from(vertices[c[2] as usize].tangent)).into();
            vertices[c[0] as usize].bitangent =
                (bitangent + cgmath::Vector3::from(vertices[c[0] as usize].bitangent)).into();
            vertices[c[1] as usize].bitangent =
                (bitangent + cgmath::Vector3::from(vertices[c[1] as usize].bitangent)).into();
            vertices[c[2] as usize].bitangent =
                (bitangent + cgmath::Vector3::from(vertices[c[2] as usize].bitangent)).into();

            triangles_included[c[0] as usize] += 1;
            triangles_included[c[1] as usize] += 1;
            triangles_included[c[2] as usize] += 1;
        }

        for (i, n) in triangles_included.into_iter().enumerate() {
            let denom = 1.0 / n as f32;
            let v = &mut vertices[i];
            v.tangent = (cgmath::Vector3::from(v.tangent) * denom).into();
            v.bitangent = (cgmath::Vector3::from(v.bitangent) * denom).into();
        }

        for v in &vertices {
            for i in 0..3 {
                global_min[i] = global_min[i].min(v.position[i]);
                global_max[i] = global_max[i].max(v.position[i]);
            }
        }

        {
            let mut mesh_min = [f32::INFINITY; 3];
            let mut mesh_max = [f32::NEG_INFINITY; 3];
            for v in &vertices {
                for i in 0..3 {
                    mesh_min[i] = mesh_min[i].min(v.position[i]);
                    mesh_max[i] = mesh_max[i].max(v.position[i]);
                }
            }
            let dx = mesh_max[0] - mesh_min[0];
            let dy = mesh_max[1] - mesh_min[1];
            let dz = mesh_max[2] - mesh_min[2];
            let mesh_diagonal = (dx * dx + dy * dy + dz * dz).sqrt();
            let scale = if mesh_diagonal > 1e-10 {
                mesh_diagonal * 0.05
            } else {
                0.1
            };

            for v in &vertices {
                let nx = v.normal[0] * scale;
                let ny = v.normal[1] * scale;
                let nz = v.normal[2] * scale;
                vertex_lines.push(v.position);
                vertex_lines.push([v.position[0] + nx, v.position[1] + ny, v.position[2] + nz]);
            }

            for c in indices.chunks(3) {
                let p0: cgmath::Vector3<f32> = vertices[c[0] as usize].position.into();
                let p1: cgmath::Vector3<f32> = vertices[c[1] as usize].position.into();
                let p2: cgmath::Vector3<f32> = vertices[c[2] as usize].position.into();
                let edge1 = p1 - p0;
                let edge2 = p2 - p0;
                let face_normal = edge1.cross(edge2);
                if face_normal.magnitude() > 1e-10 {
                    let fn_norm = face_normal.normalize();
                    let center = (p0 + p1 + p2) / 3.0;
                    face_lines.push([center.x, center.y, center.z]);
                    face_lines.push([
                        center.x + fn_norm.x * scale,
                        center.y + fn_norm.y * scale,
                        center.z + fn_norm.z * scale,
                    ]);
                }
            }
        }

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{:?} Vertex Buffer", file_path)),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{:?} Index Buffer", file_path)),
            contents: bytemuck::cast_slice(&m.mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        meshes.push(model::Mesh {
            name: file_path.to_string(),
            vertex_buffer,
            index_buffer,
            num_elements: m.mesh.indices.len() as u32,
            material: m.mesh.material_id.unwrap_or(0),
        });
    }

    for i in 0..3 {
        if global_min[i].is_infinite() {
            global_min[i] = -1.0;
            global_max[i] = 1.0;
        }
    }

    let bounds = model::AABB {
        min: cgmath::Point3::new(global_min[0], global_min[1], global_min[2]),
        max: cgmath::Point3::new(global_max[0], global_max[1], global_max[2]),
    };

    Ok((
        model::Model {
            meshes,
            materials,
            bounds,
        },
        model::NormalsGeometry {
            vertex_lines,
            face_lines,
        },
    ))
}

pub async fn load_stl(
    file_path: &str,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
) -> anyhow::Result<(model::Model, model::NormalsGeometry)> {
    let file = std::fs::File::open(file_path)?;
    let mut reader = BufReader::new(file);
    let indexed_mesh = stl_io::read_stl(&mut reader)?;

    if indexed_mesh.vertices.is_empty() || indexed_mesh.faces.is_empty() {
        anyhow::bail!("STL file contains no geometry");
    }

    let mut vertices: Vec<model::ModelVertex> = indexed_mesh
        .vertices
        .iter()
        .map(|v| model::ModelVertex {
            position: [v[0], v[1], v[2]],
            tex_coords: [0.0, 0.0],
            normal: [0.0, 0.0, 0.0],
            tangent: [0.0; 3],
            bitangent: [0.0; 3],
        })
        .collect();

    let indices: Vec<u32> = indexed_mesh
        .faces
        .iter()
        .flat_map(|f| f.vertices.iter().map(|&i| i as u32))
        .collect();

    for c in indices.chunks(3) {
        let p0: cgmath::Vector3<f32> = vertices[c[0] as usize].position.into();
        let p1: cgmath::Vector3<f32> = vertices[c[1] as usize].position.into();
        let p2: cgmath::Vector3<f32> = vertices[c[2] as usize].position.into();
        let face_normal = (p1 - p0).cross(p2 - p0);
        for &vi in c {
            let n = cgmath::Vector3::from(vertices[vi as usize].normal) + face_normal;
            vertices[vi as usize].normal = n.into();
        }
    }
    for v in &mut vertices {
        let n = cgmath::Vector3::from(v.normal);
        if n.magnitude() > 0.0 {
            v.normal = n.normalize().into();
        }
    }

    let mut global_min = [f32::INFINITY; 3];
    let mut global_max = [f32::NEG_INFINITY; 3];
    for v in &vertices {
        for i in 0..3 {
            global_min[i] = global_min[i].min(v.position[i]);
            global_max[i] = global_max[i].max(v.position[i]);
        }
    }
    for i in 0..3 {
        if global_min[i].is_infinite() {
            global_min[i] = -1.0;
            global_max[i] = 1.0;
        }
    }

    let mut vertex_lines: Vec<[f32; 3]> = Vec::new();
    let mut face_lines: Vec<[f32; 3]> = Vec::new();
    {
        let dx = global_max[0] - global_min[0];
        let dy = global_max[1] - global_min[1];
        let dz = global_max[2] - global_min[2];
        let mesh_diagonal = (dx * dx + dy * dy + dz * dz).sqrt();
        let scale = if mesh_diagonal > 1e-10 {
            mesh_diagonal * 0.05
        } else {
            0.1
        };

        for v in &vertices {
            let nx = v.normal[0] * scale;
            let ny = v.normal[1] * scale;
            let nz = v.normal[2] * scale;
            vertex_lines.push(v.position);
            vertex_lines.push([v.position[0] + nx, v.position[1] + ny, v.position[2] + nz]);
        }

        for c in indices.chunks(3) {
            let p0: cgmath::Vector3<f32> = vertices[c[0] as usize].position.into();
            let p1: cgmath::Vector3<f32> = vertices[c[1] as usize].position.into();
            let p2: cgmath::Vector3<f32> = vertices[c[2] as usize].position.into();
            let edge1 = p1 - p0;
            let edge2 = p2 - p0;
            let face_normal = edge1.cross(edge2);
            if face_normal.magnitude() > 1e-10 {
                let fn_norm = face_normal.normalize();
                let center = (p0 + p1 + p2) / 3.0;
                face_lines.push([center.x, center.y, center.z]);
                face_lines.push([
                    center.x + fn_norm.x * scale,
                    center.y + fn_norm.y * scale,
                    center.z + fn_norm.z * scale,
                ]);
            }
        }
    }

    let diffuse = create_default_texture_colored(device, queue, [147, 132, 120, 255]);
    let normal = create_default_texture(device, queue, true);
    let materials = vec![material::Material::new(device, "clay_default", diffuse, normal, layout)];

    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(&format!("{:?} Vertex Buffer", file_path)),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(&format!("{:?} Index Buffer", file_path)),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    let bounds = model::AABB {
        min: cgmath::Point3::new(global_min[0], global_min[1], global_min[2]),
        max: cgmath::Point3::new(global_max[0], global_max[1], global_max[2]),
    };

    Ok((
        model::Model {
            meshes: vec![model::Mesh {
                name: file_path.to_string(),
                vertex_buffer,
                index_buffer,
                num_elements: indices.len() as u32,
                material: 0,
            }],
            materials,
            bounds,
        },
        model::NormalsGeometry {
            vertex_lines,
            face_lines,
        },
    ))
}

fn ply_prop_to_f32(prop: &Property) -> f32 {
    match *prop {
        Property::Float(v) => v,
        Property::Double(v) => v as f32,
        Property::Int(v) => v as f32,
        Property::UInt(v) => v as f32,
        Property::Short(v) => v as f32,
        Property::UShort(v) => v as f32,
        Property::Char(v) => v as f32,
        Property::UChar(v) => v as f32,
        _ => 0.0,
    }
}

fn ply_prop_to_indices(prop: &Property) -> Vec<u32> {
    match prop {
        Property::ListInt(v) => v.iter().map(|&i| i as u32).collect(),
        Property::ListUInt(v) => v.clone(),
        Property::ListShort(v) => v.iter().map(|&i| i as u32).collect(),
        Property::ListUShort(v) => v.iter().map(|&i| i as u32).collect(),
        Property::ListUChar(v) => v.iter().map(|&i| i as u32).collect(),
        Property::ListChar(v) => v.iter().map(|&i| i as u32).collect(),
        _ => Vec::new(),
    }
}

fn find_companion_texture(ply_path: &str) -> Option<String> {
    let path = std::path::Path::new(ply_path);
    let parent = path.parent()?;
    let stem = path.file_stem()?.to_str()?;
    let suffixes = ["_0", "", "_diffuse"];
    let extensions = ["jpg", "jpeg", "png", "bmp", "tga"];
    for suffix in &suffixes {
        for ext in &extensions {
            let candidate = parent.join(format!("{}{}.{}", stem, suffix, ext));
            if candidate.exists() {
                return Some(candidate.to_string_lossy().to_string());
            }
        }
    }
    None
}

pub async fn load_ply(
    file_path: &str,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
) -> anyhow::Result<(model::Model, model::NormalsGeometry)> {
    let file = std::fs::File::open(file_path)?;
    let mut reader = BufReader::new(file);
    let parser = ply_rs_bw::parser::Parser::<ply_rs_bw::ply::DefaultElement>::new();
    let ply = parser.read_ply(&mut reader)?;

    let ply_vertices = ply
        .payload
        .get("vertex")
        .ok_or_else(|| anyhow::anyhow!("PLY file has no 'vertex' element"))?;

    let ply_faces = ply
        .payload
        .get("face")
        .ok_or_else(|| anyhow::anyhow!("PLY file has no 'face' element"))?;

    if ply_vertices.is_empty() || ply_faces.is_empty() {
        anyhow::bail!("PLY file contains no geometry");
    }

    let (has_normals, has_uvs, uv_keys) = if let Some(first) = ply_vertices.first() {
        let has_normals = first.get("nx").is_some();
        let uv_keys: Option<(&str, &str)> = if first.get("s").is_some() && first.get("t").is_some() {
            Some(("s", "t"))
        } else if first.get("u").is_some() && first.get("v").is_some() {
            Some(("u", "v"))
        } else if first.get("texture_u").is_some() && first.get("texture_v").is_some() {
            Some(("texture_u", "texture_v"))
        } else {
            None
        };
        (has_normals, uv_keys.is_some(), uv_keys)
    } else {
        (false, false, None)
    };

    let multi_tex_verts = ply.payload.get("multi_texture_vertex");
    let multi_tex_faces = ply.payload.get("multi_texture_face");
    let has_multi_tex =
        multi_tex_verts.is_some_and(|v| !v.is_empty()) && multi_tex_faces.is_some_and(|f| !f.is_empty());

    let multi_tex_uvs: Vec<[f32; 2]> = if has_multi_tex {
        multi_tex_verts
            .unwrap()
            .iter()
            .map(|elem| {
                [
                    elem.get("u").map(ply_prop_to_f32).unwrap_or(0.0),
                    elem.get("v").map(ply_prop_to_f32).unwrap_or(0.0),
                ]
            })
            .collect()
    } else {
        Vec::new()
    };

    let has_uvs = has_uvs || has_multi_tex;

    let mut vertices: Vec<model::ModelVertex> = Vec::with_capacity(ply_vertices.len());
    for elem in ply_vertices {
        let x = elem.get("x").map(ply_prop_to_f32).unwrap_or(0.0);
        let y = elem.get("y").map(ply_prop_to_f32).unwrap_or(0.0);
        let z = elem.get("z").map(ply_prop_to_f32).unwrap_or(0.0);

        let normal = if has_normals {
            [
                elem.get("nx").map(ply_prop_to_f32).unwrap_or(0.0),
                elem.get("ny").map(ply_prop_to_f32).unwrap_or(0.0),
                elem.get("nz").map(ply_prop_to_f32).unwrap_or(0.0),
            ]
        } else {
            [0.0, 0.0, 0.0]
        };

        let tex_coords = if let Some((u_key, v_key)) = uv_keys {
            [
                elem.get(u_key).map(ply_prop_to_f32).unwrap_or(0.0),
                elem.get(v_key).map(ply_prop_to_f32).unwrap_or(0.0),
            ]
        } else {
            [0.0, 0.0]
        };

        vertices.push(model::ModelVertex {
            position: [x, y, z],
            tex_coords,
            normal,
            tangent: [0.0; 3],
            bitangent: [0.0; 3],
        });
    }

    let mut indices: Vec<u32> = Vec::new();
    if has_multi_tex {
        let mt_faces = multi_tex_faces.unwrap();
        let geo_faces: Vec<Vec<u32>> = ply_faces
            .iter()
            .map(|face| {
                face.get("vertex_indices")
                    .or_else(|| face.get("vertex_index"))
                    .map(ply_prop_to_indices)
                    .unwrap_or_default()
            })
            .collect();

        let mut vert_map: std::collections::HashMap<(u32, u32), u32> = std::collections::HashMap::new();
        let mut final_vertices: Vec<model::ModelVertex> = Vec::new();

        for (fi, mt_face) in mt_faces.iter().enumerate() {
            let vis = if fi < geo_faces.len() { &geo_faces[fi] } else { continue };
            let tex_vis = mt_face
                .get("texture_vertex_indices")
                .or_else(|| mt_face.get("texture_vertex_index"))
                .map(ply_prop_to_indices)
                .unwrap_or_default();

            let mut resolved = Vec::with_capacity(vis.len());
            for (vi_idx, &pos_idx) in vis.iter().enumerate() {
                let uv_idx = tex_vis.get(vi_idx).copied().unwrap_or(0);
                let key = (pos_idx, uv_idx);
                let final_idx = *vert_map.entry(key).or_insert_with(|| {
                    let idx = final_vertices.len() as u32;
                    let mut v = vertices[pos_idx as usize];
                    if let Some(uv) = multi_tex_uvs.get(uv_idx as usize) {
                        v.tex_coords = *uv;
                    }
                    final_vertices.push(v);
                    idx
                });
                resolved.push(final_idx);
            }

            for i in 1..resolved.len().saturating_sub(1) {
                indices.push(resolved[0]);
                indices.push(resolved[i]);
                indices.push(resolved[i + 1]);
            }
        }

        vertices = final_vertices;
    } else {
        for face in ply_faces {
            let vis = face
                .get("vertex_indices")
                .or_else(|| face.get("vertex_index"))
                .map(ply_prop_to_indices)
                .unwrap_or_default();
            for i in 1..vis.len().saturating_sub(1) {
                indices.push(vis[0]);
                indices.push(vis[i] as u32);
                indices.push(vis[i + 1] as u32);
            }
        }
    }

    if !has_normals {
        for c in indices.chunks(3) {
            let p0: cgmath::Vector3<f32> = vertices[c[0] as usize].position.into();
            let p1: cgmath::Vector3<f32> = vertices[c[1] as usize].position.into();
            let p2: cgmath::Vector3<f32> = vertices[c[2] as usize].position.into();
            let face_normal = (p1 - p0).cross(p2 - p0);
            for &vi in c {
                let n = cgmath::Vector3::from(vertices[vi as usize].normal) + face_normal;
                vertices[vi as usize].normal = n.into();
            }
        }
        for v in &mut vertices {
            let n = cgmath::Vector3::from(v.normal);
            if n.magnitude() > 0.0 {
                v.normal = n.normalize().into();
            }
        }
    }

    if has_uvs {
        let mut triangles_included = vec![0u32; vertices.len()];
        for c in indices.chunks(3) {
            let v0 = vertices[c[0] as usize];
            let v1 = vertices[c[1] as usize];
            let v2 = vertices[c[2] as usize];

            let pos0: cgmath::Vector3<_> = v0.position.into();
            let pos1: cgmath::Vector3<_> = v1.position.into();
            let pos2: cgmath::Vector3<_> = v2.position.into();

            let uv0: cgmath::Vector2<_> = v0.tex_coords.into();
            let uv1: cgmath::Vector2<_> = v1.tex_coords.into();
            let uv2: cgmath::Vector2<_> = v2.tex_coords.into();

            let delta_pos1 = pos1 - pos0;
            let delta_pos2 = pos2 - pos0;
            let delta_uv1 = uv1 - uv0;
            let delta_uv2 = uv2 - uv0;

            let r = 1.0 / (delta_uv1.x * delta_uv2.y - delta_uv1.y * delta_uv2.x);
            let tangent = (delta_pos1 * delta_uv2.y - delta_pos2 * delta_uv1.y) * r;
            let bitangent = (delta_pos2 * delta_uv1.x - delta_pos1 * delta_uv2.x) * -r;

            vertices[c[0] as usize].tangent = (tangent + cgmath::Vector3::from(vertices[c[0] as usize].tangent)).into();
            vertices[c[1] as usize].tangent = (tangent + cgmath::Vector3::from(vertices[c[1] as usize].tangent)).into();
            vertices[c[2] as usize].tangent = (tangent + cgmath::Vector3::from(vertices[c[2] as usize].tangent)).into();
            vertices[c[0] as usize].bitangent =
                (bitangent + cgmath::Vector3::from(vertices[c[0] as usize].bitangent)).into();
            vertices[c[1] as usize].bitangent =
                (bitangent + cgmath::Vector3::from(vertices[c[1] as usize].bitangent)).into();
            vertices[c[2] as usize].bitangent =
                (bitangent + cgmath::Vector3::from(vertices[c[2] as usize].bitangent)).into();

            triangles_included[c[0] as usize] += 1;
            triangles_included[c[1] as usize] += 1;
            triangles_included[c[2] as usize] += 1;
        }
        for (i, n) in triangles_included.into_iter().enumerate() {
            if n > 0 {
                let denom = 1.0 / n as f32;
                let v = &mut vertices[i];
                v.tangent = (cgmath::Vector3::from(v.tangent) * denom).into();
                v.bitangent = (cgmath::Vector3::from(v.bitangent) * denom).into();
            }
        }
    }

    let mut global_min = [f32::INFINITY; 3];
    let mut global_max = [f32::NEG_INFINITY; 3];
    for v in &vertices {
        for i in 0..3 {
            global_min[i] = global_min[i].min(v.position[i]);
            global_max[i] = global_max[i].max(v.position[i]);
        }
    }
    for i in 0..3 {
        if global_min[i].is_infinite() {
            global_min[i] = -1.0;
            global_max[i] = 1.0;
        }
    }

    let mut vertex_lines: Vec<[f32; 3]> = Vec::new();
    let mut face_lines: Vec<[f32; 3]> = Vec::new();
    {
        let dx = global_max[0] - global_min[0];
        let dy = global_max[1] - global_min[1];
        let dz = global_max[2] - global_min[2];
        let mesh_diagonal = (dx * dx + dy * dy + dz * dz).sqrt();
        let scale = if mesh_diagonal > 1e-10 {
            mesh_diagonal * 0.05
        } else {
            0.1
        };

        for v in &vertices {
            let nx = v.normal[0] * scale;
            let ny = v.normal[1] * scale;
            let nz = v.normal[2] * scale;
            vertex_lines.push(v.position);
            vertex_lines.push([v.position[0] + nx, v.position[1] + ny, v.position[2] + nz]);
        }

        for c in indices.chunks(3) {
            let p0: cgmath::Vector3<f32> = vertices[c[0] as usize].position.into();
            let p1: cgmath::Vector3<f32> = vertices[c[1] as usize].position.into();
            let p2: cgmath::Vector3<f32> = vertices[c[2] as usize].position.into();
            let edge1 = p1 - p0;
            let edge2 = p2 - p0;
            let face_normal = edge1.cross(edge2);
            if face_normal.magnitude() > 1e-10 {
                let fn_norm = face_normal.normalize();
                let center = (p0 + p1 + p2) / 3.0;
                face_lines.push([center.x, center.y, center.z]);
                face_lines.push([
                    center.x + fn_norm.x * scale,
                    center.y + fn_norm.y * scale,
                    center.z + fn_norm.z * scale,
                ]);
            }
        }
    }

    let companion_tex = if has_uvs {
        find_companion_texture(file_path)
    } else {
        None
    };
    let diffuse = match &companion_tex {
        Some(tex_path) => load_texture(tex_path, false, device, queue).await.unwrap_or_else(|e| {
            eprintln!("Warning: Failed to load companion texture '{}': {}", tex_path, e);
            create_default_texture_colored(device, queue, [147, 132, 120, 255])
        }),
        None => create_default_texture_colored(device, queue, [147, 132, 120, 255]),
    };
    let normal = create_default_texture(device, queue, true);
    let mat_name = if companion_tex.is_some() {
        "ply_textured"
    } else {
        "clay_default"
    };
    let materials = vec![material::Material::new(device, mat_name, diffuse, normal, layout)];

    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(&format!("{:?} Vertex Buffer", file_path)),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(&format!("{:?} Index Buffer", file_path)),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    let bounds = model::AABB {
        min: cgmath::Point3::new(global_min[0], global_min[1], global_min[2]),
        max: cgmath::Point3::new(global_max[0], global_max[1], global_max[2]),
    };

    Ok((
        model::Model {
            meshes: vec![model::Mesh {
                name: file_path.to_string(),
                vertex_buffer,
                index_buffer,
                num_elements: indices.len() as u32,
                material: 0,
            }],
            materials,
            bounds,
        },
        model::NormalsGeometry {
            vertex_lines,
            face_lines,
        },
    ))
}

pub fn create_sphere_mesh(device: &wgpu::Device, radius: f32, rings: u32, segments: u32, name: &str) -> model::Mesh {
    use std::f32::consts::PI;

    let mut vertices = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for i in 0..=rings {
        let theta = PI * i as f32 / rings as f32;
        for j in 0..=segments {
            let phi = 2.0 * PI * j as f32 / segments as f32;
            let x = radius * theta.sin() * phi.cos();
            let y = radius * theta.cos();
            let z = radius * theta.sin() * phi.sin();
            vertices.push(model::ModelVertex {
                position: [x, y, z],
                tex_coords: [j as f32 / segments as f32, i as f32 / rings as f32],
                normal: [x / radius, y / radius, z / radius],
                tangent: [0.0; 3],
                bitangent: [0.0; 3],
            });
        }
    }

    for i in 0..rings {
        for j in 0..segments {
            let row = i * (segments + 1);
            let next_row = (i + 1) * (segments + 1);
            let a = row + j;
            let b = row + j + 1;
            let c = next_row + j;
            let d = next_row + j + 1;
            indices.extend_from_slice(&[a, b, d, a, d, c]);
        }
    }

    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(&format!("{} Vertex Buffer", name)),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(&format!("{} Index Buffer", name)),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    model::Mesh {
        name: name.to_string(),
        vertex_buffer,
        index_buffer,
        num_elements: indices.len() as u32,
        material: 0,
    }
}

pub fn create_floor_quad(device: &wgpu::Device, bounds: &model::AABB) -> model::Mesh {
    let y = bounds.min.y - 0.001;
    let he = bounds.diagonal() * 1.5;

    let vertices = [
        model::ModelVertex {
            position: [-he, y, -he],
            tex_coords: [0.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            tangent: [1.0, 0.0, 0.0],
            bitangent: [0.0, 0.0, 1.0],
        },
        model::ModelVertex {
            position: [he, y, -he],
            tex_coords: [1.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            tangent: [1.0, 0.0, 0.0],
            bitangent: [0.0, 0.0, 1.0],
        },
        model::ModelVertex {
            position: [he, y, he],
            tex_coords: [1.0, 1.0],
            normal: [0.0, 1.0, 0.0],
            tangent: [1.0, 0.0, 0.0],
            bitangent: [0.0, 0.0, 1.0],
        },
        model::ModelVertex {
            position: [-he, y, he],
            tex_coords: [0.0, 1.0],
            normal: [0.0, 1.0, 0.0],
            tangent: [1.0, 0.0, 0.0],
            bitangent: [0.0, 0.0, 1.0],
        },
    ];
    let indices: [u32; 6] = [0, 2, 1, 0, 3, 2];

    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Floor Vertex Buffer"),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Floor Index Buffer"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    model::Mesh {
        name: "floor".to_string(),
        vertex_buffer,
        index_buffer,
        num_elements: indices.len() as u32,
        material: 0,
    }
}

pub fn create_grid_quad(device: &wgpu::Device, bounds: &model::AABB) -> (model::Mesh, f32) {
    let y = bounds.min.y - 0.001;
    let he = bounds.diagonal() * 8.0;
    let cell_size = bounds.diagonal() * 0.15;

    let vertices: [[f32; 3]; 4] = [[-he, y, -he], [he, y, -he], [he, y, he], [-he, y, he]];
    let indices: [u32; 6] = [0, 2, 1, 0, 3, 2];

    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Grid Vertex Buffer"),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Grid Index Buffer"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    (
        model::Mesh {
            name: "grid".to_string(),
            vertex_buffer,
            index_buffer,
            num_elements: indices.len() as u32,
            material: 0,
        },
        cell_size,
    )
}

fn create_default_texture_colored(device: &wgpu::Device, queue: &wgpu::Queue, rgba: [u8; 4]) -> texture::Texture {
    let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(1, 1, image::Rgba(rgba)));
    texture::Texture::from_image(device, queue, &img, Some("default_texture"), false)
        .expect("Failed to create default texture")
}

fn create_default_texture(device: &wgpu::Device, queue: &wgpu::Queue, is_normal_map: bool) -> texture::Texture {
    let color = if is_normal_map {
        image::Rgba([128u8, 128, 255, 255])
    } else {
        image::Rgba([255u8, 255, 255, 255])
    };

    let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(1, 1, color));

    texture::Texture::from_image(device, queue, &img, Some("default_texture"), is_normal_map)
        .expect("Failed to create default texture")
}
