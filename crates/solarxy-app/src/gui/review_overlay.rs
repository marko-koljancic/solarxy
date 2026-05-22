//! Screen-space review-marker overlay. Replaces the 0.5-era WGSL
//! `review_marker` pipeline with an egui-painted pin + expand-on-hover
//! card with leader line.
//!
//! Why egui instead of wgpu: text rendering is free (we already ship the
//! font infrastructure), pane clipping is one builder call, and there's
//! no z-fight gymnastics with bloom / SSAO / depth state. The trade-off is
//! that markers no longer participate in post-processing — that's actually
//! a win for readability (the old SDF shapes competed with bloom).
//!
//! Per-pane plumbing comes from `state::render`: each entry pairs a 3D
//! pane's egui-logical rect with the matching camera's `view * proj`
//! matrix. UV panes are filtered out upstream.

use std::borrow::Cow;

use cgmath::{Matrix4, Vector4};
use solarxy_core::review::ReviewAnnotation;
use solarxy_renderer::model::Model;

use super::review_visuals::{category_color, category_label, category_letter};
use super::theme::Theme;
use crate::state::review::ReviewState;

/// Per-3D-pane data the overlay needs: the egui-logical rect the pane
/// occupies in window space, plus the camera's `view_proj` matrix. The
/// state layer builds one of these per active 3D pane and passes a slice
/// into [`draw_review_overlay`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct ReviewPaneOverlay {
    pub egui_rect: egui::Rect,
    pub view_proj: Matrix4<f32>,
}

const PIN_RADIUS: f32 = 6.0;
const PIN_HIT_RADIUS: f32 = 10.0;
const CARD_OFFSET: egui::Vec2 = egui::vec2(14.0, -14.0);
const CARD_WIDTH: f32 = 260.0;
/// Vertical space above the body text: top padding + the header row
/// (category chip + author / time line).
const CARD_HEADER_H: f32 = 20.0;
/// The card height is content-driven (it grows to fit the body galley);
/// clamp it to this readable range so a one-word note isn't a sliver and
/// a wall of text isn't a full-screen panel.
const CARD_MIN_HEIGHT: f32 = 54.0;
const CARD_MAX_HEIGHT: f32 = 240.0;
const CARD_PADDING: f32 = 8.0;
const CARD_ROUNDING: f32 = 6.0;
const CARD_FILL_ALPHA: u8 = 235;
const RESOLVED_ALPHA: u8 = 90;
/// Body-text font size — the galley is measured and drawn at this size.
const CARD_BODY_FONT: f32 = 12.0;
const TEXT_TRUNCATE_CHARS: usize = 240;

/// Paint review markers + expand-on-hover/select cards across the supplied
/// 3D panes. Reads from + writes to `review.hovered`; does not change any
/// other field.
///
/// Pass `suppress` = `true` to skip the overlay entirely (e.g. when a
/// blocking modal is open — markers as filled blobs in the canvas center
/// would otherwise occlude modal text on `Order::Foreground`).
pub(crate) fn draw_review_overlay(
    ctx: &egui::Context,
    panes: &[ReviewPaneOverlay],
    review: &mut ReviewState,
    suppress: bool,
    theme: Theme,
    model: Option<&Model>,
    force_expand_all: bool,
) {
    if suppress {
        return;
    }
    // No model ⇒ no markers, ever (the model was closed but annotations
    // may not be cleared yet on the frame the close lands).
    if model.is_none()
        || panes.is_empty()
        || review.annotations.iter().all(|a| a.reply_to.is_some())
    {
        review.hovered = None;
        return;
    }

    let cursor_pos = ctx.input(|i| i.pointer.hover_pos());
    let prev_hovered = review.hovered.clone();
    let mut closest_hovered: Option<(f32, String)> = None;

    let layer = egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("solarxy_review_markers"),
    );

    for pane in panes {
        let painter = ctx.layer_painter(layer).with_clip_rect(pane.egui_rect);
        let mut visible: Vec<VisiblePin<'_>> = Vec::new();
        for ann in &review.annotations {
            if ann.reply_to.is_some() {
                continue;
            }
            let Some(pos) = project_to_pane(
                &pane.view_proj,
                ann.anchor.world_pos_fallback,
                pane.egui_rect,
            ) else {
                continue;
            };
            // The card sizes to its text: lay the body galley out up front
            // so its measured height drives `compute_card_rect` (and the
            // hover hit-test, which keys off `card_rect`).
            let body_galley = layout_card_body(&painter, ann, theme);
            let has_replies = review.reply_count(&ann.id) > 0;
            let card_height = card_height_for(&body_galley, has_replies);
            let card_rect = compute_card_rect(pos, pane.egui_rect, card_height);
            let mesh_hidden = model.is_some_and(|m| {
                m.meshes
                    .get(ann.anchor.mesh_index as usize)
                    .is_some_and(|mesh| !mesh.visible)
            });
            visible.push(VisiblePin {
                ann,
                pos,
                card_rect,
                body_galley,
                mesh_hidden,
            });
        }

        if let Some(cursor) = cursor_pos
            && pane.egui_rect.contains(cursor)
        {
            for pin in &visible {
                let d = pin.pos.distance(cursor);
                if d <= PIN_HIT_RADIUS && closest_hovered.as_ref().is_none_or(|(best, _)| d < *best)
                {
                    closest_hovered = Some((d, pin.ann.id.clone()));
                }
            }
            if closest_hovered.is_none()
                && let Some(prev_id) = prev_hovered.as_deref()
                && let Some(pin) = visible.iter().find(|p| p.ann.id == prev_id)
                && let Some(card) = pin.card_rect
                && card.contains(cursor)
            {
                closest_hovered = Some((0.0, pin.ann.id.clone()));
            }
        }

        let hovered_id = closest_hovered.as_ref().map(|(_, id)| id.as_str());
        let selected_id = review.selected.as_deref();

        let mut featured: Option<&VisiblePin<'_>> = None;
        for pin in &visible {
            let is_hovered = hovered_id == Some(pin.ann.id.as_str());
            let is_selected = selected_id == Some(pin.ann.id.as_str());
            if is_hovered || is_selected {
                featured = Some(pin);
                continue;
            }
            draw_pin(&painter, pin, false, false, theme);
        }
        if let Some(pin) = featured {
            let is_hovered = hovered_id == Some(pin.ann.id.as_str());
            let is_selected = selected_id == Some(pin.ann.id.as_str());
            draw_pin(&painter, pin, is_hovered, is_selected, theme);
        }

        if force_expand_all {
            // Screenshot capture — every annotation card open at once.
            for pin in &visible {
                let is_selected = selected_id == Some(pin.ann.id.as_str());
                draw_card(&painter, pin, is_selected, review, theme);
            }
        } else {
            if let Some(pin) = visible
                .iter()
                .find(|p| selected_id == Some(p.ann.id.as_str()))
            {
                draw_card(&painter, pin, true, review, theme);
            }
            if let Some(pin) = visible.iter().find(|p| {
                hovered_id == Some(p.ann.id.as_str()) && selected_id != Some(p.ann.id.as_str())
            }) {
                draw_card(&painter, pin, false, review, theme);
            }
        }
    }

    review.hovered = closest_hovered.map(|(_, id)| id);
}

/// One annotation projected into a pane's egui-logical coordinate space,
/// pre-paired with the rect the card would occupy if expanded. `card_rect`
/// is `None` when neither side of the pin has enough room to fit a
/// readable card (we suppress the card and show only the pin).
struct VisiblePin<'a> {
    ann: &'a ReviewAnnotation,
    pos: egui::Pos2,
    card_rect: Option<egui::Rect>,
    /// Body text pre-laid-out at [`CARD_WIDTH`] — measured once so the
    /// card height matches what `draw_card` paints.
    body_galley: std::sync::Arc<egui::Galley>,
    /// The mesh this annotation is anchored to is hidden (Outliner / hide
    /// shortcuts) — the pin renders dimmed, like a resolved one.
    mesh_hidden: bool,
}

/// Lay out an annotation's body text at the card's content width. The
/// resulting galley is measured for [`card_height_for`] and reused by
/// [`draw_card`] so layout and paint never disagree.
fn layout_card_body(
    painter: &egui::Painter,
    ann: &ReviewAnnotation,
    theme: Theme,
) -> std::sync::Arc<egui::Galley> {
    let body = truncate_with_ellipsis(&ann.text, TEXT_TRUNCATE_CHARS);
    painter.layout(
        body.into_owned(),
        egui::FontId::proportional(CARD_BODY_FONT),
        theme.fg,
        CARD_WIDTH - 2.0 * CARD_PADDING,
    )
}

/// Card height needed to show `body` in full: top padding + header +
/// galley + an optional reply-badge line + bottom padding, clamped to the
/// readable range.
fn card_height_for(body: &egui::Galley, has_replies: bool) -> f32 {
    let reply_extra = if has_replies { 16.0 } else { 0.0 };
    (2.0 * CARD_PADDING + CARD_HEADER_H + body.size().y + reply_extra)
        .clamp(CARD_MIN_HEIGHT, CARD_MAX_HEIGHT)
}

/// Project a world point through `view_proj` and into the pane's
/// egui-logical pixel coordinates. Returns `None` when the point is
/// behind the camera or falls outside the pane's clip rect.
fn project_to_pane(
    view_proj: &Matrix4<f32>,
    world_pos: [f32; 3],
    pane_rect: egui::Rect,
) -> Option<egui::Pos2> {
    let [x, y, z] = world_pos;
    let clip = view_proj * Vector4::new(x, y, z, 1.0);
    if clip.w <= 0.0 {
        return None;
    }
    let ndc_x = clip.x / clip.w;
    let ndc_y = clip.y / clip.w;
    let ndc_z = clip.z / clip.w;
    if !(-1.0..=1.0).contains(&ndc_z) {
        return None;
    }
    let px = pane_rect.min.x + (ndc_x + 1.0) * 0.5 * pane_rect.width();
    let py = pane_rect.min.y + (1.0 - ndc_y) * 0.5 * pane_rect.height();
    if !pane_rect
        .expand(PIN_HIT_RADIUS)
        .contains(egui::pos2(px, py))
    {
        return None;
    }
    Some(egui::pos2(px, py))
}

/// Render a single category-colored pin. Selected pins gain a yellow
/// (Ayu accent) outline; resolved pins and pins on hidden meshes drop
/// alpha; stale pins get a dashed/dotted ring drawn manually.
fn draw_pin(
    painter: &egui::Painter,
    pin: &VisiblePin<'_>,
    is_hovered: bool,
    is_selected: bool,
    theme: Theme,
) {
    let mut fill = category_color(theme, pin.ann.category);
    let mut ring = theme.bg;
    if pin.ann.resolved || pin.mesh_hidden {
        fill = with_alpha(fill, RESOLVED_ALPHA);
        ring = with_alpha(ring, RESOLVED_ALPHA);
    }
    let radius = if is_hovered {
        PIN_RADIUS + 1.5
    } else {
        PIN_RADIUS
    };
    painter.circle_filled(pin.pos, radius + 1.0, ring);
    painter.circle_filled(pin.pos, radius, fill);
    if is_selected {
        painter.circle_stroke(
            pin.pos,
            radius + 2.0,
            egui::Stroke::new(1.6, theme.review.selection_accent),
        );
    }
    if pin.ann.stale {
        painter.circle_filled(
            pin.pos + egui::vec2(radius * 0.7, -radius * 0.7),
            2.5,
            theme.severity_warn,
        );
    }
}

/// Layout the card relative to `pin_pos`, clamped to fit inside
/// `pane_rect`. Returns `None` when the intersection with the pane is
/// smaller than the minimum readable area — in that case the caller
/// shows the pin only and lets the side panel carry the full text.
fn compute_card_rect(
    pin_pos: egui::Pos2,
    pane_rect: egui::Rect,
    card_height: f32,
) -> Option<egui::Rect> {
    const EDGE_INSET: f32 = 4.0;
    const MIN_W: f32 = 120.0;
    const MIN_H: f32 = 40.0;

    let mut card_min = pin_pos + CARD_OFFSET;
    let card_size = egui::vec2(CARD_WIDTH, card_height);

    if card_min.x + card_size.x > pane_rect.max.x - EDGE_INSET {
        card_min.x = pin_pos.x - CARD_OFFSET.x - card_size.x;
    }
    if card_min.y < pane_rect.min.y + EDGE_INSET {
        card_min.y = pin_pos.y - CARD_OFFSET.y;
    }
    if card_min.y + card_size.y > pane_rect.max.y - EDGE_INSET {
        card_min.y = pane_rect.max.y - card_size.y - EDGE_INSET;
    }
    if card_min.x < pane_rect.min.x + EDGE_INSET {
        card_min.x = pane_rect.min.x + EDGE_INSET;
    }
    let raw = egui::Rect::from_min_size(card_min, card_size);
    let bounded = pane_rect.shrink(EDGE_INSET);
    let clipped = raw.intersect(bounded);
    if clipped.width() < MIN_W || clipped.height() < MIN_H {
        None
    } else {
        Some(clipped)
    }
}

/// Render the expanded annotation card with a leader line back to the pin.
/// `is_selected` toggles the brighter accent outline (sticky vs transient).
/// Silently no-ops when `pin.card_rect` is `None` (pane too small).
fn draw_card(
    painter: &egui::Painter,
    pin: &VisiblePin<'_>,
    is_selected: bool,
    review: &ReviewState,
    theme: Theme,
) {
    let Some(card_rect) = pin.card_rect else {
        return;
    };
    let color = category_color(theme, pin.ann.category);

    let leader_target = nearest_edge_point(card_rect, pin.pos);
    painter.line_segment(
        [pin.pos, leader_target],
        egui::Stroke::new(1.0, with_alpha(color, 200)),
    );

    let bg = with_alpha(theme.bg_elevated, CARD_FILL_ALPHA);
    painter.rect_filled(card_rect, CARD_ROUNDING, bg);
    let stroke_color = if is_selected {
        theme.review.selection_accent
    } else {
        color
    };
    painter.rect_stroke(
        card_rect,
        CARD_ROUNDING,
        egui::Stroke::new(1.0, stroke_color),
        egui::StrokeKind::Inside,
    );

    let header_y = card_rect.min.y + CARD_PADDING;
    let header_x = card_rect.min.x + CARD_PADDING;
    let chip_radius = 7.0;
    let chip_center = egui::pos2(header_x + chip_radius, header_y + chip_radius);
    painter.circle_filled(chip_center, chip_radius, color);
    painter.text(
        chip_center,
        egui::Align2::CENTER_CENTER,
        category_letter(pin.ann.category),
        egui::FontId::proportional(10.0),
        egui::Color32::BLACK,
    );

    let header_label = format!(
        "{}{}{}",
        category_label(pin.ann.category),
        author_segment(pin.ann.author.as_deref()),
        time_segment(&pin.ann.updated_at),
    );
    painter.text(
        egui::pos2(chip_center.x + chip_radius + 6.0, header_y + 1.0),
        egui::Align2::LEFT_TOP,
        header_label,
        egui::FontId::proportional(11.0),
        with_alpha(theme.fg, 230),
    );

    // Body text was measured into `pin.body_galley` up front; draw it at
    // the same width/size so the card background always encloses it.
    let body_min = egui::pos2(card_rect.min.x + CARD_PADDING, header_y + CARD_HEADER_H);
    painter.galley(body_min, pin.body_galley.clone(), theme.fg);

    let reply_count = review.reply_count(&pin.ann.id);
    if reply_count > 0 {
        let badge_text = format!(
            "{reply_count} {}",
            if reply_count == 1 { "reply" } else { "replies" }
        );
        let badge_pos = egui::pos2(
            card_rect.max.x - CARD_PADDING,
            card_rect.max.y - CARD_PADDING,
        );
        painter.text(
            badge_pos,
            egui::Align2::RIGHT_BOTTOM,
            badge_text,
            egui::FontId::proportional(11.0),
            with_alpha(theme.muted, 220),
        );
    }
}

fn nearest_edge_point(rect: egui::Rect, target: egui::Pos2) -> egui::Pos2 {
    let cx = target.x.clamp(rect.min.x, rect.max.x);
    let cy = target.y.clamp(rect.min.y, rect.max.y);
    egui::pos2(cx, cy)
}

fn with_alpha(c: egui::Color32, a: u8) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

fn author_segment(author: Option<&str>) -> String {
    match author {
        Some(name) if !name.is_empty() => format!(" \u{00b7} {name}"),
        _ => String::new(),
    }
}

fn time_segment(rfc3339: &str) -> String {
    let Some(label) = relative_time_short(rfc3339) else {
        return String::new();
    };
    format!(" \u{00b7} {label}")
}

/// "just now / 3m / 4h / 2d / Jan 12 / Jan 12 2024" relative-time
/// formatter. Past one week we show month + day; if `then` is from a
/// previous year, we also append the 4-digit year so two May-19 entries
/// from different years can be told apart at a glance. Returns `None`
/// on parse failure (caller drops the segment).
fn relative_time_short(rfc3339: &str) -> Option<String> {
    use time::OffsetDateTime;
    use time::format_description::well_known::Rfc3339;
    let then = OffsetDateTime::parse(rfc3339, &Rfc3339).ok()?;
    let now = OffsetDateTime::now_utc();
    let delta = now - then;
    let secs = delta.whole_seconds();
    if secs < 60 {
        Some("just now".into())
    } else if secs < 3600 {
        Some(format!("{}m ago", secs / 60))
    } else if secs < 86_400 {
        Some(format!("{}h ago", secs / 3600))
    } else if secs < 86_400 * 7 {
        Some(format!("{}d ago", secs / 86_400))
    } else {
        let month = match then.month() as u8 {
            1 => "Jan",
            2 => "Feb",
            3 => "Mar",
            4 => "Apr",
            5 => "May",
            6 => "Jun",
            7 => "Jul",
            8 => "Aug",
            9 => "Sep",
            10 => "Oct",
            11 => "Nov",
            _ => "Dec",
        };
        if then.year() == now.year() {
            Some(format!("{month} {}", then.day()))
        } else {
            Some(format!("{month} {} {}", then.day(), then.year()))
        }
    }
}

fn truncate_with_ellipsis(text: &str, max_chars: usize) -> Cow<'_, str> {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return Cow::Borrowed(trimmed);
    }
    let mut truncated: String = trimmed.chars().take(max_chars).collect();
    truncated.push('\u{2026}');
    Cow::Owned(truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity_view_proj() -> Matrix4<f32> {
        Matrix4::from_cols(
            cgmath::Vector4::new(1.0, 0.0, 0.0, 0.0),
            cgmath::Vector4::new(0.0, 1.0, 0.0, 0.0),
            cgmath::Vector4::new(0.0, 0.0, 1.0, 0.0),
            cgmath::Vector4::new(0.0, 0.0, 0.0, 1.0),
        )
    }

    fn unit_pane() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 200.0))
    }

    #[test]
    fn project_to_pane_centers_origin_under_identity_view_proj() {
        let pos = project_to_pane(&identity_view_proj(), [0.0, 0.0, 0.0], unit_pane());
        let p = pos.expect("origin projects inside the pane");
        assert!((p.x - 100.0).abs() < 1e-3, "x = {}", p.x);
        assert!((p.y - 100.0).abs() < 1e-3, "y = {}", p.y);
    }

    #[test]
    fn project_to_pane_returns_none_for_point_behind_camera() {
        let mut m = identity_view_proj();
        m[3][3] = -1.0;
        assert!(project_to_pane(&m, [0.0, 0.0, 0.0], unit_pane()).is_none());
    }

    #[test]
    fn project_to_pane_returns_none_for_point_outside_clip_z() {
        let pos = project_to_pane(&identity_view_proj(), [0.0, 0.0, 2.0], unit_pane());
        assert!(pos.is_none());
    }

    #[test]
    fn truncate_with_ellipsis_passes_short_text_through_borrowed() {
        let s = "short";
        let out = truncate_with_ellipsis(s, 100);
        assert!(matches!(out, Cow::Borrowed(_)));
        assert_eq!(out.as_ref(), "short");
    }

    #[test]
    fn truncate_with_ellipsis_adds_ellipsis_when_truncating() {
        let long = "a".repeat(50);
        let out = truncate_with_ellipsis(&long, 10);
        assert!(matches!(out, Cow::Owned(_)));
        assert_eq!(out.chars().count(), 11, "10 chars + 1 ellipsis");
        assert!(out.ends_with('\u{2026}'));
    }

    #[test]
    fn truncate_with_ellipsis_counts_chars_not_bytes() {
        let multibyte = "\u{1f30d}".repeat(5);
        let out = truncate_with_ellipsis(&multibyte, 3);
        assert_eq!(out.chars().count(), 4, "3 chars + 1 ellipsis");
    }

    #[test]
    fn truncate_with_ellipsis_trims_whitespace_before_measuring() {
        let s = "   hi   ";
        let out = truncate_with_ellipsis(s, 100);
        assert_eq!(out.as_ref(), "hi");
    }

    #[test]
    fn relative_time_short_now_for_zero_delta() {
        use time::OffsetDateTime;
        use time::format_description::well_known::Rfc3339;
        let now = OffsetDateTime::now_utc().format(&Rfc3339).unwrap();
        let label = relative_time_short(&now).expect("now parses");
        assert_eq!(label, "just now");
    }

    #[test]
    fn relative_time_short_minutes_and_hours() {
        use time::Duration;
        use time::OffsetDateTime;
        use time::format_description::well_known::Rfc3339;
        let now = OffsetDateTime::now_utc();
        let five_min_ago = (now - Duration::minutes(5)).format(&Rfc3339).unwrap();
        assert_eq!(relative_time_short(&five_min_ago).unwrap(), "5m ago");
        let three_hours_ago = (now - Duration::hours(3)).format(&Rfc3339).unwrap();
        assert_eq!(relative_time_short(&three_hours_ago).unwrap(), "3h ago");
    }

    #[test]
    fn relative_time_short_days_within_week() {
        use time::Duration;
        use time::OffsetDateTime;
        use time::format_description::well_known::Rfc3339;
        let now = OffsetDateTime::now_utc();
        let two_days = (now - Duration::days(2)).format(&Rfc3339).unwrap();
        assert_eq!(relative_time_short(&two_days).unwrap(), "2d ago");
    }

    #[test]
    fn relative_time_short_falls_back_to_month_day_past_one_week_same_year() {
        use time::Duration;
        use time::OffsetDateTime;
        use time::format_description::well_known::Rfc3339;
        let now = OffsetDateTime::now_utc();
        let past = now - Duration::days(10);
        if past.year() != now.year() {
            return;
        }
        let label = relative_time_short(&past.format(&Rfc3339).unwrap()).unwrap();
        assert!(
            !label.contains("ago"),
            "past-one-week format should not say 'ago': got {label}"
        );
        assert!(
            label.split_whitespace().count() == 2,
            "expected 'MMM DD', got '{label}'"
        );
    }

    #[test]
    fn relative_time_short_includes_year_when_different() {
        use time::Duration;
        use time::OffsetDateTime;
        use time::format_description::well_known::Rfc3339;
        let now = OffsetDateTime::now_utc();
        let two_years = (now - Duration::days(365 * 2 + 1))
            .format(&Rfc3339)
            .unwrap();
        let label = relative_time_short(&two_years).unwrap();
        assert_eq!(
            label.split_whitespace().count(),
            3,
            "expected 'MMM DD YYYY', got '{label}'"
        );
    }

    #[test]
    fn relative_time_short_returns_none_for_garbage_input() {
        assert!(relative_time_short("not an rfc3339").is_none());
        assert!(relative_time_short("").is_none());
    }
}
