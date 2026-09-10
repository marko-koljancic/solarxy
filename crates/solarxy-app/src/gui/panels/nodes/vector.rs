//! Turning an authored outline into something the immediate-mode painter
//! can draw.
//!
//! Two shapes of input arrive here and both are authored rather than
//! computed: the seventy-six glyph paths, which are SVG path data lifted
//! from the design source, and the three node silhouettes, which are
//! rounded polygons given as vertices. Both are held verbatim rather than
//! pre-flattened into vertex arrays, for the same reason: a path string
//! can be compared character for character against the browser's copy,
//! and a table of floats cannot. That comparison is the only thing making
//! two copies of one drawing safe.
//!
//! ## Why a path parser rather than a dependency
//!
//! The subset actually used is small and closed: move, line, horizontal,
//! vertical, cubic, smooth cubic, arc and close, in both absolute and
//! relative forms. `every_glyph_parses_into_something_drawable` walks the
//! real table and holds the parser to it, so the subset is defined by
//! what the art needs rather than by what the grammar allows.
//!
//! Everything here is pure and takes no `Ui`, which is what lets the
//! whole vocabulary be tested without a frame.

use std::f32::consts::PI;

use egui::{Pos2, Rect, Vec2, pos2, vec2};

/// How finely a curve is flattened. A glyph is drawn at roughly sixteen
/// pixels and a silhouette corner spans four, so eight segments per curve
/// is already past the point where another one is visible, and the cost
/// is paid once per frame per node rather than per pixel.
const CURVE_SEGMENTS: usize = 8;

/// A polygon vertex with the radius its corner is rounded by.
pub(super) type RoundedVertex = (f32, f32, f32);

/// A closed outline with per-vertex rounded corners, in the coordinate
/// space its vertices were authored in.
///
/// Each corner enters and leaves `r` along its adjacent edges, clamped to
/// half the edge so a short edge cannot overshoot, and turns through a
/// quadratic curve at the vertex. The browser's `roundedPolygonPath`
/// emits the same geometry as SVG; this emits it as points, because the
/// painter wants points and going through a path string would mean
/// writing one only to parse it back.
#[must_use]
pub(super) fn rounded_polygon(points: &[RoundedVertex]) -> Vec<Pos2> {
    let n = points.len();
    let mut out = Vec::with_capacity(n * (CURVE_SEGMENTS + 1));
    for i in 0..n {
        let (px, py, r) = points[i];
        let (ax, ay, _) = points[(i + n - 1) % n];
        let (bx, by, _) = points[(i + 1) % n];
        let p = pos2(px, py);
        let d_in = vec2(px - ax, py - ay).length();
        let d_out = vec2(bx - px, by - py).length();
        if d_in <= f32::EPSILON || d_out <= f32::EPSILON {
            out.push(p);
            continue;
        }
        let r_in = r.min(d_in / 2.0);
        let r_out = r.min(d_out / 2.0);
        let enter = pos2(px + (ax - px) / d_in * r_in, py + (ay - py) / d_in * r_in);
        let leave = pos2(
            px + (bx - px) / d_out * r_out,
            py + (by - py) / d_out * r_out,
        );
        out.push(enter);
        for step in 1..=CURVE_SEGMENTS {
            #[allow(clippy::cast_precision_loss)]
            let t = step as f32 / CURVE_SEGMENTS as f32;
            out.push(quadratic(enter, p, leave, t));
        }
    }
    out
}

fn quadratic(start: Pos2, control: Pos2, end: Pos2, at: f32) -> Pos2 {
    let rest = 1.0 - at;
    pos2(
        rest * rest * start.x + 2.0 * rest * at * control.x + at * at * end.x,
        rest * rest * start.y + 2.0 * rest * at * control.y + at * at * end.y,
    )
}

fn cubic(start: Pos2, first: Pos2, second: Pos2, end: Pos2, at: f32) -> Pos2 {
    let rest = 1.0 - at;
    let (a, b) = (rest * rest * rest, 3.0 * rest * rest * at);
    let (c, d) = (3.0 * rest * at * at, at * at * at);
    pos2(
        a * start.x + b * first.x + c * second.x + d * end.x,
        a * start.y + b * first.y + c * second.y + d * end.y,
    )
}

/// One continuous run of an outline, flattened to points in the path's
/// own coordinate space.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SubPath {
    pub points: Vec<Pos2>,
    pub closed: bool,
}

/// Flatten SVG path data into subpaths.
///
/// Returns an empty list for data the subset cannot read, which is a
/// deliberate choice rather than a silent one: nothing calls this with an
/// unchecked string, the only caller reads a table the drift test holds
/// against the browser's, and a test walks that table asserting every
/// entry yields something drawable. A panic here would turn a bad glyph
/// into a crashed shell.
#[must_use]
pub(super) fn flatten_path(data: &str) -> Vec<SubPath> {
    let mut lexer = Lexer::new(data);
    let mut subpaths: Vec<SubPath> = Vec::new();
    let mut current: Vec<Pos2> = Vec::new();
    let mut cursor = Pos2::ZERO;
    let mut start = Pos2::ZERO;
    // The previous cubic's second control point, reflected by a smooth
    // curve. `None` whenever the previous command was not a cubic, which
    // is what the specification says makes the reflection the current
    // point itself.
    let mut last_cubic_control: Option<Pos2> = None;
    let mut command = '\0';

    while let Some(next) = lexer.peek_command_or_number() {
        match next {
            Token::Command(c) => {
                lexer.take_command();
                command = c;
            }
            // A repeated coordinate run continues the previous command,
            // except that a repeated move is a line, which is the one
            // place the grammar is not simply "do it again".
            Token::Number => match command {
                'M' => command = 'L',
                'm' => command = 'l',
                '\0' => return Vec::new(),
                _ => {}
            },
        }

        let relative = command.is_ascii_lowercase();
        let base = if relative { cursor } else { Pos2::ZERO };
        match command.to_ascii_uppercase() {
            'M' => {
                let Some(p) = lexer.point(base) else {
                    return Vec::new();
                };
                flush(&mut subpaths, &mut current, false);
                cursor = p;
                start = p;
                current.push(p);
                last_cubic_control = None;
            }
            'L' => {
                let Some(p) = lexer.point(base) else {
                    return Vec::new();
                };
                cursor = p;
                current.push(p);
                last_cubic_control = None;
            }
            'H' => {
                let Some(x) = lexer.number() else {
                    return Vec::new();
                };
                cursor = pos2(base.x + x, cursor.y);
                current.push(cursor);
                last_cubic_control = None;
            }
            'V' => {
                let Some(y) = lexer.number() else {
                    return Vec::new();
                };
                cursor = pos2(cursor.x, base.y + y);
                current.push(cursor);
                last_cubic_control = None;
            }
            'C' | 'S' => {
                let c1 = if command.eq_ignore_ascii_case(&'S') {
                    // The reflection of the previous control point about
                    // the current point, or the current point itself when
                    // the previous command was not a cubic.
                    last_cubic_control.map_or(cursor, |prev| cursor + (cursor - prev))
                } else {
                    match lexer.point(base) {
                        Some(p) => p,
                        None => return Vec::new(),
                    }
                };
                let (Some(c2), Some(end)) = (lexer.point(base), lexer.point(base)) else {
                    return Vec::new();
                };
                if current.is_empty() {
                    current.push(cursor);
                }
                for step in 1..=CURVE_SEGMENTS {
                    #[allow(clippy::cast_precision_loss)]
                    let t = step as f32 / CURVE_SEGMENTS as f32;
                    current.push(cubic(cursor, c1, c2, end, t));
                }
                cursor = end;
                last_cubic_control = Some(c2);
            }
            'A' => {
                let (Some(rx), Some(ry), Some(rotation), Some(large), Some(sweep)) = (
                    lexer.number(),
                    lexer.number(),
                    lexer.number(),
                    lexer.flag(),
                    lexer.flag(),
                ) else {
                    return Vec::new();
                };
                let Some(end) = lexer.point(base) else {
                    return Vec::new();
                };
                if current.is_empty() {
                    current.push(cursor);
                }
                arc_to(&mut current, cursor, rx, ry, rotation, large, sweep, end);
                cursor = end;
                last_cubic_control = None;
            }
            'Z' => {
                flush(&mut subpaths, &mut current, true);
                cursor = start;
                last_cubic_control = None;
            }
            _ => return Vec::new(),
        }
    }
    flush(&mut subpaths, &mut current, false);
    subpaths
}

fn flush(out: &mut Vec<SubPath>, current: &mut Vec<Pos2>, closed: bool) {
    if current.len() > 1 {
        out.push(SubPath {
            points: std::mem::take(current),
            closed,
        });
    } else {
        current.clear();
    }
}

/// Append an elliptical arc, flattened.
///
/// The endpoint parameterization the path grammar uses says where the arc
/// ends; the drawing wants a centre and two angles, so this is the
/// standard conversion between them. Two of its steps are easy to leave
/// out and both are load-bearing: radii too small to reach the endpoint
/// are scaled up rather than refused, which is what lets a glyph author
/// write a circle as two half-arcs with exact radii, and a degenerate
/// radius degrades to a straight line rather than to a division by zero.
#[allow(clippy::too_many_arguments)]
fn arc_to(
    out: &mut Vec<Pos2>,
    from: Pos2,
    rx: f32,
    ry: f32,
    rotation_deg: f32,
    large: bool,
    sweep: bool,
    to: Pos2,
) {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx < f32::EPSILON || ry < f32::EPSILON || from == to {
        out.push(to);
        return;
    }
    let phi = rotation_deg.to_radians();
    let (sin_phi, cos_phi) = phi.sin_cos();

    let dx2 = (from.x - to.x) / 2.0;
    let dy2 = (from.y - to.y) / 2.0;
    let x1 = cos_phi * dx2 + sin_phi * dy2;
    let y1 = -sin_phi * dx2 + cos_phi * dy2;

    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        let scale = lambda.sqrt();
        rx *= scale;
        ry *= scale;
    }

    let num = (rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1).max(0.0);
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    if den < f32::EPSILON {
        out.push(to);
        return;
    }
    let coef = (num / den).sqrt() * if large == sweep { -1.0 } else { 1.0 };
    let cx1 = coef * rx * y1 / ry;
    let cy1 = -coef * ry * x1 / rx;

    let cx = cos_phi * cx1 - sin_phi * cy1 + f32::midpoint(from.x, to.x);
    let cy = sin_phi * cx1 + cos_phi * cy1 + f32::midpoint(from.y, to.y);

    let angle = |ux: f32, uy: f32| -> f32 { uy.atan2(ux) };
    let theta = angle((x1 - cx1) / rx, (y1 - cy1) / ry);
    let theta_end = angle((-x1 - cx1) / rx, (-y1 - cy1) / ry);
    let mut delta = theta_end - theta;
    if !sweep && delta > 0.0 {
        delta -= 2.0 * PI;
    } else if sweep && delta < 0.0 {
        delta += 2.0 * PI;
    }

    // A half turn or more gets proportionally more segments, so a full
    // circle written as one arc is as smooth as one written as two.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let steps = (CURVE_SEGMENTS as f32 * (delta.abs() / PI)).ceil().max(2.0) as usize;
    for step in 1..=steps {
        #[allow(clippy::cast_precision_loss)]
        let t = theta + delta * (step as f32 / steps as f32);
        let (sin_t, cos_t) = t.sin_cos();
        out.push(pos2(
            cx + cos_phi * rx * cos_t - sin_phi * ry * sin_t,
            cy + sin_phi * rx * cos_t + cos_phi * ry * sin_t,
        ));
    }
}

/// Map a point authored in a `source`-sized box onto `rect`, preserving
/// the aspect ratio and centring what is left over.
pub(super) fn fit(source: Vec2, rect: Rect) -> impl Fn(Pos2) -> Pos2 {
    let scale = (rect.width() / source.x).min(rect.height() / source.y);
    let offset = rect.center() - (source * scale * 0.5).to_pos2().to_vec2();
    move |p: Pos2| pos2(offset.x + p.x * scale, offset.y + p.y * scale)
}

enum Token {
    Command(char),
    Number,
}

struct Lexer<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Lexer<'a> {
    fn new(data: &'a str) -> Self {
        Self {
            bytes: data.as_bytes(),
            at: 0,
        }
    }

    fn skip_separators(&mut self) {
        while self.at < self.bytes.len() {
            match self.bytes[self.at] {
                b' ' | b'\t' | b'\n' | b'\r' | b',' => self.at += 1,
                _ => break,
            }
        }
    }

    fn peek_command_or_number(&mut self) -> Option<Token> {
        self.skip_separators();
        let b = *self.bytes.get(self.at)?;
        if b.is_ascii_alphabetic() {
            Some(Token::Command(b as char))
        } else {
            Some(Token::Number)
        }
    }

    fn take_command(&mut self) {
        self.at += 1;
    }

    fn number(&mut self) -> Option<f32> {
        self.skip_separators();
        let start = self.at;
        if matches!(self.bytes.get(self.at), Some(b'+' | b'-')) {
            self.at += 1;
        }
        while matches!(self.bytes.get(self.at), Some(b) if b.is_ascii_digit()) {
            self.at += 1;
        }
        if self.bytes.get(self.at) == Some(&b'.') {
            self.at += 1;
            while matches!(self.bytes.get(self.at), Some(b) if b.is_ascii_digit()) {
                self.at += 1;
            }
        }
        if matches!(self.bytes.get(self.at), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.bytes.get(self.at), Some(b'+' | b'-')) {
                self.at += 1;
            }
            while matches!(self.bytes.get(self.at), Some(b) if b.is_ascii_digit()) {
                self.at += 1;
            }
        }
        if self.at == start {
            return None;
        }
        std::str::from_utf8(&self.bytes[start..self.at])
            .ok()?
            .parse()
            .ok()
    }

    /// An arc's two flags, which the grammar lets an author write with no
    /// separator at all: `a1 1 0 1 1 2 0` is seven numbers, and the fifth
    /// and sixth are single digits that may be glued to what follows.
    fn flag(&mut self) -> Option<bool> {
        self.skip_separators();
        match self.bytes.get(self.at)? {
            b'0' => {
                self.at += 1;
                Some(false)
            }
            b'1' => {
                self.at += 1;
                Some(true)
            }
            _ => None,
        }
    }

    fn point(&mut self, base: Pos2) -> Option<Pos2> {
        let x = self.number()?;
        let y = self.number()?;
        Some(pos2(base.x + x, base.y + y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ends(data: &str) -> Vec<(Pos2, Pos2)> {
        flatten_path(data)
            .into_iter()
            .map(|s| {
                (
                    *s.points.first().expect("a run has a first point"),
                    *s.points.last().expect("a run has a last point"),
                )
            })
            .collect()
    }

    fn close_to(a: Pos2, b: Pos2) -> bool {
        (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01
    }

    #[test]
    fn a_move_starts_a_run_and_a_second_pair_continues_it_as_a_line() {
        let subs = flatten_path("M1 2 3 4");
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].points, vec![pos2(1.0, 2.0), pos2(3.0, 4.0)]);
    }

    #[test]
    fn a_relative_move_starts_a_second_run_from_where_the_first_ended() {
        let subs = flatten_path("M1 1 L3 1 m0 2 l2 0");
        assert_eq!(subs.len(), 2);
        assert_eq!(subs[1].points, vec![pos2(3.0, 3.0), pos2(5.0, 3.0)]);
    }

    #[test]
    fn horizontal_and_vertical_keep_the_other_axis() {
        let subs = flatten_path("M2 3 H8 V9 h-2 v-1");
        assert_eq!(
            subs[0].points,
            vec![
                pos2(2.0, 3.0),
                pos2(8.0, 3.0),
                pos2(8.0, 9.0),
                pos2(6.0, 9.0),
                pos2(6.0, 8.0),
            ]
        );
    }

    #[test]
    fn a_close_marks_the_run_and_returns_the_cursor_to_its_start() {
        let subs = flatten_path("M0 0 L4 0 L4 4 z m1 1 l1 0");
        assert!(subs[0].closed);
        // The relative move after the close starts from the run's start,
        // not from where the line ended.
        assert_eq!(subs[1].points[0], pos2(1.0, 1.0));
    }

    #[test]
    fn a_cubic_lands_on_its_endpoint() {
        let subs = flatten_path("M0 0 C0 4 8 4 8 0");
        let last = *subs[0].points.last().expect("the curve has points");
        assert!(close_to(last, pos2(8.0, 0.0)), "landed at {last:?}");
        assert!(subs[0].points.len() > 4, "the curve was not flattened");
    }

    /// A smooth cubic reflects the previous control point about the
    /// current point. Written out longhand, the two must agree.
    #[test]
    fn a_smooth_cubic_reflects_the_previous_control_point() {
        let smooth = flatten_path("M0 0 C0 4 4 4 4 0 S8 -4 8 0");
        let longhand = flatten_path("M0 0 C0 4 4 4 4 0 C4 -4 8 -4 8 0");
        assert_eq!(smooth[0].points, longhand[0].points);
    }

    #[test]
    fn an_arc_lands_on_its_endpoint_and_bows_the_way_the_sweep_says() {
        let up = flatten_path("M2 8 a6 6 0 0 1 12 0");
        let down = flatten_path("M2 8 a6 6 0 0 0 12 0");
        for subs in [&up, &down] {
            let last = *subs[0].points.last().expect("the arc has points");
            assert!(close_to(last, pos2(14.0, 8.0)), "landed at {last:?}");
        }
        let mid_up = up[0].points[up[0].points.len() / 2];
        let mid_down = down[0].points[down[0].points.len() / 2];
        assert!(
            mid_up.y < 8.0 && mid_down.y > 8.0,
            "the two sweeps bow the same way: {mid_up:?} and {mid_down:?}"
        );
    }

    /// Two things about an arc's flags, and only the second needs the
    /// `flag` reader.
    ///
    /// A flag glued to a following negative coordinate (`1 1-12`) is how
    /// every circle in the glyph table is written, and it parses whether
    /// the flags are read as digits or as numbers, because a number stops
    /// at a sign it did not start with. **Two flags glued to each other**
    /// (`11`) does not: read as a number that is eleven, and the
    /// coordinates behind it shift by one. The grammar permits it, this
    /// art does not use it, and the reader is a digit at a time so that
    /// stays true if it ever does.
    #[test]
    fn the_arc_flags_are_read_one_digit_at_a_time() {
        let spaced = ends("M14 8 a6 6 0 1 1 -12 0");

        let glued_coordinate = ends("M14 8a6 6 0 1 1-12 0");
        assert_eq!(glued_coordinate.len(), 1);
        assert!(close_to(glued_coordinate[0].1, spaced[0].1));

        let glued_flags = ends("M14 8 a6 6 0 11 -12 0");
        assert_eq!(glued_flags.len(), 1, "the glued pair lost the arc");
        assert!(
            close_to(glued_flags[0].1, spaced[0].1),
            "the glued pair landed at {:?} rather than {:?}",
            glued_flags[0].1,
            spaced[0].1
        );
    }

    /// Radii too small to span the endpoints are scaled up rather than
    /// refused, which is what lets an author write a circle as two half
    /// arcs with exact radii and have it close.
    #[test]
    fn radii_too_small_to_reach_are_grown_rather_than_refused() {
        let subs = flatten_path("M0 0 a1 1 0 0 1 10 0");
        let last = *subs[0].points.last().expect("the arc has points");
        assert!(close_to(last, pos2(10.0, 0.0)), "landed at {last:?}");
    }

    #[test]
    fn a_zero_radius_degrades_to_a_line_rather_than_dividing_by_zero() {
        let subs = flatten_path("M0 0 a0 0 0 0 1 4 0");
        assert_eq!(subs[0].points, vec![pos2(0.0, 0.0), pos2(4.0, 0.0)]);
    }

    /// Data the subset cannot read yields nothing rather than a panic: a
    /// bad glyph must draw as an empty chip, never as a crashed shell.
    #[test]
    fn unreadable_data_yields_nothing_rather_than_panicking() {
        assert!(flatten_path("Q0 0 1 1").is_empty());
        assert!(flatten_path("M0").is_empty());
        assert!(flatten_path("4 5 6").is_empty());
        assert!(flatten_path("").is_empty());
    }

    #[test]
    fn a_rounded_corner_stays_inside_the_polygon_it_rounds() {
        let square = rounded_polygon(&[
            (0.0, 0.0, 4.0),
            (10.0, 0.0, 4.0),
            (10.0, 10.0, 4.0),
            (0.0, 10.0, 4.0),
        ]);
        for p in &square {
            assert!(
                (-0.01..=10.01).contains(&p.x) && (-0.01..=10.01).contains(&p.y),
                "{p:?} left the square"
            );
        }
        // No point sits exactly on a corner: that is what rounding means.
        assert!(!square.iter().any(|p| close_to(*p, pos2(0.0, 0.0))));
    }

    /// A radius wider than the edge it turns would overshoot into the
    /// next corner, so it is clamped to half the edge instead.
    #[test]
    fn a_radius_wider_than_its_edge_is_clamped_rather_than_overshooting() {
        let thin = rounded_polygon(&[
            (0.0, 0.0, 50.0),
            (4.0, 0.0, 50.0),
            (4.0, 4.0, 50.0),
            (0.0, 4.0, 50.0),
        ]);
        for p in &thin {
            assert!(
                (-0.01..=4.01).contains(&p.x) && (-0.01..=4.01).contains(&p.y),
                "{p:?} overshot the shape"
            );
        }
    }

    #[test]
    fn fitting_preserves_the_aspect_ratio_and_centres_the_slack() {
        let map = fit(
            vec2(16.0, 16.0),
            Rect::from_min_size(pos2(0.0, 0.0), vec2(32.0, 64.0)),
        );
        assert!(close_to(map(pos2(0.0, 0.0)), pos2(0.0, 16.0)));
        assert!(close_to(map(pos2(16.0, 16.0)), pos2(32.0, 48.0)));
        assert!(close_to(map(pos2(8.0, 8.0)), pos2(16.0, 32.0)));
    }
}
