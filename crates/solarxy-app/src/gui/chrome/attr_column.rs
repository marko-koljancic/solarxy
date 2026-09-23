//! The viewport's attribute strip: the tool column's twin down the right
//! edge of the 3D region, over the point lane the user picked.
//!
//! Three toggles (value labels, vector arrows, point markers) and the
//! settings behind a gear, in one square stack; under it the lane pill,
//! which is the one control carrying text; under that the sampling notice
//! when the scene has more points than the pin budget. An overlay inside
//! the viewport, as the browser's is, so the pane rects never move.
//!
//! A pure consumer of the strip's state and the lane inventory the state
//! layer hands it; every change travels as the whole state, as the
//! browser's does, so a control cannot leave a sibling field behind.

use egui::{Rect, Sense, pos2, vec2};

use solarxy_core::view_config::PANE_TOOLBAR_HEIGHT;
use solarxy_host::attr_viz::{AttrColorMode, AttrVizState, RampPreset};

use super::viewport_icons::{AttrIcon, paint_attr};
use crate::gui::intent::{Intent, Intents};
use crate::gui::theme::Theme;
use crate::state::attr::SceneLane;

/// What the strip reads: the state it mirrors, the lanes it can offer, and
/// the sampling facts its notice reports.
#[derive(Clone, Copy)]
pub(crate) struct AttrColumnSource<'a> {
    pub viz: &'a AttrVizState,
    pub lanes: &'a [SceneLane],
    /// The pin capacity the last rebuild sampled to, and the total
    /// displayed points; the notice shows when the second exceeds the first.
    pub capacity: u32,
    pub total: usize,
}

/// The pin cap's ceiling, the shared state's own.
const PIN_CAP_MAX: u32 = AttrVizState::MAX_CAP as u32;

const BUTTON_PX: f32 = 30.0;
const BUTTON_GAP: f32 = 4.0;
const INSET_X: f32 = 8.0;
const INSET_Y: f32 = 8.0;
/// The lane pill under the stack, and the gap above it.
const PILL_H: f32 = 22.0;
const PILL_W: f32 = 92.0;
const PILL_GAP: f32 = 8.0;

/// The ramp presets in the browser's order, with its labels.
const RAMPS: [(RampPreset, &str); 5] = [
    (RampPreset::ColdWarm, "Cold to Warm"),
    (RampPreset::Ember, "Ember"),
    (RampPreset::Ocean, "Ocean"),
    (RampPreset::Grayscale, "Grayscale"),
    (RampPreset::Signal, "Signal"),
];

/// The strip's interactive rect within `viewport`: the stack and the pill,
/// so a click on either never also reaches the camera or the pick.
fn column_rect(viewport: Rect) -> Rect {
    let stack = 4.0 * BUTTON_PX + 3.0 * BUTTON_GAP;
    let height = stack + PILL_GAP + PILL_H;
    Rect::from_min_size(
        pos2(
            viewport.right() - INSET_X - PILL_W,
            viewport.top() + PANE_TOOLBAR_HEIGHT + INSET_Y,
        ),
        vec2(PILL_W, height),
    )
}

/// Draw the strip over `viewport` and return the rect it occupies.
pub(in crate::gui) fn draw_attr_column(
    ui: &mut egui::Ui,
    viewport: Rect,
    source: AttrColumnSource<'_>,
    intents: &mut Intents,
    theme: Theme,
) -> Rect {
    let rect = column_rect(viewport);
    let viz = source.viz;
    let picked_ty = viz
        .name
        .as_deref()
        .and_then(|name| source.lanes.iter().find(|l| l.name == name))
        .map(|l| l.ty);
    // Arrows draw vec3 and vec4 lanes; the toggle stays visible but
    // disabled for float and vec2 so the strip's shape is stable.
    let can_arrow = matches!(picked_ty, Some("vec3" | "vec4"));
    let stale = viz.name.as_deref().is_some_and(|name| {
        !source.lanes.is_empty() && !source.lanes.iter().any(|l| l.name == name)
    });

    let mut next: Option<AttrVizState> = None;
    let mut y = rect.top();
    let buttons = [
        (AttrIcon::Labels, "Value labels", viz.labels, true),
        (
            AttrIcon::Vectors,
            "Vector arrows (vec3/vec4 lanes)",
            viz.vectors,
            can_arrow,
        ),
        (AttrIcon::Points, "Point numbers", viz.points, true),
        (AttrIcon::Settings, "Visualization settings", false, true),
    ];
    for (i, (icon, title, on, enabled)) in buttons.into_iter().enumerate() {
        let button = Rect::from_min_size(
            pos2(rect.right() - BUTTON_PX, y),
            vec2(BUTTON_PX, BUTTON_PX),
        );
        let response = ui
            .interact(
                button,
                ui.id().with(("attr_column", i)),
                if enabled {
                    Sense::click()
                } else {
                    Sense::hover()
                },
            )
            .on_hover_text(title);
        let hovered = enabled && response.hovered();
        let fill = if on {
            theme.accent.gamma_multiply(0.85)
        } else if hovered {
            theme.widget_hover
        } else {
            theme.bg_elevated.gamma_multiply(0.9)
        };
        let glyph = if on {
            theme.bg
        } else if enabled {
            theme.fg
        } else {
            theme.muted.gamma_multiply(0.6)
        };
        let painter = ui.painter();
        painter.rect_filled(button, 4.0, fill);
        painter.rect_stroke(
            button,
            4.0,
            egui::Stroke::new(1.0_f32, theme.border),
            egui::StrokeKind::Inside,
        );
        paint_attr(painter, button.shrink(6.0), icon, glyph);
        match icon {
            AttrIcon::Labels if response.clicked() => {
                next = Some(AttrVizState {
                    labels: !viz.labels,
                    ..viz.clone()
                });
            }
            AttrIcon::Vectors if response.clicked() => {
                next = Some(AttrVizState {
                    vectors: !viz.vectors,
                    ..viz.clone()
                });
            }
            AttrIcon::Points if response.clicked() => {
                next = Some(AttrVizState {
                    points: !viz.points,
                    ..viz.clone()
                });
            }
            AttrIcon::Settings => {
                egui::Popup::menu(&response)
                    .id(ui.id().with("attr_viz_settings"))
                    .show(|ui| {
                        ui.set_min_width(220.0);
                        if let Some(changed) = draw_settings(ui, viz, source.capacity) {
                            next = Some(changed);
                        }
                    });
            }
            _ => {}
        }
        y += BUTTON_PX + BUTTON_GAP;
    }

    // The lane pill: the one text-carrying control, outside the square
    // stack. Its label is the picked name, `attr` while none is.
    y += PILL_GAP - BUTTON_GAP;
    let pill = Rect::from_min_size(pos2(rect.left(), y), vec2(PILL_W, PILL_H));
    let pill_response = ui
        .interact(pill, ui.id().with("attr_lane_pill"), Sense::click())
        .on_hover_text(match viz.name.as_deref() {
            Some(name) => format!("Visualized attribute: {name}"),
            None => "Pick the attribute to visualize".to_string(),
        });
    let fill = if pill_response.hovered() {
        theme.widget_hover
    } else {
        theme.bg_elevated.gamma_multiply(0.9)
    };
    let painter = ui.painter();
    painter.rect_filled(pill, PILL_H * 0.5, fill);
    painter.rect_stroke(
        pill,
        PILL_H * 0.5,
        egui::Stroke::new(
            1.0_f32,
            if stale {
                theme.severity_warn
            } else {
                theme.border
            },
        ),
        egui::StrokeKind::Inside,
    );
    let name = viz.name.as_deref().unwrap_or("attr");
    painter.text(
        pill.center(),
        egui::Align2::CENTER_CENTER,
        format!("{name} \u{25be}"),
        egui::FontId::proportional(12.0),
        if stale { theme.severity_warn } else { theme.fg },
    );
    egui::Popup::menu(&pill_response)
        .id(ui.id().with("attr_lane_list"))
        .show(|ui| {
            ui.set_min_width(140.0);
            if source.lanes.is_empty() {
                ui.label(egui::RichText::new("No point attributes.").weak());
            }
            for lane in source.lanes {
                let picked = viz.name.as_deref() == Some(lane.name.as_str());
                let row = ui.horizontal(|ui| {
                    let r = ui.selectable_label(picked, &lane.name);
                    ui.label(egui::RichText::new(lane.ty).weak().small());
                    r
                });
                if row.inner.clicked() {
                    next = Some(AttrVizState {
                        name: Some(lane.name.clone()),
                        ..viz.clone()
                    });
                    ui.close();
                }
            }
        });

    // The sampling notice, when the scene has more points than the budget.
    if (viz.labels || viz.points) && source.total > source.capacity as usize && source.capacity > 0
    {
        let every = source.total.div_ceil(source.capacity as usize);
        let notice =
            Rect::from_min_size(pos2(rect.left(), pill.bottom() + 4.0), vec2(PILL_W, 16.0));
        ui.interact(notice, ui.id().with("attr_sampling_notice"), Sense::hover())
            .on_hover_text(format!(
                "More points than the pin budget: showing every {every}th point. Raise the cap in the visualization settings."
            ));
        ui.painter().text(
            notice.center(),
            egui::Align2::CENTER_CENTER,
            format!("{} of {} pts", source.capacity, source.total),
            egui::FontId::proportional(11.0),
            theme.muted,
        );
    }

    if let Some(next) = next {
        intents.raise(Intent::AttrViz(next));
    }
    rect
}

/// The settings behind the gear, in the browser's order and with its
/// ranges: the vector controls, the label appearance, then the shared pin
/// cap and the reset. Returns the changed state, if a control moved.
fn draw_settings(ui: &mut egui::Ui, viz: &AttrVizState, capacity: u32) -> Option<AttrVizState> {
    let mut next = viz.clone();
    let mut changed = false;

    egui::Grid::new("attr_viz_settings_grid")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Vector scale");
            changed |= ui
                .add(
                    egui::Slider::new(&mut next.vector_scale, 0.05..=10.0)
                        .step_by(0.05)
                        .suffix("x")
                        .fixed_decimals(2),
                )
                .changed();
            ui.end_row();

            ui.label("Normalize");
            changed |= ui.checkbox(&mut next.normalize, "").changed();
            ui.end_row();

            ui.label("Color");
            ui.horizontal(|ui| {
                changed |= ui
                    .selectable_value(&mut next.color_mode, AttrColorMode::Uniform, "Uniform")
                    .changed();
                changed |= ui
                    .selectable_value(&mut next.color_mode, AttrColorMode::Ramp, "Ramp")
                    .on_hover_text("Cold to warm over the lane's magnitude range")
                    .changed();
            });
            ui.end_row();

            match next.color_mode {
                AttrColorMode::Uniform => {
                    ui.label("Arrow color");
                    // Raw RGB in unit range, fed straight to the line
                    // pipeline as the browser feeds its picker's value.
                    changed |= ui.color_edit_button_rgb(&mut next.color).changed();
                    ui.end_row();
                }
                AttrColorMode::Ramp => {
                    ui.label("Ramp");
                    let current = RAMPS
                        .iter()
                        .find(|(p, _)| *p == next.ramp_preset)
                        .map_or("Cold to Warm", |(_, l)| *l);
                    egui::ComboBox::from_id_salt("attr_viz_ramp")
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            for (preset, label) in RAMPS {
                                changed |= ui
                                    .selectable_value(&mut next.ramp_preset, preset, label)
                                    .changed();
                            }
                        });
                    ui.end_row();
                }
            }

            ui.label("Label size");
            ui.horizontal(|ui| {
                for (value, label) in [("small", "S"), ("medium", "M"), ("large", "L")] {
                    if ui
                        .selectable_label(next.label_size == value, label)
                        .clicked()
                    {
                        next.label_size = value.to_string();
                        changed = true;
                    }
                }
            });
            ui.end_row();

            ui.label("Background");
            ui.horizontal(|ui| {
                for (value, label, title) in [
                    (
                        "chip",
                        "Chip",
                        "A rounded chip behind the text: legible over any scene",
                    ),
                    (
                        "none",
                        "None",
                        "Text and anchor dot only. Quieter, but contrast is no longer guaranteed",
                    ),
                ] {
                    if ui
                        .selectable_label(next.label_background == value, label)
                        .on_hover_text(title)
                        .clicked()
                    {
                        next.label_background = value.to_string();
                        changed = true;
                    }
                }
            });
            ui.end_row();

            ui.label("Label opacity");
            changed |= ui
                .add(
                    egui::Slider::new(&mut next.label_opacity, 0.1..=1.0)
                        .step_by(0.05)
                        .custom_formatter(|v, _| format!("{}%", (v * 100.0).round())),
                )
                .changed();
            ui.end_row();

            ui.label("Decimals");
            changed |= ui
                .add(egui::DragValue::new(&mut next.label_decimals).range(0..=4))
                .changed();
            ui.end_row();

            // The zero sentinel means every point; the field shows what
            // that resolves to right now so it is never a lie.
            ui.label("Pin cap")
                .on_hover_text("Default: every point, up to 16384 per scene");
            let mut shown = if next.cap == 0 {
                if capacity == 0 { PIN_CAP_MAX } else { capacity }
            } else {
                next.cap
            };
            if ui
                .add(
                    egui::DragValue::new(&mut shown)
                        .range(8..=PIN_CAP_MAX)
                        .speed(8.0),
                )
                .changed()
            {
                next.cap = shown.clamp(8, PIN_CAP_MAX);
                changed = true;
            }
            ui.end_row();
        });

    if ui.button("Reset to defaults").clicked() {
        // The shipped defaults for everything: this shell keeps no saved
        // label defaults of its own for the reset to prefer, where the
        // browser resets the label rows to its Preferences.
        next = AttrVizState {
            name: viz.name.clone(),
            labels: viz.labels,
            vectors: viz.vectors,
            points: viz.points,
            ..AttrVizState::default()
        };
        changed = true;
    }
    changed.then_some(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ramp presets are the browser's five, in its order and with its
    /// labels, read off its `RAMP_PRESETS` table.
    #[test]
    fn the_ramp_presets_are_the_browsers() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("the repository root");
        let source = std::fs::read_to_string(root.join("web/src/components/AttrColumn.tsx"))
            .expect("the browser's column");
        let start = source.find("const RAMP_PRESETS").expect("the preset table");
        let block = &source[start..];
        let block = &block[..block.find("];").expect("the table's end")];
        let labels: Vec<&str> = block
            .split("label: \"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .collect();
        let here: Vec<&str> = RAMPS.iter().map(|(_, l)| *l).collect();
        assert_eq!(here, labels);
    }

    /// The pin cap ceiling is the shared state's, and the strip starts
    /// below the toolbar strip at the right edge.
    #[test]
    fn the_strip_sits_under_the_toolbar_at_the_right_edge() {
        assert_eq!(PIN_CAP_MAX as usize, AttrVizState::MAX_CAP);
        let viewport = Rect::from_min_size(pos2(100.0, 50.0), vec2(800.0, 600.0));
        let rect = column_rect(viewport);
        assert!(rect.top() > viewport.top() + PANE_TOOLBAR_HEIGHT);
        assert!(rect.right() < viewport.right() && rect.left() > viewport.center().x);
        assert!((rect.height() - 162.0).abs() < f32::EPSILON);
    }
}
