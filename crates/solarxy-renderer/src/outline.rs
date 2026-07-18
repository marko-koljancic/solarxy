//! Selection-outline resources: the offscreen mask, the jump-flood
//! ping-pong pair, and the uniforms for the step and blit passes.
//!
//! The pass chain (encoded by `Renderer::render_selection_outline` and
//! `Renderer::composite_selection_outline` in `frame.rs`):
//!
//! 1. Mask: selected objects render their silhouettes into `mask_view`
//!    (`R8Unorm`), depth-ignoring, via the validation shader's
//!    transform-only vertex stage with a white color uniform.
//! 2. Init: `fs_jfa_init` seeds `ping` (`Rg32Float` nearest-seed pixel
//!    coordinates; `(-1,-1)` = none).
//! 3. Five `fs_jfa_step` passes ping-pong with steps 16, 8, 4, 2, 1
//!    (supporting rim widths up to 16 px in constant passes); the FIXED
//!    ladder always runs, so the final field always lands in `pong`.
//! 4. Blit: `fs_outline` draws the rim onto the composited swapchain
//!    view per pane (after tone mapping; never blooms, never darkened by
//!    AO).
//!
//! Style, color, and width are user preferences plumbed by the host
//! through [`crate::frame::Renderer::set_selection_highlight`].

use wgpu::util::DeviceExt;

use crate::bind_groups::BindGroupLayouts;

/// The jump-flood step ladder, largest first. Fixed, so the pass count
/// and the final texture parity never depend on the preferred width.
pub const JFA_STEPS: [i32; 5] = [16, 8, 4, 2, 1];

/// CPU mirror of outline.wgsl's `OutlineParams` (one struct serves the
/// step passes, which read only `step`, and the blit, which reads color
/// and width).
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct OutlineParams {
    pub color: [f32; 4],
    pub width: f32,
    pub step: i32,
    pub _pad0: f32,
    pub _pad1: f32,
}

const _: () = assert!(std::mem::size_of::<OutlineParams>() == 32);

pub struct OutlineState {
    pub mask_view: wgpu::TextureView,
    ping_view: wgpu::TextureView,
    pong_view: wgpu::TextureView,
    /// White fill for the mask pass (bound through the shared
    /// `validation_color` layout the mask pipeline reuses).
    pub white_bind_group: wgpu::BindGroup,
    _white_buffer: wgpu::Buffer,
    /// Blit params (color + width; rewritten by the preference setter).
    pub params_bind_group: wgpu::BindGroup,
    params_buffer: wgpu::Buffer,
    /// One pre-filled uniform per ladder step (a mid-encoder
    /// `write_buffer` would not interleave with the passes).
    pub step_bind_groups: Vec<wgpu::BindGroup>,
    _step_buffers: Vec<wgpu::Buffer>,
    /// Source bind groups over the mask (init) and each ping-pong half.
    pub init_bind_group: wgpu::BindGroup,
    pub ping_bind_group: wgpu::BindGroup,
    pub pong_bind_group: wgpu::BindGroup,
}

impl OutlineState {
    pub fn new(device: &wgpu::Device, layouts: &BindGroupLayouts, width: u32, height: u32) -> Self {
        let white_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Outline Mask White"),
            contents: bytemuck::cast_slice(&[1.0f32, 1.0, 1.0, 1.0]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let white_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Outline Mask White BG"),
            layout: &layouts.validation_color,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: white_buffer.as_entire_binding(),
            }],
        });

        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Outline Params"),
            // Linear-space #ff9e21 (the app's amber selection accent);
            // hosts overwrite it from the user preference on boot.
            contents: bytemuck::bytes_of(&OutlineParams {
                color: [1.0, 0.342, 0.015, 1.0],
                width: 3.0,
                step: 0,
                _pad0: 0.0,
                _pad1: 0.0,
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let params_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Outline Params BG"),
            layout: &layouts.outline_params,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: params_buffer.as_entire_binding(),
            }],
        });

        let mut step_buffers = Vec::with_capacity(JFA_STEPS.len());
        let mut step_bind_groups = Vec::with_capacity(JFA_STEPS.len());
        for step in JFA_STEPS {
            let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("Outline JFA Step {step}")),
                contents: bytemuck::bytes_of(&OutlineParams {
                    color: [0.0; 4],
                    width: 0.0,
                    step,
                    _pad0: 0.0,
                    _pad1: 0.0,
                }),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            step_bind_groups.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(&format!("Outline JFA Step {step} BG")),
                layout: &layouts.outline_params,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buf.as_entire_binding(),
                }],
            }));
            step_buffers.push(buf);
        }

        let (mask_view, ping_view, pong_view, init_bind_group, ping_bind_group, pong_bind_group) =
            Self::create_sized(device, layouts, width, height);

        Self {
            mask_view,
            ping_view,
            pong_view,
            white_bind_group,
            _white_buffer: white_buffer,
            params_bind_group,
            params_buffer,
            step_bind_groups,
            _step_buffers: step_buffers,
            init_bind_group,
            ping_bind_group,
            pong_bind_group,
        }
    }

    /// Recreates the sized targets (the resize cascade both hosts drive).
    pub fn resize(
        &mut self,
        device: &wgpu::Device,
        layouts: &BindGroupLayouts,
        width: u32,
        height: u32,
    ) {
        let (mask_view, ping_view, pong_view, init_bind_group, ping_bind_group, pong_bind_group) =
            Self::create_sized(device, layouts, width, height);
        self.mask_view = mask_view;
        self.ping_view = ping_view;
        self.pong_view = pong_view;
        self.init_bind_group = init_bind_group;
        self.ping_bind_group = ping_bind_group;
        self.pong_bind_group = pong_bind_group;
    }

    /// Rewrites the blit params (the preference setter).
    pub fn write_params(&self, queue: &wgpu::Queue, color: [f32; 4], width: f32) {
        queue.write_buffer(
            &self.params_buffer,
            0,
            bytemuck::bytes_of(&OutlineParams {
                color,
                width: width.clamp(1.0, 16.0),
                step: 0,
                _pad0: 0.0,
                _pad1: 0.0,
            }),
        );
    }

    /// The view holding the FINAL jump-flood field (the fixed five-step
    /// ladder always ends in pong) and its source bind group.
    #[must_use]
    pub fn final_bind_group(&self) -> &wgpu::BindGroup {
        &self.pong_bind_group
    }

    /// The two ping-pong halves in pass order: pass i writes
    /// `dst_view(i)` reading `src_bind_group(i)`.
    #[must_use]
    pub fn step_io(&self, i: usize) -> (&wgpu::BindGroup, &wgpu::TextureView) {
        if i.is_multiple_of(2) {
            (&self.ping_bind_group, &self.pong_view)
        } else {
            (&self.pong_bind_group, &self.ping_view)
        }
    }

    #[must_use]
    pub fn ping(&self) -> &wgpu::TextureView {
        &self.ping_view
    }

    fn create_sized(
        device: &wgpu::Device,
        layouts: &BindGroupLayouts,
        width: u32,
        height: u32,
    ) -> (
        wgpu::TextureView,
        wgpu::TextureView,
        wgpu::TextureView,
        wgpu::BindGroup,
        wgpu::BindGroup,
        wgpu::BindGroup,
    ) {
        let make = |label: &str, format: wgpu::TextureFormat| {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: width.max(1),
                    height: height.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            tex.create_view(&wgpu::TextureViewDescriptor::default())
        };
        let mask_view = make("Outline Mask", wgpu::TextureFormat::R8Unorm);
        let ping_view = make("Outline JFA Ping", wgpu::TextureFormat::Rg32Float);
        let pong_view = make("Outline JFA Pong", wgpu::TextureFormat::Rg32Float);
        let src_bg = |label: &str, view: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &layouts.outline_texture,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                }],
            })
        };
        let init_bind_group = src_bg("Outline Init Src BG", &mask_view);
        let ping_bind_group = src_bg("Outline Ping Src BG", &ping_view);
        let pong_bind_group = src_bg("Outline Pong Src BG", &pong_view);
        (
            mask_view,
            ping_view,
            pong_view,
            init_bind_group,
            ping_bind_group,
            pong_bind_group,
        )
    }
}
