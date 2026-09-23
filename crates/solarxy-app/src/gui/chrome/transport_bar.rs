//! The playbar: the scene-clock strip under the viewport.
//!
//! Scene-global, not per pane, so it sits below the whole viewport region
//! rather than on a pane toolbar: there is one clock, and a control that
//! appeared four times in a quad layout would imply four. Named "Playbar"
//! everywhere a user reads it, as the browser names it; the code keeps
//! `transport` for continuity with the browser's component and preference.
//!
//! A pure consumer of the clock readout: every control raises one intent
//! and the engine decides which are undo steps. The frame field and the
//! track seek live, since a seek records no undo step; the range and rate
//! fields commit on release, because each commit is an undo step and a
//! drag would otherwise mint one per pointer tick.

use egui::{Align2, FontId, Rect, Sense, pos2, vec2};

use solarxy_graph::runtime::{LoopMode, MAX_FPS, MIN_FPS};

use crate::gui::intent::{Intent, Intents, TransportIntent};
use crate::gui::settings::TransportReadout;
use crate::gui::theme::Theme;
use crate::state::keymap::{Action, hint};

/// The strip's height, logical pixels.
pub(in crate::gui) const TRANSPORT_BAR_HEIGHT: f32 = 34.0;

/// Ticks closer than this are dropped for the next rung of the ladder.
const MIN_TICK_PX: f32 = 7.0;
/// Approximate width a tick label needs, including its air.
const LABEL_PX: f32 = 44.0;
/// The tick ladder, in frames. Standard editorial increments: a scrubber
/// whose labels read 1, 25, 50 is legible in a way one reading 1, 37, 74
/// is not, so the step is chosen from this list rather than computed.
const TICK_STEPS: [i64; 10] = [1, 2, 5, 10, 25, 50, 100, 250, 500, 1000];

/// The loop modes in the browser's order, with its labels.
const LOOPS: [(LoopMode, &str); 3] = [
    (LoopMode::Once, "Once"),
    (LoopMode::Loop, "Loop"),
    (LoopMode::PingPong, "Ping-pong"),
];

/// The smallest ladder step keeping ticks at least `MIN_TICK_PX` apart.
/// Falls back to the coarsest rung rather than returning nothing, so a
/// pathologically narrow strip still draws something sane.
pub(crate) fn tick_step(frame_count: i64, width_px: f32) -> i64 {
    let coarsest = TICK_STEPS[TICK_STEPS.len() - 1];
    // Not-a-number falls back with the non-positives, as the browser's
    // negated comparison does.
    if width_px.is_nan() || width_px <= 0.0 || frame_count <= 0 {
        return coarsest;
    }
    TICK_STEPS
        .into_iter()
        .find(|step| (*step as f32 / frame_count as f32) * width_px >= MIN_TICK_PX)
        .unwrap_or(coarsest)
}

/// Every tick frame in `[start, end]` on the given step, anchored so ticks
/// land on multiples of the step rather than on the range start: a range
/// beginning at 7 should still tick at 10, 20, 30. Both ends are always
/// included, because those are the two frames worth naming.
pub(crate) fn tick_frames(start: i64, end: i64, step: i64) -> Vec<i64> {
    if end <= start {
        return vec![start];
    }
    let mut out = vec![start];
    let mut f = start.div_euclid(step) * step;
    if f < start {
        f += step;
    }
    while f < end {
        if f > start {
            out.push(f);
        }
        f += step;
    }
    out.push(end);
    out
}

/// How many ticks apart labels may be drawn without colliding.
pub(crate) fn label_stride(tick_gap_px: f32) -> usize {
    if tick_gap_px.is_nan() || tick_gap_px <= 0.0 {
        return 1;
    }
    ((LABEL_PX / tick_gap_px).ceil() as usize).max(1)
}

/// The frame under a pointer at `x` within a track of `width`, clamped to
/// the range and rounded to a whole frame.
pub(crate) fn frame_at_x(x: f32, width: f32, start: i64, end: i64) -> i64 {
    if width.is_nan() || width <= 0.0 || end <= start {
        return start;
    }
    let t = (x / width).clamp(0.0, 1.0);
    (start as f32 + t * (end - start) as f32).round() as i64
}

/// A frame's offset along the track, 0 to 1.
fn frame_to_fraction(frame: i64, start: i64, end: i64) -> f32 {
    if end <= start {
        return 0.0;
    }
    ((frame - start) as f32 / (end - start) as f32).clamp(0.0, 1.0)
}

/// The scene seconds a frame is, the value of `$T`, two decimals.
fn seconds_label(frame: i64, fps: f64) -> String {
    if fps > 0.0 {
        format!("{:.2}", frame as f64 / fps)
    } else {
        "0.00".to_string()
    }
}

/// Draw the strip into `rect`.
pub(in crate::gui) fn draw_transport_bar(
    ui: &mut egui::Ui,
    rect: Rect,
    readout: TransportReadout,
    intents: &mut Intents,
    theme: Theme,
) {
    ui.painter().rect_filled(rect, 0.0, theme.bg_elevated);
    ui.painter().hline(
        rect.x_range(),
        rect.top(),
        egui::Stroke::new(1.0_f32, theme.border),
    );
    let inner = rect.shrink2(vec2(8.0, 4.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
        ui.add_enabled_ui(readout.open, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                draw_fields(ui, readout, intents, theme);
            });
        });
    });
}

/// The controls in the browser's order: the track, the four transport
/// buttons, the frame field with its seconds, then the range, the rate and
/// the loop.
fn draw_fields(ui: &mut egui::Ui, readout: TransportReadout, intents: &mut Intents, theme: Theme) {
    // The fields on the right are laid out first so the track can take
    // whatever width is left, which is what a scrubber wants.
    let fields_width = 560.0;
    let track_rect = {
        let full = ui.max_rect();
        Rect::from_min_max(
            full.min,
            pos2(
                (full.right() - fields_width).max(full.left() + 60.0),
                full.max.y,
            ),
        )
    };
    draw_track(ui, track_rect, readout, intents, theme);
    ui.allocate_rect(track_rect, Sense::hover());

    let button = |ui: &mut egui::Ui, glyph: Glyph, title: String| -> bool {
        let (rect, response) = ui.allocate_exact_size(vec2(26.0, 22.0), Sense::click());
        let response = response.on_hover_text(title);
        let fill = if response.hovered() {
            theme.widget_hover
        } else {
            theme.widget_bg
        };
        ui.painter().rect_filled(rect, 3.0, fill);
        paint_glyph(ui.painter(), rect.shrink(6.0), glyph, theme.fg);
        response.clicked()
    };
    if button(
        ui,
        Glyph::Stop,
        hinted("Stop and rewind to the range start", Action::GoToStart),
    ) {
        intents.raise(Intent::Transport(TransportIntent::Stop));
    }
    if button(
        ui,
        Glyph::StepBack,
        hinted("Step back one frame", Action::StepBack),
    ) {
        intents.raise(Intent::Transport(TransportIntent::Step(-1)));
    }
    let (glyph, title) = if readout.playing {
        (Glyph::Pause, hinted("Pause", Action::PlayPause))
    } else {
        (Glyph::Play, hinted("Play", Action::PlayPause))
    };
    if button(ui, glyph, title) {
        intents.raise(Intent::Transport(if readout.playing {
            TransportIntent::Pause
        } else {
            TransportIntent::Play
        }));
    }
    if button(
        ui,
        Glyph::StepForward,
        hinted("Step forward one frame", Action::StepForward),
    ) {
        intents.raise(Intent::Transport(TransportIntent::Step(1)));
    }

    // The frame field seeks live: a seek is meaningful mid-drag and records
    // no undo step, so every change goes straight out.
    let mut frame = readout.frame;
    if ui
        .add(
            egui::DragValue::new(&mut frame)
                .range(readout.start..=readout.end)
                .speed(0.25),
        )
        .on_hover_text("The current frame, the value of $F")
        .changed()
    {
        intents.raise(Intent::Transport(TransportIntent::SetFrame(frame)));
    }
    ui.label(
        egui::RichText::new(format!("{} s", seconds_label(readout.frame, readout.fps)))
            .monospace()
            .weak(),
    )
    .on_hover_text("Scene seconds, the value of $T");

    // The range and the rate commit on release rather than per tick: each
    // commit is an undo step.
    let mut start = readout.start;
    ui.label("Start");
    let r = ui.add(
        egui::DragValue::new(&mut start)
            .range(0..=i64::MAX)
            .speed(0.25),
    );
    if committed(&r) && start != readout.start {
        intents.raise(Intent::Transport(TransportIntent::SetRange {
            start,
            end: readout.end,
        }));
    }
    let mut end = readout.end;
    ui.label("End");
    let r = ui.add(
        egui::DragValue::new(&mut end)
            .range(0..=i64::MAX)
            .speed(0.25),
    );
    if committed(&r) && end != readout.end {
        intents.raise(Intent::Transport(TransportIntent::SetRange {
            start: readout.start,
            end,
        }));
    }
    let mut fps = readout.fps;
    ui.label("FPS");
    let r = ui.add(
        egui::DragValue::new(&mut fps)
            .range(MIN_FPS..=MAX_FPS)
            .speed(0.25)
            .fixed_decimals(0),
    );
    if committed(&r) && (fps - readout.fps).abs() > f64::EPSILON {
        intents.raise(Intent::Transport(TransportIntent::SetFps(fps)));
    }
    let current = LOOPS
        .iter()
        .find(|(m, _)| *m == readout.loop_mode)
        .map_or("Loop", |(_, l)| *l);
    egui::ComboBox::from_id_salt("transport_loop")
        .selected_text(current)
        .width(96.0)
        .show_ui(ui, |ui| {
            for (mode, label) in LOOPS {
                if ui
                    .selectable_label(readout.loop_mode == mode, label)
                    .clicked()
                    && mode != readout.loop_mode
                {
                    intents.raise(Intent::Transport(TransportIntent::SetLoop(mode)));
                }
            }
        });
}

/// A drag-value edit that has finished: the drag lifted, or the typed
/// value was entered. Reading `changed` alone would commit once per tick.
fn committed(response: &egui::Response) -> bool {
    response.drag_stopped() || (response.lost_focus() && !response.dragged())
}

/// A title with the key its action is bound to, read from the table.
fn hinted(title: &str, action: Action) -> String {
    match hint(action) {
        Some(key) => format!("{title} ({key})"),
        None => title.to_string(),
    }
}

/// The ticked frame track: ticks and labels on the ladder, the playhead,
/// and a press or drag that seeks to the frame under the pointer.
fn draw_track(
    ui: &mut egui::Ui,
    rect: Rect,
    readout: TransportReadout,
    intents: &mut Intents,
    theme: Theme,
) {
    let response = ui.interact(
        rect,
        ui.id().with("transport_track"),
        Sense::click_and_drag(),
    );
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, theme.bg);
    let (start, end) = (readout.start, readout.end);
    let width = rect.width();
    let count = end - start;
    let step = tick_step(count, width);
    let ticks = tick_frames(start, end, step);
    let gap = if count > 0 {
        step as f32 / count as f32 * width
    } else {
        width
    };
    let stride = label_stride(gap);
    for (i, frame) in ticks.iter().enumerate() {
        let x = rect.left() + frame_to_fraction(*frame, start, end) * width;
        let labelled = i % stride == 0 || i + 1 == ticks.len();
        let tick_h = if labelled { 8.0 } else { 4.0 };
        painter.line_segment(
            [pos2(x, rect.bottom()), pos2(x, rect.bottom() - tick_h)],
            egui::Stroke::new(1.0_f32, theme.muted),
        );
        if labelled {
            painter.text(
                pos2(
                    x.clamp(rect.left() + 10.0, rect.right() - 10.0),
                    rect.top() + 2.0,
                ),
                Align2::CENTER_TOP,
                frame.to_string(),
                FontId::monospace(9.0),
                theme.muted,
            );
        }
    }
    let head_x = rect.left() + frame_to_fraction(readout.frame, start, end) * width;
    painter.line_segment(
        [pos2(head_x, rect.top()), pos2(head_x, rect.bottom())],
        egui::Stroke::new(2.0_f32, theme.accent),
    );

    // A press or a drag seeks. `SetFrame` records no undo step, so the
    // scrub can go straight out on every tick; the engine seeks directly
    // rather than through the cook's pacing, which is what makes a scrub
    // land the frame asked for.
    if (response.dragged() || response.clicked())
        && let Some(p) = response.interact_pointer_pos()
    {
        let frame = frame_at_x(p.x - rect.left(), width, start, end);
        if frame != readout.frame {
            intents.raise(Intent::Transport(TransportIntent::SetFrame(frame)));
        }
    }
}

/// The four transport glyphs.
#[derive(Debug, Clone, Copy)]
enum Glyph {
    Stop,
    StepBack,
    Play,
    Pause,
    StepForward,
}

fn paint_glyph(painter: &egui::Painter, rect: Rect, glyph: Glyph, color: egui::Color32) {
    let c = rect.center();
    let h = rect.height() * 0.5;
    let w = rect.width() * 0.5;
    let tri = |painter: &egui::Painter, tip_right: bool, cx: f32| {
        let pts = if tip_right {
            vec![
                pos2(cx - w * 0.6, c.y - h),
                pos2(cx + w * 0.6, c.y),
                pos2(cx - w * 0.6, c.y + h),
            ]
        } else {
            vec![
                pos2(cx + w * 0.6, c.y - h),
                pos2(cx - w * 0.6, c.y),
                pos2(cx + w * 0.6, c.y + h),
            ]
        };
        painter.add(egui::Shape::convex_polygon(pts, color, egui::Stroke::NONE));
    };
    let bar = |painter: &egui::Painter, x: f32| {
        painter.rect_filled(
            Rect::from_center_size(pos2(x, c.y), vec2(2.0, rect.height())),
            0.0,
            color,
        );
    };
    match glyph {
        Glyph::Stop => {
            painter.rect_filled(rect.shrink(1.0), 1.0, color);
        }
        Glyph::Play => tri(painter, true, c.x),
        Glyph::Pause => {
            bar(painter, c.x - w * 0.45);
            bar(painter, c.x + w * 0.45);
        }
        Glyph::StepForward => {
            tri(painter, true, c.x - w * 0.3);
            bar(painter, c.x + w * 0.8);
        }
        Glyph::StepBack => {
            bar(painter, c.x - w * 0.8);
            tri(painter, false, c.x + w * 0.3);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn browser_track() -> String {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("the repository root");
        std::fs::read_to_string(root.join("web/src/components/TransportTrack.tsx"))
            .expect("the browser's track")
    }

    /// The three constants the ladder is built from are the browser's, read
    /// off its source, so the two tracks pick the same rung for the same
    /// width.
    #[test]
    fn the_ladder_constants_are_the_browsers() {
        let source = browser_track();
        let value = |name: &str| -> String {
            let start = source.find(name).expect(name);
            let rest = &source[start + name.len()..];
            rest.split(';').next().expect("a value").trim().to_string()
        };
        assert_eq!(value("const MIN_TICK_PX = "), format!("{MIN_TICK_PX:.0}"));
        assert_eq!(value("const LABEL_PX = "), format!("{LABEL_PX:.0}"));
        let steps = value("const TICK_STEPS = [");
        let steps: Vec<i64> = steps
            .split(']')
            .next()
            .expect("the list")
            .split(',')
            .map(|s| s.trim().parse().expect("a step"))
            .collect();
        assert_eq!(steps, TICK_STEPS.to_vec());
    }

    /// The rung chosen keeps ticks at least the minimum apart, and a
    /// degenerate width or range falls back to the coarsest rung.
    #[test]
    fn the_tick_step_keeps_ticks_apart() {
        assert_eq!(tick_step(240, 1000.0), 2);
        assert_eq!(tick_step(240, 100.0), 25);
        assert_eq!(tick_step(0, 100.0), 1000);
        assert_eq!(tick_step(240, 0.0), 1000);
    }

    /// Ticks land on multiples of the step, both ends are always named,
    /// and an inverted range names its start only.
    #[test]
    fn ticks_land_on_multiples_and_name_both_ends() {
        assert_eq!(tick_frames(7, 42, 10), vec![7, 10, 20, 30, 40, 42]);
        assert_eq!(tick_frames(10, 30, 10), vec![10, 20, 30]);
        assert_eq!(tick_frames(5, 5, 10), vec![5]);
    }

    /// Labels are spaced so they cannot collide, and a pointer maps to the
    /// nearest whole frame clamped to the range.
    #[test]
    fn labels_and_the_pointer_map_as_the_browser_maps_them() {
        assert_eq!(label_stride(44.0), 1);
        assert_eq!(label_stride(20.0), 3);
        assert_eq!(label_stride(0.0), 1);
        assert_eq!(frame_at_x(50.0, 100.0, 1, 241), 121);
        assert_eq!(frame_at_x(-10.0, 100.0, 1, 241), 1);
        assert_eq!(frame_at_x(500.0, 100.0, 1, 241), 241);
        assert_eq!(frame_at_x(50.0, 0.0, 1, 241), 1);
    }

    /// The loop labels are the browser's three, in its order.
    #[test]
    fn the_loop_labels_are_the_browsers() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("the repository root");
        let source = std::fs::read_to_string(root.join("web/src/components/TransportBar.tsx"))
            .expect("the browser's bar");
        let start = source.find("const LOOP_LABEL").expect("the loop table");
        let block = &source[start..];
        let block = &block[..block.find("};").expect("the table's end")];
        let labels: Vec<&str> = block
            .split(": \"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .collect();
        let here: Vec<&str> = LOOPS.iter().map(|(_, l)| *l).collect();
        assert_eq!(here, labels);
    }

    /// The seconds readout is frame over rate to two places, and a zero
    /// rate reads as zero rather than dividing.
    #[test]
    fn the_seconds_readout_is_frame_over_rate() {
        assert_eq!(seconds_label(48, 24.0), "2.00");
        assert_eq!(seconds_label(1, 30.0), "0.03");
        assert_eq!(seconds_label(10, 0.0), "0.00");
    }
}
