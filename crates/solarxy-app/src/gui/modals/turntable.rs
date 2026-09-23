//! Turntable export modal.
//!
//! One full turn of the active pane's view, written as a numbered image
//! sequence. The browser offers two video formats beside the sequence
//! because its platform ships an encoder; a native build has none without a
//! large dependency, which the milestone's decision log rejected, so the
//! capability degrades to the sequence rather than vanishing and the dialog
//! says so rather than leaving a person looking for the missing choice.
//!
//! Escape cancels a running export before it dismisses the dialog, which is
//! the still dialog's priority and the browser's: while a run is going the
//! key answers the run, and only an idle dialog is closed by it.
//!
//! The controls are the browser's, minus the video formats, and so are their
//! ranges and their hover texts. What differs is the destination: the browser
//! zips every frame in memory and hands over one file at the end, so a
//! cancelled run there produces nothing, while a native export writes each
//! frame as it finishes and a cancelled one keeps what it wrote.

use std::path::PathBuf;

use crate::gui::theme::Theme;

/// The frame size, as a multiple of the pane's own.
///
/// The browser's three. Its screenshot dialog also offers `4x` and a custom
/// size; its turntable deliberately does not, because every frame pays for
/// the choice and a turn is a hundred frames.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurntableResolution {
    #[default]
    Viewport,
    OneAndHalf,
    Two,
}

impl TurntableResolution {
    pub(crate) const ALL: [Self; 3] = [Self::Viewport, Self::OneAndHalf, Self::Two];

    /// The multiplier on the pane's edge.
    pub(crate) fn factor(self) -> f32 {
        match self {
            Self::Viewport => 1.0,
            Self::OneAndHalf => 1.5,
            Self::Two => 2.0,
        }
    }
}

impl std::fmt::Display for TurntableResolution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Viewport => "Viewport",
            Self::OneAndHalf => "1.5x",
            Self::Two => "2x",
        })
    }
}

/// What each frame keeps beside the scene.
///
/// Each flag only ever turns its feature **off**, which is the browser's
/// screenshot rule: the pane's own settings are the starting point, and an
/// unchecked box forces that feature off for the export. A checked box does
/// not switch on something the pane has switched off.
#[derive(Clone, Copy)]
pub(crate) struct TurntableIncludes {
    pub grid: bool,
    pub axes: bool,
    pub validation: bool,
}

impl Default for TurntableIncludes {
    fn default() -> Self {
        Self {
            grid: true,
            axes: false,
            validation: false,
        }
    }
}

/// How many frames one turn is.
///
/// The browser rounds because its two inputs are free-typed numbers that can
/// carry a fraction; these are whole by construction, so the product is the
/// count and the floor of two is all that is left of the rule. Two because a
/// turn needs a start and somewhere else to be.
pub(crate) fn frame_count(fps: u32, duration: u32) -> u32 {
    fps.saturating_mul(duration).max(2)
}

/// Where frame `index` sits on the turn, in degrees.
///
/// Absolute from the base pose rather than a step applied to the last frame,
/// so a hundred and twenty frames of floating-point addition cannot walk the
/// camera off the turn. The last frame lands one step short of a full turn,
/// which is what makes the sequence loop.
pub(crate) fn azimuth_deg(index: u32, count: u32) -> f32 {
    360.0 * index as f32 / count.max(1) as f32
}

/// What a fresh export was asked for, drained by the state layer.
pub(crate) struct TurntableRequest {
    pub resolution: TurntableResolution,
    pub fps: u32,
    pub duration: u32,
    pub includes: TurntableIncludes,
    pub folder: PathBuf,
    pub stem: String,
}

/// What the export is doing, which is what the dialog shows.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum TurntablePhase {
    #[default]
    Idle,
    Running,
    Cancelled,
    Done,
    Failed,
}

/// State for the turntable modal, owned by `EguiRenderer`.
pub(crate) struct TurntableModal {
    pub open: bool,
    phase: TurntablePhase,
    resolution: TurntableResolution,
    fps: u32,
    duration: u32,
    includes: TurntableIncludes,
    /// Where the sequence is written. `None` until picked, which is what the
    /// Export button waits on: a native export has a destination and asking
    /// for it afterwards would mean holding a hundred frames in memory to
    /// find out where they go.
    folder: Option<PathBuf>,
    stem: String,
    /// `(written, total)`, from the job each pump.
    progress: (u32, u32),
    /// What the run last did, in the browser's words.
    status: String,
    /// Set when Export is pressed; drained by the state layer, which starts
    /// the job.
    start_request: bool,
    /// Set by Cancel or Escape while running; drained by the state layer.
    cancel_request: bool,
    /// Set when the folder button is pressed; drained by the state layer,
    /// which owns the native picker.
    folder_request: bool,
}

impl Default for TurntableModal {
    fn default() -> Self {
        Self {
            open: false,
            phase: TurntablePhase::Idle,
            resolution: TurntableResolution::default(),
            fps: 30,
            duration: 4,
            includes: TurntableIncludes::default(),
            folder: None,
            stem: "turntable".to_owned(),
            progress: (0, 0),
            status: String::new(),
            start_request: false,
            cancel_request: false,
            folder_request: false,
        }
    }
}

impl TurntableModal {
    /// Open the dialog. The settings from the last run survive, so a second
    /// export of the same shot is one press rather than four.
    pub fn open_dialog(&mut self) {
        self.open = true;
        self.phase = TurntablePhase::Idle;
        self.progress = (0, 0);
        self.status.clear();
        self.start_request = false;
        self.cancel_request = false;
        self.folder_request = false;
    }

    /// The export was asked for, or nothing.
    pub fn take_start_request(&mut self) -> Option<TurntableRequest> {
        if !std::mem::take(&mut self.start_request) {
            return None;
        }
        let folder = self.folder.clone()?;
        Some(TurntableRequest {
            resolution: self.resolution,
            fps: self.fps,
            duration: self.duration,
            includes: self.includes,
            folder,
            stem: self.stem.clone(),
        })
    }

    pub fn take_cancel_request(&mut self) -> bool {
        std::mem::take(&mut self.cancel_request)
    }

    pub fn take_folder_request(&mut self) -> bool {
        std::mem::take(&mut self.folder_request)
    }

    pub fn set_folder(&mut self, folder: PathBuf) {
        self.folder = Some(folder);
    }

    /// The job has begun.
    pub fn begin(&mut self, total: u32) {
        self.phase = TurntablePhase::Running;
        self.progress = (0, total);
        self.status = format!("Rendering 0 / {total}");
    }

    pub fn set_progress(&mut self, written: u32, total: u32) {
        self.progress = (written, total);
        self.status = format!("Rendering {written} / {total}");
    }

    /// Every frame is written.
    pub fn finish(&mut self) {
        self.phase = TurntablePhase::Done;
        "Done".clone_into(&mut self.status);
    }

    /// The run was stopped. The frames already written stay on disk, which is
    /// the whole reason the count is said here rather than swallowed.
    pub fn mark_cancelled(&mut self, written: u32) {
        if self.phase == TurntablePhase::Running {
            self.phase = TurntablePhase::Cancelled;
            self.status = format!("Cancelled after {written} frames");
        }
    }

    pub fn fail(&mut self, why: &str) {
        self.phase = TurntablePhase::Failed;
        why.clone_into(&mut self.status);
    }

    fn close(&mut self) {
        self.open = false;
    }
}

/// Draw the turntable export modal.
#[allow(clippy::too_many_lines)]
pub(in crate::gui) fn draw_turntable_modal(
    ctx: &egui::Context,
    modal: &mut TurntableModal,
    theme: &Theme,
) {
    if !modal.open {
        return;
    }

    // Escape: cancel first, dismiss second. Consumed here so the shell's
    // escape chain never sees it while this dialog is up.
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        if modal.phase == TurntablePhase::Running {
            modal.cancel_request = true;
        } else {
            modal.close();
            return;
        }
    }

    let running = modal.phase == TurntablePhase::Running;
    let mut keep_open = true;
    let mut close_clicked = false;
    let default_pos = ctx.content_rect().center() - egui::vec2(230.0, 200.0);

    let mut window = egui::Window::new("Export Turntable")
        .resizable(false)
        .collapsible(false)
        .movable(true)
        .default_pos(default_pos);
    // No corner X while running, for the still dialog's reason: the two ways
    // out of a running export are Cancel and Escape, and both say what they
    // left behind.
    if !running {
        window = window.open(&mut keep_open);
    }
    window.show(ctx, |ui| {
        ui.add_enabled_ui(!running, |ui| {
            // Format is one row with one choice rather than no row at all:
            // a person who knows the browser will look for it, and an absent
            // row reads as a dialog that forgot rather than as a platform
            // that cannot.
            ui.horizontal(|ui| {
                ui.label("Format");
                ui.label(egui::RichText::new("PNG sequence").color(theme.fg));
            });
            ui.label(
                egui::RichText::new(
                    "A numbered frame per file, which any editor can import. Encoding a \
                     video needs a browser's encoder, so this shell writes the sequence.",
                )
                .small()
                .color(theme.muted),
            );
            ui.add_space(6.0);

            if let Some(v) = crate::gui::widgets::combo_with_tooltip(
                ui,
                "Resolution",
                "Frame size, relative to the pane's current on-screen size. Larger \
                 multipliers cost proportionally more time per frame.",
                modal.resolution,
                &TurntableResolution::ALL,
            ) {
                modal.resolution = v;
            }

            ui.horizontal(|ui| {
                ui.label("Frame rate").on_hover_text(
                    "Frames per second in the exported sequence. Independent of the scene \
                     clock: a turntable is a camera move, not scene time, so this does not \
                     read the playbar's rate.",
                );
                ui.add(
                    egui::DragValue::new(&mut modal.fps)
                        .speed(1.0)
                        .range(1..=60),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Duration").on_hover_text(
                    "How long one full 360-degree rotation takes. Frame rate times duration \
                     is the frame count shown below, and every frame is rendered, so both \
                     directly cost export time.",
                );
                ui.add(
                    egui::DragValue::new(&mut modal.duration)
                        .speed(1.0)
                        .range(1..=30)
                        .suffix(" s"),
                );
            });

            ui.add_space(6.0);
            ui.label(egui::RichText::new("Include in each frame").strong());
            ui.horizontal(|ui| {
                ui.checkbox(&mut modal.includes.grid, "Grid");
                ui.checkbox(&mut modal.includes.axes, "Axes");
                ui.checkbox(&mut modal.includes.validation, "Validation");
            });
            ui.label(
                egui::RichText::new(format!(
                    "{} frames, one 360 rotation",
                    frame_count(modal.fps, modal.duration)
                ))
                .small()
                .color(theme.muted),
            );

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Folder\u{2026}").clicked() {
                    modal.folder_request = true;
                }
                match &modal.folder {
                    Some(p) => {
                        ui.label(
                            egui::RichText::new(p.display().to_string())
                                .monospace()
                                .small()
                                .color(theme.muted),
                        );
                    }
                    None => {
                        ui.label(
                            egui::RichText::new("No folder chosen")
                                .small()
                                .color(theme.muted),
                        );
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Name").on_hover_text(
                    "Each frame is written as this name followed by its number, so one \
                     folder can hold more than one export.",
                );
                ui.add(egui::TextEdit::singleline(&mut modal.stem).desired_width(160.0));
            });
        });

        ui.add_space(8.0);
        let (written, total) = modal.progress;
        match modal.phase {
            TurntablePhase::Running => {
                let pct = if total == 0 {
                    0.0
                } else {
                    written as f32 / total as f32
                };
                ui.add(egui::ProgressBar::new(pct.clamp(0.0, 1.0)).desired_width(360.0));
                ui.label(egui::RichText::new(&modal.status).small().color(theme.fg));
            }
            TurntablePhase::Idle => {
                if modal.folder.is_none() {
                    ui.label(
                        egui::RichText::new("Choose a folder to export into.")
                            .small()
                            .color(theme.muted),
                    );
                } else {
                    ui.label(
                        egui::RichText::new("Ready. Press Export to begin.")
                            .small()
                            .color(theme.muted),
                    );
                }
            }
            TurntablePhase::Done | TurntablePhase::Cancelled => {
                ui.label(egui::RichText::new(&modal.status).small().color(theme.fg));
            }
            TurntablePhase::Failed => {
                ui.label(
                    egui::RichText::new(&modal.status)
                        .small()
                        .color(theme.severity_error),
                );
            }
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if running {
                if ui.button("Cancel").clicked() {
                    modal.cancel_request = true;
                }
            } else {
                if ui.button("Close").clicked() {
                    close_clicked = true;
                }
                let ready = modal.folder.is_some() && !modal.stem.trim().is_empty();
                let label = match modal.phase {
                    TurntablePhase::Idle => "Export",
                    _ => "Export again",
                };
                if ui
                    .add_enabled(ready, egui::Button::new(label))
                    .on_disabled_hover_text("Choose a folder and a name before exporting")
                    .clicked()
                {
                    modal.start_request = true;
                }
            }
        });
    });

    if !keep_open || close_clicked {
        modal.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_is_the_rate_times_the_duration() {
        assert_eq!(frame_count(30, 4), 120);
        assert_eq!(frame_count(24, 1), 24);
        assert_eq!(frame_count(60, 30), 1800);
    }

    #[test]
    fn a_turn_is_never_shorter_than_two_frames() {
        // The floor is what stops a one-frame "turn", which is a still with
        // extra steps and no way to read a rotation out of.
        assert_eq!(frame_count(1, 1), 2);
    }

    #[test]
    fn the_last_frame_stops_one_step_short_of_a_full_turn() {
        // A frame at 360 degrees would be the first frame again, so the
        // sequence would stutter when it loops.
        let count = frame_count(30, 4);
        assert!((azimuth_deg(0, count) - 0.0).abs() < f32::EPSILON);
        let last = azimuth_deg(count - 1, count);
        let step = 360.0 / count as f32;
        assert!((last - (360.0 - step)).abs() < 1e-3, "last was {last}");
    }

    #[test]
    fn the_azimuths_are_absolute_rather_than_cumulative() {
        // Each frame is derived from the base pose, so the midpoint of a turn
        // is exactly half a turn however many frames precede it.
        let count = 120;
        assert!((azimuth_deg(60, count) - 180.0).abs() < 1e-3);
        assert!((azimuth_deg(30, count) - 90.0).abs() < 1e-3);
    }

    #[test]
    fn the_includes_start_at_the_browsers_defaults() {
        let d = TurntableIncludes::default();
        assert!(d.grid, "the browser starts with the grid on");
        assert!(!d.axes);
        assert!(!d.validation);
    }

    #[test]
    fn the_resolutions_are_the_browsers_three() {
        let labels: Vec<String> = TurntableResolution::ALL
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(labels, ["Viewport", "1.5x", "2x"]);
        assert!((TurntableResolution::Viewport.factor() - 1.0).abs() < f32::EPSILON);
        assert!((TurntableResolution::OneAndHalf.factor() - 1.5).abs() < f32::EPSILON);
        assert!((TurntableResolution::Two.factor() - 2.0).abs() < f32::EPSILON);
    }
}
