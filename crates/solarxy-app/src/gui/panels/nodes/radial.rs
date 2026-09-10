//! The hover radial: six operations on the node under the pointer.
//!
//! **Disabled rather than absent**, and that is the whole design. An
//! operation that does not apply is drawn dimmed and stays where it was,
//! so the same wedge is always in the same direction and the ring becomes
//! muscle memory. A radial that reflows is a menu that has to be read.
//!
//! ## Screen space, not graph space
//!
//! The ring is measured in pixels and drawn outside the canvas's
//! transform layer, so it reads the same at every zoom rather than
//! ballooning with the node. Only its centre comes from the graph, mapped
//! through the transform each frame, which is what makes it track a pan
//! without the pointer having to move.

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, pos2};
use solarxy_graph::document::NodeId;

use crate::gui::theme::Theme;

/// How long the pointer rests on a node before the ring opens.
pub(super) const DWELL_MS: f64 = 400.0;

/// The band's width, in pixels.
const RING_WIDTH: f32 = 38.0;
/// The smallest inner radius, which clears the fixed body's half width
/// with air so the ring never sits on the side wings.
const MIN_INNER: f32 = 72.0;
/// Added to a measured node radius before the band starts.
const INNER_CLEARANCE: f32 = 10.0;
/// The cap on a measured node radius, so a zoomed-in node does not push
/// the ring off the screen.
const RADIUS_CAP: f32 = 70.0;
/// The pitch is sixty degrees and the sweep fifty-four, which leaves the
/// six-degree gaps between wedges.
const GAP_DEG: f32 = 6.0;
/// How far past the band the pointer may stray before the ring closes.
const GRACE: f32 = 44.0;

/// One operation on the ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Wedge {
    Rename,
    /// The display flag inside a container, or root visibility outside
    /// one. One wedge, because they are the same question asked of two
    /// kinds of network, and the browser puts them in the same place.
    DisplayOrVisibility,
    Dive,
    Info,
    Bypass,
    Delete,
}

impl Wedge {
    /// In ring order, starting at the right and going anticlockwise.
    pub(super) const ALL: [Self; 6] = [
        Self::Rename,
        Self::DisplayOrVisibility,
        Self::Dive,
        Self::Info,
        Self::Bypass,
        Self::Delete,
    ];

    /// The wedge's centre angle in degrees: zero is right, and positive
    /// turns anticlockwise.
    fn angle(self) -> f32 {
        let index = Self::ALL
            .iter()
            .position(|w| *w == self)
            .unwrap_or_default();
        #[allow(clippy::cast_precision_loss)]
        {
            index as f32 * 60.0
        }
    }

    /// The mark drawn in the band. Short, because a wedge is read by
    /// direction first and by mark second.
    fn mark(self, root: bool, on: bool) -> &'static str {
        match self {
            Self::Rename => "ab",
            Self::DisplayOrVisibility => {
                if root {
                    if on { "eye" } else { "hid" }
                } else {
                    "disp"
                }
            }
            Self::Dive => "in",
            Self::Info => "i",
            Self::Bypass => {
                if on {
                    "on"
                } else {
                    "byp"
                }
            }
            Self::Delete => "del",
        }
    }

    fn title(self, root: bool) -> &'static str {
        match self {
            Self::Rename => "Rename (F2)",
            Self::DisplayOrVisibility => {
                if root {
                    "Show or hide"
                } else {
                    "Set the display flag"
                }
            }
            Self::Dive => "Enter subflow",
            Self::Info => "Node info",
            Self::Bypass => "Toggle bypass",
            Self::Delete => "Delete node",
        }
    }
}

/// Which operations apply to the node the ring is on.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Applies {
    pub dive: bool,
    pub bypass: bool,
    pub display_or_visibility: bool,
    /// Whether the node is at the root, which decides what the second
    /// wedge means and what it is called.
    pub root: bool,
    pub is_display: bool,
    pub visible: bool,
    pub bypassed: bool,
}

impl Applies {
    fn enabled(self, wedge: Wedge) -> bool {
        match wedge {
            Wedge::Dive => self.dive,
            Wedge::Bypass => self.bypass,
            Wedge::DisplayOrVisibility => self.display_or_visibility,
            Wedge::Rename | Wedge::Info | Wedge::Delete => true,
        }
    }

    fn active(self, wedge: Wedge) -> bool {
        match wedge {
            Wedge::Bypass => self.bypassed,
            Wedge::DisplayOrVisibility => {
                if self.root {
                    self.visible
                } else {
                    self.is_display
                }
            }
            _ => false,
        }
    }
}

/// The ring the canvas is showing, if any.
#[derive(Debug, Clone, Copy)]
pub(super) struct Radial {
    pub node: NodeId,
    /// The node's centre in screen pixels, refreshed every frame so the
    /// ring follows a pan without the pointer moving.
    pub centre: Pos2,
    /// Half the node's larger side in screen pixels, capped.
    pub radius: f32,
}

/// The band's inner radius for a measured node radius.
///
/// Shared by the geometry and by the stray distance, so the ring and the
/// distance at which it closes cannot drift apart.
#[must_use]
pub(super) fn inner_radius(node_radius: f32) -> f32 {
    MIN_INNER.max(node_radius + INNER_CLEARANCE)
}

/// The node radius the ring is built from, capped so a zoomed-in node
/// does not push the band off the screen.
#[must_use]
pub(super) fn anchor_radius(node: Rect) -> f32 {
    (node.width().max(node.height()) / 2.0).min(RADIUS_CAP)
}

/// How far the pointer may be from the centre before the ring closes.
#[must_use]
pub(super) fn stray_distance(node_radius: f32) -> f32 {
    inner_radius(node_radius) + RING_WIDTH + GRACE
}

/// Which wedge a point falls in, or `None` when it is off the band.
///
/// Pure, and the same arithmetic the drawing uses, so what is highlighted
/// is what a click picks.
#[must_use]
pub(super) fn wedge_at(centre: Pos2, node_radius: f32, at: Pos2) -> Option<Wedge> {
    let offset = at - centre;
    let distance = offset.length();
    let inner = inner_radius(node_radius);
    if distance < inner || distance > inner + RING_WIDTH {
        return None;
    }
    // Screen y grows downward, so the angle is negated to keep the ring
    // anticlockwise the way it is authored.
    let degrees = (-offset.y).atan2(offset.x).to_degrees().rem_euclid(360.0);
    Wedge::ALL.into_iter().find(|wedge| {
        let delta = (degrees - wedge.angle() + 180.0).rem_euclid(360.0) - 180.0;
        delta.abs() <= (60.0 - GAP_DEG) / 2.0
    })
}

/// Draw the ring and answer which wedge is under the pointer.
pub(super) fn draw(
    painter: &Painter,
    radial: Radial,
    applies: Applies,
    pointer: Option<Pos2>,
    theme: Theme,
) -> Option<Wedge> {
    let inner = inner_radius(radial.radius);
    let outer = inner + RING_WIDTH;
    let hovered = pointer.and_then(|at| wedge_at(radial.centre, radial.radius, at));

    for wedge in Wedge::ALL {
        let enabled = applies.enabled(wedge);
        let lit = enabled && hovered == Some(wedge);
        let fill = if lit {
            theme.accent
        } else if applies.active(wedge) {
            theme.accent.gamma_multiply(0.55)
        } else {
            theme.bg_elevated
        };
        paint_wedge(
            painter,
            radial.centre,
            inner,
            outer,
            wedge.angle(),
            fill,
            theme,
        );

        let ink = if enabled { theme.fg } else { theme.muted };
        let mid = f32::midpoint(inner, outer);
        let radians = wedge.angle().to_radians();
        let at = pos2(
            radial.centre.x + mid * radians.cos(),
            radial.centre.y - mid * radians.sin(),
        );
        painter.text(
            at,
            Align2::CENTER_CENTER,
            wedge.mark(applies.root, applies.active(wedge)),
            FontId::proportional(11.0),
            ink,
        );
        if lit {
            painter.text(
                pos2(radial.centre.x, radial.centre.y - outer - 12.0),
                Align2::CENTER_CENTER,
                wedge.title(applies.root),
                FontId::proportional(11.0),
                theme.fg,
            );
        }
    }
    hovered.filter(|wedge| applies.enabled(*wedge))
}

/// One band segment, as a filled ring sector.
fn paint_wedge(
    painter: &Painter,
    centre: Pos2,
    inner: f32,
    outer: f32,
    angle: f32,
    fill: Color32,
    theme: Theme,
) {
    const STEPS: usize = 10;
    let sweep = 60.0 - GAP_DEG;
    let start = angle - sweep / 2.0;
    let mut points = Vec::with_capacity((STEPS + 1) * 2);
    for step in 0..=STEPS {
        #[allow(clippy::cast_precision_loss)]
        let t = (start + sweep * (step as f32 / STEPS as f32)).to_radians();
        points.push(pos2(centre.x + outer * t.cos(), centre.y - outer * t.sin()));
    }
    for step in (0..=STEPS).rev() {
        #[allow(clippy::cast_precision_loss)]
        let t = (start + sweep * (step as f32 / STEPS as f32)).to_radians();
        points.push(pos2(centre.x + inner * t.cos(), centre.y - inner * t.sin()));
    }
    painter.add(egui::Shape::convex_polygon(
        points,
        fill,
        Stroke::new(1.0_f32, theme.border),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    const CENTRE: Pos2 = pos2(400.0, 300.0);

    fn at(angle_deg: f32, distance: f32) -> Pos2 {
        let t = angle_deg.to_radians();
        pos2(CENTRE.x + distance * t.cos(), CENTRE.y - distance * t.sin())
    }

    /// Six wedges on a sixty-degree pitch, and each one is found at the
    /// direction it is authored at. The whole point of disabled rather
    /// than absent is that this stays true whatever the node is.
    #[test]
    fn every_wedge_is_found_at_its_own_direction() {
        let mid = inner_radius(16.0) + RING_WIDTH / 2.0;
        for wedge in Wedge::ALL {
            assert_eq!(
                wedge_at(CENTRE, 16.0, at(wedge.angle(), mid)),
                Some(wedge),
                "{wedge:?} is not where it is drawn"
            );
        }
        let angles: Vec<f32> = Wedge::ALL.into_iter().map(Wedge::angle).collect();
        assert_eq!(angles, vec![0.0, 60.0, 120.0, 180.0, 240.0, 300.0]);
    }

    /// The gaps are real: a point exactly between two wedges belongs to
    /// neither, so a click on the seam does nothing rather than picking
    /// whichever wedge rounds first.
    #[test]
    fn the_gaps_between_wedges_belong_to_nothing() {
        let mid = inner_radius(16.0) + RING_WIDTH / 2.0;
        for wedge in Wedge::ALL {
            assert_eq!(wedge_at(CENTRE, 16.0, at(wedge.angle() + 30.0, mid)), None);
        }
    }

    /// Inside the hole and outside the band are both off the ring, so the
    /// node itself stays clickable and so does the canvas beyond it.
    #[test]
    fn the_hole_and_the_outside_are_both_off_the_ring() {
        let inner = inner_radius(16.0);
        assert_eq!(wedge_at(CENTRE, 16.0, at(0.0, inner - 4.0)), None);
        assert_eq!(
            wedge_at(CENTRE, 16.0, at(0.0, inner + RING_WIDTH + 4.0)),
            None
        );
    }

    /// The distance the ring closes at is measured from the same inner
    /// radius it is drawn at, so a pointer inside the band can never be
    /// beyond the grace radius.
    #[test]
    fn the_grace_radius_is_outside_the_band_it_guards() {
        for node_radius in [0.0, 16.0, 40.0, 200.0] {
            let inner = inner_radius(node_radius);
            assert!(
                stray_distance(node_radius) > inner + RING_WIDTH,
                "the ring would close while the pointer was still on it"
            );
        }
    }

    /// A measured radius is capped, so a node zoomed to fill the screen
    /// does not push the band off it.
    #[test]
    fn the_anchor_radius_is_capped() {
        let huge = Rect::from_min_size(pos2(0.0, 0.0), vec2(4000.0, 4000.0));
        assert!((anchor_radius(huge) - RADIUS_CAP).abs() < 0.01);
        let small = Rect::from_min_size(pos2(0.0, 0.0), vec2(112.0, 32.0));
        assert!((anchor_radius(small) - 56.0).abs() < 0.01);
    }

    /// Three operations are conditional and three always apply. A wedge
    /// that does not apply is still at its own angle: `wedge_at` knows
    /// nothing about whether it is enabled, which is what keeps the
    /// geometry stable.
    #[test]
    fn a_wedge_that_does_not_apply_keeps_its_place() {
        let none = Applies::default();
        assert!(!none.enabled(Wedge::Dive));
        assert!(!none.enabled(Wedge::Bypass));
        assert!(!none.enabled(Wedge::DisplayOrVisibility));
        assert!(none.enabled(Wedge::Rename));
        assert!(none.enabled(Wedge::Info));
        assert!(none.enabled(Wedge::Delete));

        let mid = inner_radius(16.0) + RING_WIDTH / 2.0;
        assert_eq!(
            wedge_at(CENTRE, 16.0, at(Wedge::Dive.angle(), mid)),
            Some(Wedge::Dive),
            "a disabled wedge still occupies its direction"
        );
    }

    /// The second wedge asks the same question of two kinds of network
    /// and says so differently, because "set the display flag" means
    /// nothing at the root and "show" means nothing inside a container.
    #[test]
    fn the_second_wedge_reads_differently_at_the_root() {
        assert_ne!(
            Wedge::DisplayOrVisibility.title(true),
            Wedge::DisplayOrVisibility.title(false)
        );
        assert_ne!(
            Wedge::DisplayOrVisibility.mark(true, true),
            Wedge::DisplayOrVisibility.mark(true, false),
            "hidden and shown must not read the same"
        );
    }
}
