//! UV overlap GPU readback polling: maps the overlap counter buffer once
//! the readback fence resolves and surfaces the per-mesh overlap percentage
//! to the sidebar.

use super::{State, UvOverlapResources};

impl State {
    pub(super) fn poll_overlap_stats(&mut self) {
        if !self.renderer.uv_overlap.readback_pending {
            return;
        }
        let Some(buf) = self.renderer.uv_overlap.staging_buffer.take() else {
            return;
        };
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        let _ = self.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });
        if rx.recv().is_ok_and(|r| r.is_ok()) {
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
            self.renderer.uv_overlap.overlap_pct = if total_nonzero > 0 {
                Some(overlap as f32 / total_nonzero as f32 * 100.0)
            } else {
                Some(0.0)
            };
        }
        self.renderer.uv_overlap.readback_pending = false;
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
    uv_overlap.stats_dirty = false;
}
