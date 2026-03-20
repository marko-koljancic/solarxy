use std::io::{BufReader, Cursor};
use tobj::LoadError;
use wgpu::util::DeviceExt;
use cgmath::InnerSpace;
use super::{material, texture, model};

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
) -> anyhow::Result<model::Model> {
    println!("Loading model from path: {}", file_path);
    let obj_text = load_string(file_path).await?;
    let obj_cursor = Cursor::new(obj_text);
    let mut obj_reader = BufReader::new(obj_cursor);

    let obj_dir = std::path::Path::new(file_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());

    // TODO: Fix use of depricated function
    let (models, obj_materials) = tobj::load_obj_buf_async(
        &mut obj_reader,
        &tobj::LoadOptions {
            triangulate: true,
            single_index: true,
            ..Default::default()
        },
        |p| {
            let value = obj_dir.clone();
            async move {
                let mat_path = std::path::Path::new(&value).join(&p);
                let mat_text = load_string(&mat_path.to_string_lossy())
                    .await
                    .map_err(|_| LoadError::ReadError)?;
                tobj::load_mtl_buf(&mut BufReader::new(Cursor::new(mat_text)))
            }
        },
    )
    .await?;

    let mut materials = Vec::new();
    for m in obj_materials.unwrap_or_default() {
        println!("Material: {:#?}", m);

        let diffuse_path = m
            .diffuse_texture
            .as_deref()
            .map(|p| std::path::Path::new(&obj_dir).join(p).to_string_lossy().to_string());

        let normal_path = m.normal_texture.as_deref().map(|p| {
            // Strip out map_Bump parameters like "-bm 1.000000"
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

    let mut meshes = Vec::new();
    for m in models {
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
                normal: [0.0, 0.0, 0.0],  // filled below if missing

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

            vertices[c[0] as usize].tangent =
                (tangent + cgmath::Vector3::from(vertices[c[0] as usize].tangent)).into();
            vertices[c[1] as usize].tangent =
                (tangent + cgmath::Vector3::from(vertices[c[1] as usize].tangent)).into();
            vertices[c[2] as usize].tangent =
                (tangent + cgmath::Vector3::from(vertices[c[2] as usize].tangent)).into();
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

        // Accumulate bounding box
        for v in &vertices {
            for i in 0..3 {
                global_min[i] = global_min[i].min(v.position[i]);
                global_max[i] = global_max[i].max(v.position[i]);
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

    // Guard against empty models
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

    Ok(model::Model { meshes, materials, bounds })
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
