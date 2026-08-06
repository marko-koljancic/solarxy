//! What a UV layout actually covers, counted the way the renderer counts it.
//!
//! # Three numbers a delivery check turns on
//!
//! Coverage, overlap and wasted area, none of which is reachable in the
//! shipped shell at any cost. They are computed here on the CPU rather than
//! read back from the GPU, because this crate has no device and no business
//! acquiring one to answer a question about a text report.
//!
//! The definitions are the renderer's, not new ones. It rasterises into a
//! 512 by 512 counter and reports overlap as the texels touched more than once
//! over the texels touched at least once; coverage is the texels touched at
//! least once over the whole grid, and wasted is what is left. Matching the
//! grid size as well as the arithmetic is what keeps the terminal's answer and
//! the viewport's answer the same number.
//!
//! # Why triangles and not vertices
//!
//! A texel is covered because a triangle lies over it, not because a vertex
//! landed on it. Sampling the three corners of a shell that spans a quarter of
//! the layout would report almost no coverage at all, which is the opposite of
//! the truth and exactly the case a delivery check exists to catch.

use super::geometry::ModelView;

/// The grid the renderer uses, matched so the two agree.
pub const GRID: usize = 512;

/// What a UV layout occupies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Occupancy {
    /// Texels covered at least once, over the whole grid.
    pub coverage: f32,
    /// Texels covered more than once, over the texels covered at all.
    pub overlap: f32,
}

impl Occupancy {
    /// The layout a delivery check is paying for and not using.
    pub fn wasted(self) -> f32 {
        1.0 - self.coverage
    }
}

/// A rasterised UV layout: how many times each texel was covered.
pub struct Coverage {
    counts: Vec<u16>,
}

impl Coverage {
    /// Rasterise every triangle of every mesh into one grid.
    ///
    /// One grid for the whole model rather than one per mesh, because two
    /// meshes landing on the same texel is a real overlap and the single
    /// commonest one: it is what happens when someone copies a shell between
    /// objects and forgets to move it.
    pub fn rasterise(view: &ModelView<'_>) -> Self {
        let mut counts = vec![0u16; GRID * GRID];
        for mesh in &view.meshes {
            if mesh.texcoords.is_empty() {
                continue;
            }
            let uv = |index: u32| -> Option<(f32, f32)> {
                let base = index as usize * 2;
                Some((*mesh.texcoords.get(base)?, *mesh.texcoords.get(base + 1)?))
            };
            for triangle in mesh.indices.chunks_exact(3) {
                let (Some(a), Some(b), Some(c)) =
                    (uv(triangle[0]), uv(triangle[1]), uv(triangle[2]))
                else {
                    continue;
                };
                fill_triangle(&mut counts, a, b, c);
            }
        }
        Self { counts }
    }

    pub fn occupancy(&self) -> Occupancy {
        let mut touched = 0u32;
        let mut overlapping = 0u32;
        for count in &self.counts {
            if *count > 0 {
                touched += 1;
            }
            if *count > 1 {
                overlapping += 1;
            }
        }
        Occupancy {
            coverage: f64::from(touched) as f32 / (GRID * GRID) as f32,
            overlap: if touched == 0 {
                0.0
            } else {
                f64::from(overlapping) as f32 / f64::from(touched) as f32
            },
        }
    }

    /// The tightest box containing anything, in unit-square coordinates.
    ///
    /// What the zoomed view frames. `None` when nothing was covered at all.
    pub fn used_bounds(&self) -> Option<(f32, f32, f32, f32)> {
        let (mut lo_x, mut lo_y, mut hi_x, mut hi_y) = (usize::MAX, usize::MAX, 0usize, 0usize);
        let mut seen = false;
        for (index, count) in self.counts.iter().enumerate() {
            if *count == 0 {
                continue;
            }
            seen = true;
            let (x, y) = (index % GRID, index / GRID);
            lo_x = lo_x.min(x);
            lo_y = lo_y.min(y);
            hi_x = hi_x.max(x);
            hi_y = hi_y.max(y);
        }
        let scale = GRID as f32;
        seen.then(|| {
            (
                lo_x as f32 / scale,
                lo_y as f32 / scale,
                (hi_x + 1) as f32 / scale,
                (hi_y + 1) as f32 / scale,
            )
        })
    }

    /// How many times a point in the unit square was covered.
    pub fn at(&self, x: f32, y: f32) -> u16 {
        if !(0.0..1.0).contains(&x) || !(0.0..1.0).contains(&y) {
            return 0;
        }
        let column = ((x * GRID as f32) as usize).min(GRID - 1);
        let row = ((y * GRID as f32) as usize).min(GRID - 1);
        self.counts[row * GRID + column]
    }
}

/// Scanline-fill one triangle into the counter.
///
/// # The fill rule is load-bearing, not a detail
///
/// Two triangles sharing an edge, which is every quad in every model, both
/// contain the texels whose centres lie exactly on that edge. Counting those
/// twice makes the diagonal of every quad report as overlapping, and the
/// overlap statistic then measures tessellation rather than the fault it
/// exists to find.
///
/// So a texel exactly on an edge belongs to the triangle on one specific side
/// of it: top and left edges take their boundary, the other two do not. Every
/// texel then belongs to exactly one triangle of a shared edge, and real
/// overlap between separate shells still counts.
fn fill_triangle(counts: &mut [u16], a: (f32, f32), b: (f32, f32), c: (f32, f32)) {
    let to_grid = |v: f32| v * GRID as f32;
    let (ax, ay) = (to_grid(a.0), to_grid(a.1));
    let (bx, by) = (to_grid(b.0), to_grid(b.1));
    let (cx, cy) = (to_grid(c.0), to_grid(c.1));

    let lo_x = ax.min(bx).min(cx).floor().max(0.0) as usize;
    let hi_x = (ax.max(bx).max(cx).ceil() as isize).clamp(0, GRID as isize) as usize;
    let lo_y = ay.min(by).min(cy).floor().max(0.0) as usize;
    let hi_y = (ay.max(by).max(cy).ceil() as isize).clamp(0, GRID as isize) as usize;
    if lo_x >= GRID || lo_y >= GRID {
        return;
    }

    let signed = edge(ax, ay, bx, by, cx, cy);
    if signed.abs() < f32::EPSILON {
        // A degenerate triangle has no area to cover. Counting its bounding
        // box instead would invent coverage that is not there.
        return;
    }
    // Normalise the winding so one fill rule serves both. Winding is a
    // modelling choice and must not change what a layout covers.
    let (bx, by, cx, cy) = if signed < 0.0 {
        (cx, cy, bx, by)
    } else {
        (bx, by, cx, cy)
    };

    let bias = |fx: f32, fy: f32, tx: f32, ty: f32| {
        let (dx, dy) = (tx - fx, ty - fy);
        if dy < 0.0 || (dy == 0.0 && dx < 0.0) {
            0.0
        } else {
            // Just inside one texel, which excludes the boundary without
            // eroding anything a reader could see.
            -EDGE_BIAS
        }
    };
    let (bias0, bias1, bias2) = (
        bias(bx, by, cx, cy),
        bias(cx, cy, ax, ay),
        bias(ax, ay, bx, by),
    );

    for row in lo_y..hi_y.min(GRID) {
        for column in lo_x..hi_x.min(GRID) {
            // The texel's centre, so a triangle covers a texel when it
            // actually lies over it rather than merely touching its corner.
            let (px, py) = (column as f32 + 0.5, row as f32 + 0.5);
            let inside = edge(bx, by, cx, cy, px, py) + bias0 >= 0.0
                && edge(cx, cy, ax, ay, px, py) + bias1 >= 0.0
                && edge(ax, ay, bx, by, px, py) + bias2 >= 0.0;
            if inside {
                counts[row * GRID + column] = counts[row * GRID + column].saturating_add(1);
            }
        }
    }
}

/// How far inside an edge a texel centre must sit to belong to the triangle
/// that does not own that edge. Small enough to move no visible boundary.
const EDGE_BIAS: f32 = 1e-3;

fn edge(ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32) -> f32 {
    (bx - ax) * (py - ay) - (by - ay) * (px - ax)
}

#[cfg(test)]
mod tests {
    use super::super::geometry::MeshView;
    use super::*;

    /// One axis-aligned right triangle covering exactly the lower left half
    /// of the unit square, twice over when doubled.
    fn quad(offset: f32, extent: f32) -> (Vec<f32>, Vec<u32>) {
        let uv = vec![
            offset,
            offset,
            offset + extent,
            offset,
            offset + extent,
            offset + extent,
            offset,
            offset + extent,
        ];
        (uv, vec![0, 1, 2, 0, 2, 3])
    }

    fn view<'a>(texcoords: &'a [f32], indices: &'a [u32]) -> ModelView<'a> {
        ModelView {
            meshes: vec![MeshView {
                positions: &[],
                texcoords,
                indices,
            }],
        }
    }

    /// The fixture with a known answer. A quad over exactly a quarter of the
    /// layout must report a quarter of it covered, and nothing overlapping.
    #[test]
    fn a_quarter_of_the_layout_reads_as_a_quarter_covered() {
        let (uv, indices) = quad(0.0, 0.5);
        let occupancy = Coverage::rasterise(&view(&uv, &indices)).occupancy();
        assert!(
            (occupancy.coverage - 0.25).abs() < 0.002,
            "coverage was {}",
            occupancy.coverage
        );
        assert!(
            occupancy.overlap < 0.002,
            "a single shell overlapped itself: {}",
            occupancy.overlap
        );
        assert!((occupancy.wasted() - 0.75).abs() < 0.002);
    }

    /// The whole point of the overlap number. Two shells in the same place is
    /// what a delivery check is looking for.
    #[test]
    fn two_shells_in_the_same_place_read_as_wholly_overlapping() {
        let (mut uv, mut indices) = quad(0.0, 0.5);
        let (second, second_indices) = quad(0.0, 0.5);
        let base = (uv.len() / 2) as u32;
        uv.extend(second);
        indices.extend(second_indices.iter().map(|i| i + base));

        let occupancy = Coverage::rasterise(&view(&uv, &indices)).occupancy();
        assert!(
            (occupancy.coverage - 0.25).abs() < 0.002,
            "two copies should cover the same quarter, not half: {}",
            occupancy.coverage
        );
        assert!(
            occupancy.overlap > 0.99,
            "the doubled shell should be wholly overlapping: {}",
            occupancy.overlap
        );
    }

    /// Half-overlapping is the interesting middle case, and the one that
    /// distinguishes counting texels from counting triangles.
    #[test]
    fn partly_overlapping_shells_report_the_overlapping_share() {
        let (mut uv, mut indices) = quad(0.0, 0.5);
        let (second, second_indices) = quad(0.25, 0.5);
        let base = (uv.len() / 2) as u32;
        uv.extend(second);
        indices.extend(second_indices.iter().map(|i| i + base));

        let occupancy = Coverage::rasterise(&view(&uv, &indices)).occupancy();
        // Two quarter-squares offset by half their extent share a sixteenth,
        // over a covered area of seven sixteenths.
        assert!(
            (occupancy.overlap - 1.0 / 7.0).abs() < 0.01,
            "expected about a seventh overlapping, got {}",
            occupancy.overlap
        );
    }

    /// A shell folded along an edge is not two shells. Counting a texel twice
    /// for one triangle would report a fault that is not there.
    #[test]
    fn one_triangle_never_counts_a_texel_twice() {
        let uv = vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
        let occupancy = Coverage::rasterise(&view(&uv, &[0, 1, 2])).occupancy();
        assert_eq!(
            occupancy.overlap, 0.0,
            "a single triangle overlapped itself"
        );
        assert!((occupancy.coverage - 0.5).abs() < 0.005);
    }

    /// Winding is a modelling choice, not a coverage one. A mirrored shell
    /// covers exactly as much layout as the original.
    #[test]
    fn winding_order_does_not_change_what_is_covered() {
        let (uv, indices) = quad(0.0, 0.5);
        let forward = Coverage::rasterise(&view(&uv, &indices)).occupancy();
        let reversed: Vec<u32> = indices
            .chunks_exact(3)
            .flat_map(|t| [t[0], t[2], t[1]])
            .collect();
        let backward = Coverage::rasterise(&view(&uv, &reversed)).occupancy();
        assert_eq!(forward, backward);
    }

    #[test]
    fn a_degenerate_triangle_covers_nothing() {
        let uv = vec![0.2, 0.2, 0.2, 0.2, 0.8, 0.2];
        let occupancy = Coverage::rasterise(&view(&uv, &[0, 1, 2])).occupancy();
        assert_eq!(occupancy.coverage, 0.0);
    }

    #[test]
    fn a_model_with_no_uvs_covers_nothing_and_does_not_panic() {
        let occupancy = Coverage::rasterise(&ModelView::default()).occupancy();
        assert_eq!(occupancy.coverage, 0.0);
        assert_eq!(occupancy.overlap, 0.0);
        assert_eq!(occupancy.wasted(), 1.0);
    }

    /// What the zoomed view frames: the box that actually holds the layout,
    /// which for an atlas in one corner is a quarter of the square.
    #[test]
    fn the_used_box_is_the_layout_rather_than_the_square() {
        let (uv, indices) = quad(0.5, 0.25);
        let coverage = Coverage::rasterise(&view(&uv, &indices));
        let (lo_x, lo_y, hi_x, hi_y) = coverage.used_bounds().expect("something is covered");
        assert!((lo_x - 0.5).abs() < 0.01, "{lo_x}");
        assert!((lo_y - 0.5).abs() < 0.01, "{lo_y}");
        assert!((hi_x - 0.75).abs() < 0.01, "{hi_x}");
        assert!((hi_y - 0.75).abs() < 0.01, "{hi_y}");

        assert_eq!(
            Coverage::rasterise(&ModelView::default()).used_bounds(),
            None
        );
    }

    /// The grid has to match the renderer's, or the terminal and the viewport
    /// report different numbers for the same model.
    #[test]
    fn the_grid_matches_the_one_the_renderer_counts_on() {
        assert_eq!(GRID, 512);
    }
}
