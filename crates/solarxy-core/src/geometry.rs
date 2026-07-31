//! Raw mesh + material data types ([`RawModelData`], [`RawMeshData`],
//! [`RawMaterialData`], [`RawImageData`]) and topology helpers
//! ([`compute_normals`], [`compute_tangent_basis`], [`extract_edges`],
//! [`compute_bounds`]).
//!
//! This is the type loaders in `solarxy-formats` produce and the renderer
//! consumes. Held briefly during model load, then transformed into GPU
//! resources in `solarxy-renderer/src/resources.rs` and dropped — these
//! types are not long-lived in steady state.

use std::collections::HashSet;
use std::path::PathBuf;

use cgmath::InnerSpace;

use crate::aabb::AABB;

/// How a mesh's vertices connect into primitives. `indices` are read as
/// triples for `Triangles` and pairs for `Lines`; `Points` ignores indices
/// entirely (every position is its own primitive). The tag rides the mesh
/// from loader output through the kernel to the renderer contract, so every
/// consumer interprets the same buffers the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MeshTopology {
    #[default]
    Triangles,
    Lines,
    Points,
}

/// One mesh inside a [`RawModelData`].
/// Indices interpreted per `topology` (triangle triples by default),
/// optional per-vertex `normals` / `tex_coords` / `colors`, and an
/// optional `material_index` into the parent [`RawModelData::materials`].
#[derive(Debug)]
pub struct RawMeshData {
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub normals: Option<Vec<[f32; 3]>>,
    pub tex_coords: Option<Vec<[f32; 2]>>,
    pub material_index: Option<usize>,
    pub topology: MeshTopology,
    /// Per-vertex RGBA colors, linear (loaders decode sRGB sources at
    /// import so every format agrees; glTF `COLOR_0` is already linear).
    pub colors: Option<Vec<[f32; 4]>>,
}

/// Decode one sRGB-encoded channel (0..1) to linear, the standard
/// piecewise transfer curve. Loaders apply it to sRGB color sources (PLY
/// vertex colors); the renderer's linear pipeline consumes the result.
#[must_use]
pub fn srgb_to_linear(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Encode one linear channel (0..1) to sRGB: the exact inverse of
/// [`srgb_to_linear`]. Exporters apply it when writing to formats whose
/// color convention is sRGB (PLY vertex colors), so an import/export pair
/// round-trips up to 8-bit quantization.
#[must_use]
pub fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Decoded image bytes (RGBA8) plus dimensions, ready for GPU upload.
///
/// Carries a content hash computed once at construction: the GPU texture
/// cache key and the merge-dedup identity. Construct via [`Self::new`]
/// (or [`Self::from_parts`] when a trusted hash travels with the bytes,
/// e.g. the kernel transfer codec) and treat instances as immutable —
/// images are shared behind `Arc` across materials, cook outputs, and the
/// renderer, and a mutated pixel buffer would silently invalidate the hash.
#[derive(Debug, Clone, PartialEq)]
pub struct RawImageData {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// FNV-1a over dimensions and pixel bytes; see [`Self::new`].
    pub hash: u64,
}

impl RawImageData {
    /// Build an image and stamp its content hash.
    #[must_use]
    pub fn new(pixels: Vec<u8>, width: u32, height: u32) -> Self {
        let hash = Self::content_hash(&pixels, width, height);
        Self {
            pixels,
            width,
            height,
            hash,
        }
    }

    /// Rebuild an image whose hash was computed earlier and traveled with
    /// the bytes (the transfer codec); callers must pass the hash exactly
    /// as [`Self::new`] produced it.
    #[must_use]
    pub fn from_parts(pixels: Vec<u8>, width: u32, height: u32, hash: u64) -> Self {
        Self {
            pixels,
            width,
            height,
            hash,
        }
    }

    /// FNV-1a 64 over `width`, `height`, then the pixel bytes. Stable
    /// across platforms and runs (unlike `DefaultHasher`), cheap enough to
    /// run once per decoded image, and 64 bits is plenty for a per-session
    /// texture-identity key.
    #[must_use]
    pub fn content_hash(pixels: &[u8], width: u32, height: u32) -> u64 {
        let mut h = Fnv1a::over_dimensions(width, height);
        h.eat(pixels);
        h.finish()
    }
}

/// FNV-1a 64, the content-hash primitive shared by [`RawImageData`] and
/// [`RawImageHdr`]. Stable across platforms and runs (unlike
/// `DefaultHasher`), which is what makes it usable as a texture-identity
/// and dedup key rather than only as a within-process hash.
struct Fnv1a(u64);

impl Fnv1a {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    /// Seed with the image dimensions, so two buffers with identical bytes
    /// but different shapes do not collide.
    fn over_dimensions(width: u32, height: u32) -> Self {
        let mut h = Self(Self::OFFSET);
        h.eat(&width.to_le_bytes());
        h.eat(&height.to_le_bytes());
        h
    }

    fn eat(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    fn finish(self) -> u64 {
        self.0
    }
}

/// Decoded high-dynamic-range image pixels plus dimensions.
///
/// **Three `f32` per pixel, row-major, linear RGB, no alpha**, so a pixel
/// at `(x, y)` occupies `pixels[(y * width + x) * 3 ..][..3]` and
/// `pixels.len() == width * height * 3`. The flat layout is what the
/// decoders produce natively and what GPU upload and the CPU-side
/// convolutions read through a cast, so nothing on the path copies to
/// reshape it.
///
/// The float sibling of [`RawImageData`], carrying the same content hash
/// stamped once at construction and the same immutability expectation:
/// instances are shared behind `Arc`, and a mutated pixel buffer would
/// silently invalidate the hash. `f32` rather than `f16` because CPU-side
/// consumers want the precision and the GPU upload converts once.
#[derive(Debug, Clone, PartialEq)]
pub struct RawImageHdr {
    pub pixels: Vec<f32>,
    pub width: u32,
    pub height: u32,
    /// FNV-1a over dimensions and pixel bits; see [`Self::new`].
    pub hash: u64,
}

impl RawImageHdr {
    /// Build an image and stamp its content hash.
    #[must_use]
    pub fn new(pixels: Vec<f32>, width: u32, height: u32) -> Self {
        debug_assert_eq!(
            pixels.len() as u64,
            u64::from(width) * u64::from(height) * 3,
            "RawImageHdr is three floats per pixel"
        );
        let hash = Self::content_hash(&pixels, width, height);
        Self {
            pixels,
            width,
            height,
            hash,
        }
    }

    /// Rebuild an image whose hash was computed earlier and traveled with
    /// the samples; callers must pass the hash exactly as [`Self::new`]
    /// produced it.
    #[must_use]
    pub fn from_parts(pixels: Vec<f32>, width: u32, height: u32, hash: u64) -> Self {
        Self {
            pixels,
            width,
            height,
            hash,
        }
    }

    /// FNV-1a 64 over `width`, `height`, then each sample's IEEE-754 bit
    /// pattern little-endian. Hashing the bits rather than the bytes of the
    /// backing allocation keeps the result identical on a big-endian target,
    /// which matters because this hash is a cache key that outlives the
    /// process that produced it.
    #[must_use]
    pub fn content_hash(pixels: &[f32], width: u32, height: u32) -> u64 {
        let mut h = Fnv1a::over_dimensions(width, height);
        for sample in pixels {
            h.eat(&sample.to_bits().to_le_bytes());
        }
        h.finish()
    }
}

/// PBR alpha-blending mode for [`RawMaterialData`].
/// Discriminants match the GPU wire format
/// (`solarxy-renderer::material::MaterialUniform.alpha_mode: u32`)
/// and the WGSL shaders. Conversion at the CPU↔GPU boundary in
/// `solarxy-renderer/src/resources.rs` via `From<AlphaMode> for u32`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AlphaMode {
    /// Fully opaque — no alpha test, no blending.
    #[default]
    Opaque = 0,
    /// Alpha-test cutoff at [`RawMaterialData::alpha_cutoff`].
    Mask = 1,
    /// Alpha-blended (BLEND), drawn in a separate pass.
    Blend = 2,
}

impl From<AlphaMode> for u32 {
    fn from(m: AlphaMode) -> u32 {
        m as u32
    }
}

/// The per-material shading model. `Pbr` is
/// the full Cook-Torrance metallic-roughness path and the default;
/// everything else is a stylized branch in `fs_main`. The discriminants
/// mirror the WGSL `switch material.shading_model` arms; conversion via
/// `From<ShadingModel> for u32` at the CPU-GPU boundary, exactly like
/// [`AlphaMode`]. `Matcap` samples the material's BASE COLOR texture slot
/// by view-space normal (a matcap material's base texture IS the matcap),
/// so no new texture role exists anywhere in the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum ShadingModel {
    /// Cook-Torrance metallic-roughness with IBL (the existing path).
    #[default]
    Pbr = 0,
    /// View-space-normal lookup into the base-color texture; unlit.
    Matcap = 1,
    /// Ramp-quantized diffuse with stepped specular
    /// ([`RawMaterialData::toon_steps`] bands).
    Toon = 2,
    /// Flat base color, no lighting (glTF `KHR_materials_unlit`).
    Unlit = 3,
    /// The promoted Clay viewport look (matte light gray).
    Clay = 4,
    /// The promoted Clay Dark look.
    ClayDark = 5,
    /// The promoted Chrome look (mirror metal, env-only).
    Chrome = 6,
    /// Solid black silhouette.
    Silhouette = 7,
}

impl From<ShadingModel> for u32 {
    fn from(m: ShadingModel) -> u32 {
        m as u32
    }
}

/// One PBR material inside a [`RawModelData`].
/// Holds factor scalars, optional textures (path or in-memory bytes), and the
/// PBR alpha mode. `solarxy-renderer/src/resources.rs` consumes this and
/// produces a `MaterialUniform` + GPU textures. `Default` is a convenience
/// for constructing a bare material (all factors zeroed, no textures); the
/// render path never relies on the default values.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RawMaterialData {
    pub name: String,
    pub diffuse_texture_path: Option<PathBuf>,
    pub normal_texture_path: Option<PathBuf>,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub diffuse_texture_data: Option<std::sync::Arc<RawImageData>>,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub normal_texture_data: Option<std::sync::Arc<RawImageData>>,
    pub metallic_roughness_texture_path: Option<PathBuf>,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub metallic_roughness_texture_data: Option<std::sync::Arc<RawImageData>>,
    pub occlusion_texture_path: Option<PathBuf>,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub occlusion_texture_data: Option<std::sync::Arc<RawImageData>>,
    pub emissive_texture_path: Option<PathBuf>,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub emissive_texture_data: Option<std::sync::Arc<RawImageData>>,
    pub roughness_factor: f32,
    pub metallic_factor: f32,
    /// glTF `occlusionTexture.strength`: how strongly the occlusion map
    /// attenuates ambient light (1.0 = full, 0.0 = none). Defaults to 1.0,
    /// the glTF default, so materials without an occlusion map and older
    /// files (which never carried the field) render unchanged.
    #[cfg_attr(feature = "serde", serde(default = "default_occlusion_strength"))]
    pub occlusion_strength: f32,
    pub emissive_factor: [f32; 3],
    /// Linear RGBA multiplied into the base-color texture sample (glTF's
    /// `baseColorFactor`); white when a map alone drives the channel.
    /// Defaults to white, the multiplicative identity (the one factor
    /// whose zero default would render everything black).
    #[cfg_attr(feature = "serde", serde(default = "white_rgba"))]
    pub base_color_factor: [f32; 4],
    pub alpha_mode: AlphaMode,
    pub alpha_cutoff: f32,
    pub ambient: Option<[f32; 3]>,
    pub diffuse: Option<[f32; 3]>,
    pub specular: Option<[f32; 3]>,
    pub shininess: Option<f32>,
    pub dissolve: Option<f32>,
    pub optical_density: Option<f32>,
    pub ambient_texture_name: Option<String>,
    pub diffuse_texture_name: Option<String>,
    pub specular_texture_name: Option<String>,
    pub normal_texture_name: Option<String>,
    pub shininess_texture_name: Option<String>,
    pub dissolve_texture_name: Option<String>,
    /// The per-material shading model. Defaults to `Pbr`, so
    /// every pre-existing material (imports, `.slxy`, the transfer codec's
    /// serde header) renders exactly as before.
    #[cfg_attr(feature = "serde", serde(default))]
    pub shading_model: ShadingModel,
    /// Toon band count (only read when `shading_model` is `Toon`).
    #[cfg_attr(feature = "serde", serde(default = "default_toon_steps"))]
    pub toon_steps: f32,
}

fn default_toon_steps() -> f32 {
    3.0
}

fn white_rgba() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}

#[cfg(feature = "serde")]
fn default_occlusion_strength() -> f32 {
    1.0
}

impl Default for RawMaterialData {
    /// Everything zeroed/empty EXCEPT `base_color_factor`, which defaults
    /// to white: it multiplies the base-color sample, so the identity is
    /// the only safe default (zero would render untextured materials
    /// black).
    fn default() -> Self {
        Self {
            name: String::new(),
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
            roughness_factor: 0.0,
            metallic_factor: 0.0,
            occlusion_strength: 1.0,
            emissive_factor: [0.0; 3],
            base_color_factor: white_rgba(),
            alpha_mode: AlphaMode::default(),
            alpha_cutoff: 0.0,
            ambient: None,
            diffuse: None,
            specular: None,
            shininess: None,
            dissolve: None,
            optical_density: None,
            shading_model: ShadingModel::default(),
            toon_steps: default_toon_steps(),
            ambient_texture_name: None,
            diffuse_texture_name: None,
            specular_texture_name: None,
            normal_texture_name: None,
            shininess_texture_name: None,
            dissolve_texture_name: None,
        }
    }
}

/// One loaded model — the unit `solarxy-formats` produces and
/// `solarxy-renderer::resources` consumes.
/// `polygon_count` is preserved from the source file (number of polygons
/// before triangulation), distinct from `meshes[i].indices.len() / 3`
/// which counts triangles after triangulation.
pub struct RawModelData {
    pub meshes: Vec<RawMeshData>,
    pub materials: Vec<RawMaterialData>,
    pub polygon_count: usize,
}

/// Computes per-vertex normals by accumulating face normals across all
/// triangles touching a vertex, then normalising.
/// Degenerate triangles contribute zero-magnitude face normals, which leave
/// affected vertices with NaN-or-zero normals. Validators flag these.
pub fn compute_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut normals = vec![[0.0f32; 3]; positions.len()];

    for c in indices.chunks(3) {
        let p0: cgmath::Vector3<f32> = positions[c[0] as usize].into();
        let p1: cgmath::Vector3<f32> = positions[c[1] as usize].into();
        let p2: cgmath::Vector3<f32> = positions[c[2] as usize].into();
        let face_normal = (p1 - p0).cross(p2 - p0);
        for &vi in c {
            let n = cgmath::Vector3::from(normals[vi as usize]) + face_normal;
            normals[vi as usize] = n.into();
        }
    }
    for n in &mut normals {
        let v = cgmath::Vector3::from(*n);
        if v.magnitude() > 0.0 {
            *n = v.normalize().into();
        }
    }
    normals
}

/// Computes per-vertex tangent + bitangent vectors from position deltas
/// scaled by UV deltas (the standard MikkT-adjacent derivation), averaged
/// across triangles touching a vertex.
/// `normals` is currently unused but kept in the signature for future
/// orthonormalisation work. Returns `(tangents, bitangents)`, both in the
/// same vertex order as `positions`.
pub fn compute_tangent_basis(
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    tex_coords: &[[f32; 2]],
    indices: &[u32],
) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let n = positions.len();
    let mut tangents = vec![[0.0f32; 3]; n];
    let mut bitangents = vec![[0.0f32; 3]; n];
    let mut triangles_included = vec![0u32; n];

    for c in indices.chunks(3) {
        let pos0: cgmath::Vector3<f32> = positions[c[0] as usize].into();
        let pos1: cgmath::Vector3<f32> = positions[c[1] as usize].into();
        let pos2: cgmath::Vector3<f32> = positions[c[2] as usize].into();

        let uv0: cgmath::Vector2<f32> = tex_coords[c[0] as usize].into();
        let uv1: cgmath::Vector2<f32> = tex_coords[c[1] as usize].into();
        let uv2: cgmath::Vector2<f32> = tex_coords[c[2] as usize].into();

        let delta_pos1 = pos1 - pos0;
        let delta_pos2 = pos2 - pos0;
        let delta_uv1 = uv1 - uv0;
        let delta_uv2 = uv2 - uv0;

        let r = 1.0 / (delta_uv1.x * delta_uv2.y - delta_uv1.y * delta_uv2.x);
        let tangent = (delta_pos1 * delta_uv2.y - delta_pos2 * delta_uv1.y) * r;
        let bitangent = (delta_pos2 * delta_uv1.x - delta_pos1 * delta_uv2.x) * -r;

        for &vi in c {
            let i = vi as usize;
            tangents[i] = (tangent + cgmath::Vector3::from(tangents[i])).into();
            bitangents[i] = (bitangent + cgmath::Vector3::from(bitangents[i])).into();
            triangles_included[i] += 1;
        }
    }

    for (i, count) in triangles_included.into_iter().enumerate() {
        if count > 0 {
            let denom = 1.0 / count as f32;
            tangents[i] = (cgmath::Vector3::from(tangents[i]) * denom).into();
            bitangents[i] = (cgmath::Vector3::from(bitangents[i]) * denom).into();
        }
    }

    let _ = normals;
    (tangents, bitangents)
}

/// Synthesises a tangent basis purely from per-vertex normals (no UVs).
/// Used as a fallback for meshes loaded without tex-coords.
pub fn compute_tangent_from_normal(normals: &[[f32; 3]]) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let mut tangents = Vec::with_capacity(normals.len());
    let mut bitangents = Vec::with_capacity(normals.len());
    for n in normals {
        let normal = cgmath::Vector3::from(*n);
        let up = if normal.y.abs() < 0.999 {
            cgmath::Vector3::new(0.0, 1.0, 0.0)
        } else {
            cgmath::Vector3::new(1.0, 0.0, 0.0)
        };
        let tangent = up.cross(normal).normalize();
        let bitangent = normal.cross(tangent);
        tangents.push(tangent.into());
        bitangents.push(bitangent.into());
    }
    (tangents, bitangents)
}

/// Tight axis-aligned bounding box around `positions`. Empty input returns
/// the unit cube at origin (avoids degenerate zero-volume bounds downstream).
pub fn compute_bounds(positions: &[[f32; 3]]) -> AABB {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];

    for p in positions {
        for i in 0..3 {
            min[i] = min[i].min(p[i]);
            max[i] = max[i].max(p[i]);
        }
    }

    for i in 0..3 {
        if min[i].is_infinite() {
            min[i] = -1.0;
            max[i] = 1.0;
        }
    }

    AABB {
        min: cgmath::Point3::new(min[0], min[1], min[2]),
        max: cgmath::Point3::new(max[0], max[1], max[2]),
    }
}

/// Deduplicated edge index pairs (`[v0, v1, v0, v1, …]`) for line-list
/// rendering. Each undirected edge appears once regardless of how many
/// triangles share it.
pub fn extract_edges(indices: &[u32]) -> Vec<u32> {
    let mut edge_set = HashSet::with_capacity(indices.len());
    for tri in indices.chunks(3) {
        if tri.len() < 3 {
            continue;
        }
        let (a, b, c) = (tri[0], tri[1], tri[2]);
        edge_set.insert((a.min(b), a.max(b)));
        edge_set.insert((b.min(c), b.max(c)));
        edge_set.insert((a.min(c), a.max(c)));
    }
    let mut result = Vec::with_capacity(edge_set.len() * 2);
    for (i0, i1) in edge_set {
        result.push(i0);
        result.push(i1);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Locks `AlphaMode` discriminants to the GPU wire format
    /// (`MaterialUniform.alpha_mode: u32` and the WGSL shaders).
    /// If anyone reorders variants, this test fails before reaching the GPU.
    #[test]
    fn alpha_mode_discriminants_match_wire_format() {
        assert_eq!(u32::from(AlphaMode::Opaque), 0);
        assert_eq!(u32::from(AlphaMode::Mask), 1);
        assert_eq!(u32::from(AlphaMode::Blend), 2);
        assert_eq!(u32::from(AlphaMode::default()), 0);
    }

    fn assert_vec3_approx(a: [f32; 3], b: [f32; 3], eps: f32) {
        assert!(
            (a[0] - b[0]).abs() < eps && (a[1] - b[1]).abs() < eps && (a[2] - b[2]).abs() < eps,
            "expected {:?} ≈ {:?}",
            a,
            b
        );
    }

    #[test]
    fn compute_normals_single_triangle() {
        let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let indices = [0u32, 1, 2];
        let normals = compute_normals(&positions, &indices);
        assert_eq!(normals.len(), 3);
        for n in &normals {
            assert_vec3_approx(*n, [0.0, 0.0, 1.0], 1e-6);
        }
    }

    #[test]
    fn compute_normals_degenerate() {
        let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        let indices = [0u32, 1, 2];
        let normals = compute_normals(&positions, &indices);
        assert_eq!(normals.len(), 3);
        for n in &normals {
            let mag = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            assert!(
                mag < 1e-6,
                "degenerate triangle normal should be near zero, got magnitude {}",
                mag
            );
        }
    }

    #[test]
    fn compute_bounds_single_point() {
        let positions = [[3.0, -1.0, 2.5]];
        let bounds = compute_bounds(&positions);
        assert!((bounds.min.x - 3.0).abs() < 1e-6);
        assert!((bounds.min.y - (-1.0)).abs() < 1e-6);
        assert!((bounds.min.z - 2.5).abs() < 1e-6);
        assert!((bounds.max.x - 3.0).abs() < 1e-6);
        assert!((bounds.max.y - (-1.0)).abs() < 1e-6);
        assert!((bounds.max.z - 2.5).abs() < 1e-6);
    }

    #[test]
    fn compute_bounds_cube() {
        let positions = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
        ];
        let bounds = compute_bounds(&positions);
        assert!((bounds.min.x - 0.0).abs() < 1e-6);
        assert!((bounds.min.y - 0.0).abs() < 1e-6);
        assert!((bounds.min.z - 0.0).abs() < 1e-6);
        assert!((bounds.max.x - 1.0).abs() < 1e-6);
        assert!((bounds.max.y - 1.0).abs() < 1e-6);
        assert!((bounds.max.z - 1.0).abs() < 1e-6);
    }

    #[test]
    fn compute_bounds_negative() {
        let positions = [[-5.0, -3.0, -1.0], [-2.0, -4.0, -6.0]];
        let bounds = compute_bounds(&positions);
        assert!((bounds.min.x - (-5.0)).abs() < 1e-6);
        assert!((bounds.min.y - (-4.0)).abs() < 1e-6);
        assert!((bounds.min.z - (-6.0)).abs() < 1e-6);
        assert!((bounds.max.x - (-2.0)).abs() < 1e-6);
        assert!((bounds.max.y - (-3.0)).abs() < 1e-6);
        assert!((bounds.max.z - (-1.0)).abs() < 1e-6);
    }

    #[test]
    fn compute_tangent_basis_unit_triangle() {
        let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let normals = [[0.0, 0.0, 1.0]; 3];
        let tex_coords = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        let indices = [0u32, 1, 2];
        let (tangents, _bitangents) =
            compute_tangent_basis(&positions, &normals, &tex_coords, &indices);
        assert_eq!(tangents.len(), 3);
        for t in &tangents {
            assert!(
                (t[0] - 1.0).abs() < 1e-5,
                "tangent X should be ~1.0, got {}",
                t[0]
            );
            assert!(t[1].abs() < 1e-5, "tangent Y should be ~0.0, got {}", t[1]);
            assert!(t[2].abs() < 1e-5, "tangent Z should be ~0.0, got {}", t[2]);
        }
    }

    #[test]
    fn compute_tangent_basis_perpendicular() {
        let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let normals_data = [[0.0, 0.0, 1.0]; 3];
        let tex_coords = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        let indices = [0u32, 1, 2];
        let (tangents, _) = compute_tangent_basis(&positions, &normals_data, &tex_coords, &indices);
        for (t, n) in tangents.iter().zip(normals_data.iter()) {
            let dot = t[0] * n[0] + t[1] * n[1] + t[2] * n[2];
            assert!(
                dot.abs() < 1e-5,
                "tangent dot normal should be ~0, got {}",
                dot
            );
        }
    }

    #[test]
    fn extract_edges_single_triangle() {
        let indices = vec![0u32, 1, 2];
        let edges = extract_edges(&indices);
        assert_eq!(edges.len(), 6);
        let edge_set: HashSet<(u32, u32)> = edges
            .chunks(2)
            .map(|e| (e[0].min(e[1]), e[0].max(e[1])))
            .collect();
        assert!(edge_set.contains(&(0, 1)));
        assert!(edge_set.contains(&(1, 2)));
        assert!(edge_set.contains(&(0, 2)));
    }

    #[test]
    fn extract_edges_shared_dedup() {
        let indices = vec![0, 1, 2, 1, 2, 3];
        let edges = extract_edges(&indices);
        let edge_set: HashSet<(u32, u32)> = edges
            .chunks(2)
            .map(|e| (e[0].min(e[1]), e[0].max(e[1])))
            .collect();
        assert_eq!(edge_set.len(), 5);
    }

    #[test]
    fn extract_edges_empty() {
        let edges = extract_edges(&[]);
        assert!(edges.is_empty());
    }

    #[test]
    fn compute_tangent_from_normal_orthogonality() {
        let normals = vec![
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
            [0.577, 0.577, 0.577],
        ];
        let (tangents, bitangents) = compute_tangent_from_normal(&normals);
        for (i, n) in normals.iter().enumerate() {
            let t = &tangents[i];
            let b = &bitangents[i];
            let dot_tn = t[0] * n[0] + t[1] * n[1] + t[2] * n[2];
            let dot_bn = b[0] * n[0] + b[1] * n[1] + b[2] * n[2];
            assert!(dot_tn.abs() < 1e-3, "tangent not perpendicular to normal");
            assert!(dot_bn.abs() < 1e-3, "bitangent not perpendicular to normal");
        }
    }

    #[test]
    fn compute_tangent_from_normal_up_facing() {
        let normals = vec![[0.0, 1.0, 0.0]];
        let (tangents, bitangents) = compute_tangent_from_normal(&normals);
        let t_mag =
            (tangents[0][0].powi(2) + tangents[0][1].powi(2) + tangents[0][2].powi(2)).sqrt();
        let b_mag =
            (bitangents[0][0].powi(2) + bitangents[0][1].powi(2) + bitangents[0][2].powi(2)).sqrt();
        assert!((t_mag - 1.0).abs() < 1e-6, "tangent should be unit length");
        assert!(
            (b_mag - 1.0).abs() < 1e-6,
            "bitangent should be unit length"
        );
    }

    #[test]
    fn compute_bounds_empty() {
        let bounds = compute_bounds(&[]);
        assert!((bounds.min.x - (-1.0)).abs() < 1e-6);
        assert!((bounds.max.x - 1.0).abs() < 1e-6);
    }

    #[test]
    fn compute_normals_shared_vertex() {
        let positions = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        let indices = [0, 1, 2, 0, 1, 3];
        let normals = compute_normals(&positions, &indices);
        let n0 = cgmath::Vector3::from(normals[0]);
        assert!(
            n0.magnitude() > 0.99,
            "shared vertex normal should be normalized"
        );
        assert!(normals[0][2] < 0.95, "should be tilted from pure Z");
    }

    #[test]
    fn compute_tangent_basis_degenerate_uvs() {
        let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let normals = [[0.0, 0.0, 1.0]; 3];
        let tex_coords = [[0.5, 0.5]; 3];
        let indices = [0u32, 1, 2];
        let (tangents, bitangents) =
            compute_tangent_basis(&positions, &normals, &tex_coords, &indices);
        assert_eq!(tangents.len(), 3);
        assert_eq!(bitangents.len(), 3);
    }

    fn hdr_2x1() -> Vec<f32> {
        vec![1.0, 0.5, 0.25, 8.0, 4.0, 2.0]
    }

    #[test]
    fn raw_image_hdr_stamps_its_content_hash() {
        let img = RawImageHdr::new(hdr_2x1(), 2, 1);
        assert_eq!(img.hash, RawImageHdr::content_hash(&hdr_2x1(), 2, 1));
        assert_eq!(img.pixels.len(), 6);
    }

    #[test]
    fn raw_image_hdr_from_parts_keeps_the_hash_it_was_given() {
        // The transfer path recomputes nothing: a hash that traveled with
        // the samples is taken on trust, exactly as the 8-bit type does.
        let img = RawImageHdr::from_parts(hdr_2x1(), 2, 1, 0xdead_beef);
        assert_eq!(img.hash, 0xdead_beef);
    }

    #[test]
    fn raw_image_hdr_hash_separates_shape_from_samples() {
        // Same six samples, transposed dimensions. Seeding the accumulator
        // with the dimensions is what keeps these apart.
        let wide = RawImageHdr::content_hash(&hdr_2x1(), 2, 1);
        let tall = RawImageHdr::content_hash(&hdr_2x1(), 1, 2);
        assert_ne!(wide, tall);

        let mut altered = hdr_2x1();
        altered[4] = 4.5;
        assert_ne!(wide, RawImageHdr::content_hash(&altered, 2, 1));
    }

    #[test]
    fn raw_image_hashes_stay_pinned_to_their_published_values() {
        // Both hashes are cache keys that outlive the process that made
        // them, so a refactor of the shared accumulator must not move
        // them. These literals are the values produced before the two
        // types began sharing one FNV-1a implementation.
        assert_eq!(
            RawImageData::content_hash(&[1u8, 2, 3, 4], 1, 1),
            0xef73_dc3a_80c8_da6d
        );
        assert_eq!(
            RawImageHdr::content_hash(&[1.0f32, 0.5, 0.25], 1, 1),
            0xc739_37a6_695b_27eb
        );
    }
}
