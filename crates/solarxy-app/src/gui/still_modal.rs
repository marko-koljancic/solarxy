//! Still-render modal.
//!
//! `Render → Render Still…` starts the tiled still job; this modal shows
//! tile and sample progress with a live preview assembled from finished
//! tiles, and offers Cancel while running and `Save As…` when done.
//! Nothing is written to disk until the user picks a path.
//!
//! Escape cancels the running render before it dismisses anything else:
//! the modal consumes the key itself while running, ahead of the shell's
//! escape chain, which is the same priority the web dialog implements.

use image::RgbaImage;

use super::theme::Theme;

/// What the render is doing, which is what the modal shows.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum StillPhase {
    #[default]
    Running,
    Finished,
    Cancelled,
    Failed,
}

/// State for the still-render modal — owned by `EguiRenderer`.
#[derive(Default)]
pub(crate) struct StillRenderModal {
    pub open: bool,
    phase: StillPhase,
    width: u32,
    height: u32,
    traced: bool,
    samples: u32,
    denoise: bool,
    /// `(tile, tiles, sample, samples)`, from the job each pump.
    progress: (u32, u32, u32, u32),
    /// How long the render has taken so far.
    elapsed_ms: u64,
    /// How much longer, or nothing while there is not enough to say.
    remaining_ms: Option<u64>,
    /// The finished picture at full resolution, kept until save or close.
    image: Option<RgbaImage>,
    /// A fresh preview-sized frame from the state layer, uploaded on the
    /// next draw. Swapped in before anything is drawn this frame, so the
    /// handle being replaced is only referenced by frames already
    /// submitted.
    pending_preview: Option<RgbaImage>,
    preview: Option<egui::TextureHandle>,
    filename: String,
    /// Set by Cancel or Escape while running; drained by the state layer.
    cancel_request: bool,
    /// Set when `Save As…` is clicked; drained by the state layer.
    save_request: bool,
}

impl StillRenderModal {
    /// Open for a fresh run.
    pub fn start(
        &mut self,
        width: u32,
        height: u32,
        traced: bool,
        samples: u32,
        denoise: bool,
        filename: String,
    ) {
        self.open = true;
        self.phase = StillPhase::Running;
        self.width = width;
        self.height = height;
        self.traced = traced;
        self.samples = samples;
        self.denoise = denoise;
        self.progress = (0, 0, 0, samples);
        self.elapsed_ms = 0;
        self.remaining_ms = None;
        self.image = None;
        self.pending_preview = None;
        self.preview = None;
        self.filename = filename;
        self.cancel_request = false;
        self.save_request = false;
    }

    pub fn set_progress(&mut self, tile: u32, tiles: u32, sample: u32, samples: u32) {
        self.progress = (tile, tiles, sample, samples);
    }

    /// How long the render has taken, and how much longer it will take.
    ///
    /// Both computed by the shared job rather than here, so this shell reads
    /// the same answer the browser dialog and the terminal dashboard read.
    pub fn set_timing(&mut self, elapsed_ms: u64, remaining_ms: Option<u64>) {
        self.elapsed_ms = elapsed_ms;
        self.remaining_ms = remaining_ms;
    }

    pub fn set_preview(&mut self, preview: RgbaImage) {
        self.pending_preview = Some(preview);
    }

    pub fn finish(&mut self, image: RgbaImage) {
        self.image = Some(image);
        self.phase = StillPhase::Finished;
    }

    pub fn fail(&mut self) {
        self.phase = StillPhase::Failed;
        self.image = None;
    }

    /// The run was cancelled from outside the modal's own buttons.
    pub fn mark_cancelled(&mut self) {
        if self.phase == StillPhase::Running {
            self.phase = StillPhase::Cancelled;
        }
    }

    pub fn is_running(&self) -> bool {
        self.open && self.phase == StillPhase::Running
    }

    /// The running job's progress for the status bar, `None` when idle.
    pub fn running_progress(&self) -> Option<(u32, u32, u32, u32)> {
        self.is_running().then_some(self.progress)
    }

    pub fn take_cancel_request(&mut self) -> bool {
        std::mem::take(&mut self.cancel_request)
    }

    pub fn take_save_request(&mut self) -> bool {
        std::mem::take(&mut self.save_request)
    }

    pub fn suggested_filename(&self) -> &str {
        &self.filename
    }

    /// Take the finished image out and close the modal — called once the
    /// user has chosen a save path.
    pub fn take_image(&mut self) -> Option<RgbaImage> {
        self.open = false;
        self.image.take()
    }

    fn close(&mut self) {
        self.open = false;
        self.image = None;
    }
}

/// Draw the still-render modal.
pub(super) fn draw_still_modal(ctx: &egui::Context, modal: &mut StillRenderModal, theme: &Theme) {
    if !modal.open {
        return;
    }

    // Escape: cancel first, dismiss second. Consumed here so the shell's
    // escape chain below never sees it while this modal is up.
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        if modal.phase == StillPhase::Running {
            modal.cancel_request = true;
        } else {
            modal.close();
            return;
        }
    }

    if let Some(fresh) = modal.pending_preview.take() {
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [fresh.width() as usize, fresh.height() as usize],
            fresh.as_raw(),
        );
        modal.preview =
            Some(ctx.load_texture("solarxy_still_preview", color, egui::TextureOptions::LINEAR));
    }

    let running = modal.phase == StillPhase::Running;
    let mut keep_open = true;
    let mut close_clicked = false;
    let default_pos = ctx.content_rect().center() - egui::vec2(250.0, 220.0);

    let mut window = egui::Window::new("Render Still")
        .resizable(false)
        .collapsible(false)
        .movable(true)
        .default_pos(default_pos);
    // No corner X while running: the two ways out of a running render are
    // Cancel and Escape, and both say what they did.
    if !running {
        window = window.open(&mut keep_open);
    }
    window.show(ctx, |ui| {
        let engine = if modal.traced {
            let d = if modal.denoise { ", denoised" } else { "" };
            format!("path traced \u{00b7} {} spp{d}", modal.samples)
        } else {
            "raster".to_owned()
        };
        ui.label(
            egui::RichText::new(format!(
                "{} \u{00d7} {} \u{00b7} {engine}",
                modal.width, modal.height
            ))
            .small()
            .color(theme.muted),
        );
        ui.add_space(6.0);

        if let Some(preview) = &modal.preview {
            ui.add(egui::Image::new(preview));
            ui.add_space(6.0);
        }

        let (tile, tiles, sample, samples) = modal.progress;
        match modal.phase {
            StillPhase::Running => {
                let pct = if tiles == 0 {
                    0.0
                } else {
                    (tile as f32 + sample as f32 / samples.max(1) as f32) / tiles as f32
                };
                ui.add(egui::ProgressBar::new(pct.clamp(0.0, 1.0)).desired_width(360.0));
                let counts = if samples > 1 {
                    format!(
                        "Tile {} of {tiles}, sample {sample} of {samples}",
                        (tile + 1).min(tiles.max(1))
                    )
                } else {
                    format!("Tile {} of {tiles}", (tile + 1).min(tiles.max(1)))
                };
                // Nothing rather than a guess while the estimate has no rate to
                // work from, which is the first chunks of any render.
                let timing = match modal.remaining_ms {
                    Some(left) => format!(
                        "{} elapsed, {} left",
                        solarxy_host::still::format_duration_ms(modal.elapsed_ms),
                        solarxy_host::still::format_duration_ms(left)
                    ),
                    None => format!(
                        "{} elapsed",
                        solarxy_host::still::format_duration_ms(modal.elapsed_ms)
                    ),
                };
                ui.label(
                    egui::RichText::new(format!("{counts} · {timing}"))
                        .small()
                        .color(theme.fg),
                );
            }
            StillPhase::Finished => {
                ui.label(
                    egui::RichText::new(format!(
                        "Done: {tiles} tiles in {}",
                        solarxy_host::still::format_duration_ms(modal.elapsed_ms)
                    ))
                    .small()
                    .color(theme.fg),
                );
                ui.label(
                    egui::RichText::new(&modal.filename)
                        .monospace()
                        .small()
                        .color(theme.muted),
                );
            }
            StillPhase::Cancelled => {
                ui.label(egui::RichText::new("Cancelled").small().color(theme.muted));
            }
            StillPhase::Failed => {
                ui.label(
                    egui::RichText::new("Failed; the picture is incomplete")
                        .small()
                        .color(theme.severity_error),
                );
            }
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);
        ui.horizontal(|ui| match modal.phase {
            StillPhase::Running => {
                if ui.button("Cancel").clicked() {
                    modal.cancel_request = true;
                }
            }
            StillPhase::Finished => {
                if ui.button("Close").clicked() {
                    close_clicked = true;
                }
                if ui.button("Save As\u{2026}").clicked() {
                    modal.save_request = true;
                }
            }
            StillPhase::Cancelled | StillPhase::Failed => {
                if ui.button("Close").clicked() {
                    close_clicked = true;
                }
            }
        });
    });

    if !keep_open || close_clicked {
        modal.close();
    }
}
