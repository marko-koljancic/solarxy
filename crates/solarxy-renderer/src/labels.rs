//! The GPU attribute-label channel: per-point value/number labels drawn as
//! SDF glyph quads straight from storage buffers, replacing the web shell's
//! pooled DOM overlay (which capped out at a couple thousand elements and
//! re-projected every pin on the CPU every frame). The host uploads label
//! anchors and packed glyph words once per cook or viz change; the vertex
//! shader (`shaders/label.wgsl`) expands chip, dot, and glyph quads
//! camera-facing per pane per frame, so projection costs nothing on the CPU
//! and pane scissor clips for free.
//!
//! The atlas is baked by `examples/gen_glyph_atlas.rs` from the bundled
//! Lilex face; the consts here are its contract (charset order = atlas
//! cell index). Regenerate the blob whenever they change.

use wgpu::util::DeviceExt;

/// Charset in atlas-cell order; must stay byte-identical to the generator.
pub const CHARSET: &str = "0123456789.,-: e+NaInfity";
pub const ATLAS_W: u32 = 240;
pub const ATLAS_H: u32 = 320;
pub const CELL_W: u32 = 48;
pub const CELL_H: u32 = 64;
pub const GRID_COLS: u32 = 5;
/// Bake-time font size; shader metrics scale from it.
pub const EM_PX: f32 = 40.0;
/// Lilex advance width per em (printed by the generator, pinned here).
pub const ADVANCE_RATIO: f32 = 0.461_538;

/// Max glyphs per label: the packed word's column field is 6 bits.
pub const TEXT_MAX: u32 = 63;
/// Max labels: the packed word's label-index field is 21 bits.
pub const LABELS_MAX: u32 = 1 << 21;

const ATLAS_BYTES: &[u8] = include_bytes!("shaders/label_atlas.r8");

/// One label anchor. Matches WGSL `Label { pos: vec3<f32>, glyph_count: u32 }`
/// (the `PointDatum` 16-byte trick: vec3 aligns to 16, the u32 rides the
/// pad slot).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LabelInstance {
    pub pos: [f32; 3],
    pub glyph_count: u32,
}
const _: () = assert!(std::mem::size_of::<LabelInstance>() == 16);

/// Packs one glyph occurrence: bits 0..5 the atlas glyph id, 5..11 the
/// column within its label, 11..32 the owning label index.
#[must_use]
pub fn pack_glyph(label: u32, col: u32, glyph: u32) -> u32 {
    debug_assert!(glyph < 32 && col <= TEXT_MAX && label < LABELS_MAX);
    (label << 11) | (col << 5) | glyph
}

/// The atlas cell for a character, or `None` when it is outside the baked
/// charset (the encoder skips such characters rather than guessing).
#[must_use]
pub fn glyph_index(c: char) -> Option<u32> {
    // 25 entries: a linear scan is cheaper than any table for this size.
    #[allow(clippy::cast_possible_truncation)]
    CHARSET.chars().position(|g| g == c).map(|i| i as u32)
}

/// The text size a label draws at, in CSS px before the device pixel ratio.
///
/// Three rungs rather than a free number: the SDF bake is one size and the
/// shader scales it, so any value works, but a slider here would be a knob
/// nobody can set meaningfully. `Medium` is the 9px the retired DOM overlay
/// used, so the default is unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LabelSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl LabelSize {
    /// Text height in CSS px.
    #[must_use]
    pub const fn text_px(self) -> f32 {
        match self {
            Self::Small => 7.0,
            Self::Medium => 9.0,
            Self::Large => 13.0,
        }
    }

    /// The stable wire name, for the host boundary and session state.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }

    /// Parses a wire name, falling back to the default on anything else so
    /// a stale or hand-edited payload cannot fail to load.
    #[must_use]
    pub fn from_key(key: &str) -> Self {
        match key {
            "small" => Self::Small,
            "large" => Self::Large,
            _ => Self::Medium,
        }
    }
}

/// What a label draws behind its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LabelBackground {
    /// The rounded chip plus the anchor dot: legible over anything.
    #[default]
    Chip,
    /// The anchor dot only, text bare on the scene. Cheaper by six vertices
    /// per label, and much quieter over a flat background; it gives up the
    /// contrast the chip was there to guarantee.
    None,
}

impl LabelBackground {
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Chip => "chip",
            Self::None => "none",
        }
    }

    #[must_use]
    pub fn from_key(key: &str) -> Self {
        match key {
            "none" => Self::None,
            _ => Self::Chip,
        }
    }

    #[must_use]
    const fn draws_chip(self) -> bool {
        matches!(self, Self::Chip)
    }
}

/// Host-facing label style: linear-RGB theme colors, the device pixel ratio
/// that scales every CSS-px metric below, and the user's size / background /
/// opacity choices.
#[derive(Debug, Clone, Copy)]
pub struct LabelStyle {
    pub text: [f32; 3],
    pub chip: [f32; 3],
    pub dot: [f32; 3],
    pub dpr: f32,
    pub size: LabelSize,
    pub background: LabelBackground,
    /// Overall label opacity, 0 to 1. Scales the text and dot alpha, and
    /// multiplies the chip's own 82 percent.
    pub opacity: f32,
}

impl LabelStyle {
    /// The neutral style: medium text, chip background, fully opaque. Colors
    /// are placeholders until the host pushes the theme.
    #[must_use]
    pub fn new_default() -> Self {
        Self {
            text: [0.9, 0.9, 0.9],
            chip: [0.1, 0.1, 0.1],
            dot: [1.0, 0.62, 0.15],
            dpr: 1.0,
            size: LabelSize::default(),
            background: LabelBackground::default(),
            opacity: 1.0,
        }
    }
}

/// The chip's own alpha at full label opacity. The chip exists to guarantee
/// contrast against an arbitrary scene; below this it stops doing that.
const CHIP_ALPHA: f32 = 0.82;

/// Every CSS-px metric other than the text height is expressed as a
/// multiple of it, so one size choice scales the whole label coherently
/// instead of leaving a large glyph in a chip sized for a small one. The
/// ratios are the retired DOM overlay's numbers divided by its 9px text.
const DOT_RATIO: f32 = 6.0 / 9.0;
const TEXT_GAP_RATIO: f32 = 5.5 / 9.0;
const CHIP_PAD_X_RATIO: f32 = 4.0 / 9.0;
const CHIP_PAD_Y_RATIO: f32 = 1.5 / 9.0;
const CHIP_RADIUS_RATIO: f32 = 3.0 / 9.0;

/// The GPU uniform. Field order and padding must match `LabelParams` in
/// `shaders/label.wgsl` exactly: a mismatch is a silent garbage read, not a
/// compile error.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LabelParams {
    text_color: [f32; 4],
    chip_color: [f32; 4],
    dot_color: [f32; 4],
    text_px: f32,
    advance_px: f32,
    dot_px: f32,
    text_gap_px: f32,
    chip_pad_x: f32,
    chip_pad_y: f32,
    chip_radius: f32,
    label_count: u32,
    /// 1 when the chip is drawn. Shifts the dot and glyph vertex ranges as
    /// well as gating the chip, so a chipless label costs six fewer
    /// vertices rather than six rasterized-then-discarded ones.
    chip_on: u32,
    /// Pads to the 16-byte multiple a uniform struct requires (84 would
    /// round up to 96 anyway; making it explicit keeps Rust and WGSL
    /// agreeing on where each field starts).
    _pad: [u32; 3],
}
const _: () = assert!(std::mem::size_of::<LabelParams>() == 96);

impl LabelParams {
    fn from_style(style: &LabelStyle, label_count: u32) -> Self {
        let d = if style.dpr.is_finite() && style.dpr > 0.0 {
            style.dpr
        } else {
            1.0
        };
        let a = if style.opacity.is_finite() {
            style.opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let text_px = style.size.text_px() * d;
        Self {
            text_color: [style.text[0], style.text[1], style.text[2], a],
            chip_color: [style.chip[0], style.chip[1], style.chip[2], CHIP_ALPHA * a],
            dot_color: [style.dot[0], style.dot[1], style.dot[2], a],
            text_px,
            advance_px: text_px * ADVANCE_RATIO,
            dot_px: text_px * DOT_RATIO,
            text_gap_px: text_px * TEXT_GAP_RATIO,
            chip_pad_x: text_px * CHIP_PAD_X_RATIO,
            chip_pad_y: text_px * CHIP_PAD_Y_RATIO,
            chip_radius: text_px * CHIP_RADIUS_RATIO,
            label_count,
            chip_on: u32::from(style.background.draws_chip()),
            _pad: [0; 3],
        }
    }
}

/// GPU residency for the channel: the immutable atlas plus the growable
/// label/glyph storage buffers (the `scene_objects` 1.5x `COPY_DST`
/// pattern; the bind group is recreated whenever a buffer regrows).
pub struct LabelResources {
    pub bind_group: wgpu::BindGroup,
    params_buf: wgpu::Buffer,
    instance_buf: wgpu::Buffer,
    instance_capacity: u64,
    glyph_buf: wgpu::Buffer,
    glyph_capacity: u64,
    atlas_view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    style: LabelStyle,
    pub label_count: u32,
    pub glyph_count: u32,
}

impl LabelResources {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout) -> Self {
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Label Glyph Atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_W,
                height: ATLAS_H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &atlas,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            ATLAS_BYTES,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(ATLAS_W),
                rows_per_image: Some(ATLAS_H),
            },
            wgpu::Extent3d {
                width: ATLAS_W,
                height: ATLAS_H,
                depth_or_array_layers: 1,
            },
        );
        let atlas_view = atlas.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Label Atlas Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let style = LabelStyle::new_default();
        let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Label Params"),
            contents: bytemuck::bytes_of(&LabelParams::from_style(&style, 0)),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let instance_capacity = 256 * std::mem::size_of::<LabelInstance>() as u64;
        let instance_buf = create_storage(device, "Label Instances", instance_capacity);
        let glyph_capacity = 4096 * 4;
        let glyph_buf = create_storage(device, "Label Glyphs", glyph_capacity);

        let bind_group = create_bind_group(
            device,
            layout,
            &params_buf,
            &instance_buf,
            &glyph_buf,
            &atlas_view,
            &sampler,
        );
        Self {
            bind_group,
            params_buf,
            instance_buf,
            instance_capacity,
            glyph_buf,
            glyph_capacity,
            atlas_view,
            sampler,
            style,
            label_count: 0,
            glyph_count: 0,
        }
    }

    /// Replaces the label set. Event-driven (cook / lane change / toggle),
    /// never per frame; in-place writes while capacity holds, 1.5x regrow
    /// plus bind-group rebuild on overflow.
    #[allow(clippy::cast_possible_truncation)]
    pub fn set_labels(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        instances: &[LabelInstance],
        glyph_words: &[u32],
    ) {
        let inst_bytes: &[u8] = bytemuck::cast_slice(instances);
        let glyph_bytes: &[u8] = bytemuck::cast_slice(glyph_words);
        let mut regrown = false;
        if inst_bytes.len() as u64 > self.instance_capacity {
            self.instance_capacity = grow(inst_bytes.len() as u64);
            self.instance_buf = create_storage(device, "Label Instances", self.instance_capacity);
            regrown = true;
        }
        if glyph_bytes.len() as u64 > self.glyph_capacity {
            self.glyph_capacity = grow(glyph_bytes.len() as u64);
            self.glyph_buf = create_storage(device, "Label Glyphs", self.glyph_capacity);
            regrown = true;
        }
        if regrown {
            self.bind_group = create_bind_group(
                device,
                layout,
                &self.params_buf,
                &self.instance_buf,
                &self.glyph_buf,
                &self.atlas_view,
                &self.sampler,
            );
        }
        if !inst_bytes.is_empty() {
            queue.write_buffer(&self.instance_buf, 0, inst_bytes);
        }
        if !glyph_bytes.is_empty() {
            queue.write_buffer(&self.glyph_buf, 0, glyph_bytes);
        }
        self.label_count = instances.len() as u32;
        self.glyph_count = glyph_words.len() as u32;
        self.write_params(queue);
    }

    /// Updates the theme colors / device pixel ratio without touching the
    /// label set.
    pub fn write_style(&mut self, queue: &wgpu::Queue, style: &LabelStyle) {
        self.style = *style;
        self.write_params(queue);
    }

    /// Updates only the device pixel ratio (browser zoom, monitor move),
    /// keeping the last pushed theme colors.
    pub fn write_dpr(&mut self, queue: &wgpu::Queue, dpr: f32) {
        self.style.dpr = dpr;
        self.write_params(queue);
    }

    fn write_params(&self, queue: &wgpu::Queue) {
        let params = LabelParams::from_style(&self.style, self.label_count);
        queue.write_buffer(&self.params_buf, 0, bytemuck::bytes_of(&params));
    }

    /// The single draw's vertex count: 6 for every label's chip (only when
    /// the chip is drawn), 6 for its dot, 6 per glyph.
    ///
    /// The shader decodes element kind from `vertex_index` against exactly
    /// this arithmetic, so the two must agree; `chip_on` is the one term
    /// that can differ between them.
    #[must_use]
    pub fn vertex_count(&self) -> u32 {
        let per_label = if self.style.background.draws_chip() {
            12
        } else {
            6
        };
        self.label_count * per_label + self.glyph_count * 6
    }
}

fn grow(needed: u64) -> u64 {
    (needed + needed / 2).next_multiple_of(4)
}

fn create_storage(device: &wgpu::Device, label: &str, capacity: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: capacity.max(4),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    params: &wgpu::Buffer,
    instances: &wgpu::Buffer,
    glyphs: &wgpu::Buffer,
    atlas_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Label Bind Group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: params.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: instances.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: glyphs.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(atlas_view),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_pack_round_trips_at_field_boundaries() {
        for (label, col, glyph) in [(0, 0, 0), (7, 12, 24), (LABELS_MAX - 1, TEXT_MAX, 31)] {
            let w = pack_glyph(label, col, glyph);
            assert_eq!(w & 31, glyph);
            assert_eq!((w >> 5) & 63, col);
            assert_eq!(w >> 11, label);
        }
    }

    #[test]
    fn glyph_index_covers_the_charset_and_rejects_others() {
        for (i, c) in CHARSET.chars().enumerate() {
            assert_eq!(glyph_index(c), Some(u32::try_from(i).unwrap()), "{c:?}");
        }
        assert!(glyph_index('x').is_none());
        assert!(glyph_index('%').is_none());
        assert!(CHARSET.chars().count() <= 32, "the glyph field is 5 bits");
    }

    #[test]
    fn atlas_blob_matches_the_declared_grid() {
        assert_eq!(ATLAS_BYTES.len(), (ATLAS_W * ATLAS_H) as usize);
        // Every non-space glyph cell carries ink (an inside texel near 255);
        // the space cell carries none.
        let cell_max = |idx: u32| {
            let cx = (idx % GRID_COLS) * CELL_W;
            let cy = (idx / GRID_COLS) * CELL_H;
            let mut max = 0u8;
            for y in cy..cy + CELL_H {
                for x in cx..cx + CELL_W {
                    max = max.max(ATLAS_BYTES[(y * ATLAS_W + x) as usize]);
                }
            }
            max
        };
        for (i, c) in CHARSET.chars().enumerate() {
            let max = cell_max(u32::try_from(i).unwrap());
            if c == ' ' {
                assert!(max < 128, "space cell must be empty, max {max}");
            } else {
                // 128 is the outline; anything clearly above it is interior
                // ink. Thin monospace strokes top out well below the full
                // spread, so the bar is "has an inside", not "is bold".
                assert!(max > 135, "glyph {c:?} cell looks empty, max {max}");
            }
        }
    }

    #[test]
    fn advance_ratio_is_monospace_plausible() {
        const { assert!(ADVANCE_RATIO > 0.3 && ADVANCE_RATIO < 0.8) }
    }

    #[test]
    fn params_scale_with_dpr_and_survive_junk() {
        let style = LabelStyle {
            text: [1.0, 1.0, 1.0],
            chip: [0.0, 0.0, 0.0],
            dot: [1.0, 0.5, 0.0],
            dpr: 2.0,
            ..LabelStyle::new_default()
        };
        let p = LabelParams::from_style(&style, 7);
        // Medium is the retired DOM overlay's 9px, doubled by the dpr.
        assert!((p.text_px - 18.0).abs() < f32::EPSILON);
        assert!((p.dot_px - 12.0).abs() < f32::EPSILON);
        assert!((p.advance_px - 18.0 * ADVANCE_RATIO).abs() < 1e-5);
        assert_eq!(p.label_count, 7);

        let bad = LabelStyle {
            dpr: f32::NAN,
            ..style
        };
        assert!((LabelParams::from_style(&bad, 0).text_px - 9.0).abs() < f32::EPSILON);
    }

    #[test]
    fn every_metric_scales_with_the_size_choice() {
        // The point of the ratios: one size choice moves the whole label
        // coherently. A large glyph in a chip padded for a small one is
        // the failure this prevents.
        let base = LabelStyle::new_default();
        let small = LabelParams::from_style(
            &LabelStyle {
                size: LabelSize::Small,
                ..base
            },
            1,
        );
        let large = LabelParams::from_style(
            &LabelStyle {
                size: LabelSize::Large,
                ..base
            },
            1,
        );
        for (s, l) in [
            (small.text_px, large.text_px),
            (small.advance_px, large.advance_px),
            (small.dot_px, large.dot_px),
            (small.text_gap_px, large.text_gap_px),
            (small.chip_pad_x, large.chip_pad_x),
            (small.chip_pad_y, large.chip_pad_y),
            (small.chip_radius, large.chip_radius),
        ] {
            assert!(l > s, "every metric must grow with the size: {s} -> {l}");
        }
        // And in proportion, so the label keeps its shape.
        let k = large.text_px / small.text_px;
        assert!((large.chip_radius / small.chip_radius - k).abs() < 1e-5);
        assert!((large.dot_px / small.dot_px - k).abs() < 1e-5);
    }

    #[test]
    fn opacity_scales_both_the_text_and_the_chip() {
        let half = LabelParams::from_style(
            &LabelStyle {
                opacity: 0.5,
                ..LabelStyle::new_default()
            },
            1,
        );
        assert!((half.text_color[3] - 0.5).abs() < f32::EPSILON);
        assert!((half.dot_color[3] - 0.5).abs() < f32::EPSILON);
        // The chip keeps its own 82 percent underneath, so it stays the
        // more transparent of the two at every setting.
        assert!((half.chip_color[3] - CHIP_ALPHA * 0.5).abs() < f32::EPSILON);
        assert!(half.chip_color[3] < half.text_color[3]);

        // Junk opacity falls back to opaque rather than rendering nothing.
        let bad = LabelParams::from_style(
            &LabelStyle {
                opacity: f32::NAN,
                ..LabelStyle::new_default()
            },
            1,
        );
        assert!((bad.text_color[3] - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_chip_flag_and_the_vertex_count_agree() {
        // The shader decodes element kind from `vertex_index` against
        // exactly `vertex_count`'s arithmetic. If these two ever disagree,
        // labels silently render as garbage rather than failing to build.
        for (background, per_label) in [
            (LabelBackground::Chip, 12u32),
            (LabelBackground::None, 6u32),
        ] {
            let params = LabelParams::from_style(
                &LabelStyle {
                    background,
                    ..LabelStyle::new_default()
                },
                5,
            );
            assert_eq!(
                params.chip_on,
                u32::from(background == LabelBackground::Chip)
            );
            // Reproduce `vertex_count` from the uniform the shader reads.
            let from_uniform = 5 * (6 + 6 * params.chip_on) + 3 * 6;
            assert_eq!(from_uniform, 5 * per_label + 18);
        }
    }

    #[test]
    fn size_and_background_keys_round_trip_and_fall_back() {
        for s in [LabelSize::Small, LabelSize::Medium, LabelSize::Large] {
            assert_eq!(LabelSize::from_key(s.key()), s);
        }
        for b in [LabelBackground::Chip, LabelBackground::None] {
            assert_eq!(LabelBackground::from_key(b.key()), b);
        }
        // A key from a newer build must render something, not panic.
        assert_eq!(LabelSize::from_key("enormous"), LabelSize::Medium);
        assert_eq!(LabelBackground::from_key("frosted"), LabelBackground::Chip);
    }
}
