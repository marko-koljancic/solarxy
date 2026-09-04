//! Points in, a dot grid out, then braille or blocks.
//!
//! # Why one rasteriser rather than two plots
//!
//! The silhouette and the UV map are the same operation with a different
//! projection. Written once, the pair costs less than either would alone, and
//! the only way to keep it written once is to make this module unable to know
//! which of them is calling: there is nothing here about meshes, UV sets or
//! the analysis report, and nothing here that could be.
//!
//! # Two dots across and four down
//!
//! A braille cell holds eight dots in a two-by-four block, so a panel forty
//! cells wide and twelve tall is an eighty by forty-eight image. That is the
//! only reason a terminal can draw a shape at all, and it is why the plots go
//! to spatial data: a monitoring dashboard spends its braille on time series,
//! and an analyze report has no time axis to spend it on.
//!
//! Where braille is unavailable the **same grid** encodes to one character per
//! cell from the density ramp, so the plot loses its detail and keeps its
//! meaning rather than disappearing.
//!
//! # The failure this is shaped to avoid
//!
//! A dense mesh splatted naively lights every dot inside its own bounding box
//! and reads as a filled rectangle. So points carry a depth, near ones count
//! for more than far ones, and a dot lights only once its accumulated weight
//! clears a floor relative to the busiest dot in the grid. The interior of a
//! solid object is far and thinly covered; its near surface is not. That is
//! what turns a point cloud into a form.

use super::caps::{Glyphs, PlotStyle};

/// One sample to plot.
///
/// `x` and `y` are normalised to the unit square with the origin at the top
/// left, which is the terminal's own convention and saves every caller a flip.
/// `depth` runs from zero at the nearest to one at the furthest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
    pub depth: f32,
}

impl Point {
    /// A sample with no depth information, which weighs the same as every
    /// other. Correct for genuinely flat data such as a UV layout.
    pub fn flat(x: f32, y: f32) -> Self {
        Self { x, y, depth: 0.0 }
    }
}

/// How much less a point at the back counts than one at the front.
///
/// Not zero: the far side of a shape still contributes to its outline, and
/// dropping it entirely would hollow out every silhouette. Not one either, or
/// there would be no falloff and the interior would fill.
const FAR_WEIGHT: f32 = 0.15;

/// The share of the busiest dot a dot needs before it lights.
///
/// Low, because the aim is to suppress the thin interior rather than to
/// posterise the shape. Raising it eats the outline first, which is the part
/// worth keeping.
const LIGHT_THRESHOLD: f32 = 0.08;

/// An accumulation of weight per dot, sized in cells.
#[derive(Debug, Clone)]
pub struct Raster {
    cells_wide: u16,
    cells_high: u16,
    /// Row-major over the dot grid, which is twice as wide and four times as
    /// tall as the cell grid.
    weight: Vec<f32>,
}

/// Dots per cell, which is what a braille cell holds.
pub const DOTS_ACROSS: u16 = 2;
pub const DOTS_DOWN: u16 = 4;

impl Raster {
    pub fn new(cells_wide: u16, cells_high: u16) -> Self {
        let dots = usize::from(cells_wide) * usize::from(DOTS_ACROSS);
        let rows = usize::from(cells_high) * usize::from(DOTS_DOWN);
        Self {
            cells_wide,
            cells_high,
            weight: vec![0.0; dots * rows],
        }
    }

    pub fn dots_wide(&self) -> u16 {
        self.cells_wide * DOTS_ACROSS
    }

    pub fn dots_high(&self) -> u16 {
        self.cells_high * DOTS_DOWN
    }

    /// Accumulate a set of points.
    ///
    /// Order-independent by construction: every point adds its own weight and
    /// addition commutes, so the same set in any order produces the same grid.
    /// That is what makes the output deterministic without the caller having
    /// to sort first.
    pub fn plot(&mut self, points: &[Point]) {
        let (wide, high) = (self.dots_wide(), self.dots_high());
        if wide == 0 || high == 0 {
            return;
        }
        for point in points {
            if !(0.0..=1.0).contains(&point.x) || !(0.0..=1.0).contains(&point.y) {
                continue;
            }
            // The far edge of the unit square belongs to the last dot rather
            // than to a dot past the end.
            let column = ((point.x * f32::from(wide)) as u16).min(wide - 1);
            let row = ((point.y * f32::from(high)) as u16).min(high - 1);
            let index = usize::from(row) * usize::from(wide) + usize::from(column);
            self.weight[index] += weight_at(point.depth);
        }
    }

    /// The busiest dot, which every threshold is relative to.
    fn peak(&self) -> f32 {
        self.weight.iter().copied().fold(0.0_f32, f32::max)
    }

    /// Whether a dot is lit, and how strongly, from zero to one.
    fn intensity(&self, column: u16, row: u16, peak: f32) -> f32 {
        if peak <= 0.0 {
            return 0.0;
        }
        let index = usize::from(row) * usize::from(self.dots_wide()) + usize::from(column);
        let share = self.weight.get(index).copied().unwrap_or(0.0) / peak;
        if share < LIGHT_THRESHOLD { 0.0 } else { share }
    }

    /// Encode the grid, one string per cell row.
    ///
    /// Density rides on how many dots are set, never on colour, so the plot
    /// reads the same at the monochrome tier as anywhere else. Colour is the
    /// caller's to add on top and is never the only signal.
    pub fn render(&self, style: PlotStyle) -> Vec<String> {
        let peak = self.peak();
        (0..self.cells_high)
            .map(|cell_row| {
                (0..self.cells_wide)
                    .map(|cell_column| self.cell(cell_column, cell_row, peak, style))
                    .collect()
            })
            .collect()
    }

    fn cell(&self, cell_column: u16, cell_row: u16, peak: f32, style: PlotStyle) -> char {
        let origin_column = cell_column * DOTS_ACROSS;
        let origin_row = cell_row * DOTS_DOWN;

        match style {
            PlotStyle::Braille => {
                let mut bits = 0u8;
                for (across, down, bit) in BRAILLE_BITS {
                    if self.intensity(origin_column + across, origin_row + down, peak) > 0.0 {
                        bits |= bit;
                    }
                }
                char::from_u32(BRAILLE_BLANK + u32::from(bits)).unwrap_or(' ')
            }
            PlotStyle::Ascii => {
                // One character per cell, chosen by how many of that cell's
                // own eight dots are lit. Deliberately the same lit-or-not
                // decision braille makes rather than a second, softer one:
                // summing fractional weights instead keeps almost every cell
                // in the bottom rung, because most dots are a small share of
                // the busiest one, and the ramp goes unused.
                //
                // Half the resolution of braille in the sense that matters:
                // the shape survives, the detail inside a cell does not.
                let lit = BRAILLE_BITS
                    .iter()
                    .filter(|(across, down, _)| {
                        self.intensity(origin_column + across, origin_row + down, peak) > 0.0
                    })
                    .count();
                let ramp = Glyphs::ASCII_DENSITY;
                // Two dots per rung, so any coverage at all draws something
                // and a full cell reaches the densest mark.
                let step = if lit == 0 {
                    0
                } else {
                    (1 + (lit - 1) / 2).min(ramp.len() - 1)
                };
                ramp[step].chars().next().unwrap_or(' ')
            }
        }
    }
}

/// How much a point at a given depth counts.
///
/// Linear between the near and far weights, clamped, so a caller that hands in
/// unnormalised depth still gets something sensible rather than a negative
/// contribution.
fn weight_at(depth: f32) -> f32 {
    let depth = depth.clamp(0.0, 1.0);
    1.0 - depth * (1.0 - FAR_WEIGHT)
}

/// U+2800, the braille cell with no dots raised.
const BRAILLE_BLANK: u32 = 0x2800;

/// Where each dot of the two-by-four block sits in the codepoint.
///
/// The numbering is inherited from six-dot braille with the bottom row added
/// afterwards, which is why the last row is not where a reader would guess:
/// the left and right dots of row four are the two high bits rather than a
/// continuation of the pattern above them.
const BRAILLE_BITS: [(u16, u16, u8); 8] = [
    (0, 0, 0x01),
    (0, 1, 0x02),
    (0, 2, 0x04),
    (1, 0, 0x08),
    (1, 1, 0x10),
    (1, 2, 0x20),
    (0, 3, 0x40),
    (1, 3, 0x80),
];

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // clamped inputs must evaluate bit-identically

    use super::*;

    fn render(points: &[Point], w: u16, h: u16, style: PlotStyle) -> Vec<String> {
        let mut raster = Raster::new(w, h);
        raster.plot(points);
        raster.render(style)
    }

    /// Not an assertion: projects a real model so the legibility question the
    /// design leaves open can actually be looked at.
    #[test]
    #[ignore = "manual preview over a model on disk, not an assertion"]
    fn preview_a_real_model() {
        // Resolved from the manifest rather than the working directory, so
        // the tool works from wherever the test runner happens to start.
        const DEFAULT: &str = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../res/models/xyzrgb_dragon.obj"
        );
        let path = std::env::var("SOLARXY_PREVIEW_MODEL").unwrap_or_else(|_| DEFAULT.to_owned());
        let analyzer = crate::calc::analyze::ModelAnalyzer::new_with_config(&path, None)
            .expect("the sample loads");

        let mut points = Vec::new();
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for mesh in &analyzer.meshes {
            for xyz in mesh.positions.as_chunks::<3>().0 {
                for axis in 0..3 {
                    lo[axis] = lo[axis].min(xyz[axis]);
                    hi[axis] = hi[axis].max(xyz[axis]);
                }
            }
        }
        let span = |axis: usize| (hi[axis] - lo[axis]).max(f32::EPSILON);
        for mesh in &analyzer.meshes {
            for xyz in mesh.positions.as_chunks::<3>().0 {
                points.push(Point {
                    x: (xyz[0] - lo[0]) / span(0),
                    y: 1.0 - (xyz[1] - lo[1]) / span(1),
                    depth: (xyz[2] - lo[2]) / span(2),
                });
            }
        }

        for style in [PlotStyle::Braille, PlotStyle::Ascii] {
            println!("\n{style:?}, {} points", points.len());
            let mut raster = Raster::new(46, 11);
            raster.plot(&points);
            for row in raster.render(style) {
                println!("{row}");
            }
        }
    }

    #[test]
    fn a_panel_is_two_dots_across_and_four_down_per_cell() {
        let raster = Raster::new(40, 12);
        assert_eq!(raster.dots_wide(), 80);
        assert_eq!(raster.dots_high(), 48);
    }

    /// Each dot of the block maps to its own bit, and getting one wrong would
    /// mirror or shear every plot in a way that still looks plausible.
    #[test]
    fn every_dot_of_the_block_lands_on_its_own_bit() {
        for (across, down, bit) in BRAILLE_BITS {
            let x = f32::midpoint(f32::from(across), 0.5);
            let y = (f32::from(down) + 0.5) / 4.0;
            let rows = render(&[Point::flat(x, y)], 1, 1, PlotStyle::Braille);
            let drawn = rows[0].chars().next().expect("one cell");
            let expected = char::from_u32(BRAILLE_BLANK + u32::from(bit)).expect("valid");
            assert_eq!(
                drawn, expected,
                "dot at ({across},{down}) drew {drawn:?}, expected {expected:?}"
            );
        }
    }

    #[test]
    fn an_empty_grid_is_blank_braille_rather_than_spaces() {
        let rows = render(&[], 3, 1, PlotStyle::Braille);
        assert_eq!(rows, vec!["\u{2800}\u{2800}\u{2800}"]);
    }

    /// A grid with every dot lit is the full cell, which is the other end of
    /// the encoding and the one a dense mesh threatens to produce everywhere.
    #[test]
    fn a_fully_covered_cell_is_the_full_braille_block() {
        let points: Vec<Point> = BRAILLE_BITS
            .iter()
            .map(|(across, down, _)| {
                Point::flat(
                    f32::midpoint(f32::from(*across), 0.5),
                    (f32::from(*down) + 0.5) / 4.0,
                )
            })
            .collect();
        let rows = render(&points, 1, 1, PlotStyle::Braille);
        assert_eq!(rows, vec!["\u{28ff}"]);
    }

    /// The golden case: a diagonal across a small grid, which fails visibly if
    /// the row or column arithmetic is off by one or transposed.
    ///
    /// Each cell row spans four dot rows and each cell two dot columns, so a
    /// diagonal crosses two cells per row: dots (0,0) and (1,1) fall in the
    /// first, which is bits 0x01 and 0x10, and dots (2,2) and (3,3) in the
    /// second, which is 0x04 and 0x80.
    #[test]
    fn a_diagonal_renders_to_a_known_grid() {
        let points: Vec<Point> = (0..16u16)
            .map(|i| {
                let t = f32::from(i) / 16.0;
                Point::flat(t, t)
            })
            .collect();
        assert_eq!(
            render(&points, 8, 4, PlotStyle::Braille),
            vec![
                "\u{2811}\u{2884}\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}",
                "\u{2800}\u{2800}\u{2811}\u{2884}\u{2800}\u{2800}\u{2800}\u{2800}",
                "\u{2800}\u{2800}\u{2800}\u{2800}\u{2811}\u{2884}\u{2800}\u{2800}",
                "\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}\u{2800}\u{2811}\u{2884}",
            ]
        );
    }

    /// The same grid, the other encoding. Detail inside a cell is lost and the
    /// shape is not, which is the rule every degradation here follows.
    #[test]
    fn the_same_grid_encodes_to_blocks_at_the_ascii_tier() {
        let points: Vec<Point> = (0..16u16)
            .map(|i| {
                let t = f32::from(i) / 16.0;
                Point::flat(t, t)
            })
            .collect();
        let rows = render(&points, 8, 4, PlotStyle::Ascii);
        assert_eq!(rows.len(), 4);
        for row in &rows {
            assert_eq!(row.chars().count(), 8);
            assert!(row.is_ascii(), "{row:?} left the repertoire");
        }
        // The diagonal is still a diagonal: every row draws something, and
        // each row's marks sit strictly right of the row above. A transposed
        // or mirrored grid fails this even though it would still be ASCII.
        let mut previous: Option<usize> = None;
        for (index, row) in rows.iter().enumerate() {
            let marked: Vec<usize> = row
                .char_indices()
                .filter(|(_, c)| *c != ' ')
                .map(|(i, _)| i)
                .collect();
            assert!(!marked.is_empty(), "row {index} drew nothing: {row:?}");
            if let Some(last) = previous {
                assert!(
                    marked[0] > last,
                    "row {index} did not advance: {row:?} after column {last}"
                );
            }
            previous = marked.last().copied();
        }
    }

    /// The failure this module is shaped to avoid, stated precisely.
    ///
    /// A closed surface *should* fill its own outline: that is what a
    /// silhouette is, and the design's own mockup shows a solid form. What
    /// must not happen is the **bounding box** filling, which is what a naive
    /// splat of a point cloud produces. A disc has to leave its corners empty.
    #[test]
    fn a_round_body_does_not_render_as_a_filled_rectangle() {
        const FULL: char = '\u{28ff}';
        let mut points = Vec::new();
        for xi in 0..64u16 {
            for yi in 0..64u16 {
                let x = f32::from(xi) / 63.0;
                let y = f32::from(yi) / 63.0;
                let (dx, dy) = (x - 0.5, y - 0.5);
                if dx * dx + dy * dy <= 0.25 {
                    points.push(Point::flat(x, y));
                }
            }
        }

        let rows = render(&points, 8, 4, PlotStyle::Braille);

        // The corners clip one dot each, because a disc inscribed in the grid
        // genuinely reaches into those cells. What must not happen is their
        // being *full*, which is what a splat of the bounding box gives.
        for (row, index, corner) in [
            (0usize, 0usize, "top left"),
            (0, 7, "top right"),
            (3, 0, "bottom left"),
            (3, 7, "bottom right"),
        ] {
            let cell = rows[row].chars().nth(index).expect("in range");
            assert_ne!(cell, FULL, "the {corner} corner filled: {rows:?}");
        }
        let centre = rows[1].chars().nth(4).expect("in range");
        assert_eq!(centre, FULL, "the body itself is not solid: {rows:?}");
    }

    /// What depth actually buys. A thin far scattering falls under the floor
    /// set by a dense near body, so it does not light cells the form does not
    /// occupy. Without it, one stray far vertex draws as loudly as the surface.
    #[test]
    fn a_thin_far_scattering_stays_under_the_floor_a_near_body_sets() {
        let near: Vec<Point> = (0..400)
            .map(|_| Point {
                x: 0.1,
                y: 0.1,
                depth: 0.0,
            })
            .collect();
        let stray = Point {
            x: 0.9,
            y: 0.9,
            depth: 1.0,
        };

        let mut with_body = near.clone();
        with_body.push(stray);
        let shadowed = render(&with_body, 4, 2, PlotStyle::Braille);
        assert_eq!(
            shadowed[1].chars().last(),
            Some('\u{2800}'),
            "a single far point lit a cell beside a dense near body: {shadowed:?}"
        );

        // On its own it is the whole grid's peak, so it does draw. The floor
        // is relative, not an absolute cull that would lose sparse models.
        let alone = render(&[stray], 4, 2, PlotStyle::Braille);
        assert_ne!(
            alone[1].chars().last(),
            Some('\u{2800}'),
            "the floor swallowed the only point there was"
        );
    }

    /// The ramp is used across its range rather than bunching in one rung,
    /// which is what makes an ASCII plot read as shading rather than as a
    /// stencil.
    #[test]
    fn the_ascii_ramp_uses_more_than_one_rung_on_a_real_shape() {
        let mut points = Vec::new();
        for xi in 0..64u16 {
            for yi in 0..64u16 {
                let x = f32::from(xi) / 63.0;
                let y = f32::from(yi) / 63.0;
                let (dx, dy) = (x - 0.5, y - 0.5);
                if dx * dx + dy * dy <= 0.25 {
                    points.push(Point::flat(x, y));
                }
            }
        }
        let drawn: String = render(&points, 12, 6, PlotStyle::Ascii).join("");
        let mut rungs: Vec<char> = drawn.chars().filter(|c| *c != ' ').collect();
        rungs.sort_unstable();
        rungs.dedup();
        assert!(
            rungs.len() >= 2,
            "the ramp collapsed to {rungs:?}, so the plot is a stencil"
        );
    }

    /// Addition commutes, so the grid cannot depend on the order points
    /// arrived in. That is what makes the output deterministic without the
    /// caller sorting first.
    #[test]
    fn the_same_points_in_any_order_render_identically() {
        let mut points: Vec<Point> = (0..64u16)
            .map(|i| {
                let t = f32::from(i) / 64.0;
                Point {
                    x: t,
                    y: 1.0 - t,
                    depth: t,
                }
            })
            .collect();
        let forward = render(&points, 10, 3, PlotStyle::Braille);
        points.reverse();
        let backward = render(&points, 10, 3, PlotStyle::Braille);
        assert_eq!(forward, backward);
    }

    /// Anything outside the unit square is dropped rather than folded back on
    /// to an edge, where it would draw a line that is not in the data.
    #[test]
    fn points_outside_the_unit_square_are_dropped() {
        let rows = render(
            &[
                Point::flat(-0.1, 0.5),
                Point::flat(1.1, 0.5),
                Point::flat(0.5, -2.0),
                Point::flat(0.5, 9.0),
            ],
            4,
            2,
            PlotStyle::Braille,
        );
        assert!(
            rows.iter().all(|row| row.chars().all(|c| c == '\u{2800}')),
            "{rows:?}"
        );
    }

    /// The far edge belongs to the last dot rather than to one past the end.
    #[test]
    fn the_far_corner_lands_inside_the_grid() {
        let rows = render(&[Point::flat(1.0, 1.0)], 2, 1, PlotStyle::Braille);
        assert_eq!(rows[0].chars().count(), 2);
        assert_ne!(
            rows[0].chars().nth(1),
            Some('\u{2800}'),
            "the corner point did not reach the last cell"
        );
    }

    #[test]
    fn a_zero_sized_panel_renders_nothing_rather_than_panicking() {
        assert!(
            render(&[Point::flat(0.5, 0.5)], 0, 4, PlotStyle::Braille).is_empty()
                || render(&[Point::flat(0.5, 0.5)], 0, 4, PlotStyle::Braille)
                    .iter()
                    .all(String::is_empty)
        );
        assert!(render(&[Point::flat(0.5, 0.5)], 4, 0, PlotStyle::Braille).is_empty());
    }

    /// Nothing here may know what it is drawing. A near point and a far point
    /// differ only by their weight, and that is the whole of the model this
    /// module has.
    #[test]
    fn depth_weighting_is_monotonic_and_never_reaches_zero() {
        assert!(weight_at(0.0) > weight_at(0.5));
        assert!(weight_at(0.5) > weight_at(1.0));
        assert!(weight_at(1.0) > 0.0, "the far side must still contribute");
        assert_eq!(weight_at(-5.0), weight_at(0.0), "clamped");
        assert_eq!(weight_at(5.0), weight_at(1.0), "clamped");
    }
}
