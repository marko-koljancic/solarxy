//! The attribute channel assembled: the label set and the arrow segments
//! both shells hand the renderer for the picked point lane.
//!
//! Pure over the displayed geometries and the strip's state, so the two
//! shells cannot sample points or colour arrows differently. The stride
//! walk, the deterministic cap, the object and normal matrices and the ramp
//! over the frame's magnitude range all lived in the browser host until the
//! desktop drew the channel; only the upload is a shell's.
//!
//! A geometry arrives as its cooked set and the world matrix the object is
//! placed by, which is what the engine answers for its displayed nodes and
//! what a shell can hand over without this crate seeing the engine.

use std::sync::Arc;

use cgmath::{InnerSpace, Matrix3, Matrix4, Point3, SquareMatrix, Transform, Vector3};

use solarxy_kernel::{GeometrySet, KernelMesh, lane::resolve_lane};
use solarxy_renderer::labels::LabelInstance;
use solarxy_renderer::model::GizmoVertex;

use crate::attr_labels::{LabelCandidate, build_labels};
use crate::attr_viz::{AttrColorMode, AttrVizState, ramp_color};

/// One displayed geometry: its cooked set and its object-to-world matrix.
pub type DisplayedGeometry = (Arc<GeometrySet>, [[f32; 4]; 4]);

/// The label set for the displayed geometries: one instance per sampled
/// point plus the flat glyph stream, and the sampling facts the strip's
/// notice reports as `(capacity, total points)`.
///
/// A deterministic stride sample of every displayed geometry's points, all
/// of them up to the budget, with the value from the picked lane when the
/// strip shows values. Empty when the strip wants no pins or the scene has
/// no points, so the caller uploads an empty set rather than keeping a
/// stale one.
#[must_use]
pub fn build_label_set(
    geos: &[DisplayedGeometry],
    viz: &AttrVizState,
) -> (Vec<LabelInstance>, Vec<u32>, u32, usize) {
    if !viz.pins_wanted() {
        return (Vec::new(), Vec::new(), 0, 0);
    }
    let lane = viz.name.as_deref().filter(|_| viz.labels);
    let total: usize = geos
        .iter()
        .flat_map(|(set, _)| set.meshes.iter())
        .map(KernelMesh::vertex_count)
        .sum();
    if total == 0 {
        return (Vec::new(), Vec::new(), 0, 0);
    }
    let cap = viz.effective_cap(total);
    let stride = total.div_ceil(cap).max(1);

    let mut candidates: Vec<LabelCandidate> = Vec::with_capacity(cap);
    let mut global = 0usize;
    for (set, m) in geos {
        let matrix = Matrix4::from(*m);
        let mut ptnum: u64 = 0;
        for mesh in &set.meshes {
            let len = mesh.vertex_count();
            let values = lane.and_then(|n| resolve_lane(mesh, n));
            let first = global.next_multiple_of(stride);
            let mut g = first;
            while g < global + len {
                let i = g - global;
                let tp = matrix.transform_point(Point3::from(mesh.positions[i]));
                candidates.push(LabelCandidate {
                    world: [tp.x, tp.y, tp.z],
                    ptnum: ptnum + i as u64,
                    value: values.map(|l| l.components(i).unwrap_or_default()),
                });
                g += stride;
            }
            ptnum += len as u64;
            global += len;
        }
    }
    let (instances, words) = build_labels(&candidates, viz.labels, viz.points, viz.label_decimals);
    (instances, words, cap as u32, total)
}

/// World-space arrow segments for the picked point lane (vec3, or the xyz
/// of vec4; map lane or the fixed `N` buffer), over every displayed
/// geometry: positions through the object matrix, directions through the
/// normal matrix for the reserved `N` lane (bivector semantics under
/// nonuniform scale) and the plain linear part for everything else. Length
/// is the bounds-derived factor times the strip's scale multiplier, over
/// the value (or its unit direction under normalize); colour is the uniform
/// pick, or the ramp over this frame's magnitude range.
#[must_use]
pub fn build_vector_lines(geos: &[DisplayedGeometry], viz: &AttrVizState) -> Vec<GizmoVertex> {
    let Some(name) = viz.name.as_deref() else {
        return Vec::new();
    };
    let is_normal_lane = name == solarxy_kernel::reserved::NORMAL;
    let multiplier = viz.scale_multiplier();
    let normalize = viz.normalize;

    // First pass: world-space segments plus each arrow's magnitude
    // (pre-normalization), so the ramp can span the real range.
    let mut segments: Vec<([f32; 3], [f32; 3], f32)> = Vec::new();
    for (set, m) in geos {
        let matrix = Matrix4::from(*m);
        let linear = Matrix3::from_cols(
            matrix.x.truncate(),
            matrix.y.truncate(),
            matrix.z.truncate(),
        );
        let dir_matrix = if is_normal_lane {
            linear
                .invert()
                .map_or(linear, |inv| cgmath::Matrix::transpose(&inv))
        } else {
            linear
        };
        let scale = {
            let d = set.bounds.diagonal();
            if d > 1e-10 { d * 0.05 } else { 0.1 }
        } * multiplier;
        for mesh in &set.meshes {
            // Vec3 and vec4 (xyz) lanes draw, map or fixed-buffer N;
            // float/vec2 lanes have no spatial reading and skip.
            let Some(lane) = resolve_lane(mesh, name) else {
                continue;
            };
            for (i, p) in mesh.positions.iter().enumerate() {
                let Some(v) = lane.direction(i) else { continue };
                let tp = matrix.transform_point(Point3::from(*p));
                let mut dir = dir_matrix * Vector3::from(v);
                let magnitude = dir.magnitude();
                if normalize {
                    if magnitude <= 1e-10 {
                        continue;
                    }
                    dir /= magnitude;
                }
                segments.push((
                    [tp.x, tp.y, tp.z],
                    [
                        tp.x + dir.x * scale,
                        tp.y + dir.y * scale,
                        tp.z + dir.z * scale,
                    ],
                    magnitude,
                ));
            }
        }
    }

    // Second pass: colours. Flat per arrow (both vertices alike) so
    // direction stays readable under the ramp.
    let color_for: Box<dyn Fn(f32) -> [f32; 3]> = match viz.color_mode {
        AttrColorMode::Uniform => {
            let c = viz.color;
            Box::new(move |_| c)
        }
        AttrColorMode::Ramp => {
            let (min, max) = segments
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), (_, _, m)| {
                    (lo.min(*m), hi.max(*m))
                });
            if max - min <= 1e-10 {
                // A degenerate range has nothing to rank; fall back to the
                // uniform colour.
                let c = viz.color;
                Box::new(move |_| c)
            } else {
                let preset = viz.ramp_preset;
                Box::new(move |m: f32| {
                    let t = ((m - min) / (max - min)).clamp(0.0, 1.0);
                    ramp_color(preset, t)
                })
            }
        }
    };
    segments
        .into_iter()
        .flat_map(|(a, b, magnitude)| {
            let color = color_for(magnitude);
            [
                GizmoVertex { position: a, color },
                GizmoVertex { position: b, color },
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_kernel::{AttributeData, KernelMesh};

    fn quad(with_normals: bool) -> Arc<GeometrySet> {
        let mut mesh = KernelMesh::new(
            "quad",
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
            ],
            vec![0, 1, 2, 0, 2, 3],
        );
        if with_normals {
            mesh.normals = Some(Arc::new(vec![[0.0, 0.0, 1.0]; 4]));
        }
        mesh.attributes.insert(
            "mass".into(),
            AttributeData::Float(Arc::new(vec![1.0, 2.0, 3.0, 4.0])),
        );
        Arc::new(GeometrySet::from_mesh(mesh))
    }

    fn translate(x: f32) -> [[f32; 4]; 4] {
        Matrix4::from_translation(Vector3::new(x, 0.0, 0.0)).into()
    }

    /// Every point is sampled under the cap, numbered per geometry, and
    /// placed by its object matrix; a value rides each label when the
    /// strip shows values for a lane the mesh has.
    #[test]
    fn labels_sample_every_point_under_the_cap_through_the_object_matrix() {
        let viz = AttrVizState {
            labels: true,
            points: true,
            name: Some("mass".into()),
            ..AttrVizState::default()
        };
        let geos = [
            (quad(false), translate(0.0)),
            (quad(false), translate(10.0)),
        ];
        let (instances, words, cap, total) = build_label_set(&geos, &viz);
        assert_eq!(total, 8);
        assert_eq!(cap as usize, viz.effective_cap(8));
        assert_eq!(instances.len(), 8);
        assert!(!words.is_empty());
        assert!(
            (instances[4].pos[0] - 10.0).abs() < 1e-6,
            "the second quad sits at x=10"
        );
    }

    /// A strip that wants no pins yields nothing, and so does an empty scene.
    #[test]
    fn no_pins_or_no_points_yield_an_empty_set() {
        let mut viz = AttrVizState::default();
        let geos = [(quad(false), translate(0.0))];
        assert_eq!(build_label_set(&geos, &viz).0.len(), 0);
        viz.points = true;
        assert_eq!(build_label_set(&[], &viz).3, 0);
    }

    /// The reserved `N` lane draws one segment per point, two vertices
    /// each, starting at the transformed point; a float lane draws none.
    #[test]
    fn arrows_draw_for_a_vector_lane_and_not_for_a_float_one() {
        let mut viz = AttrVizState {
            vectors: true,
            name: Some(solarxy_kernel::reserved::NORMAL.into()),
            ..AttrVizState::default()
        };
        let geos = [(quad(true), translate(3.0))];
        let lines = build_vector_lines(&geos, &viz);
        assert_eq!(lines.len(), 8);
        assert!((lines[0].position[0] - 3.0).abs() < 1e-6);
        assert!(
            lines[1].position[2] > lines[0].position[2],
            "the arrow points along +z"
        );
        viz.name = Some("mass".into());
        assert!(build_vector_lines(&geos, &viz).is_empty());
    }

    /// Under the ramp a degenerate magnitude range falls back to the
    /// uniform colour rather than dividing by zero.
    #[test]
    fn a_flat_magnitude_range_ramps_to_the_uniform_colour() {
        let viz = AttrVizState {
            vectors: true,
            name: Some(solarxy_kernel::reserved::NORMAL.into()),
            color_mode: AttrColorMode::Ramp,
            color: [0.25, 0.5, 0.75],
            ..AttrVizState::default()
        };
        let geos = [(quad(true), translate(0.0))];
        let lines = build_vector_lines(&geos, &viz);
        assert!(lines.iter().all(|v| {
            v.color
                .iter()
                .zip([0.25, 0.5, 0.75])
                .all(|(a, b)| (a - b).abs() < 1e-6)
        }));
    }
}
