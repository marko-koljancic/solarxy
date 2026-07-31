//! Template instancing onto points (the `copy_to_points` node's kernel).
//!
//! v1 is CPU bake-and-merge on the array precedent, with one deliberate
//! twist: instead of one output mesh per copy (which would hand the
//! renderer one draw object per point), the copies of each template mesh
//! flatten into a single concatenated mesh, so a 10k-point copy of a
//! one-mesh template stays one draw object. Geometry, materials, and
//! attribute lanes are identical to what a per-copy merge would produce;
//! only the mesh partitioning differs.
//!
//! Each copy's placement is translate to the point, optionally rotate the
//! template's +Y onto the point normal (the reserved `N` lane first, the
//! mesh normal buffer as fallback), and scale uniformly with seeded
//! per-point variance. Uniform positive scale means normals transform by
//! the rotation alone, no inverse-transpose needed.

use std::sync::Arc;

use cgmath::{InnerSpace, Matrix3, Rad, SquareMatrix, Vector3};

use crate::array::MAX_OUTPUT_PRIMITIVES;
use crate::rng;
use crate::set::{AttributeData, AttributeMap, GeometrySet, KernelMesh, reserved};

/// Whether copies become real geometry or placements of one prototype.
///
/// The distinction Houdini draws between a copy and an instance, and it
/// is a representation choice rather than a rendering one: both modes
/// produce the same image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CopyMode {
    /// One prototype plus a transform per placement. The renderer issues
    /// one instanced draw, so ten thousand copies of a five-thousand
    /// triangle rock cost five thousand triangles of memory rather than
    /// fifty million.
    ///
    /// The cost is that downstream operations see the prototype and the
    /// placements, never the individual copies: there is nothing to
    /// select, wrangle, or delete per copy, because the copies do not
    /// exist as geometry.
    #[default]
    Instance,
    /// Every copy baked into real vertices. What you choose when you need
    /// to edit the copies afterwards, and the only mode that existed
    /// before instancing.
    Bake,
}

/// How each copy orients at its target point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CopyOrient {
    /// Keep the template's orientation everywhere.
    None,
    /// Rotate the template's +Y axis onto the point's normal, when the
    /// point has one; points without a normal keep the template
    /// orientation.
    #[default]
    Normal,
}

/// Copies `template` onto every point of `points` (every vertex of every
/// mesh, whatever its topology; scatter output is the canonical source).
/// Deterministic in `seed`.
///
/// # Errors
/// Returns a user-facing message when the projected output would exceed
/// the [`MAX_OUTPUT_PRIMITIVES`] ceiling (counting primitives and
/// vertices, whichever projects larger), before any copy is allocated.
pub fn copy_to_points(
    template: &GeometrySet,
    points: &GeometrySet,
    orient: CopyOrient,
    scale: f32,
    scale_variance: f32,
    seed: u32,
    mode: CopyMode,
) -> Result<GeometrySet, String> {
    let targets = gather_targets(points);
    if targets.is_empty() || template.meshes.is_empty() {
        return Ok(GeometrySet::empty());
    }

    let template_prims: usize = template
        .meshes
        .iter()
        .map(KernelMesh::primitive_count)
        .sum();
    let template_verts: usize = template.meshes.iter().map(KernelMesh::vertex_count).sum();
    let per_copy = template_prims.max(template_verts);
    // The ceiling counts what the mode actually allocates: baked output in
    // Bake, the prototype once in Instance. So the same scatter that
    // refuses to bake places happily as instances, which is the whole
    // point of having the modes.
    let projected = match mode {
        CopyMode::Bake => per_copy.saturating_mul(targets.len()),
        CopyMode::Instance => per_copy,
    };
    if projected > MAX_OUTPUT_PRIMITIVES {
        return Err(ceiling_message("copy_to_points", projected, mode));
    }

    // Per-point placement, shared by every template mesh so a multi-mesh
    // template stays rigid within each copy.
    let placements: Vec<Placement> = targets
        .iter()
        .enumerate()
        .map(|(i, target)| {
            let rotation = match (orient, target.normal) {
                (CopyOrient::Normal, Some(n)) => rotation_from_y(n),
                _ => Matrix3::identity(),
            };
            let factor = 1.0 + (2.0 * rng::unit_f32(i as u64, 0, seed) - 1.0) * scale_variance;
            // `pscale` MULTIPLIES rather than replaces. The node's own Scale
            // stays a global dial and the lane varies around it, so a wrangle
            // can drive per-point size without making the parameter above it
            // dead. A negative value would mirror the copy inside out, so it
            // clamps at zero.
            let per_point = target.pscale.map_or(1.0, |v| v.max(0.0));
            Placement {
                translate: target.position,
                rotation,
                scale: scale * factor * per_point,
            }
        })
        .collect();

    match mode {
        CopyMode::Bake => {
            let meshes = template
                .meshes
                .iter()
                .map(|mesh| flatten_copies(mesh, &placements))
                .collect();
            Ok(GeometrySet::from_parts(meshes, template.materials.clone()))
        }
        // The prototype travels untouched and the placements become
        // matrices. Note what is absent: `flatten_copies` never runs, so
        // no vertex is allocated per copy at all.
        CopyMode::Instance => Ok(GeometrySet::from_parts_instanced(
            template.meshes.clone(),
            template.materials.clone(),
            placements.iter().map(Placement::to_instance).collect(),
        )),
    }
}

/// The over-the-ceiling message, naming the active mode and the way out.
///
/// "Lower the point count" is the wrong advice when switching modes would
/// have solved it, so the message says which mode is running and what the
/// other one would do.
pub(crate) fn ceiling_message(op: &str, projected: usize, mode: CopyMode) -> String {
    match mode {
        CopyMode::Bake => format!(
            "{op} in Bake mode would produce {projected} primitives (over the \
             {MAX_OUTPUT_PRIMITIVES} ceiling). Switch the node to Instance mode to place \
             the same copies without allocating them, or lower the count."
        ),
        CopyMode::Instance => format!(
            "{op} in Instance mode would produce {projected} primitives (over the \
             {MAX_OUTPUT_PRIMITIVES} ceiling). Instance mode already allocates the \
             prototype only once, so the prototype itself is too heavy: simplify it."
        ),
    }
}

struct Target {
    position: [f32; 3],
    normal: Option<[f32; 3]>,
    /// The reserved `pscale` lane's value at this point, or `None` when the
    /// input does not carry one.
    pscale: Option<f32>,
}

struct Placement {
    translate: [f32; 3],
    rotation: Matrix3<f32>,
    scale: f32,
}

impl Placement {
    /// This placement as a column-major model matrix.
    ///
    /// The same composition `flatten_copies` applies vertex by vertex
    /// (scale, then rotate, then translate), so Instance mode and Bake
    /// mode put every copy in exactly the same place. A kernel test
    /// asserts that rather than trusting it.
    fn to_instance(&self) -> solarxy_core::scene::InstanceXform {
        let r = self.rotation * self.scale;
        solarxy_core::scene::InstanceXform([
            [r.x.x, r.x.y, r.x.z, 0.0],
            [r.y.x, r.y.y, r.y.z, 0.0],
            [r.z.x, r.z.y, r.z.z, 0.0],
            [self.translate[0], self.translate[1], self.translate[2], 1.0],
        ])
    }
}

/// Every vertex of every mesh in the points input, with its normal from
/// the reserved `N` lane first (what scatter writes) and the mesh normal
/// buffer as fallback.
fn gather_targets(points: &GeometrySet) -> Vec<Target> {
    let mut targets = Vec::new();
    for mesh in &points.meshes {
        let lane = match mesh.attributes.get(reserved::NORMAL) {
            Some(AttributeData::Vec3(v)) if v.len() == mesh.positions.len() => Some(v.as_slice()),
            _ => None,
        };
        let fallback = mesh
            .normals
            .as_ref()
            .filter(|buf| buf.len() == mesh.positions.len())
            .map(|buf| buf.as_slice());
        // The reserved per-point scale. Reserved since 0.8.0 with no
        // producer; the attribute wrangle is the first thing that can author
        // it, which is what makes consuming it useful now.
        let pscale = match mesh.attributes.get(reserved::PSCALE) {
            Some(AttributeData::Float(v)) if v.len() == mesh.positions.len() => Some(v.as_slice()),
            _ => None,
        };
        for (i, position) in mesh.positions.iter().enumerate() {
            let normal = lane.or(fallback).map(|buf| buf[i]);
            targets.push(Target {
                position: *position,
                normal,
                pscale: pscale.map(|buf| buf[i]),
            });
        }
    }
    targets
}

/// All copies of one template mesh concatenated into a single mesh:
/// positions and normals transformed per placement, indices offset per
/// copy, UVs and attribute lanes tiled verbatim (matching
/// `bake_transform`, which also leaves attribute lanes untransformed).
fn flatten_copies(mesh: &KernelMesh, placements: &[Placement]) -> KernelMesh {
    let vcount = mesh.positions.len();
    let n = placements.len();

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(vcount * n);
    for placement in placements {
        for p in mesh.positions.iter() {
            let v = placement.rotation * (Vector3::from(*p) * placement.scale);
            positions.push([
                v.x + placement.translate[0],
                v.y + placement.translate[1],
                v.z + placement.translate[2],
            ]);
        }
    }

    let normals = mesh.normals.as_ref().map(|buf| {
        let mut out: Vec<[f32; 3]> = Vec::with_capacity(buf.len() * n);
        for placement in placements {
            for normal in buf.iter() {
                out.push((placement.rotation * Vector3::from(*normal)).into());
            }
        }
        Arc::new(out)
    });

    let mut indices: Vec<u32> = Vec::with_capacity(mesh.indices.len() * n);
    for copy in 0..n {
        let base = (copy * vcount) as u32;
        indices.extend(mesh.indices.iter().map(|i| i + base));
    }

    KernelMesh {
        name: mesh.name.clone(),
        positions: Arc::new(positions),
        normals,
        tex_coords: mesh.tex_coords.as_ref().map(|buf| Arc::new(tile(buf, n))),
        indices: Arc::new(indices),
        material_index: mesh.material_index,
        topology: mesh.topology,
        attributes: tile_lanes(&mesh.attributes, n),
        primitive_attributes: tile_lanes(&mesh.primitive_attributes, n),
        // Baked output IS the copies, so it carries no placements.
        instances: None,
    }
}

fn tile<T: Copy>(buffer: &[T], n: usize) -> Vec<T> {
    let mut out = Vec::with_capacity(buffer.len() * n);
    for _ in 0..n {
        out.extend_from_slice(buffer);
    }
    out
}

fn tile_lanes(lanes: &AttributeMap, n: usize) -> AttributeMap {
    lanes
        .iter()
        .map(|(name, data)| {
            let tiled = match data {
                AttributeData::Float(v) => AttributeData::Float(Arc::new(tile(v, n))),
                AttributeData::Vec2(v) => AttributeData::Vec2(Arc::new(tile(v, n))),
                AttributeData::Vec3(v) => AttributeData::Vec3(Arc::new(tile(v, n))),
                AttributeData::Vec4(v) => AttributeData::Vec4(Arc::new(tile(v, n))),
            };
            (name.clone(), tiled)
        })
        .collect()
}

/// The rotation taking +Y onto `n` by the shortest arc. A zero or
/// unnormalizable `n` yields identity; the antiparallel case (straight
/// down) flips about X.
fn rotation_from_y(n: [f32; 3]) -> Matrix3<f32> {
    let v = Vector3::from(n);
    let len = v.magnitude();
    if !len.is_finite() || len < 1e-12 {
        return Matrix3::identity();
    }
    let n = v / len;
    let dot = n.y.clamp(-1.0, 1.0);
    if dot > 0.999_999 {
        return Matrix3::identity();
    }
    if dot < -0.999_999 {
        return Matrix3::from_angle_x(Rad(std::f32::consts::PI));
    }
    let axis = Vector3::unit_y().cross(n).normalize();
    Matrix3::from_axis_angle(axis, Rad(dot.acos()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests

    use super::*;
    use crate::primitives::{generate_box, generate_plane};
    use crate::scatter::scatter;
    use solarxy_core::geometry::MeshTopology;

    fn three_points() -> GeometrySet {
        GeometrySet::from_mesh(KernelMesh::points(
            "p",
            vec![[0.0, 0.0, 0.0], [5.0, 0.0, 0.0], [0.0, 3.0, 0.0]],
        ))
    }

    #[test]
    fn copies_flatten_into_one_mesh_per_template_mesh() {
        let template = GeometrySet::from_mesh(generate_box(1.0, 1.0, 1.0, 1, 1, 1));
        let out = copy_to_points(
            &template,
            &three_points(),
            CopyOrient::None,
            1.0,
            0.0,
            0,
            CopyMode::Bake,
        )
        .unwrap();
        assert_eq!(out.mesh_count(), 1, "one draw object, not one per point");
        let mesh = &out.meshes[0];
        assert_eq!(mesh.primitive_count(), 12 * 3);
        assert_eq!(mesh.vertex_count(), template.meshes[0].vertex_count() * 3);
        assert_eq!(mesh.topology, MeshTopology::Triangles);
        // The second copy is the box translated to (5, 0, 0).
        let vcount = template.meshes[0].vertex_count();
        for (i, p) in mesh.positions[vcount..2 * vcount].iter().enumerate() {
            let src = template.meshes[0].positions[i];
            assert!((p[0] - (src[0] + 5.0)).abs() < 1e-5, "{p:?} vs {src:?}");
            assert!((p[1] - src[1]).abs() < 1e-5);
            assert!((p[2] - src[2]).abs() < 1e-5);
        }
    }

    /// A triangle in the XZ plane whose normals point +Y: the copy
    /// orientation convention's own up axis, so rotations read directly.
    fn y_up_template() -> GeometrySet {
        let mut mesh = KernelMesh::new(
            "up",
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]],
            vec![0, 1, 2],
        );
        mesh.normals = Some(Arc::new(vec![[0.0, 1.0, 0.0]; 3]));
        GeometrySet::from_mesh(mesh)
    }

    #[test]
    fn orient_normal_rotates_the_template_onto_the_point_frame() {
        // A +Y-normal template copied onto a point whose N is +X: every
        // baked normal must point +X.
        let template = y_up_template();
        let mut points = KernelMesh::points("p", vec![[0.0, 0.0, 0.0]]);
        points.attributes.insert(
            reserved::NORMAL.to_string(),
            AttributeData::Vec3(Arc::new(vec![[1.0, 0.0, 0.0]])),
        );
        let out = copy_to_points(
            &template,
            &GeometrySet::from_mesh(points),
            CopyOrient::Normal,
            1.0,
            0.0,
            0,
            CopyMode::Bake,
        )
        .unwrap();
        let normals = out.meshes[0]
            .normals
            .as_ref()
            .expect("template has normals");
        for n in normals.iter() {
            assert!((n[0] - 1.0).abs() < 1e-5, "+Y rotated onto +X: {n:?}");
            assert!(n[1].abs() < 1e-5 && n[2].abs() < 1e-5);
        }

        // A straight-down normal (the antiparallel case) flips, not NaNs.
        let mut down = KernelMesh::points("d", vec![[0.0; 3]]);
        down.attributes.insert(
            reserved::NORMAL.to_string(),
            AttributeData::Vec3(Arc::new(vec![[0.0, -1.0, 0.0]])),
        );
        let out = copy_to_points(
            &template,
            &GeometrySet::from_mesh(down),
            CopyOrient::Normal,
            1.0,
            0.0,
            0,
            CopyMode::Bake,
        )
        .unwrap();
        for n in out.meshes[0].normals.as_ref().unwrap().iter() {
            assert!((n[1] + 1.0).abs() < 1e-5, "flipped straight down: {n:?}");
        }
    }

    #[test]
    fn orient_none_keeps_the_template_orientation() {
        let template = y_up_template();
        let mut points = KernelMesh::points("p", vec![[2.0, 0.0, 0.0]]);
        points.attributes.insert(
            reserved::NORMAL.to_string(),
            AttributeData::Vec3(Arc::new(vec![[1.0, 0.0, 0.0]])),
        );
        let out = copy_to_points(
            &template,
            &GeometrySet::from_mesh(points),
            CopyOrient::None,
            1.0,
            0.0,
            0,
            CopyMode::Bake,
        )
        .unwrap();
        for n in out.meshes[0].normals.as_ref().unwrap().iter() {
            assert!((n[1] - 1.0).abs() < 1e-5, "orientation untouched: {n:?}");
        }
    }

    #[test]
    fn scale_variance_is_seeded_and_deterministic() {
        let template = GeometrySet::from_mesh(generate_box(1.0, 1.0, 1.0, 1, 1, 1));
        let points = three_points();
        let a = copy_to_points(
            &template,
            &points,
            CopyOrient::None,
            1.0,
            0.5,
            9,
            CopyMode::Bake,
        )
        .unwrap();
        let b = copy_to_points(
            &template,
            &points,
            CopyOrient::None,
            1.0,
            0.5,
            9,
            CopyMode::Bake,
        )
        .unwrap();
        assert_eq!(a.meshes[0].positions, b.meshes[0].positions);
        let c = copy_to_points(
            &template,
            &points,
            CopyOrient::None,
            1.0,
            0.5,
            10,
            CopyMode::Bake,
        )
        .unwrap();
        assert_ne!(a.meshes[0].positions, c.meshes[0].positions);
        // Variance actually varies: the three copies span different sizes.
        let vcount = template.meshes[0].vertex_count();
        let extent = |copy: usize| -> f32 {
            a.meshes[0].positions[copy * vcount..(copy + 1) * vcount]
                .iter()
                .map(|p| p[1].abs())
                .fold(0.0, f32::max)
        };
        assert_ne!(extent(0), extent(1), "per-point scale varies");
    }

    #[test]
    fn the_ceiling_errors_before_allocating() {
        let template = GeometrySet::from_mesh(generate_box(1.0, 1.0, 1.0, 1, 1, 1));
        let cloud = GeometrySet::from_mesh(KernelMesh::points("big", vec![[0.0; 3]; 1_000_000]));
        let err = copy_to_points(
            &template,
            &cloud,
            CopyOrient::None,
            1.0,
            0.0,
            0,
            CopyMode::Bake,
        )
        .unwrap_err();
        assert!(err.contains("ceiling"), "got: {err}");
    }

    #[test]
    fn attribute_lanes_and_materials_ride_the_copies() {
        let mut template_mesh = generate_box(1.0, 1.0, 1.0, 1, 1, 1);
        let vcount = template_mesh.vertex_count();
        template_mesh.attributes.insert(
            reserved::COLOR.to_string(),
            AttributeData::Vec4(Arc::new(vec![[1.0, 0.0, 0.0, 1.0]; vcount])),
        );
        template_mesh.material_index = Some(0);
        let mut template = GeometrySet::from_mesh(template_mesh);
        template.materials = vec![Arc::new(solarxy_core::geometry::RawMaterialData {
            name: "red".to_string(),
            ..Default::default()
        })];

        let out = copy_to_points(
            &template,
            &three_points(),
            CopyOrient::None,
            1.0,
            0.0,
            0,
            CopyMode::Bake,
        )
        .unwrap();
        let mesh = &out.meshes[0];
        let Some(AttributeData::Vec4(colors)) = mesh.attributes.get(reserved::COLOR) else {
            panic!("color lane rides");
        };
        assert_eq!(colors.len(), vcount * 3, "lane tiled to the new length");
        assert_eq!(mesh.material_index, Some(0));
        assert_eq!(out.materials.len(), 1);
    }

    #[test]
    fn vertices_of_any_topology_are_targets_and_empty_points_yield_empty() {
        let template = GeometrySet::from_mesh(generate_box(0.1, 0.1, 0.1, 1, 1, 1));
        // A triangle mesh as the points input: its 3 vertices are targets.
        let tri = GeometrySet::from_mesh(KernelMesh::new(
            "t",
            vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            vec![0, 1, 2],
        ));
        let out = copy_to_points(
            &template,
            &tri,
            CopyOrient::None,
            1.0,
            0.0,
            0,
            CopyMode::Bake,
        )
        .unwrap();
        assert_eq!(out.meshes[0].primitive_count(), 12 * 3);

        let empty = copy_to_points(
            &template,
            &GeometrySet::empty(),
            CopyOrient::None,
            1.0,
            0.0,
            0,
            CopyMode::Bake,
        )
        .unwrap();
        assert!(empty.is_renderable_empty());
    }

    #[test]
    fn scatter_output_feeds_copy_directly() {
        // The canonical chain: scatter a plane, copy a small box onto the
        // cloud with normal orientation; the result is renderable and
        // sized count x template.
        let surface = GeometrySet::from_mesh(generate_plane(4.0, 4.0, 1, 1));
        let cloud = scatter(&surface, 50, 7);
        let template = GeometrySet::from_mesh(generate_box(0.2, 0.2, 0.2, 1, 1, 1));
        let out = copy_to_points(
            &template,
            &cloud,
            CopyOrient::Normal,
            1.0,
            0.25,
            7,
            CopyMode::Bake,
        )
        .unwrap();
        assert_eq!(out.meshes[0].primitive_count(), 12 * 50);
        assert!(!out.is_renderable_empty());
    }
}

#[cfg(test)]
mod pscale_tests {
    use super::*;
    use crate::primitives::generate_box;
    use std::sync::Arc;

    /// Two points, the second carrying twice the first's `pscale`.
    fn points_with_pscale(values: Option<Vec<f32>>) -> GeometrySet {
        let mut mesh = KernelMesh::points("pts", vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]]);
        if let Some(v) = values {
            mesh.attributes.insert(
                reserved::PSCALE.to_string(),
                AttributeData::Float(Arc::new(v)),
            );
        }
        GeometrySet::from_parts(vec![mesh], Vec::new())
    }

    fn template() -> GeometrySet {
        GeometrySet::from_parts(vec![generate_box(1.0, 1.0, 1.0, 1, 1, 1)], Vec::new())
    }

    /// The size of copy `i`, measured as its bounding-box width.
    fn copy_width(set: &GeometrySet, i: usize) -> f32 {
        let mesh = &set.meshes[0];
        let per = mesh.positions.len() / 2;
        let slice = &mesh.positions[i * per..(i + 1) * per];
        let max = slice.iter().fold(f32::MIN, |a, p| a.max(p[0]));
        let min = slice.iter().fold(f32::MAX, |a, p| a.min(p[0]));
        max - min
    }

    #[test]
    fn pscale_multiplies_the_scale_parameter_rather_than_replacing_it() {
        let out = copy_to_points(
            &template(),
            &points_with_pscale(Some(vec![1.0, 2.0])),
            CopyOrient::None,
            1.0,
            0.0,
            0,
            CopyMode::Bake,
        )
        .expect("copies");
        let a = copy_width(&out, 0);
        let b = copy_width(&out, 1);
        assert!((b - a * 2.0).abs() < 1e-4, "{b} should be twice {a}");
    }

    #[test]
    fn the_scale_parameter_still_applies_on_top_of_the_lane() {
        // The parameter is a global dial: doubling it doubles every copy,
        // whatever the lane says. Replacing rather than multiplying would
        // have made this parameter dead.
        let out = copy_to_points(
            &template(),
            &points_with_pscale(Some(vec![1.0, 2.0])),
            CopyOrient::None,
            3.0,
            0.0,
            0,
            CopyMode::Bake,
        )
        .expect("copies");
        let plain = copy_to_points(
            &template(),
            &points_with_pscale(Some(vec![1.0, 2.0])),
            CopyOrient::None,
            1.0,
            0.0,
            0,
            CopyMode::Bake,
        )
        .expect("copies");
        assert!((copy_width(&out, 0) - copy_width(&plain, 0) * 3.0).abs() < 1e-4);
        assert!((copy_width(&out, 1) - copy_width(&plain, 1) * 3.0).abs() < 1e-4);
    }

    #[test]
    fn points_without_the_lane_copy_at_the_parameter_size() {
        let with = copy_to_points(
            &template(),
            &points_with_pscale(Some(vec![1.0, 1.0])),
            CopyOrient::None,
            2.0,
            0.0,
            0,
            CopyMode::Bake,
        )
        .expect("copies");
        let without = copy_to_points(
            &template(),
            &points_with_pscale(None),
            CopyOrient::None,
            2.0,
            0.0,
            0,
            CopyMode::Bake,
        )
        .expect("copies");
        assert!((copy_width(&with, 0) - copy_width(&without, 0)).abs() < 1e-5);
    }

    #[test]
    fn a_negative_pscale_clamps_to_zero_rather_than_mirroring_the_copy() {
        let out = copy_to_points(
            &template(),
            &points_with_pscale(Some(vec![1.0, -3.0])),
            CopyOrient::None,
            1.0,
            0.0,
            0,
            CopyMode::Bake,
        )
        .expect("copies");
        assert!(copy_width(&out, 1) < 1e-5, "collapsed, not inside out");
    }
}

#[cfg(test)]
mod mode_tests {
    use super::*;

    fn template() -> GeometrySet {
        GeometrySet::from_mesh(KernelMesh::new(
            "t",
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        ))
    }

    fn points(at: &[[f32; 3]]) -> GeometrySet {
        GeometrySet::from_mesh(KernelMesh::points("p", at.to_vec()))
    }

    /// Every vertex the Bake path produces, in order.
    fn baked_positions(set: &GeometrySet) -> Vec<[f32; 3]> {
        set.meshes
            .iter()
            .flat_map(|m| m.positions.to_vec())
            .collect()
    }

    #[test]
    fn instance_mode_carries_the_transforms_bake_would_have_applied() {
        // The parity that makes the two modes the same picture. Applying
        // each instance matrix to the prototype's vertices must reproduce
        // Bake's output vertex for vertex, or switching modes moves the
        // geometry and the choice stops being free.
        let targets = [[0.0, 0.0, 0.0], [4.0, 1.0, -2.0], [-3.0, 0.0, 5.0]];
        let pts = points(&targets);

        let baked = copy_to_points(
            &template(),
            &pts,
            CopyOrient::None,
            2.0,
            0.0,
            7,
            CopyMode::Bake,
        )
        .expect("bake");
        let instanced = copy_to_points(
            &template(),
            &pts,
            CopyOrient::None,
            2.0,
            0.0,
            7,
            CopyMode::Instance,
        )
        .expect("instance");

        let placements = instanced.meshes[0].instances.as_ref().expect("placements");
        assert_eq!(placements.len(), targets.len());
        // The prototype is untouched: one triangle, not three copies of one.
        assert_eq!(instanced.meshes[0].positions.len(), 3);

        let proto = &instanced.meshes[0].positions;
        let want = baked_positions(&baked);
        let mut got = Vec::new();
        for placement in placements.iter() {
            for p in proto.iter() {
                got.push(placement.apply(*p));
            }
        }
        assert_eq!(got.len(), want.len());
        for (g, w) in got.iter().zip(want.iter()) {
            for axis in 0..3 {
                assert!((g[axis] - w[axis]).abs() < 1e-4, "{g:?} vs {w:?}");
            }
        }
    }

    #[test]
    fn instance_mode_carries_orientation_and_per_point_scale_too() {
        // Not just translation: the rotation onto the point normal and the
        // seeded scale variance both have to ride the matrix, or oriented
        // scatters differ between the modes.
        let mut pts = KernelMesh::points("p", vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]]);
        pts.attributes.insert(
            reserved::NORMAL.to_string(),
            AttributeData::Vec3(Arc::new(vec![[0.0, 0.0, 1.0], [1.0, 0.0, 0.0]])),
        );
        let pts = GeometrySet::from_mesh(pts);

        let baked = copy_to_points(
            &template(),
            &pts,
            CopyOrient::Normal,
            1.5,
            0.4,
            42,
            CopyMode::Bake,
        )
        .expect("bake");
        let instanced = copy_to_points(
            &template(),
            &pts,
            CopyOrient::Normal,
            1.5,
            0.4,
            42,
            CopyMode::Instance,
        )
        .expect("instance");

        let placements = instanced.meshes[0].instances.as_ref().expect("placements");
        let proto = &instanced.meshes[0].positions;
        let want = baked_positions(&baked);
        let mut got = Vec::new();
        for placement in placements.iter() {
            for p in proto.iter() {
                got.push(placement.apply(*p));
            }
        }
        for (g, w) in got.iter().zip(want.iter()) {
            for axis in 0..3 {
                assert!((g[axis] - w[axis]).abs() < 1e-4, "{g:?} vs {w:?}");
            }
        }
    }

    #[test]
    fn instance_mode_allocates_the_prototype_once_however_many_points() {
        // The whole reason the mode exists. Ten thousand copies of a
        // triangle is one triangle plus ten thousand matrices.
        let many: Vec<[f32; 3]> = (0..10_000).map(|i| [i as f32, 0.0, 0.0]).collect();
        let out = copy_to_points(
            &template(),
            &points(&many),
            CopyOrient::None,
            1.0,
            0.0,
            0,
            CopyMode::Instance,
        )
        .expect("instance");
        assert_eq!(out.meshes[0].positions.len(), 3);
        assert_eq!(out.instance_count(), 10_000);
    }

    #[test]
    fn the_ceiling_counts_per_mode_and_the_message_names_the_way_out() {
        // A scatter that cannot bake can still instance, so the ceiling
        // has to be a per-mode question and the error has to say so
        // rather than telling the user to lower a count that is fine.
        let heavy: Vec<[f32; 3]> = (0..4_000_000).map(|i| [i as f32, 0.0, 0.0]).collect();
        let pts = points(&heavy);

        let err = copy_to_points(
            &template(),
            &pts,
            CopyOrient::None,
            1.0,
            0.0,
            0,
            CopyMode::Bake,
        )
        .expect_err("over the ceiling in Bake");
        assert!(err.contains("Bake mode"), "{err}");
        assert!(err.contains("Instance mode"), "{err}");

        // The same scatter places fine, because the prototype is counted
        // once.
        let ok = copy_to_points(
            &template(),
            &pts,
            CopyOrient::None,
            1.0,
            0.0,
            0,
            CopyMode::Instance,
        );
        assert!(ok.is_ok(), "instancing the same scatter must succeed");
    }

    #[test]
    fn an_empty_input_is_empty_in_both_modes() {
        for mode in [CopyMode::Bake, CopyMode::Instance] {
            let out = copy_to_points(
                &template(),
                &GeometrySet::empty(),
                CopyOrient::None,
                1.0,
                0.0,
                0,
                mode,
            )
            .expect("empty");
            assert!(out.meshes.is_empty(), "{mode:?}");
            assert!(!out.is_instanced(), "{mode:?}");
        }
    }
}
