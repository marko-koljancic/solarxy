//! Review-annotation marker rendering — instanced SDF billboard quads.
//!
//! Each annotation in [`solarxy_core::review`] becomes a
//! [`ReviewMarkerInstance`] in this per-`ModelScene` buffer; the shader
//! at `shaders/review_marker.wgsl` draws a procedural shape per category
//! with anti-aliased edges. Always-on-top by depth-state choice (see
//! `pipelines.rs`'s `review_marker` builder).
//!
//! No vertex buffer — 6 vertices generated from `@builtin(vertex_index)`
//! in the shader form one quad per instance.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

/// Per-instance data uploaded each frame for one annotation marker.
///
/// `flags` packs three orthogonal bits of state into one `u32`:
/// - bits 0..=3 — category (0=Info, 1=Warning, 2=Question, 3=Change)
/// - bit 4     — resolved (alpha dimmed)
/// - bit 5     — selected (cyan ring overlay)
///
/// Total size = 16 bytes (one f32×3 + one u32, naturally aligned). No
/// padding needed.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, PartialEq)]
pub struct ReviewMarkerInstance {
    pub world_pos: [f32; 3],
    pub flags: u32,
}

const _: () = assert!(std::mem::size_of::<ReviewMarkerInstance>() == 16);

/// Bit packed into `flags` to mark an annotation as resolved.
pub const FLAG_RESOLVED: u32 = 1 << 4;
/// Bit packed into `flags` to mark an annotation as selected.
pub const FLAG_SELECTED: u32 = 1 << 5;

impl ReviewMarkerInstance {
    /// Build a marker for one annotation. `category` is the
    /// `AnnotationCategory` discriminant cast to `u32` (Info=0, Warning=1,
    /// Question=2, Change=3).
    pub fn new(world_pos: [f32; 3], category: u32, resolved: bool, selected: bool) -> Self {
        let mut flags = category & 0xF;
        if resolved {
            flags |= FLAG_RESOLVED;
        }
        if selected {
            flags |= FLAG_SELECTED;
        }
        Self { world_pos, flags }
    }

    /// `wgpu::VertexBufferLayout` for an instance-step-mode buffer of
    /// this struct. Attributes match `InstanceInput` in
    /// `shaders/review_marker.wgsl`.
    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Uint32,
                },
            ],
        }
    }
}

/// Per-`ModelScene` resources for the marker pipeline.
///
/// Owns the instance-data GPU buffer plus a count of valid entries.
/// Re-uploaded on annotation set changes (add / edit / resolve / delete
/// / select) — orchestrated by `state::review::write_markers_to_scene`
/// in `solarxy-app`.
pub struct ReviewMarkerResources {
    pub instance_buffer: wgpu::Buffer,
    pub instance_capacity: u32,
    pub instance_count: u32,
}

impl ReviewMarkerResources {
    /// Initial capacity hint — small but non-zero, so the first few
    /// annotations don't trigger an allocation. Growth doubles on demand
    /// inside [`Self::update`].
    pub const INITIAL_CAPACITY: u32 = 16;

    pub fn new(device: &wgpu::Device) -> Self {
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("review_marker_instance_buffer"),
            size: u64::from(Self::INITIAL_CAPACITY)
                * std::mem::size_of::<ReviewMarkerInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            instance_buffer,
            instance_capacity: Self::INITIAL_CAPACITY,
            instance_count: 0,
        }
    }

    /// Replace the on-GPU instance set. Grows the buffer (with headroom)
    /// when `instances.len() > instance_capacity`.
    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[ReviewMarkerInstance],
    ) {
        let len = u32::try_from(instances.len()).unwrap_or(u32::MAX);

        if len > self.instance_capacity {
            let mut new_cap = self.instance_capacity.max(Self::INITIAL_CAPACITY);
            while new_cap < len {
                new_cap = new_cap.saturating_mul(2);
            }
            self.instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("review_marker_instance_buffer"),
                contents: bytemuck::cast_slice(instances),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            });
            self.instance_capacity = new_cap;
        } else if len == 0 {
        } else {
            queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(instances));
        }

        self.instance_count = len;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_struct_is_16_bytes() {
        assert_eq!(std::mem::size_of::<ReviewMarkerInstance>(), 16);
    }

    #[test]
    fn flags_packing_round_trip() {
        let plain = ReviewMarkerInstance::new([1.0, 2.0, 3.0], 2, false, false);
        assert_eq!(plain.flags & 0xF, 2);
        assert_eq!(plain.flags & FLAG_RESOLVED, 0);
        assert_eq!(plain.flags & FLAG_SELECTED, 0);

        let resolved = ReviewMarkerInstance::new([0.0; 3], 1, true, false);
        assert_eq!(resolved.flags & 0xF, 1);
        assert!(resolved.flags & FLAG_RESOLVED != 0);
        assert_eq!(resolved.flags & FLAG_SELECTED, 0);

        let selected = ReviewMarkerInstance::new([0.0; 3], 3, false, true);
        assert_eq!(selected.flags & 0xF, 3);
        assert_eq!(selected.flags & FLAG_RESOLVED, 0);
        assert!(selected.flags & FLAG_SELECTED != 0);

        let both = ReviewMarkerInstance::new([0.0; 3], 0, true, true);
        assert_eq!(both.flags & 0xF, 0);
        assert!(both.flags & FLAG_RESOLVED != 0);
        assert!(both.flags & FLAG_SELECTED != 0);
    }

    #[test]
    fn category_bits_only_low_nibble() {
        let inst = ReviewMarkerInstance::new([0.0; 3], 0xFF, false, false);
        assert_eq!(inst.flags & 0xF, 0xF);
        assert_eq!(inst.flags & FLAG_RESOLVED, 0);
        assert_eq!(inst.flags & FLAG_SELECTED, 0);
    }
}
