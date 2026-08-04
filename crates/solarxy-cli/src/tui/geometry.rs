//! The model's geometry, borrowed rather than copied.
//!
//! # Why this is slices and not data
//!
//! The analyzer holds every vertex position for the whole session, and a
//! projection of the model therefore needs no new loading, no new field on the
//! report and no new dependency. What it does need is a way to reach that
//! memory from a panel, and copying it would throw away the entire reason the
//! plots are cheap.
//!
//! So this is a view: a small vector of slice pairs pointing at what the
//! analyzer already owns. It also keeps vertex arrays out of `AnalysisReport`,
//! which is a type the plain text renderer formats and which has no business
//! carrying a hundred thousand floats.
//!
//! # Why it is defined here rather than taken as the analyzer
//!
//! The analyzer lives behind a different feature from this shell. A plain
//! borrowed view has no such problem, and it also makes the boundary explicit:
//! the plots see positions, texture coordinates and indices, and nothing else
//! about how a model was loaded.

/// One mesh's raw arrays.
#[derive(Debug, Clone, Copy)]
pub struct MeshView<'a> {
    /// Interleaved xyz.
    pub positions: &'a [f32],
    /// Interleaved uv, empty when the mesh carries none.
    pub texcoords: &'a [f32],
    /// Triangle indices into the vertex arrays.
    pub indices: &'a [u32],
}

/// Everything the plots can see.
#[derive(Debug, Clone, Default)]
pub struct ModelView<'a> {
    pub meshes: Vec<MeshView<'a>>,
}

/// An axis-aligned box over the model, or `None` when there are no vertices.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Bounds {
    pub fn size(self) -> [f32; 3] {
        [
            self.max[0] - self.min[0],
            self.max[1] - self.min[1],
            self.max[2] - self.min[2],
        ]
    }

    /// The extent along one axis, never zero.
    ///
    /// A flat model is a real thing: a plane has no thickness, and dividing by
    /// its zero extent would put every point at infinity.
    pub fn span(self, axis: usize) -> f32 {
        (self.max[axis] - self.min[axis]).max(f32::EPSILON)
    }
}

impl ModelView<'_> {
    pub fn is_empty(&self) -> bool {
        self.meshes.iter().all(|mesh| mesh.positions.is_empty())
    }

    pub fn has_uvs(&self) -> bool {
        self.meshes.iter().any(|mesh| !mesh.texcoords.is_empty())
    }

    pub fn vertex_count(&self) -> usize {
        self.meshes.iter().map(|m| m.positions.len() / 3).sum()
    }

    /// The box every projection is normalised against.
    pub fn bounds(&self) -> Option<Bounds> {
        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        let mut seen = false;
        for mesh in &self.meshes {
            for xyz in mesh.positions.chunks_exact(3) {
                seen = true;
                for axis in 0..3 {
                    min[axis] = min[axis].min(xyz[axis]);
                    max[axis] = max[axis].max(xyz[axis]);
                }
            }
        }
        seen.then_some(Bounds { min, max })
    }
}

/// Which way the silhouette is looking.
///
/// Named for what a reader sees rather than for the axis pointing at them,
/// because "front" is what anyone says out loud and "negative z" is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Axis {
    #[default]
    Front,
    Side,
    Top,
}

impl Axis {
    pub fn name(self) -> &'static str {
        match self {
            Self::Front => "front",
            Self::Side => "side",
            Self::Top => "top",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Front => Self::Side,
            Self::Side => Self::Top,
            Self::Top => Self::Front,
        }
    }

    /// Which model axis maps to screen right, screen down, and away.
    ///
    /// Screen y is inverted for front and side because model up is positive y
    /// and terminal down is positive row. The top view already looks down that
    /// axis, so its screen y is model z and needs no flip.
    pub fn axes(self) -> (usize, usize, usize, bool) {
        match self {
            Self::Front => (0, 1, 2, true),
            Self::Side => (2, 1, 0, true),
            Self::Top => (0, 2, 1, false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube() -> Vec<f32> {
        vec![
            -1.0, 0.0, -0.5, //
            1.0, 2.0, 0.5, //
            0.0, 1.0, 0.0,
        ]
    }

    #[test]
    fn bounds_cover_every_vertex_of_every_mesh() {
        let a = cube();
        let b = vec![5.0, -3.0, 9.0];
        let view = ModelView {
            meshes: vec![
                MeshView {
                    positions: &a,
                    texcoords: &[],
                    indices: &[],
                },
                MeshView {
                    positions: &b,
                    texcoords: &[],
                    indices: &[],
                },
            ],
        };
        let bounds = view.bounds().expect("vertices exist");
        assert_eq!(bounds.min, [-1.0, -3.0, -0.5]);
        assert_eq!(bounds.max, [5.0, 2.0, 9.0]);
        assert_eq!(view.vertex_count(), 4);
    }

    #[test]
    fn a_model_with_no_vertices_has_no_bounds() {
        let view = ModelView::default();
        assert!(view.is_empty());
        assert_eq!(view.bounds(), None);
    }

    /// A plane has no thickness and a projection still has to divide by its
    /// extent, so the span can never be zero.
    #[test]
    fn a_flat_model_still_has_a_usable_span() {
        let flat = vec![0.0, 0.0, 0.0, 1.0, 1.0, 0.0];
        let view = ModelView {
            meshes: vec![MeshView {
                positions: &flat,
                texcoords: &[],
                indices: &[],
            }],
        };
        let bounds = view.bounds().expect("vertices exist");
        assert_eq!(bounds.size()[2], 0.0);
        assert!(bounds.span(2) > 0.0, "a zero span would divide by nothing");
    }

    #[test]
    fn the_axis_cycles_through_all_three_and_returns() {
        let mut axis = Axis::Front;
        let mut names = Vec::new();
        for _ in 0..3 {
            names.push(axis.name());
            axis = axis.next();
        }
        assert_eq!(names, vec!["front", "side", "top"]);
        assert_eq!(axis, Axis::Front);
    }

    /// Every view has to pick three distinct model axes, or a projection
    /// silently collapses one dimension onto another.
    #[test]
    fn every_view_maps_three_distinct_axes() {
        for axis in [Axis::Front, Axis::Side, Axis::Top] {
            let (right, down, away, _) = axis.axes();
            let mut used = [right, down, away];
            used.sort_unstable();
            assert_eq!(used, [0, 1, 2], "{} reuses an axis", axis.name());
        }
    }

    /// Model up is positive y and terminal down is positive row, so the two
    /// views that show height must flip and the one looking down must not.
    #[test]
    fn only_the_views_that_show_height_invert_the_screen_axis() {
        assert!(Axis::Front.axes().3);
        assert!(Axis::Side.axes().3);
        assert!(!Axis::Top.axes().3);
    }
}
