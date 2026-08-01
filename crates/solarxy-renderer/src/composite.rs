//! Final composite pass: tone-mapping HDR onto the swapchain, plus the
//! per-pane viewport/scissor rectangle that splits the surface in F2/F3.

use crate::bind_groups::BindGroupLayouts;
use crate::bloom::BLOOM_STRENGTH;
use crate::lut::{LutSlot, LutSlots};
use crate::pipelines::Pipelines;
use crate::ssao::SsaoState;
use solarxy_core::preferences::{InspectionMode, ToneMode};
use solarxy_core::{LUT_LOG_MAX_STOP, LUT_LOG_MIN_STOP};
use wgpu::util::DeviceExt;

const SSAO_STRENGTH: f32 = 0.8;

/// The shot's rendering intent, resolved for one pane.
///
/// Everything here is what a colourist would call the look, as opposed to
/// what the scene is: it changes the picture without changing the
/// geometry, the materials or the lights. From 0.8.2 the look is owned by
/// the camera when a pane looks through one, and by the pane otherwise, so
/// this struct is what the two resolve *into* rather than where either
/// stores it.
///
/// [`Default`] is the neutral look, and neutral means bit-identical
/// output: exposure 1, no grade, no table. That is load-bearing, because
/// it is what lets the grading feature ship without moving a single golden
/// capture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompositeLook {
    pub tone_mode: ToneMode,
    pub exposure: f32,
    /// Added after the tone map: raises or lowers the floor. Neutral 0.
    pub lift: [f32; 3],
    /// Applied as a power after lift and gain. Neutral 1.
    pub gamma: [f32; 3],
    /// Multiplied before lift: scales the ceiling. Neutral 1.
    pub gain: [f32; 3],
    /// How much of the pre-tone-map table to blend in, 0 to 1.
    pub lut_a_strength: f32,
    /// How much of the display-referred table to blend in, 0 to 1.
    pub lut_b_strength: f32,
}

impl Default for CompositeLook {
    fn default() -> Self {
        Self {
            tone_mode: ToneMode::default(),
            exposure: 1.0,
            lift: [0.0; 3],
            gamma: [1.0; 3],
            gain: [1.0; 3],
            lut_a_strength: 1.0,
            lut_b_strength: 1.0,
        }
    }
}

impl CompositeLook {
    /// The look a host with no per-pane model yet resolves: the global
    /// tone mapper and exposure it already had, and a neutral grade.
    #[must_use]
    pub fn from_tone(tone_mode: ToneMode, exposure: f32) -> Self {
        Self {
            tone_mode,
            exposure,
            ..Self::default()
        }
    }

    /// Whether the grade would change the image.
    ///
    /// This gates the grade in the shader, and it has to: `pow(x, 1.0)`
    /// compiles to `exp2(1.0 * log2(x))` and is **not** bit-identical to
    /// `x`. An always-on grade at neutral values would therefore move
    /// every golden capture by a unit of last place, for a feature that is
    /// meant to be inert until someone reaches for it. Skipping it is what
    /// keeps neutral meaning neutral.
    #[must_use]
    // Exact comparison is the point rather than an oversight. The question
    // is not "is this grade close to neutral" but "is it the untouched
    // default", because that is what decides whether the shader runs a
    // transform at all. An epsilon here would silently discard a grade
    // somebody dialled in just below it.
    #[allow(clippy::float_cmp)]
    pub fn grade_is_neutral(&self) -> bool {
        self.lift == [0.0; 3] && self.gamma == [1.0; 3] && self.gain == [1.0; 3]
    }
}

pub struct CompositeState {
    params_buffer: wgpu::Buffer,
    params_bind_group: wgpu::BindGroup,
    bind_group: wgpu::BindGroup,
}

impl CompositeState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: &wgpu::Device,
        layouts: &BindGroupLayouts,
        hdr_resolve_view: &wgpu::TextureView,
        bloom_ping_view: &wgpu::TextureView,
        bloom_sampler: &wgpu::Sampler,
        luts: &LutSlots,
        bloom_enabled: bool,
        ssao_enabled: bool,
        tone_mode: ToneMode,
        exposure: f32,
    ) -> Self {
        let params_data = build_params(
            bloom_enabled,
            ssao_enabled,
            &CompositeLook::from_tone(tone_mode, exposure),
            luts,
            InspectionMode::Shaded,
        );
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Composite Params Uniform"),
            contents: bytemuck::bytes_of(&params_data),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let params_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Composite Params Bind Group"),
            layout: &layouts.composite_params,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: params_buffer.as_entire_binding(),
            }],
        });
        let bind_group = create_bind_group(
            device,
            &layouts.composite,
            hdr_resolve_view,
            bloom_ping_view,
            bloom_sampler,
            luts,
        );

        Self {
            params_buffer,
            params_bind_group,
            bind_group,
        }
    }

    /// Rebuild the group. Called on surface resize, when the HDR target's
    /// views are recreated, **and** whenever a LUT slot's bound texture
    /// changes, because a bind group captures the views it was built from.
    pub fn rebuild_bind_group(
        &mut self,
        device: &wgpu::Device,
        layouts: &BindGroupLayouts,
        hdr_resolve_view: &wgpu::TextureView,
        bloom_ping_view: &wgpu::TextureView,
        bloom_sampler: &wgpu::Sampler,
        luts: &LutSlots,
    ) {
        self.bind_group = create_bind_group(
            device,
            &layouts.composite,
            hdr_resolve_view,
            bloom_ping_view,
            bloom_sampler,
            luts,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipelines: &Pipelines,
        view: &wgpu::TextureView,
        ssao_enabled: bool,
        ssao: &SsaoState,
        viewport: Option<[f32; 4]>,
        clear: bool,
    ) {
        let load = if clear {
            wgpu::LoadOp::Clear(wgpu::Color::BLACK)
        } else {
            wgpu::LoadOp::Load
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Composite Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        if let Some([x, y, w, h]) = viewport {
            pass.set_viewport(x, y, w, h, 0.0, 1.0);
            pass.set_scissor_rect(x as u32, y as u32, w as u32, h as u32);
        }
        pass.set_pipeline(&pipelines.post.composite);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_bind_group(1, &self.params_bind_group, &[]);
        if ssao_enabled {
            pass.set_bind_group(2, &ssao.read_bind_group, &[]);
        } else {
            pass.set_bind_group(2, &ssao.read_off_bind_group, &[]);
        }
        pass.draw(0..3, 0..1);
    }

    /// Write the pane's composite uniform.
    ///
    /// Takes the resolved [`CompositeLook`] rather than a tone mode and an
    /// exposure, because the look is now nine values rather than two and a
    /// ninth positional argument is how the wrong pane's grade gets
    /// written.
    pub fn write_params(
        &self,
        queue: &wgpu::Queue,
        bloom_enabled: bool,
        ssao_enabled: bool,
        look: &CompositeLook,
        luts: &LutSlots,
        inspection_mode: InspectionMode,
    ) {
        let params = build_params(bloom_enabled, ssao_enabled, look, luts, inspection_mode);
        queue.write_buffer(&self.params_buffer, 0, bytemuck::bytes_of(&params));
    }
}

/// The composite pass's uniform. **Grown by appending only**, per the
/// renderer's uniform rule, and `pub` so `tests/uniform_layout.rs` can
/// compare its size against the naga span of the WGSL struct that declares
/// it whole.
///
/// The three grade vectors sit at offsets 128, 144 and 160 with a scalar
/// of padding behind each, because WGSL aligns a `vec3<f32>` to 16 bytes
/// in the uniform address space while Rust aligns `[f32; 3]` to 4. Get
/// that wrong and the Rust size assert still passes, the shader still
/// compiles, and the viewport goes wrong at draw time. Same reasoning
/// `MaterialUniform` records for its own appended blocks.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CompositeParams {
    bloom_strength: f32,
    bloom_enabled: u32,
    ssao_enabled: u32,
    ssao_strength: f32,

    tone_mode: u32,
    exposure: f32,
    inspection_mode: u32,
    _pad: u32,

    lut_a_enabled: u32,
    lut_a_strength: f32,
    lut_b_enabled: u32,
    lut_b_strength: f32,

    /// The pre-tone-map slot's log window, in stops. Carried rather than
    /// baked into the shader so Rust owns the number the parameter help
    /// documents.
    log_lo: f32,
    log_hi: f32,
    /// Zero whenever the grade is neutral, which skips it in the shader.
    /// See [`CompositeLook::grade_is_neutral`] for why that is not merely
    /// an optimization.
    grade_enabled: u32,
    _pad_grade: f32,

    lut_a_scale: [f32; 3],
    _pad_a_scale: f32,
    lut_a_bias: [f32; 3],
    _pad_a_bias: f32,
    lut_b_scale: [f32; 3],
    _pad_b_scale: f32,
    lut_b_bias: [f32; 3],
    _pad_b_bias: f32,

    lift: [f32; 3],
    _pad_lift: f32,
    gamma: [f32; 3],
    _pad_gamma: f32,
    gain: [f32; 3],
    _pad_gain: f32,
}

const _: () = assert!(std::mem::size_of::<CompositeParams>() == 176);

fn build_params(
    bloom_enabled: bool,
    ssao_enabled: bool,
    look: &CompositeLook,
    luts: &LutSlots,
    inspection_mode: InspectionMode,
) -> CompositeParams {
    // A slot is on only when it holds a real table: a strength left turned
    // up on an empty slot must not blend towards the identity texture that
    // stands in for it, because that would read as the table quietly
    // failing to load rather than as no table at all.
    let a_on = luts.is_loaded(LutSlot::A) && look.lut_a_strength > 0.0;
    let b_on = luts.is_loaded(LutSlot::B) && look.lut_b_strength > 0.0;
    let a = luts.sampling(LutSlot::A);
    let b = luts.sampling(LutSlot::B);
    CompositeParams {
        bloom_strength: BLOOM_STRENGTH,
        bloom_enabled: u32::from(bloom_enabled),
        ssao_enabled: u32::from(ssao_enabled),
        ssao_strength: SSAO_STRENGTH,

        tone_mode: look.tone_mode.as_u32(),
        exposure: look.exposure,
        inspection_mode: inspection_mode.as_u32(),
        _pad: 0,

        lut_a_enabled: u32::from(a_on),
        lut_a_strength: look.lut_a_strength.clamp(0.0, 1.0),
        lut_b_enabled: u32::from(b_on),
        lut_b_strength: look.lut_b_strength.clamp(0.0, 1.0),

        log_lo: LUT_LOG_MIN_STOP,
        log_hi: LUT_LOG_MAX_STOP,
        grade_enabled: u32::from(!look.grade_is_neutral()),
        _pad_grade: 0.0,

        lut_a_scale: a.scale,
        _pad_a_scale: 0.0,
        lut_a_bias: a.bias,
        _pad_a_bias: 0.0,
        lut_b_scale: b.scale,
        _pad_b_scale: 0.0,
        lut_b_bias: b.bias,
        _pad_b_bias: 0.0,

        lift: look.lift,
        _pad_lift: 0.0,
        gamma: look.gamma,
        _pad_gamma: 0.0,
        gain: look.gain,
        _pad_gain: 0.0,
    }
}

fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    scene_view: &wgpu::TextureView,
    bloom_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    luts: &LutSlots,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Composite Bind Group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(scene_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(bloom_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(luts.view(LutSlot::A)),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(luts.view(LutSlot::B)),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::Sampler(luts.sampler()),
            },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Neutral has to mean bit-identical, not merely close: it is what
    /// lets grading ship without moving a golden capture. Asserted on the
    /// values rather than on a render, because the render is what the
    /// golden gate checks.
    #[test]
    fn the_default_look_is_neutral() {
        let look = CompositeLook::default();
        assert!(look.grade_is_neutral());
        assert_eq!(look.exposure, 1.0);
        assert_eq!(look.tone_mode, ToneMode::default());
        assert!(CompositeLook::from_tone(ToneMode::Reinhard, 2.0).grade_is_neutral());
    }
}
