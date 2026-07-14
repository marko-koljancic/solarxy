//! glTF/GLB loading. `load_gltf_bytes` parses from memory: GLB and
//! data-URI assets are self-contained; external `.bin` buffers AND external
//! image files resolve through the [`AssetResolver`] (images decode via the
//! shared [`crate::decode_image_bytes`] path; a missing or undecodable image
//! degrades to the default texture with a warning, it never fails the
//! model). `load_gltf` (std-fs) keeps the crate's full filesystem importer.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cgmath::{InnerSpace, Matrix as _, Matrix3, Matrix4, SquareMatrix, Vector3, Vector4};

use crate::{AssetResolver, FormatsError};
use solarxy_core::{AlphaMode, RawImageData, RawMaterialData, RawMeshData, RawModelData};

/// Load a glTF or GLB from disk via the gltf crate's importer (buffers and
/// images resolved from the file's directory).
#[cfg(feature = "std-fs")]
pub fn load_gltf(file_path: &str) -> Result<RawModelData, FormatsError> {
    // Read the file and resolve external buffers/images through the byte path's
    // hardened `DirResolver`, rather than `::gltf::import`, which resolves a
    // glTF's `uri` references relative to the file with no `..`/absolute
    // rejection -- the same arbitrary-file-read the OBJ loader had. Routing
    // through `load_gltf_bytes` closes it and puts desktop on the exact loader
    // the web app already uses.
    let bytes = std::fs::read(file_path).map_err(|source| FormatsError::Io {
        path: file_path.to_string(),
        source,
    })?;
    let parent = Path::new(file_path)
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let mut resolver = crate::DirResolver::new(parent);
    load_gltf_bytes(&bytes, &mut resolver)
}

/// Parse glTF/GLB bytes. External buffers and external images come from
/// `resolver` (URIs percent-decoded first). A missing buffer is an error; a
/// missing or undecodable image is a warning and its texture slot stays
/// empty (the renderer falls back to the per-role default).
pub fn load_gltf_bytes(
    bytes: &[u8],
    resolver: &mut dyn AssetResolver,
) -> Result<RawModelData, FormatsError> {
    let ::gltf::Gltf { document, blob } = ::gltf::Gltf::from_slice(bytes)?;
    let mut blob = blob;

    let mut buffers: Vec<::gltf::buffer::Data> = Vec::new();
    for buffer in document.buffers() {
        let data = match buffer.source() {
            ::gltf::buffer::Source::Uri(uri) if !uri.starts_with("data:") => {
                let rel = percent_decode(uri);
                let mut raw = resolver
                    .read(&rel)
                    .ok_or_else(|| FormatsError::MissingAsset(rel.clone()))?;
                if raw.len() < buffer.length() {
                    return Err(FormatsError::Invalid(format!(
                        "glTF buffer '{rel}' is {} bytes, expected at least {}",
                        raw.len(),
                        buffer.length()
                    )));
                }
                // The gltf importer pads to 4-byte alignment; match it.
                while raw.len() % 4 != 0 {
                    raw.push(0);
                }
                ::gltf::buffer::Data(raw)
            }
            source => ::gltf::buffer::Data::from_source_and_blob(source, None, &mut blob)?,
        };
        buffers.push(data);
    }

    // Per-image decode: buffer-view images go through the gltf crate's
    // decoder; data-URI and external-file images decode via the shared
    // image path (external bytes read through the resolver). Either way a
    // failure leaves the slot None (path still recorded by
    // resolve_texture; renderer default texture takes over) rather than
    // failing the whole model.
    let images: Vec<Option<Arc<RawImageData>>> = document
        .images()
        .map(|img| match img.source() {
            ::gltf::image::Source::Uri { uri, .. } => {
                let bytes = if uri.starts_with("data:") {
                    let Some(bytes) = decode_data_uri(uri) else {
                        tracing::warn!("glTF data-URI image failed base64 decode");
                        return None;
                    };
                    bytes
                } else {
                    let rel = percent_decode(uri);
                    let Some(bytes) = resolver.read(&rel) else {
                        tracing::warn!("glTF image '{rel}' not provided; using default texture");
                        return None;
                    };
                    bytes
                };
                match crate::decode_image_bytes(&bytes) {
                    Ok(data) => Some(Arc::new(data)),
                    Err(e) => {
                        tracing::warn!("glTF image failed to decode: {e}");
                        None
                    }
                }
            }
            source @ ::gltf::image::Source::View { .. } => {
                ::gltf::image::Data::from_source(source, None, &buffers)
                    .ok()
                    .as_ref()
                    .and_then(image_data_to_raw)
                    .map(Arc::new)
            }
        })
        .collect();

    Ok(build_model(&document, &buffers, &images, None))
}

/// Percent-decode a glTF URI the same way the gltf importer does for
/// relative references ("my%20buffer.bin" names a file "my buffer.bin").
fn percent_decode(uri: &str) -> String {
    urlencoding::decode(uri).map_or_else(|_| uri.to_string(), std::borrow::Cow::into_owned)
}

/// Extract the payload of a `data:<mime>;base64,<payload>` URI. glTF
/// exporters emit base64 exclusively; a percent-encoded data URI returns
/// `None` and degrades like an undecodable image.
fn decode_data_uri(uri: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let (meta, payload) = uri.split_once(',')?;
    if !meta.ends_with(";base64") {
        return None;
    }
    base64::engine::general_purpose::STANDARD
        .decode(payload)
        .ok()
}

fn build_model(
    document: &::gltf::Document,
    buffers: &[::gltf::buffer::Data],
    images: &[Option<Arc<RawImageData>>],
    texture_base: Option<&Path>,
) -> RawModelData {
    let mut materials = extract_materials(document, images, texture_base);
    let (meshes, polygon_count) = extract_meshes(document, buffers);

    if materials.is_empty() && meshes.iter().any(|m| m.material_index.is_some()) {
        materials.push(RawMaterialData {
            name: "gltf_default".to_string(),
            diffuse_texture_path: None,
            normal_texture_path: None,
            diffuse_texture_data: None,
            normal_texture_data: None,
            metallic_roughness_texture_path: None,
            metallic_roughness_texture_data: None,
            occlusion_texture_path: None,
            occlusion_texture_data: None,
            emissive_texture_path: None,
            emissive_texture_data: None,
            roughness_factor: 0.5,
            metallic_factor: 0.0,
            emissive_factor: [0.0, 0.0, 0.0],
            base_color_factor: [1.0, 1.0, 1.0, 1.0],
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            ambient: None,
            diffuse: None,
            specular: None,
            shininess: None,
            dissolve: None,
            optical_density: None,
            ambient_texture_name: None,
            diffuse_texture_name: None,
            specular_texture_name: None,
            normal_texture_name: None,
            shininess_texture_name: None,
            dissolve_texture_name: None,
        });
    }

    RawModelData {
        meshes,
        materials,
        polygon_count,
    }
}

fn extract_materials(
    document: &::gltf::Document,
    images: &[Option<Arc<RawImageData>>],
    texture_base: Option<&Path>,
) -> Vec<RawMaterialData> {
    document
        .materials()
        .map(|mat| {
            let pbr = mat.pbr_metallic_roughness();

            let (diffuse_path, diffuse_data) = match pbr.base_color_texture() {
                Some(info) => resolve_texture(&info.texture(), images, texture_base),
                None => (None, None),
            };

            let (normal_path, normal_data) = match mat.normal_texture() {
                Some(info) => resolve_texture(&info.texture(), images, texture_base),
                None => (None, None),
            };

            let (mr_path, mr_data) = match pbr.metallic_roughness_texture() {
                Some(info) => resolve_texture(&info.texture(), images, texture_base),
                None => (None, None),
            };

            let (occ_path, occ_data) = match mat.occlusion_texture() {
                Some(info) => resolve_texture(&info.texture(), images, texture_base),
                None => (None, None),
            };

            let (emissive_path, emissive_data) = match mat.emissive_texture() {
                Some(info) => resolve_texture(&info.texture(), images, texture_base),
                None => (None, None),
            };

            let emissive_factor = mat.emissive_factor();

            let alpha_mode = match mat.alpha_mode() {
                ::gltf::material::AlphaMode::Opaque => AlphaMode::Opaque,
                ::gltf::material::AlphaMode::Mask => AlphaMode::Mask,
                ::gltf::material::AlphaMode::Blend => AlphaMode::Blend,
            };
            let alpha_cutoff = mat.alpha_cutoff().unwrap_or(0.5);

            let base_color = pbr.base_color_factor();

            RawMaterialData {
                name: mat.name().unwrap_or("gltf_material").to_string(),
                diffuse_texture_path: diffuse_path,
                normal_texture_path: normal_path,
                diffuse_texture_data: diffuse_data,
                normal_texture_data: normal_data,
                metallic_roughness_texture_path: mr_path,
                metallic_roughness_texture_data: mr_data,
                occlusion_texture_path: occ_path,
                occlusion_texture_data: occ_data,
                emissive_texture_path: emissive_path,
                emissive_texture_data: emissive_data,
                roughness_factor: pbr.roughness_factor(),
                metallic_factor: pbr.metallic_factor(),
                emissive_factor,
                base_color_factor: base_color,
                alpha_mode,
                alpha_cutoff,
                ambient: None,
                diffuse: Some([base_color[0], base_color[1], base_color[2]]),
                specular: None,
                shininess: None,
                dissolve: Some(base_color[3]),
                optical_density: None,
                ambient_texture_name: None,
                diffuse_texture_name: pbr
                    .base_color_texture()
                    .map(|t| format!("texture_index:{}", t.texture().source().index())),
                specular_texture_name: None,
                normal_texture_name: mat
                    .normal_texture()
                    .map(|t| format!("texture_index:{}", t.texture().source().index())),
                shininess_texture_name: None,
                dissolve_texture_name: None,
            }
        })
        .collect()
}

fn resolve_texture(
    texture: &::gltf::Texture,
    images: &[Option<Arc<RawImageData>>],
    texture_base: Option<&Path>,
) -> (Option<PathBuf>, Option<Arc<RawImageData>>) {
    let image = texture.source();
    let decoded = images.get(image.index()).and_then(Option::as_ref).cloned();

    match image.source() {
        ::gltf::image::Source::Uri { uri, .. } => {
            if uri.starts_with("data:") {
                (None, decoded)
            } else {
                let path = texture_base.map_or_else(|| PathBuf::from(uri), |base| base.join(uri));
                (Some(path), decoded)
            }
        }
        ::gltf::image::Source::View { .. } => (None, decoded),
    }
}

#[allow(clippy::unnecessary_wraps)]
fn image_data_to_raw(img: &::gltf::image::Data) -> Option<RawImageData> {
    let pixels = match img.format {
        ::gltf::image::Format::R8G8B8A8 => img.pixels.clone(),
        ::gltf::image::Format::R8G8B8 => img
            .pixels
            .chunks_exact(3)
            .flat_map(|rgb| [rgb[0], rgb[1], rgb[2], 255])
            .collect(),
        ::gltf::image::Format::R16G16B16A16 => img
            .pixels
            .chunks_exact(8)
            .flat_map(|c| [c[0], c[2], c[4], c[6]])
            .collect(),
        ::gltf::image::Format::R16G16B16 => img
            .pixels
            .chunks_exact(6)
            .flat_map(|c| [c[0], c[2], c[4], 255])
            .collect(),
        ::gltf::image::Format::R8 => img.pixels.iter().flat_map(|&r| [r, r, r, 255]).collect(),
        ::gltf::image::Format::R16 => img
            .pixels
            .chunks_exact(2)
            .flat_map(|c| [c[0], c[0], c[0], 255])
            .collect(),
        ::gltf::image::Format::R8G8 => img
            .pixels
            .chunks_exact(2)
            .flat_map(|rg| [rg[0], rg[1], 0, 255])
            .collect(),
        ::gltf::image::Format::R16G16 => img
            .pixels
            .chunks_exact(4)
            .flat_map(|c| [c[0], c[2], 0, 255])
            .collect(),
        ::gltf::image::Format::R32G32B32A32FLOAT => img
            .pixels
            .chunks_exact(16)
            .flat_map(|c| {
                let r = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                let g = f32::from_le_bytes([c[4], c[5], c[6], c[7]]);
                let b = f32::from_le_bytes([c[8], c[9], c[10], c[11]]);
                let a = f32::from_le_bytes([c[12], c[13], c[14], c[15]]);
                [
                    (r.clamp(0.0, 1.0) * 255.0) as u8,
                    (g.clamp(0.0, 1.0) * 255.0) as u8,
                    (b.clamp(0.0, 1.0) * 255.0) as u8,
                    (a.clamp(0.0, 1.0) * 255.0) as u8,
                ]
            })
            .collect(),
        ::gltf::image::Format::R32G32B32FLOAT => img
            .pixels
            .chunks_exact(12)
            .flat_map(|c| {
                let r = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                let g = f32::from_le_bytes([c[4], c[5], c[6], c[7]]);
                let b = f32::from_le_bytes([c[8], c[9], c[10], c[11]]);
                [
                    (r.clamp(0.0, 1.0) * 255.0) as u8,
                    (g.clamp(0.0, 1.0) * 255.0) as u8,
                    (b.clamp(0.0, 1.0) * 255.0) as u8,
                    255,
                ]
            })
            .collect(),
    };

    Some(RawImageData::new(pixels, img.width, img.height))
}

fn extract_meshes(
    document: &::gltf::Document,
    buffers: &[::gltf::buffer::Data],
) -> (Vec<RawMeshData>, usize) {
    let mut meshes = Vec::new();
    let mut total_polygons = 0usize;

    for scene in document.scenes() {
        for node in scene.nodes() {
            collect_meshes_recursive(
                &node,
                Matrix4::identity(),
                buffers,
                &mut meshes,
                &mut total_polygons,
            );
        }
    }

    (meshes, total_polygons)
}

fn collect_meshes_recursive(
    node: &::gltf::Node,
    parent_transform: Matrix4<f32>,
    buffers: &[::gltf::buffer::Data],
    meshes: &mut Vec<RawMeshData>,
    total_polygons: &mut usize,
) {
    let local: [[f32; 4]; 4] = node.transform().matrix();
    let local_mat = Matrix4::from(local);
    let world_transform = parent_transform * local_mat;

    if let Some(mesh) = node.mesh() {
        for primitive in mesh.primitives() {
            if primitive.mode() != ::gltf::mesh::Mode::Triangles {
                tracing::warn!(
                    "Skipping non-triangle primitive in mesh '{}' (mode: {:?})",
                    mesh.name().unwrap_or("unnamed"),
                    primitive.mode()
                );
                continue;
            }

            let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));

            let positions: Vec<[f32; 3]> = match reader.read_positions() {
                Some(iter) => iter
                    .map(|p| {
                        let v = world_transform * Vector4::new(p[0], p[1], p[2], 1.0);
                        [v.x, v.y, v.z]
                    })
                    .collect(),
                None => continue,
            };

            let indices: Vec<u32> = match reader.read_indices() {
                Some(iter) => iter.into_u32().collect(),
                None => (0..positions.len() as u32).collect(),
            };

            let normals: Option<Vec<[f32; 3]>> = reader.read_normals().map(|iter| {
                let normal_matrix = extract_normal_matrix(&world_transform);
                iter.map(|n| {
                    let v = normal_matrix * Vector3::new(n[0], n[1], n[2]);
                    let len = v.magnitude();
                    if len > 1e-10 {
                        [v.x / len, v.y / len, v.z / len]
                    } else {
                        n
                    }
                })
                .collect()
            });

            let tex_coords: Option<Vec<[f32; 2]>> = reader
                .read_tex_coords(0)
                .map(|iter| iter.into_f32().collect());

            let material_index = Some(primitive.material().index().unwrap_or(0));

            *total_polygons += indices.len() / 3;

            meshes.push(RawMeshData {
                name: mesh.name().unwrap_or("gltf_mesh").to_string(),
                positions,
                indices,
                normals,
                tex_coords,
                material_index,
            });
        }
    }

    for child in node.children() {
        collect_meshes_recursive(&child, world_transform, buffers, meshes, total_polygons);
    }
}

fn extract_normal_matrix(transform: &Matrix4<f32>) -> Matrix3<f32> {
    let upper3x3 = Matrix3::new(
        transform.x.x,
        transform.x.y,
        transform.x.z,
        transform.y.x,
        transform.y.y,
        transform.y.z,
        transform.z.x,
        transform.z.y,
        transform.z.z,
    );
    upper3x3.invert().unwrap_or(Matrix3::identity()).transpose()
}
