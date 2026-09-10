//! How a node looks, and every state it can be in while looking that way.
//!
//! **The rules are shared, the art is not.** Which silhouette family a
//! node belongs to, which glyph key it falls back to, and what its
//! summary line says are all answered by `solarxy_studio`; what a
//! hexagon looks like and how thick its stroke is are answered here,
//! because a rule that returned a rectangle would have picked a toolkit.
//! The release's exit criteria name that split and sanction the two
//! copies of the art; what they do not sanction is the two disagreeing,
//! which is why the glyph table beside this one is compared against the
//! browser's character for character.
//!
//! ## The box, and why the body may be smaller
//!
//! Every role occupies one 112 by 32 layout box. Wires, handles, labels,
//! selection and auto-layout all measure against that box, so a role
//! cannot change how a graph lays out by changing how it looks. The
//! visible body inside it may be smaller when the size means something:
//! the three root-placeable roles are fixture pills, the text datablock
//! recedes, and a terminal is a donut. Only three roles carry a shaped
//! outline, all of them subflow operators, and all three are left-right
//! symmetric.
//!
//! ## The states are a vocabulary, not decoration
//!
//! Eight of them, and each answers a question a user asks while working:
//! which node am I editing, which one feeds the viewport, is this one
//! switched off, is the graph up to date, is it working right now, is
//! this object hidden. Two are approximations of what the browser draws
//! in CSS, both noted where they are painted.

use egui::{Color32, CornerRadius, Painter, Pos2, Rect, Stroke, Vec2, pos2, vec2};
use solarxy_graph::registry::NodeRole;

use super::vector;
use crate::gui::theme::Theme;

/// The one layout box every role occupies.
pub(super) const NODE_BOX: Vec2 = vec2(112.0, 32.0);

/// The corner radius of an unshaped body, matching the browser's 6px.
const BODY_RADIUS: u8 = 6;

/// The corner radius of a root fixture pill.
const PILL_RADIUS: u8 = 12;

/// The glyph chip's side, and the stroke every glyph is drawn with.
const GLYPH_SIDE: f32 = 20.0;
const GLYPH_STROKE: f32 = 1.5;

/// Below this scale the type label and the description are dropped, so a
/// zoomed-out graph stays legible rather than becoming a smear of text.
const LOD_TYPE_LABEL: f32 = 0.7;
const LOD_DESCRIPTION: f32 = 0.9;

/// The visible body's size for a role, centred inside [`NODE_BOX`].
///
/// A body is never larger than the box, and the difference is even on
/// both axes so the centring lands on whole pixels.
#[must_use]
pub(super) fn body_size(role: NodeRole) -> Vec2 {
    match role {
        // The three root-placeable roles are fixture pills, told apart by
        // pastel, glyph and label rather than by outline.
        NodeRole::Container | NodeRole::Light | NodeRole::Camera => vec2(96.0, 28.0),
        // A stored datablock rather than an operation: quiet by size, so
        // a script library does not read as graph structure.
        NodeRole::Text => vec2(76.0, 26.0),
        // A terminal is a donut, and the only round body.
        NodeRole::Terminal => vec2(28.0, 28.0),
        NodeRole::Standard
        | NodeRole::Gather
        | NodeRole::Branch
        | NodeRole::Analyzer
        | NodeRole::ImageSource
        | NodeRole::Note => NODE_BOX,
    }
}

/// The shaped outline for a role, in box coordinates, or `None` for a
/// role that draws as a rectangle or a pill.
///
/// Three roles, all subflow operators, all symmetric about the box's
/// vertical centre line. `every_silhouette_is_symmetric` holds them to
/// it, because an asymmetric one reads as a mistake rather than as a
/// direction.
#[must_use]
pub(super) fn silhouette(role: NodeRole) -> Option<Vec<Pos2>> {
    let points: &[vector::RoundedVertex] = match role {
        // The two-way junction: a symmetric hexagon.
        NodeRole::Branch => &[
            (0.0, 16.0, 5.0),
            (18.0, 0.0, 4.0),
            (94.0, 0.0, 4.0),
            (112.0, 16.0, 5.0),
            (94.0, 32.0, 4.0),
            (18.0, 32.0, 4.0),
        ],
        // Wide intake, narrowed readout: a symmetric trapezoid.
        NodeRole::Analyzer => &[
            (0.0, 0.0, 4.0),
            (112.0, 0.0, 4.0),
            (102.0, 32.0, 4.0),
            (10.0, 32.0, 4.0),
        ],
        // A file ticket with both top corners chamfered.
        NodeRole::ImageSource => &[
            (14.0, 0.0, 3.0),
            (98.0, 0.0, 3.0),
            (112.0, 14.0, 3.0),
            (112.0, 32.0, 4.0),
            (0.0, 32.0, 4.0),
            (0.0, 14.0, 3.0),
        ],
        _ => return None,
    };
    Some(vector::rounded_polygon(points))
}

/// Everything about a node that changes how it is painted.
#[derive(Debug, Clone, Copy)]
pub(super) struct NodeVisual {
    pub role: NodeRole,
    pub fill: Color32,
    pub selected: bool,
    pub bypassed: bool,
    /// Manual mode, and the node needs a recook that is not coming until
    /// asked for.
    pub stale: bool,
    /// Auto mode, and a recook is on its way.
    pub pending: bool,
    pub cooking: bool,
    /// Holds the display flag in a subflow: what this context shows.
    pub is_display: bool,
    /// A root object whose `visible` param is off. Still cooked.
    pub hidden: bool,
}

/// Paint a node's body and every state riding on it, into the box the
/// substrate laid out.
///
/// Order is the drawing order and it matters: the halo sits behind the
/// body so wires read through it, the hatch and the glyph sit on the
/// body, and the selection ring and cook arc sit above everything so
/// neither is ever hidden by a fill.
pub(super) fn paint_body(
    painter: &Painter,
    box_rect: Rect,
    glyph: &str,
    visual: NodeVisual,
    theme: Theme,
) {
    if visual.is_display {
        paint_display_halo(painter, box_rect, theme);
    }

    let body = body_rect(box_rect, visual.role);
    let fill = state_fill(visual, theme);
    let border = Stroke::new(1.0_f32, theme.border);

    if let Some(outline) = silhouette(visual.role) {
        let map = vector::fit(NODE_BOX, box_rect);
        let points: Vec<Pos2> = outline.into_iter().map(map).collect();
        painter.add(egui::Shape::convex_polygon(points, fill, border));
    } else if visual.role == NodeRole::Terminal {
        // A donut: a thick ring in the category fill with a small core.
        let radius = body.width() / 2.0;
        painter.circle_stroke(body.center(), radius - 3.5, Stroke::new(7.0_f32, fill));
        painter.circle_filled(body.center(), 5.0, theme.fg);
    } else {
        painter.rect(
            body,
            CornerRadius::same(pill_or_box_radius(visual.role)),
            fill,
            border,
            egui::StrokeKind::Inside,
        );
    }

    if visual.role == NodeRole::Gather {
        paint_gather_dome(painter, box_rect, theme);
    }
    if visual.bypassed {
        paint_bypass_hatch(painter, body, visual.role);
    }
    if visual.role != NodeRole::Terminal {
        let chip = Rect::from_center_size(box_rect.center(), Vec2::splat(GLYPH_SIDE));
        super::glyphs::paint(painter, glyph, chip, glyph_ink(fill, theme), GLYPH_STROKE);
    }
    if visual.selected {
        painter.rect_stroke(
            body.expand(2.0),
            CornerRadius::same(pill_or_box_radius(visual.role)),
            Stroke::new(2.0_f32, theme.accent),
            egui::StrokeKind::Outside,
        );
    }
    if visual.cooking {
        paint_cook_arc(painter, box_rect, theme);
    }
}

/// The visible body inside the layout box.
#[must_use]
pub(super) fn body_rect(box_rect: Rect, role: NodeRole) -> Rect {
    let size = body_size(role);
    let scale = box_rect.width() / NODE_BOX.x;
    Rect::from_center_size(box_rect.center(), size * scale)
}

fn pill_or_box_radius(role: NodeRole) -> u8 {
    match role {
        NodeRole::Container | NodeRole::Light | NodeRole::Camera => PILL_RADIUS,
        _ => BODY_RADIUS,
    }
}

/// The category fill, dimmed by whatever the node's cook is doing.
///
/// Bypassed and hidden are the two a user acts on and both stay legible;
/// stale desaturates towards the surface because the node is telling you
/// it is out of date rather than switched off.
fn state_fill(visual: NodeVisual, theme: Theme) -> Color32 {
    let mut fill = visual.fill;
    if visual.stale {
        fill = blend(fill, theme.bg_elevated, 0.45);
    }
    if visual.pending {
        fill = blend(fill, theme.bg_elevated, 0.25);
    }
    if visual.bypassed {
        fill = blend(fill, theme.bg, 0.35);
    }
    if visual.hidden {
        fill = blend(fill, theme.bg, 0.25);
    }
    fill
}

/// The glyph inks directly on the body, so it is the fill pulled towards
/// the ink rather than a colour of its own. The browser mixes the same
/// two at the same ratio.
fn glyph_ink(fill: Color32, theme: Theme) -> Color32 {
    blend(fill, theme.fg, 0.65)
}

fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_lossless
        )]
        {
            (f32::from(x) * (1.0 - t) + f32::from(y) * t)
                .round()
                .clamp(0.0, 255.0) as u8
        }
    };
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

/// The display flag's halo.
///
/// **An approximation, deliberately.** The browser paints a two-stop
/// radial gradient in CSS; the painter here has no gradient shape, so it
/// is concentric rings falling off in alpha. Behind the body, at a
/// radius wider than the box, so a wire passing under it still reads.
fn paint_display_halo(painter: &Painter, box_rect: Rect, theme: Theme) {
    const RINGS: usize = 6;
    let centre = box_rect.center();
    let outer = box_rect.width() * 0.66;
    for ring in (0..RINGS).rev() {
        #[allow(clippy::cast_precision_loss)]
        let t = (ring + 1) as f32 / RINGS as f32;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let alpha = (46.0 * (1.0 - t) + 10.0) as u8;
        painter.circle_filled(
            centre,
            outer * t,
            theme.accent.gamma_multiply(f32::from(alpha) / 255.0),
        );
    }
}

/// The gather role's dome: a riser on the box's top edge saying that this
/// node collects rather than transforms.
fn paint_gather_dome(painter: &Painter, box_rect: Rect, theme: Theme) {
    let scale = box_rect.width() / NODE_BOX.x;
    let dome = Rect::from_min_size(
        pos2(box_rect.left() + 16.0 * scale, box_rect.top() - 8.0 * scale),
        vec2(80.0 * scale, 8.0 * scale),
    );
    painter.rect_filled(
        dome,
        CornerRadius {
            nw: PILL_RADIUS,
            ne: PILL_RADIUS,
            sw: 0,
            se: 0,
        },
        theme.muted,
    );
}

/// Bypass reads as diagonal stripes over the body, which is a texture
/// rather than a colour and therefore survives a reader who cannot tell
/// the dimmed fill from the undimmed one.
fn paint_bypass_hatch(painter: &Painter, body: Rect, role: NodeRole) {
    let stripe = Stroke::new(1.0_f32, Color32::from_black_alpha(38));
    let step = 10.0;
    let mut x = body.left() - body.height();
    while x < body.right() {
        let a = pos2(x.max(body.left()), body.bottom() - (x.max(body.left()) - x));
        let b = pos2(
            (x + body.height()).min(body.right()),
            body.bottom() - ((x + body.height()).min(body.right()) - x),
        );
        if role == NodeRole::Terminal {
            break;
        }
        painter.line_segment([a, b], stripe);
        x += step;
    }
}

/// The cook arc: a three-quarter ring saying this node is working right
/// now.
///
/// **The second approximation.** The browser spins its ring in CSS; this
/// draws the arc and asks for a repaint, so the shell animates it the way
/// it animates everything else. The gap's direction is fixed rather than
/// rotating, which is the part not carried over: what the ring says is
/// "working", and it says it either way.
fn paint_cook_arc(painter: &Painter, box_rect: Rect, theme: Theme) {
    const SEGMENTS: usize = 24;
    let centre = box_rect.center();
    let radius = 13.0 * (box_rect.width() / NODE_BOX.x);
    let mut points = Vec::with_capacity(SEGMENTS + 1);
    for step in 0..=SEGMENTS {
        #[allow(clippy::cast_precision_loss)]
        let t = step as f32 / SEGMENTS as f32;
        let angle = t * std::f32::consts::TAU * 0.75;
        points.push(pos2(
            centre.x + radius * angle.cos(),
            centre.y + radius * angle.sin(),
        ));
    }
    painter.add(egui::Shape::line(
        points,
        Stroke::new(2.0_f32, theme.accent),
    ));
}

/// Which rows of the label stack this zoom level shows.
///
/// Two thresholds rather than a continuous fade, so panning never
/// re-lays-out the stack: a row is in or out, and it changes only when
/// the zoom crosses one of two numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LabelDetail {
    pub type_label: bool,
    pub description: bool,
}

#[must_use]
pub(super) fn label_detail(scale: f32) -> LabelDetail {
    LabelDetail {
        type_label: scale >= LOD_TYPE_LABEL,
        description: scale >= LOD_DESCRIPTION,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EVERY_ROLE: [NodeRole; 11] = [
        NodeRole::Standard,
        NodeRole::Container,
        NodeRole::Gather,
        NodeRole::Branch,
        NodeRole::Terminal,
        NodeRole::Analyzer,
        NodeRole::ImageSource,
        NodeRole::Light,
        NodeRole::Camera,
        NodeRole::Text,
        NodeRole::Note,
    ];

    /// A body that outgrew its box would push into a neighbour without
    /// changing how the graph lays out, since layout measures the box.
    /// An odd difference would centre it on a half pixel.
    #[test]
    fn every_body_fits_its_box_and_centres_on_whole_pixels() {
        for role in EVERY_ROLE {
            let size = body_size(role);
            assert!(
                size.x <= NODE_BOX.x && size.y <= NODE_BOX.y,
                "{role:?} is larger than the layout box"
            );
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let (dx, dy) = ((NODE_BOX.x - size.x) as i32, (NODE_BOX.y - size.y) as i32);
            assert_eq!(dx % 2, 0, "{role:?} centres on a half pixel horizontally");
            assert_eq!(dy % 2, 0, "{role:?} centres on a half pixel vertically");
        }
    }

    /// A silhouette that leaned would read as a direction the node does
    /// not have. Every point must have a mirror twin across the box's
    /// vertical centre line.
    #[test]
    fn every_silhouette_is_symmetric_and_inside_its_box() {
        let shaped: Vec<NodeRole> = EVERY_ROLE
            .into_iter()
            .filter(|r| silhouette(*r).is_some())
            .collect();
        assert_eq!(
            shaped,
            vec![NodeRole::Branch, NodeRole::Analyzer, NodeRole::ImageSource],
            "exactly the three subflow operators carry a shaped outline"
        );

        for role in shaped {
            let points = silhouette(role).expect("filtered to the shaped roles");
            for p in &points {
                assert!(
                    (-0.01..=NODE_BOX.x + 0.01).contains(&p.x)
                        && (-0.01..=NODE_BOX.y + 0.01).contains(&p.y),
                    "{role:?} leaves its box at {p:?}"
                );
                let twin = NODE_BOX.x - p.x;
                assert!(
                    points.iter().any(|q| (q.x - twin).abs() < 0.05),
                    "{role:?} has no mirror for {p:?}"
                );
            }
        }
    }

    /// The stack drops rows as it zooms out, never gains them, and the
    /// description goes before the type label because it is the longest
    /// and wraps.
    #[test]
    fn the_label_stack_only_sheds_rows_as_it_zooms_out() {
        let far = label_detail(0.4);
        let mid = label_detail(0.8);
        let near = label_detail(1.0);

        assert_eq!(
            far,
            LabelDetail {
                type_label: false,
                description: false
            }
        );
        assert_eq!(
            mid,
            LabelDetail {
                type_label: true,
                description: false
            }
        );
        assert_eq!(
            near,
            LabelDetail {
                type_label: true,
                description: true
            }
        );
    }
}
