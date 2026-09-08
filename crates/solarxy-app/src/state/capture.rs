//! Screenshot capture: the shell's half of a screenshot, which is choosing
//! the rect, naming the file, and running the save dialog.
//!
//! The readback is `solarxy_renderer::capture`: encoding the copy, stripping
//! the row padding, swizzling a BGRA surface and polling the map without
//! blocking. This shell carried a private copy of all of that until 0.10.0,
//! for no better reason than that it predated the shared module, and the
//! shared module's own header said so.
//!
//! The capture is cropped to the **active pane's content rect** (toolbar
//! strip excluded). Writing the PNG is the screenshot modal's job; this
//! module only produces the in-memory image.
//!
//! **The browser's capture ceiling is deliberately not inherited.** It exists
//! because a large capture can lose the WebGPU device with no recovery, which
//! is a limit that platform imposes on itself rather than a property of a
//! screenshot. This shell has never had one and does not gain one here.

use solarxy_renderer::capture::{CapturePoll, encode_capture};

use super::*;

impl State {
    /// Arm the async readback on a freshly-encoded capture: request the map
    /// (after the copy's submission) and stash it with the modal context.
    /// Completion is polled by [`State::poll_pending_capture`] on later
    /// frames, never waited on, so a screenshot costs no frame hitch.
    pub(super) fn arm_pending_capture(
        &mut self,
        buffer: wgpu::Buffer,
        padded_row_bytes: u32,
        width: u32,
        height: u32,
    ) {
        self.pending_capture = Some(super::PendingCapture {
            readback: solarxy_renderer::capture::PendingCapture::arm(
                buffer,
                padded_row_bytes,
                width,
                height,
            ),
            filename: self.screenshot_filename(),
            review_active: self.review.active,
            expand_review: self.screenshot_expand_review,
        });
    }

    /// Check the in-flight capture readback; on completion hand the image to
    /// the screenshot modal. Called once per frame.
    pub(super) fn poll_pending_capture(&mut self) {
        let Some(pending) = &self.pending_capture else {
            return;
        };
        // The surface format decides the swizzle, exactly as it did when this
        // shell did the swizzling itself.
        let (width, height) = (pending.readback.width, pending.readback.height);
        match pending.readback.poll(&self.device, self.config.format) {
            // Not resolved yet; the buffer stays armed and the next frame
            // asks again.
            CapturePoll::Pending => {}
            CapturePoll::Failed => self.pending_capture = None,
            CapturePoll::Ready(pixels) => {
                let Some(pending) = self.pending_capture.take() else {
                    return;
                };
                if let Some(image) = image::RgbaImage::from_raw(width, height, pixels) {
                    self.gui.set_screenshot_capture(
                        image,
                        pending.filename,
                        pending.review_active,
                        pending.expand_review,
                    );
                } else {
                    tracing::error!("Failed to create image from captured pixel data");
                }
            }
        }
    }

    /// Encode a capture of the active pane's content rect (toolbar strip
    /// excluded). `None` when there is no pane to capture.
    pub(super) fn encode_active_pane_capture(
        &self,
        panes: &[Pane],
        texture: &wgpu::Texture,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Option<(wgpu::Buffer, u32, u32, u32)> {
        let pane = panes.get(self.view.active_pane)?;
        let content = pane.content(self.pane_toolbar_height_px());
        let rect = clamp_capture_rect(&content, self.config.width, self.config.height);
        let (buffer, padded) = encode_capture(&self.device, encoder, texture, rect);
        Some((buffer, padded, rect.2, rect.3))
    }

    /// Suggested screenshot file name: `<model-stem>_<YYYYMMDD-HHMMSS>`
    /// (`solarxy_…` when no model is loaded).
    pub(super) fn screenshot_filename(&self) -> String {
        let stem = self
            .scene
            .as_ref()
            .map(|s| s.model_path.as_str())
            .and_then(|p| std::path::Path::new(p).file_stem())
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("solarxy");
        let now =
            time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
        let stamp = now
            .format(&time::macros::format_description!(
                "[year][month][day]-[hour][minute][second]"
            ))
            .unwrap_or_default();
        format!("{stem}_{stamp}.png")
    }

    /// Drain the screenshot modal's deferred actions: run the native save
    /// dialog for a `Save As…`, or arm a re-capture for an expand toggle.
    pub(super) fn handle_screenshot_modal(&mut self) {
        if self.gui.take_screenshot_save_request() {
            let suggested = self.gui.screenshot_suggested_filename();
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name(&suggested)
                .add_filter("PNG image", &["png"])
                .save_file()
                && let Some(image) = self.gui.take_screenshot_image()
            {
                match image.save_with_format(&path, image::ImageFormat::Png) {
                    Ok(()) => {
                        let name = path
                            .file_name()
                            .and_then(std::ffi::OsStr::to_str)
                            .unwrap_or("screenshot")
                            .to_string();
                        self.gui.set_capture_message(name);
                    }
                    Err(e) => {
                        tracing::error!("Failed to save screenshot: {e}");
                        self.gui.set_toast(
                            &format!("Couldn't save screenshot: {e}"),
                            crate::gui::ToastSeverity::Error,
                        );
                    }
                }
            }
        }

        if let Some(expand) = self.gui.take_screenshot_recapture() {
            self.capture_requested = true;
            self.screenshot_expand_review = expand;
        }
    }
}

/// Clamp a pane content rect (physical px, `f32`) to integer surface
/// bounds, guaranteeing a non-empty region.
fn clamp_capture_rect(content: &Pane, surface_w: u32, surface_h: u32) -> (u32, u32, u32, u32) {
    let x = (content.x.max(0.0) as u32).min(surface_w.saturating_sub(1));
    let y = (content.y.max(0.0) as u32).min(surface_h.saturating_sub(1));
    let w = (content.width.max(1.0) as u32).clamp(1, surface_w - x);
    let h = (content.height.max(1.0) as u32).clamp(1, surface_h - y);
    (x, y, w, h)
}
