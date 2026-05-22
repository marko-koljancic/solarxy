//! Screenshot review modal.
//!
//! `C` (or `Render → Save Screenshot…`) captures the active pane's 3D
//! content into memory; this modal then shows a downscaled preview and a
//! `Save As…` button. Nothing is written to disk until the user picks a
//! path — `Cancel` / `Esc` discards.
//!
//! In review mode the modal also offers an "Expand all review notes"
//! toggle. Toggling it re-captures the pane with every annotation card
//! forced open (the state layer drives the re-capture; the modal is
//! suppressed for that single frame so it cannot appear in the shot).
//!
//! The captured full-resolution image lives here until save/cancel. The
//! preview `TextureHandle` is built lazily on first draw — the capture
//! readback only resolves after the egui pass that requested it.

use image::RgbaImage;

use super::theme::Theme;

/// Longest edge of the in-modal preview, logical px.
const PREVIEW_MAX_EDGE: f32 = 480.0;

/// State for the screenshot modal — owned by `EguiRenderer`.
#[derive(Default)]
pub(crate) struct ScreenshotModal {
    pub open: bool,
    /// Full-resolution capture of the active pane, kept until save/cancel.
    image: Option<RgbaImage>,
    /// Downscaled preview, uploaded lazily from `image` on first draw.
    ///
    /// Only ever dropped/replaced in [`Self::set_capture`] — never in
    /// `close` / `take_image`. Dropping an `egui::TextureHandle` inside
    /// the `ctx.run` closure that drew it makes egui free the GPU texture
    /// the *same* frame, before the recorded draw is submitted (a
    /// "texture has been destroyed" validation crash). `set_capture` runs
    /// only on a capture frame, where the modal is suppressed and never
    /// draws this `Image` — so the swap there is always race-free.
    preview: Option<egui::TextureHandle>,
    /// Suggested file name — shown to the user and pre-filled into the
    /// native save dialog.
    filename: String,
    /// `true` while the active capture was taken in review mode — gates
    /// the "Expand all review notes" toggle.
    review_available: bool,
    /// Checkbox state — mirrors the expand setting the current `image`
    /// was captured with.
    expand_review: bool,
    /// Set when the checkbox is toggled; drained by the state layer to
    /// trigger a re-capture.
    recapture: bool,
    /// Set when `Save As…` is clicked; drained by the state layer to run
    /// the native save dialog.
    save_request: bool,
}

impl ScreenshotModal {
    /// Install a fresh capture — called after every readback (the initial
    /// `C` press and each re-capture). `expand_review` is the setting the
    /// image was actually captured with, so the checkbox always matches
    /// the preview.
    pub fn set_capture(
        &mut self,
        image: RgbaImage,
        filename: String,
        review_available: bool,
        expand_review: bool,
    ) {
        self.image = Some(image);
        self.preview = None;
        self.filename = filename;
        self.review_available = review_available;
        self.expand_review = expand_review;
        self.recapture = false;
        self.save_request = false;
        self.open = true;
    }

    /// Drain a pending re-capture request, returning the desired
    /// expand-review setting.
    pub fn take_recapture(&mut self) -> Option<bool> {
        std::mem::take(&mut self.recapture).then_some(self.expand_review)
    }

    /// Drain a pending `Save As…` request.
    pub fn take_save_request(&mut self) -> bool {
        std::mem::take(&mut self.save_request)
    }

    pub fn suggested_filename(&self) -> &str {
        &self.filename
    }

    /// Take the captured image out and close the modal — called once the
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

/// Build the downscaled preview texture from the captured pixels.
fn build_preview(ctx: &egui::Context, image: &RgbaImage) -> egui::TextureHandle {
    let (w, h) = (image.width(), image.height());
    let scale = (PREVIEW_MAX_EDGE / w.max(h) as f32).min(1.0);
    let pw = ((w as f32 * scale).round() as u32).max(1);
    let ph = ((h as f32 * scale).round() as u32).max(1);
    let scaled = image::imageops::thumbnail(image, pw, ph);
    let color =
        egui::ColorImage::from_rgba_unmultiplied([pw as usize, ph as usize], scaled.as_raw());
    ctx.load_texture(
        "solarxy_screenshot_preview",
        color,
        egui::TextureOptions::LINEAR,
    )
}

/// Draw the screenshot modal. `Esc` / `Cancel` discards; the save and
/// re-capture are deferred to the state layer via the drained flags.
pub(super) fn draw_screenshot_modal(
    ctx: &egui::Context,
    modal: &mut ScreenshotModal,
    theme: &Theme,
) {
    if !modal.open {
        return;
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        modal.close();
        return;
    }

    // The capture readback resolves after the egui pass that requested
    // it, so the preview can only be uploaded from the next frame on.
    if modal.preview.is_none()
        && let Some(image) = &modal.image
    {
        modal.preview = Some(build_preview(ctx, image));
    }

    let mut keep_open = true;
    let mut cancel = false;
    let default_pos = ctx.content_rect().center() - egui::vec2(250.0, 200.0);

    egui::Window::new("Screenshot")
        .open(&mut keep_open)
        .resizable(false)
        .collapsible(false)
        .movable(true)
        .default_pos(default_pos)
        .show(ctx, |ui| {
            if let Some(preview) = &modal.preview {
                ui.add(egui::Image::new(preview));
            }
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(&modal.filename)
                    .monospace()
                    .small()
                    .color(theme.muted),
            );

            if modal.review_available {
                ui.add_space(4.0);
                if ui
                    .checkbox(&mut modal.expand_review, "Expand all review notes")
                    .on_hover_text("Re-captures with every annotation card open")
                    .changed()
                {
                    modal.recapture = true;
                }
            }

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
                if ui.button("Save As\u{2026}").clicked() {
                    modal.save_request = true;
                }
            });
        });

    if !keep_open || cancel {
        modal.close();
    }
}
