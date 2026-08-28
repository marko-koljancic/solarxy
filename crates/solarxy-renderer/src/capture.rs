//! Shared screenshot capture: encode a texture sub-rect copy into a
//! CPU-mappable staging buffer, then poll the readback WITHOUT blocking
//! (the `UvOverlapResources` pattern: `map_async` + mpsc + `PollType::Poll`,
//! wasm-safe because WebGPU has no blocking wait). The web host drives this
//! for its screenshot modal; the desktop shell's `state/capture.rs` predates
//! the module and migrates in a later cleanup (TODO), so desktop behavior is
//! untouched this phase.

use std::sync::mpsc::{Receiver, TryRecvError};

use crate::panes::PaneRect;

/// An offscreen render target for a capture, with the pane rect that covers it.
///
/// Bundled because the composite and the readback both need most of it and
/// passing the pieces separately runs the argument count into the lint.
///
/// Shared rather than per shell: the web screenshot path and the still render
/// job allocate the same thing for the same reason, and a second copy would be
/// the one that stopped matching when the usage flags changed.
pub struct CaptureTarget {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    /// The whole target, as a pane rect, which is what a capture composites
    /// with: full-rect viewport, always clearing, never a sub-rect of a layout.
    pub rect: PaneRect,
    pub width: u32,
    pub height: u32,
}

impl CaptureTarget {
    #[must_use]
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Capture Target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            #[allow(clippy::cast_precision_loss)]
            rect: PaneRect {
                x: 0.0,
                y: 0.0,
                width: width as f32,
                height: height as f32,
            },
            width,
            height,
        }
    }
}

/// Bytes per padded row for a `width`-pixel RGBA8 copy
/// (`COPY_BYTES_PER_ROW_ALIGNMENT`).
#[must_use]
pub fn padded_row_bytes(width: u32) -> u32 {
    padded_row_bytes_for(width, 4)
}

/// Bytes per padded row for a copy of `width` texels at `bytes_per_pixel`.
///
/// The tracer's targets are `Rgba32Float`, four times the width of a screenshot
/// row, so the alignment arithmetic cannot assume a byte count.
#[must_use]
pub fn padded_row_bytes_for(width: u32, bytes_per_pixel: u32) -> u32 {
    let unpadded = width * bytes_per_pixel;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    unpadded.div_ceil(align) * align
}

/// Encodes a copy of `rect` (physical pixels: x, y, w, h) of `texture` into
/// a fresh `MAP_READ` staging buffer. Arm the readback with
/// [`PendingCapture::arm`] AFTER submitting the encoder.
#[must_use]
pub fn encode_capture(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    rect: (u32, u32, u32, u32),
) -> (wgpu::Buffer, u32) {
    let (x, y, width, height) = rect;
    // Taken from the texture rather than assumed: every caller today copies an
    // 8-bit-per-channel surface, and the tracer's float targets are the first
    // that are not.
    let bytes_per_pixel = texture
        .format()
        .block_copy_size(Some(wgpu::TextureAspect::All))
        .unwrap_or(4);
    let padded = padded_row_bytes_for(width, bytes_per_pixel);

    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Capture Staging Buffer"),
        size: u64::from(padded) * u64::from(height),
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
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );

    (buffer, padded)
}

/// One in-flight capture readback.
pub struct PendingCapture {
    buffer: wgpu::Buffer,
    padded_row_bytes: u32,
    pub width: u32,
    pub height: u32,
    receiver: Receiver<Result<(), wgpu::BufferAsyncError>>,
}

/// The state of a polled capture.
pub enum CapturePoll {
    /// Not resolved yet; poll again next frame.
    Pending,
    /// The map failed; the capture is abandoned.
    Failed,
    /// Tightly-packed RGBA8 pixels, row padding stripped, swizzled if the
    /// source format was BGRA.
    Ready(Vec<u8>),
}

impl PendingCapture {
    /// Requests the async map on a submitted capture copy.
    #[must_use]
    pub fn arm(buffer: wgpu::Buffer, padded_row_bytes: u32, width: u32, height: u32) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        Self {
            buffer,
            padded_row_bytes,
            width,
            height,
            receiver: rx,
        }
    }

    /// Pumps the device (non-blocking) and checks the map. On `Ready`, the
    /// returned pixels are unpadded RGBA8; `source_format` decides the
    /// BGRA swizzle (typical browser surfaces are `Bgra8UnormSrgb`).
    pub fn poll(&self, device: &wgpu::Device, source_format: wgpu::TextureFormat) -> CapturePoll {
        let _ = device.poll(wgpu::PollType::Poll);
        match self.receiver.try_recv() {
            Ok(Ok(())) => {}
            Err(TryRecvError::Empty) => return CapturePoll::Pending,
            Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
                tracing::error!("capture readback map failed");
                return CapturePoll::Failed;
            }
        }

        let mut pixels = self.unpad(4);

        if matches!(
            source_format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            for chunk in pixels.as_chunks_mut::<4>().0 {
                chunk.swap(0, 2);
            }
        }
        CapturePoll::Ready(pixels)
    }

    /// The same readback, decoded to floats: four per pixel from a four-channel
    /// source, one from a single-channel one.
    ///
    /// For an image that is going to a float file rather than to a screen. The
    /// eight-bit path above is not a step on the way here: it has already
    /// quantized, and no amount of widening afterwards puts back what the
    /// rounding removed.
    ///
    /// Half-float and full-float sources both arrive as `f32`, because the
    /// difference is a storage decision the caller made and not something a
    /// consumer of the pixels should have to know.
    ///
    /// A source that is neither is a caller error rather than a runtime one, so
    /// it returns `Failed` rather than guessing at a conversion.
    pub fn poll_floats(
        &self,
        device: &wgpu::Device,
        source_format: wgpu::TextureFormat,
    ) -> CaptureFloatPoll {
        let bytes_per_pixel = match source_format {
            wgpu::TextureFormat::Rgba16Float => 8,
            wgpu::TextureFormat::Rgba32Float => 16,
            // One float per pixel rather than four, which is what a depth pass
            // writes: a distance is one number, and the caller reads the result
            // back at the width the format states rather than at four.
            wgpu::TextureFormat::R32Float => 4,
            _ => {
                tracing::error!("a float readback was asked for a {source_format:?} source");
                return CaptureFloatPoll::Failed;
            }
        };

        let _ = device.poll(wgpu::PollType::Poll);
        match self.receiver.try_recv() {
            Ok(Ok(())) => {}
            Err(TryRecvError::Empty) => return CaptureFloatPoll::Pending,
            Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
                tracing::error!("capture readback map failed");
                return CaptureFloatPoll::Failed;
            }
        }

        let bytes = self.unpad(bytes_per_pixel);
        let floats = match source_format {
            wgpu::TextureFormat::Rgba16Float => bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| f32::from(half::f16::from_le_bytes(*b)))
                .collect(),
            _ => bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b))
                .collect(),
        };
        CaptureFloatPoll::Ready(floats)
    }

    /// Copies the mapped rows out without their alignment padding.
    ///
    /// The padding is the copy's, not the image's: a texture-to-buffer copy
    /// aligns every row to 256 bytes, so the buffer is wider than the picture
    /// by an amount that depends on the width and the pixel size. Reading it as
    /// though it were tight gives an image that shears progressively, which
    /// looks like a wrong stride rather than like padding.
    fn unpad(&self, bytes_per_pixel: u32) -> Vec<u8> {
        let data = self.buffer.slice(..).get_mapped_range();
        let unpadded_row = (self.width * bytes_per_pixel) as usize;
        let mut out = Vec::with_capacity(unpadded_row * self.height as usize);
        for row in 0..self.height {
            let start = (row * self.padded_row_bytes) as usize;
            out.extend_from_slice(&data[start..start + unpadded_row]);
        }
        drop(data);
        self.buffer.unmap();
        out
    }
}

/// The float readback's state, mirroring [`CapturePoll`].
pub enum CaptureFloatPoll {
    Pending,
    Ready(Vec<f32>),
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padded_rows_align_to_256() {
        assert_eq!(padded_row_bytes(64), 256); // 256 bytes exactly
        assert_eq!(padded_row_bytes(63), 256); // rounds up
        assert_eq!(padded_row_bytes(65), 512);
        assert_eq!(padded_row_bytes(1920), 7680); // already aligned
    }
}
