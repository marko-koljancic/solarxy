//! UV overlap GPU readback: a polled `map_async` state machine. Arm the
//! map request once, then check completion on later frame ticks — no
//! blocking `device.poll(Wait)` (which does not exist on WebGPU and
//! hitches a desktop frame).

use super::{State, UvOverlapResources};

impl State {
    pub(super) fn poll_overlap_stats(&mut self) {
        if !self.renderer.uv_overlap.readback_pending {
            return;
        }

        // Arm once: request the async map on the staged buffer.
        if self.renderer.uv_overlap.map_receiver.is_none() {
            let Some(buf) = &self.renderer.uv_overlap.staging_buffer else {
                self.renderer.uv_overlap.readback_pending = false;
                return;
            };
            let (tx, rx) = std::sync::mpsc::channel();
            buf.slice(..).map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
            self.renderer.uv_overlap.map_receiver = Some(rx);
        }

        // Pump the device without blocking, then check for completion.
        let _ = self.device.poll(wgpu::PollType::Poll);
        let ready = match &self.renderer.uv_overlap.map_receiver {
            Some(rx) => match rx.try_recv() {
                Ok(Ok(())) => true,
                // Not resolved yet — try again next frame.
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                // Map failed or the sender vanished: abandon this readback.
                Ok(Err(_)) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    tracing::error!("UV overlap readback map failed");
                    false
                }
            },
            None => false,
        };

        let ov = &mut self.renderer.uv_overlap;
        ov.map_receiver = None;
        ov.readback_pending = false;
        let Some(buf) = ov.staging_buffer.take() else {
            return;
        };
        if !ready {
            return;
        }

        let slice = buf.slice(..);
        let data = slice.get_mapped_range();
        let mut total_nonzero = 0u64;
        let mut overlap = 0u64;
        for &byte in data.iter() {
            if byte > 0 {
                total_nonzero += 1;
            }
            if byte > 1 {
                overlap += 1;
            }
        }
        drop(data);
        buf.unmap();
        ov.overlap_pct = if total_nonzero > 0 {
            Some(overlap as f32 / total_nonzero as f32 * 100.0)
        } else {
            Some(0.0)
        };
    }
}

pub(super) fn request_overlap_readback_impl(
    device: &wgpu::Device,
    uv_overlap: &mut UvOverlapResources,
    encoder: &mut wgpu::CommandEncoder,
) {
    const STATS_SIZE: u32 = 512;
    let bytes_per_row = STATS_SIZE;
    let buffer_size = u64::from(bytes_per_row * STATS_SIZE);
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("UV Overlap Readback"),
        size: buffer_size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &uv_overlap.stats_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(STATS_SIZE),
            },
        },
        wgpu::Extent3d {
            width: STATS_SIZE,
            height: STATS_SIZE,
            depth_or_array_layers: 1,
        },
    );
    uv_overlap.staging_buffer = Some(staging);
    uv_overlap.readback_pending = true;
    uv_overlap.map_receiver = None;
    uv_overlap.stats_dirty = false;
}
