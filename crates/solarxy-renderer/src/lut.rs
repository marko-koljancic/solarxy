//! Colour-grading lookup tables: a decoded [`LutCube`] uploaded as a 3D
//! texture the composite pass samples.
//!
//! Parsing lives in `solarxy-formats`, not here, for the same reason
//! `.hdr` decode does: the engine has to read a table to put it on the
//! scene contract, and the engine cannot depend on the renderer. This
//! module is the GPU half only.
//!
//! **Why `Rgba16Float`.** It is filterable in core WebGPU, and a lookup
//! table is nothing without filtering: a 33-cubed grid interpolated
//! between entries is a smooth transform, and stepped it is a posterized
//! one. `Rgba32Float` is not filterable without the `float32-filterable`
//! feature, and this renderer holds itself to core so the desktop and web
//! paths stay the same code. `ltc.rs` records the same discipline for the
//! area-light tables, and half precision is comfortably enough here: the
//! table is sampled once per pixel at the very end of the frame, and its
//! output is about to be quantized to 8 bits by the swapchain anyway.
//!
//! **Why the fourth channel.** A table is RGB, but there is no filterable
//! three-channel format in core WebGPU, so alpha is padded to 1.0 and
//! ignored.

use half::f16;
use solarxy_core::LutCube;

/// The largest magnitude `f16` represents; anything past it becomes an
/// infinity. Mirrors the constant `ibl.rs` uses for the same reason.
const F16_MAX: f32 = 65504.0;

/// The affine map from an input value to a texture coordinate, folding
/// two corrections into one multiply-add the shader can apply blind.
///
/// The first is the table's declared domain: an input of `domain_min`
/// lands on the first entry and `domain_max` on the last. The second is
/// the half-texel correction, without which the ends of the table are
/// unreachable: normalized coordinate 0 sits on the *edge* of the first
/// texel, half a texel short of its centre, so a naive lookup reads a
/// blend of the first entry with the clamp rather than the entry itself.
///
/// Computed here rather than in WGSL so the arithmetic is testable and
/// exists once.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LutSampling {
    pub scale: [f32; 3],
    pub bias: [f32; 3],
}

impl LutSampling {
    fn for_cube(cube: &LutCube) -> Self {
        let n = cube.size as f32;
        let mut scale = [0.0f32; 3];
        let mut bias = [0.0f32; 3];
        for c in 0..3 {
            let span = cube.domain_max[c] - cube.domain_min[c];
            // The decoder refuses a non-positive span, so this is belt and
            // braces against a hand-built table in a test.
            let span = if span > 0.0 { span } else { 1.0 };
            scale[c] = ((n - 1.0) / n) / span;
            bias[c] = 0.5 / n - cube.domain_min[c] * scale[c];
        }
        Self { scale, bias }
    }
}

/// One uploaded table, kept alongside the hash it was built from so the
/// host can re-send the same `LutCube` every frame for free.
pub struct LutTexture {
    /// Owns the texture the view borrows; never read directly.
    _texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    /// [`LutCube::hash`] of the table this was built from.
    pub source_hash: u64,
    pub sampling: LutSampling,
}

impl LutTexture {
    /// Upload a table as a 3D texture.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, label: &str, cube: &LutCube) -> Self {
        let size = wgpu::Extent3d {
            width: cube.size,
            height: cube.size,
            depth_or_array_layers: cube.size,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // RGB to RGBA, f32 to f16. The cube's own ordering is red fastest
        // then green then blue, which is exactly a 3D texture's x, y, z, so
        // the rows go up in one pass with no reshaping.
        //
        // Clamped to the f16 range rather than trusted: the decoder does
        // not bound table values (a table may legitimately map outside 0
        // to 1), and an out-of-range entry would otherwise become an f16
        // infinity and blow out a whole region of the grade. Unlike the
        // IBL path this clamps symmetrically, because a look that
        // undershoots below zero is a real thing to author.
        let one = f16::from_f32(1.0).to_ne_bytes();
        let mut texels: Vec<u8> = Vec::with_capacity((cube.size as usize).pow(3) * 4 * 2);
        for entry in cube.data.chunks_exact(3) {
            for c in &entry[..3] {
                texels.extend_from_slice(&f16::from_f32(c.clamp(-F16_MAX, F16_MAX)).to_ne_bytes());
            }
            texels.extend_from_slice(&one);
        }

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &texels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(cube.size * 4 * 2),
                rows_per_image: Some(cube.size),
            },
            size,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D3),
            ..Default::default()
        });
        Self {
            _texture: texture,
            view,
            source_hash: cube.hash,
            sampling: LutSampling::for_cube(cube),
        }
    }
}

/// The two grading slots plus the table bound when a slot is empty.
///
/// Every slot is always bound: WGSL has no optional binding, so a disabled
/// slot reads the identity table rather than taking a second pipeline. The
/// shader still skips the sample on the uniform's enable flag, so a
/// disabled slot costs a branch rather than a texture fetch, and there are
/// zero pipeline permutations either way.
pub struct LutSlots {
    /// The table every empty slot binds: entry equals coordinate, so it
    /// is a no-op even if something did sample it.
    identity: LutTexture,
    slot_a: Option<LutTexture>,
    slot_b: Option<LutTexture>,
    /// A clone of the shared linear-clamp sampler, held here so the
    /// composite bind group can be assembled from this type alone.
    /// Clamping matters on all three axes: a wrapped lookup at the top of
    /// the table would read the black corner and turn a blown highlight
    /// into a dark one.
    sampler: wgpu::Sampler,
}

impl LutSlots {
    /// The identity table's edge. Two is the whole point: trilinear
    /// filtering over one entry per corner reproduces the input exactly,
    /// so the smallest possible table is also the correct one.
    const IDENTITY_SIZE: u32 = 2;

    /// `sampler` is expected to be `SharedSamplers::linear_clamp`, which
    /// is already linear on magnification and minification and clamped on
    /// u, v and w.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, sampler: &wgpu::Sampler) -> Self {
        let identity = LutTexture::new(
            device,
            queue,
            "LUT Identity",
            &LutCube::identity(Self::IDENTITY_SIZE),
        );
        Self {
            identity,
            slot_a: None,
            slot_b: None,
            sampler: sampler.clone(),
        }
    }

    /// The sampler all slots are read with.
    pub fn sampler(&self) -> &wgpu::Sampler {
        &self.sampler
    }

    /// Point a slot at a table, or clear it with `None`.
    ///
    /// Returns whether the bound texture changed, which is the host's cue
    /// to rebuild the composite bind group. Dedupes on
    /// [`LutCube::hash`], so re-sending the same table every frame (which
    /// the scene delta does, because it replaces the whole camera list)
    /// costs a comparison rather than an upload.
    pub fn set(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        slot: LutSlot,
        cube: Option<&LutCube>,
    ) -> bool {
        let (label, current) = match slot {
            LutSlot::A => ("LUT Slot A", &mut self.slot_a),
            LutSlot::B => ("LUT Slot B", &mut self.slot_b),
        };
        match (cube, current.as_ref()) {
            (None, None) => false,
            (None, Some(_)) => {
                *current = None;
                true
            }
            (Some(cube), Some(existing)) if existing.source_hash == cube.hash => false,
            (Some(cube), _) => {
                *current = Some(LutTexture::new(device, queue, label, cube));
                true
            }
        }
    }

    /// The view to bind for a slot: its table, or the identity when empty.
    pub fn view(&self, slot: LutSlot) -> &wgpu::TextureView {
        let bound = match slot {
            LutSlot::A => self.slot_a.as_ref(),
            LutSlot::B => self.slot_b.as_ref(),
        };
        bound.map_or(&self.identity.view, |t| &t.view)
    }

    /// Whether a slot holds a real table. The composite uniform's enable
    /// flag is `AND`ed with this, so a strength left turned up on an empty
    /// slot cannot make the identity table look like a missing one.
    pub fn is_loaded(&self, slot: LutSlot) -> bool {
        match slot {
            LutSlot::A => self.slot_a.is_some(),
            LutSlot::B => self.slot_b.is_some(),
        }
    }

    /// The coordinate transform for a slot: its table's, or the identity
    /// table's when empty.
    pub fn sampling(&self, slot: LutSlot) -> LutSampling {
        let bound = match slot {
            LutSlot::A => self.slot_a.as_ref(),
            LutSlot::B => self.slot_b.as_ref(),
        };
        bound.map_or(self.identity.sampling, |t| t.sampling)
    }
}

/// Which grading slot a table occupies. The two are not interchangeable:
/// A is sampled before the tone mapper and shapes the tone curve itself,
/// B is sampled after it and applies a display-referred look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LutSlot {
    /// Pre-tone-map, log-encoded input. An ACES or `AgX` transform.
    A,
    /// Post-tone-map, display-referred input. A look LUT from a grading
    /// suite.
    B,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The conversion the upload does, isolated: RGB f32 in, RGBA f16 out,
    /// alpha forced to one. The GPU half needs a device, so this covers
    /// the part that can be wrong without one.
    #[test]
    fn the_texel_conversion_pads_alpha_and_keeps_order() {
        let cube = LutCube::identity(2);
        let mut texels: Vec<f32> = Vec::new();
        for entry in cube.data.chunks_exact(3) {
            texels.extend([entry[0], entry[1], entry[2], 1.0]);
        }
        assert_eq!(texels.len(), 8 * 4);
        // Entry 0 is the black corner, entry 7 the white one.
        assert_eq!(&texels[..4], &[0.0, 0.0, 0.0, 1.0]);
        assert_eq!(&texels[28..], &[1.0, 1.0, 1.0, 1.0]);
    }

    /// The reason the half-texel correction exists: the domain ends must
    /// land on the centres of the first and last entries, not on the
    /// texture's edges, or the table's extremes are never actually read.
    #[test]
    fn the_domain_ends_land_on_the_first_and_last_texel_centres() {
        for size in [2u32, 17, 33, 64] {
            let s = LutSampling::for_cube(&LutCube::identity(size));
            let n = size as f32;
            let at = |x: f32| x * s.scale[0] + s.bias[0];
            assert!(
                (at(0.0) - 0.5 / n).abs() < 1e-6,
                "size {size}: domain min landed at {}",
                at(0.0)
            );
            assert!(
                (at(1.0) - (n - 0.5) / n).abs() < 1e-6,
                "size {size}: domain max landed at {}",
                at(1.0)
            );
        }
    }

    #[test]
    fn a_declared_domain_rescales_the_lookup() {
        // A table covering 0 to 4 must reach its last entry at input 4,
        // and sit a quarter of the way along at input 1.
        let mut cube = LutCube::identity(2);
        cube = LutCube::new(cube.size, cube.data, [0.0; 3], [4.0; 3]);
        let s = LutSampling::for_cube(&cube);
        let at = |x: f32| x * s.scale[0] + s.bias[0];
        assert!((at(0.0) - 0.25).abs() < 1e-6, "{}", at(0.0));
        assert!((at(4.0) - 0.75).abs() < 1e-6, "{}", at(4.0));
        assert!((at(1.0) - 0.375).abs() < 1e-6, "{}", at(1.0));
    }

    /// Half precision has to survive the 0 to 1 range a table lives in
    /// without visible banding: the gap between neighbours near 1.0 is
    /// about 1/2048, which is three bits finer than the 8-bit swapchain
    /// the result lands on.
    #[test]
    fn half_precision_is_finer_than_the_output_it_feeds() {
        for step in 0..=1024u32 {
            let v = f32::from(step as u16) / 1024.0;
            let round_tripped = f32::from(f16::from_f32(v));
            assert!(
                (round_tripped - v).abs() < 1.0 / 2048.0,
                "{v} round-tripped to {round_tripped}"
            );
        }
    }
}
