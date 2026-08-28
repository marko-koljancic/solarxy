//! Screenshot capture: blits a sub-rect of the current frame into a
//! one-shot CPU-mappable buffer and reads it back into an `RgbaImage`.
//!
//! The capture is cropped to the **active pane's content rect** (toolbar
//! strip excluded). Writing the PNG is the screenshot modal's job — this
//! module only produces the in-memory image.

use super::*;

impl State {
    /// Blit `rect` (physical pixels: `x, y, w, h`) of `texture` into a
    /// fresh staging buffer. Pair with [`State::read_capture`] once the
    /// encoder's submission has completed.
    pub(super) fn encode_capture(
        &self,
        texture: &wgpu::Texture,
        encoder: &mut wgpu::CommandEncoder,
        rect: (u32, u32, u32, u32),
    ) -> (wgpu::Buffer, u32, u32, u32) {
        let (x, y, width, height) = rect;
        let bytes_per_pixel = 4u32;
        let unpadded_row_bytes = width * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_row_bytes = unpadded_row_bytes.div_ceil(align) * align;

        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Capture Staging Buffer"),
            size: u64::from(padded_row_bytes * height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row_bytes),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        (buffer, padded_row_bytes, width, height)
    }

    /// Arm the async readback on a freshly-encoded capture: request the
    /// map (after the copy's submission) and stash the buffer plus the
    /// modal context. Completion is polled by [`State::poll_pending_capture`]
    /// on later frames — no blocking wait (WebGPU has none, and desktop
    /// avoids the frame hitch).
    pub(super) fn arm_pending_capture(
        &mut self,
        buffer: wgpu::Buffer,
        padded_row_bytes: u32,
        width: u32,
        height: u32,
    ) {
        let (tx, rx) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        self.pending_capture = Some(super::PendingCapture {
            buffer,
            padded_row_bytes,
            width,
            height,
            receiver: rx,
            filename: self.screenshot_filename(),
            review_active: self.review.active,
            expand_review: self.screenshot_expand_review,
        });
    }

    /// Check the in-flight capture readback; on completion hand the image
    /// to the screenshot modal. Called once per frame.
    pub(super) fn poll_pending_capture(&mut self) {
        if self.pending_capture.is_none() {
            return;
        }
        let _ = self.device.poll(wgpu::PollType::Poll);

        let ready = match &self.pending_capture {
            Some(pending) => match pending.receiver.try_recv() {
                Ok(Ok(())) => true,
                // Not resolved yet — try again next frame.
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Ok(Err(_)) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    tracing::error!("Failed to map capture buffer");
                    false
                }
            },
            None => false,
        };

        let Some(pending) = self.pending_capture.take() else {
            return;
        };
        if !ready {
            return;
        }

        if let Some(image) = Self::process_mapped_capture(&pending, self.config.format) {
            self.gui.set_screenshot_capture(
                image,
                pending.filename,
                pending.review_active,
                pending.expand_review,
            );
        }
    }

    /// Strip row padding from the (already mapped) staging buffer, swizzle
    /// BGRA→RGBA if the surface needs it, and return the captured image.
    /// `None` on a malformed pixel buffer.
    fn process_mapped_capture(
        pending: &super::PendingCapture,
        surface_format: wgpu::TextureFormat,
    ) -> Option<image::RgbaImage> {
        let data = pending.buffer.slice(..).get_mapped_range();
        let bytes_per_pixel = 4u32;
        let unpadded_row_bytes = pending.width * bytes_per_pixel;

        let mut pixels = Vec::with_capacity((unpadded_row_bytes * pending.height) as usize);
        for row in 0..pending.height {
            let start = (row * pending.padded_row_bytes) as usize;
            let end = start + unpadded_row_bytes as usize;
            pixels.extend_from_slice(&data[start..end]);
        }
        drop(data);
        pending.buffer.unmap();

        let needs_swizzle = matches!(
            surface_format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        if needs_swizzle {
            for chunk in pixels.as_chunks_mut::<4>().0 {
                chunk.swap(0, 2);
            }
        }

        let image = image::RgbaImage::from_raw(pending.width, pending.height, pixels);
        if image.is_none() {
            tracing::error!("Failed to create image from captured pixel data");
        }
        image
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
        Some(self.encode_capture(texture, encoder, rect))
    }

    /// Suggested screenshot file name — `<model-stem>_<YYYYMMDD-HHMMSS>`
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
