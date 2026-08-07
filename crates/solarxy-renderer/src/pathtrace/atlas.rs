//! The texture atlas: every scene texture in one sampled resource.
//!
//! A raster draw binds its material's textures before it. A tracer cannot: one
//! dispatch shades every surface in the scene, and core WebGPU grants no
//! binding arrays (`max_binding_array_elements_per_shader_stage` is zero), so
//! there is no way to name a per-material texture from inside the kernel. Every
//! texture therefore has to be reachable from one binding, which means one
//! image, which means packing.
//!
//! **There is no `wgpu` in this file, for the same reason there is none in
//! [`super::scene`].** Packing is arithmetic over rectangles and the pixels are
//! never touched here; keeping it that way lets it be tested without a device
//! and lets it move off the main thread later without a rewrite. The GPU half
//! is [`super::TraceAtlas`].
//!
//! # Why not a plain array texture
//!
//! An array texture forces one resolution on every layer, so a scene mixing a
//! 4096 albedo with a 64 mask pays 4096 for both. That is what makes the naive
//! implementation of this cost hundreds of megabytes. Pages plus sub-rectangles
//! cost what the textures actually cost.
//!
//! # What the page size costs, and how it is chosen
//!
//! Memory is `layers * page * page`, and the two factors pull against each
//! other: a small page wastes less per layer and needs more layers, a large one
//! the reverse. Neither direction wins in general, so the packer does not
//! guess. It packs at every candidate page size and keeps the arrangement that
//! allocates the fewest bytes, which is a handful of runs over a scene's worth
//! of rectangles and is exact rather than heuristic.
//!
//! # Guard borders follow the wrap mode
//!
//! Every texture is inset by one texel of border, because a bilinear tap at a
//! sub-rectangle's edge reaches a texel outside it and would otherwise read the
//! neighbour. What that border must contain depends on how the texture tiles: a
//! repeating texture's left border is its own rightmost column, so the seam
//! interpolates the way the hardware's `Repeat` mode would, while a clamped or
//! mirrored one's border is its own edge column. Filling every border by
//! clamping would put a visible seam on every tiled texture the raster path
//! renders seamlessly, and every material texture Solarxy uploads today is
//! uploaded as `Repeat` ([`crate::texture::TextureOpts::material`]). The wrap
//! mode is therefore part of a texture's packing identity: one image used both
//! ways is packed twice.
//!
//! # No mip chain
//!
//! The raster path builds mips; this does not. A mip level of an atlas page
//! blends across sub-rectangle boundaries, so the guard border would have to
//! grow with every level, and the kernel is bound to `textureSampleLevel` by
//! the uniformity discipline in any case. The consequence is real and worth
//! stating rather than discovering: a traced texture aliases at grazing angles
//! where the raster one does not, until ray differentials pick a level.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use solarxy_core::RawImageData;

/// One texel of border on every side of every packed texture.
pub const GUARD: u32 = 1;

/// Bytes per texel in the atlas, which is `Rgba8Unorm` throughout.
pub const BYTES_PER_TEXEL: u32 = 4;

/// The smallest page the packer will allocate.
///
/// Below this the per-page bookkeeping outweighs the memory saved, and a scene
/// with textures small enough to care is not a scene with a memory problem.
pub const PAGE_MIN: u32 = 256;

/// The largest page the packer will allocate.
///
/// Well inside `max_texture_dimension_2d`, which core WebGPU puts at 8192. The
/// cap is a memory decision rather than a limit: one 4096 page is 64 MB, and a
/// browser tab that allocates several of those is a browser tab that stops.
pub const PAGE_MAX: u32 = 4096;

/// Layers the atlas may allocate, which is core WebGPU's
/// `max_texture_array_layers` exactly.
///
/// It is also why [`TextureDescriptor::layer`] is eight bits: the field cannot
/// be too small for a legal atlas, and cannot address one that is illegal.
pub const MAX_LAYERS: u32 = 256;

/// Bit 31 of a packed descriptor: this material slot carries no texture.
///
/// The sign bit rather than a sentinel index, so no legal index has to be
/// reserved and the kernel's test is one comparison. Note that a zeroed word is
/// therefore a *legal* descriptor naming layer zero, so a slot must always be
/// written explicitly; [`TextureDescriptor::none`] is what writes an empty one.
pub const TEXTURE_UNUSED: u32 = 1 << 31;

/// How a texture's coordinates resolve outside the unit square.
///
/// Two bits in the descriptor, so there is room for a fourth mode without
/// moving anything. `Clamp` and `Mirror` share a border fill because a mirror
/// reflected at the boundary repeats the edge texel, which is what a clamp
/// writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AtlasWrap {
    /// Tile. The default, and what every Solarxy material texture uses today.
    #[default]
    Repeat,
    /// Extend the edge texel outward.
    Clamp,
    /// Tile, reflecting every other repetition.
    Mirror,
}

impl AtlasWrap {
    fn bits(self) -> u32 {
        match self {
            Self::Repeat => 0,
            Self::Clamp => 1,
            Self::Mirror => 2,
        }
    }

    fn from_bits(bits: u32) -> Option<Self> {
        match bits {
            0 => Some(Self::Repeat),
            1 => Some(Self::Clamp),
            2 => Some(Self::Mirror),
            _ => None,
        }
    }

    /// Whether the border on this axis comes from the opposite edge.
    fn borders_wrap(self) -> bool {
        matches!(self, Self::Repeat)
    }
}

/// Which reconstruction filter the kernel samples a slot with.
///
/// One bit, and the kernel branches on it rather than indexing a sampler, which
/// WGSL does not allow. Both samplers are bound; the branch picks one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AtlasFilter {
    Nearest,
    #[default]
    Linear,
}

/// What makes two texture requests the same packed rectangle.
///
/// The content hash alone would be wrong: the guard border's contents depend on
/// the wrap mode, so one image tiled in one material and clamped in another is
/// two rectangles. Nothing else belongs here. The filter and the colour space
/// are resolved in the shader from the descriptor and do not change a texel,
/// which is why one image serving as both a colour map and a data map packs
/// once, where the raster path's cache (keyed on `(hash, linear)`) packs twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureKey {
    /// [`solarxy_core::RawImageData::hash`], an FNV-1a over the dimensions and
    /// the pixel bytes.
    pub hash: u64,
    pub wrap_s: AtlasWrap,
    pub wrap_t: AtlasWrap,
}

/// One texture offered to the packer.
///
/// Dimensions only. The pixels are read once, at upload, by the GPU half, which
/// is what keeps this file free of both `wgpu` and image data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlasSource {
    pub key: TextureKey,
    pub width: u32,
    pub height: u32,
}

/// Where one texture landed.
///
/// `x` and `y` name the image's own top-left texel, inside the guard ring, so
/// the ring occupies `x - GUARD .. x + width + GUARD` and the same in `y`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlasEntry {
    pub key: TextureKey,
    pub layer: u32,
    pub x: u32,
    pub y: u32,
    /// Size after any halving, which is the size actually packed.
    pub width: u32,
    pub height: u32,
    /// How many times the source was halved to fit a page. Zero for everything
    /// a scene normally contains.
    pub halvings: u32,
}

/// A texture offered to the atlas together with the pixels the upload reads.
///
/// The packer works from [`AtlasTexture::source`] alone; the image is here so
/// that one list serves both halves and the two cannot fall out of step, which
/// they would the moment a scene produced them in two passes.
#[derive(Debug, Clone)]
pub struct AtlasTexture {
    pub key: TextureKey,
    pub image: Arc<RawImageData>,
}

impl AtlasTexture {
    #[must_use]
    pub fn source(&self) -> AtlasSource {
        AtlasSource {
            key: self.key,
            width: self.image.width,
            height: self.image.height,
        }
    }
}

/// A finished arrangement: the page size, the layers, and where everything sat.
///
/// Produced by [`AtlasPlan::pack`] and consumed by [`super::TraceAtlas`], which
/// is the only thing that needs the pixels.
#[derive(Debug, Clone, Default)]
pub struct AtlasPlan {
    page: u32,
    layers: u32,
    entries: Vec<AtlasEntry>,
    index: HashMap<TextureKey, u32>,
    dropped: u32,
    halved: u32,
}

impl AtlasPlan {
    /// Packs `sources`, deduplicating by [`TextureKey`].
    ///
    /// Deterministic: the arrangement is a function of the set of sources, not
    /// of the order they arrive in, so two cooks producing the same textures
    /// produce the same atlas and the same descriptors.
    ///
    /// A source too large for [`PAGE_MAX`] is halved until it fits and counted
    /// in [`AtlasPlan::halved`]; sources past the [`MAX_LAYERS`] budget are
    /// dropped and counted in [`AtlasPlan::dropped`]. Neither panics, because
    /// both are properties of a scene someone opened rather than of this code.
    #[must_use]
    pub fn pack(sources: &[AtlasSource]) -> Self {
        let mut unique: Vec<AtlasSource> = Vec::new();
        let mut seen: HashSet<TextureKey> = HashSet::new();
        for source in sources {
            if source.width == 0 || source.height == 0 {
                continue;
            }
            if seen.insert(source.key) {
                unique.push(*source);
            }
        }
        if unique.is_empty() {
            return Self::default();
        }

        // Halve anything that cannot fit the largest page, before choosing a
        // page: the choice is over what will actually be packed.
        let mut halvings: HashMap<TextureKey, u32> = HashMap::new();
        let mut halved = 0u32;
        for source in &mut unique {
            let mut count = 0u32;
            while padded(source.width) > PAGE_MAX || padded(source.height) > PAGE_MAX {
                source.width = (source.width / 2).max(1);
                source.height = (source.height / 2).max(1);
                count += 1;
            }
            if count > 0 {
                halvings.insert(source.key, count);
                halved += 1;
            }
        }

        // Tallest first, then widest, then by key so the order is total. A
        // skyline packer is markedly better fed tall rectangles first, and a
        // total order is what makes the result reproducible.
        unique.sort_unstable_by(|a, b| {
            b.height
                .cmp(&a.height)
                .then(b.width.cmp(&a.width))
                .then(a.key.hash.cmp(&b.key.hash))
                .then(a.key.wrap_s.bits().cmp(&b.key.wrap_s.bits()))
                .then(a.key.wrap_t.bits().cmp(&b.key.wrap_t.bits()))
        });

        // Pack at every candidate page and keep the best arrangement. Fewest
        // dropped textures first, because no amount of memory saved is worth
        // losing one; then fewest bytes; then fewest layers, which is what
        // decides the common tie where halving the page doubles the layer
        // count for exactly the same allocation.
        let largest = unique
            .iter()
            .map(|s| padded(s.width).max(padded(s.height)))
            .max()
            .unwrap_or(PAGE_MIN);
        let mut best: Option<Self> = None;
        let mut page = PAGE_MIN.max(largest.next_power_of_two()).min(PAGE_MAX);
        while page <= PAGE_MAX {
            let candidate = Self::pack_at(&unique, page, halved, &halvings);
            let wins = best.as_ref().is_none_or(|b| {
                (candidate.dropped, candidate.bytes(), candidate.layers())
                    < (b.dropped, b.bytes(), b.layers())
            });
            if wins {
                best = Some(candidate);
            }
            page *= 2;
        }
        best.unwrap_or_default()
    }

    /// [`AtlasPlan::pack`] over the list that also carries the pixels.
    #[must_use]
    pub fn pack_textures(textures: &[AtlasTexture]) -> Self {
        let sources: Vec<AtlasSource> = textures.iter().map(AtlasTexture::source).collect();
        Self::pack(&sources)
    }

    /// One arrangement at one page size. `sources` must already be sorted and
    /// deduplicated.
    fn pack_at(
        sources: &[AtlasSource],
        page: u32,
        halved: u32,
        halvings: &HashMap<TextureKey, u32>,
    ) -> Self {
        let mut pages: Vec<Skyline> = vec![Skyline::new(page)];
        let mut entries = Vec::with_capacity(sources.len());
        let mut index = HashMap::with_capacity(sources.len());
        let mut dropped = 0u32;

        for source in sources {
            let (w, h) = (padded(source.width), padded(source.height));
            let mut placed = None;
            for (layer, skyline) in pages.iter_mut().enumerate() {
                if let Some((x, y)) = skyline.insert(w, h) {
                    placed = Some((layer as u32, x, y));
                    break;
                }
            }
            if placed.is_none() {
                if pages.len() as u32 >= MAX_LAYERS {
                    dropped += 1;
                    continue;
                }
                let mut skyline = Skyline::new(page);
                let Some((x, y)) = skyline.insert(w, h) else {
                    // Unreachable while the page-size search only offers pages
                    // at least as large as the largest padded source, but a
                    // dropped texture is a better answer than a panic.
                    dropped += 1;
                    continue;
                };
                placed = Some((pages.len() as u32, x, y));
                pages.push(skyline);
            }
            let Some((layer, x, y)) = placed else {
                continue;
            };
            index.insert(source.key, entries.len() as u32);
            entries.push(AtlasEntry {
                key: source.key,
                layer,
                x: x + GUARD,
                y: y + GUARD,
                width: source.width,
                height: source.height,
                halvings: halvings.get(&source.key).copied().unwrap_or(0),
            });
        }

        Self {
            page,
            layers: pages.len() as u32,
            entries,
            index,
            dropped,
            halved,
        }
    }

    /// The page edge in texels, or one for an empty plan.
    #[must_use]
    pub fn page(&self) -> u32 {
        self.page.max(1)
    }

    /// Array layers, or one for an empty plan.
    #[must_use]
    pub fn layers(&self) -> u32 {
        self.layers.max(1)
    }

    /// What the atlas texture will occupy, which is what the page-size search
    /// minimizes.
    #[must_use]
    pub fn bytes(&self) -> u64 {
        u64::from(self.page()) * u64::from(self.page()) * u64::from(self.layers()) * 4
    }

    /// Every placement, in pack order.
    #[must_use]
    pub fn entries(&self) -> &[AtlasEntry] {
        &self.entries
    }

    /// Textures dropped because the layer budget was exhausted.
    #[must_use]
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    /// Textures halved to fit a page.
    #[must_use]
    pub fn halved(&self) -> u32 {
        self.halved
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Where a texture landed, if it landed.
    #[must_use]
    pub fn entry(&self, key: TextureKey) -> Option<&AtlasEntry> {
        self.index
            .get(&key)
            .and_then(|i| self.entries.get(*i as usize))
    }

    /// The sub-rectangle a shader multiplies a wrapped coordinate by, in
    /// page-normalized units: `(u_scale, v_scale, u_offset, v_offset)`.
    ///
    /// `atlas_uv = fract_or_clamp(uv) * scale + offset`, which lands strictly
    /// inside the guard ring, so a bilinear tap at either extreme reaches the
    /// border rather than the neighbour.
    #[must_use]
    pub fn rect(&self, key: TextureKey) -> Option<[f32; 4]> {
        let entry = self.entry(key)?;
        let page = self.page() as f32;
        Some([
            entry.width as f32 / page,
            entry.height as f32 / page,
            entry.x as f32 / page,
            entry.y as f32 / page,
        ])
    }

    /// The packed descriptor for a slot, or [`TEXTURE_UNUSED`] when the texture
    /// is not in this atlas.
    #[must_use]
    pub fn descriptor(
        &self,
        key: TextureKey,
        uv_channel: u32,
        filter: AtlasFilter,
        srgb: bool,
    ) -> u32 {
        let Some(entry) = self.entry(key) else {
            return TEXTURE_UNUSED;
        };
        TextureDescriptor {
            layer: entry.layer,
            uv_channel,
            wrap_s: key.wrap_s,
            wrap_t: key.wrap_t,
            filter,
            srgb,
        }
        .pack()
    }
}

/// One texture slot of one material, as the kernel reads it.
///
/// The material record that carries one of these per texture role does not
/// exist yet; it arrives with the traced material model. What is here is the
/// encoding, which the packer owns because it owns the layer number and the
/// rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureDescriptor {
    /// Array layer, `0..MAX_LAYERS`.
    pub layer: u32,
    /// Which vertex uv set to read, `0..8`.
    pub uv_channel: u32,
    pub wrap_s: AtlasWrap,
    pub wrap_t: AtlasWrap,
    pub filter: AtlasFilter,
    /// Whether the kernel decodes sRGB after sampling.
    ///
    /// A flag rather than a texture format, which is what lets one page hold
    /// both a base-colour map and a normal map: the atlas is `Rgba8Unorm`
    /// throughout and the transfer function is applied per texture.
    pub srgb: bool,
}

impl TextureDescriptor {
    /// The descriptor of a slot with no texture.
    #[must_use]
    pub fn none() -> u32 {
        TEXTURE_UNUSED
    }

    /// Packs to the word the kernel reads.
    ///
    /// Fields wider than their field are masked rather than rejected: a caller
    /// passing a layer past the budget has already been counted as dropped, and
    /// wrapping the number is a wrong texture where panicking is no image at
    /// all.
    #[must_use]
    pub fn pack(self) -> u32 {
        (self.layer & 0xFF)
            | ((self.uv_channel & 0x7) << 8)
            | (self.wrap_s.bits() << 11)
            | (self.wrap_t.bits() << 13)
            | (u32::from(self.filter == AtlasFilter::Linear) << 15)
            | (u32::from(self.srgb) << 16)
    }

    /// Unpacks, or `None` when the slot is empty.
    ///
    /// Also `None` for a reserved wrap code, because a descriptor that names a
    /// mode nothing implements is corruption rather than a texture.
    #[must_use]
    pub fn unpack(bits: u32) -> Option<Self> {
        if bits & TEXTURE_UNUSED != 0 {
            return None;
        }
        Some(Self {
            layer: bits & 0xFF,
            uv_channel: (bits >> 8) & 0x7,
            wrap_s: AtlasWrap::from_bits((bits >> 11) & 0x3)?,
            wrap_t: AtlasWrap::from_bits((bits >> 13) & 0x3)?,
            filter: if bits & (1 << 15) == 0 {
                AtlasFilter::Nearest
            } else {
                AtlasFilter::Linear
            },
            srgb: bits & (1 << 16) != 0,
        })
    }
}

/// A source's footprint including its guard ring.
fn padded(edge: u32) -> u32 {
    edge.saturating_add(2 * GUARD)
}

/// The block the uploader writes for one entry: the image at its packed size,
/// surrounded by the guard ring its wrap modes call for.
///
/// Returned as one `(width + 2) * (height + 2)` RGBA8 buffer so the upload is a
/// single `write_texture` per texture rather than one for the image and eight
/// for the ring. The halvings the packer recorded are applied here, because
/// this is the only place that has the pixels.
#[must_use]
pub fn compose(entry: &AtlasEntry, image: &RawImageData) -> Vec<u8> {
    let (pixels, width, height) = halve_to(image, entry.halvings);
    let (w, h) = (width as usize, height as usize);
    let (pw, ph) = (w + 2 * GUARD as usize, h + 2 * GUARD as usize);
    let mut out = vec![0u8; pw * ph * BYTES_PER_TEXEL as usize];
    if w == 0 || h == 0 {
        return out;
    }

    // A border row or column comes from the opposite edge when the axis tiles,
    // and from the same edge when it clamps or mirrors. That is what makes a
    // bilinear tap at the sub-rectangle's boundary blend the way the hardware
    // address mode would, instead of reading whatever was packed next door.
    let axis = |i: usize, len: usize, wraps: bool| -> usize {
        if i == 0 {
            if wraps { len - 1 } else { 0 }
        } else if i == len + 1 {
            if wraps { 0 } else { len - 1 }
        } else {
            i - 1
        }
    };
    let wrap_s = entry.key.wrap_s.borders_wrap();
    let wrap_t = entry.key.wrap_t.borders_wrap();

    for oy in 0..ph {
        let sy = axis(oy, h, wrap_t);
        for ox in 0..pw {
            let sx = axis(ox, w, wrap_s);
            let src = (sy * w + sx) * BYTES_PER_TEXEL as usize;
            let dst = (oy * pw + ox) * BYTES_PER_TEXEL as usize;
            if let Some(texel) = pixels.get(src..src + BYTES_PER_TEXEL as usize) {
                out[dst..dst + BYTES_PER_TEXEL as usize].copy_from_slice(texel);
            }
        }
    }
    out
}

/// Box-halves `count` times, returning tightly-packed RGBA8 and its size.
///
/// Zero halvings is the overwhelmingly common case and copies once rather than
/// resampling, so an ordinary scene pays nothing for the existence of the
/// oversize path.
fn halve_to(image: &RawImageData, count: u32) -> (Vec<u8>, u32, u32) {
    let stride = BYTES_PER_TEXEL as usize;
    let (mut w, mut h) = (image.width, image.height);
    let mut pixels = image.pixels.clone();
    // A short buffer is a malformed image rather than a reason to index out of
    // bounds; pad it to the size its own header claims.
    pixels.resize(w as usize * h as usize * stride, 0);

    for _ in 0..count {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = vec![0u8; nw as usize * nh as usize * stride];
        for y in 0..nh as usize {
            for x in 0..nw as usize {
                for c in 0..stride {
                    // Average the 2x2 block, clamping on an odd edge so the
                    // last row or column contributes rather than vanishing.
                    let mut sum = 0u32;
                    for dy in 0..2usize {
                        for dx in 0..2usize {
                            let sx = (x * 2 + dx).min(w as usize - 1);
                            let sy = (y * 2 + dy).min(h as usize - 1);
                            sum += u32::from(pixels[(sy * w as usize + sx) * stride + c]);
                        }
                    }
                    next[(y * nw as usize + x) * stride + c] = (sum / 4) as u8;
                }
            }
        }
        pixels = next;
        w = nw;
        h = nh;
    }
    (pixels, w, h)
}

/// Skyline bottom-left packing over one page.
///
/// The skyline is the upper contour of what is already placed, as a list of
/// horizontal segments left to right. A rectangle goes where its top edge would
/// sit lowest, ties to the left, which is the classic arrangement and the one
/// that behaves best on the mix of sizes a material set actually contains.
struct Skyline {
    page: u32,
    /// `(x, y, width)` segments, contiguous and covering `0..page`.
    nodes: Vec<(u32, u32, u32)>,
}

impl Skyline {
    fn new(page: u32) -> Self {
        Self {
            page,
            nodes: vec![(0, 0, page)],
        }
    }

    /// Places a `w` by `h` rectangle, returning its top-left corner.
    fn insert(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w > self.page || h > self.page {
            return None;
        }
        let mut best: Option<(u32, u32, usize)> = None;
        for i in 0..self.nodes.len() {
            let Some(y) = self.fit(i, w) else { continue };
            if y + h > self.page {
                continue;
            }
            let x = self.nodes[i].0;
            // Lowest top edge wins; among equals the leftmost, which is what
            // keeps the contour from growing a staircase of one-texel steps.
            if best.is_none_or(|(bx, by, _)| y < by || (y == by && x < bx)) {
                best = Some((x, y, i));
            }
        }
        let (x, y, _) = best?;
        self.add(x, y + h, w);
        Some((x, y))
    }

    /// The height a rectangle of width `w` would rest at, starting at node `i`,
    /// or `None` if it runs off the page.
    fn fit(&self, i: usize, w: u32) -> Option<u32> {
        let x = self.nodes[i].0;
        if x + w > self.page {
            return None;
        }
        let mut remaining = w;
        let mut y = 0;
        for node in &self.nodes[i..] {
            y = y.max(node.1);
            if remaining <= node.2 {
                return Some(y);
            }
            remaining -= node.2;
        }
        None
    }

    /// Raises the contour over `x .. x + w` to `y`, merging equal neighbours.
    fn add(&mut self, x: u32, y: u32, w: u32) {
        let mut next: Vec<(u32, u32, u32)> = Vec::with_capacity(self.nodes.len() + 2);
        for &(nx, ny, nw) in &self.nodes {
            let (start, end) = (nx, nx + nw);
            if end <= x || start >= x + w {
                next.push((nx, ny, nw));
                continue;
            }
            // The part of this segment left of the new rectangle survives, and
            // so does the part right of it. The overlap is replaced.
            if start < x {
                next.push((start, ny, x - start));
            }
            if end > x + w {
                next.push((x + w, ny, end - (x + w)));
            }
        }
        next.push((x, y, w));
        next.sort_unstable_by_key(|n| n.0);

        // Merge equal-height neighbours so the contour cannot accumulate an
        // unbounded number of segments across a long pack.
        self.nodes.clear();
        for node in next {
            match self.nodes.last_mut() {
                Some(last) if last.1 == node.1 && last.0 + last.2 == node.0 => last.2 += node.2,
                _ => self.nodes.push(node),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AtlasEntry, AtlasFilter, AtlasPlan, AtlasSource, AtlasWrap, MAX_LAYERS, PAGE_MAX,
        RawImageData, TEXTURE_UNUSED, TextureDescriptor, TextureKey, padded,
    };

    fn key(hash: u64) -> TextureKey {
        TextureKey {
            hash,
            wrap_s: AtlasWrap::Repeat,
            wrap_t: AtlasWrap::Repeat,
        }
    }

    fn source(hash: u64, width: u32, height: u32) -> AtlasSource {
        AtlasSource {
            key: key(hash),
            width,
            height,
        }
    }

    /// A deterministic pseudo-random sequence, so the randomized cases below
    /// are reproducible from a seed rather than merely varied.
    fn lcg(state: &mut u64) -> u32 {
        *state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        (*state >> 33) as u32
    }

    /// Every packed rectangle, guard ring included, as `(layer, x0, y0, x1, y1)`.
    fn footprints(plan: &AtlasPlan) -> Vec<(u32, u32, u32, u32, u32)> {
        plan.entries()
            .iter()
            .map(|e| {
                (
                    e.layer,
                    e.x - super::GUARD,
                    e.y - super::GUARD,
                    e.x + e.width + super::GUARD,
                    e.y + e.height + super::GUARD,
                )
            })
            .collect()
    }

    #[test]
    fn nothing_overlaps_and_nothing_leaves_its_page() {
        let mut state = 0x5eed_u64;
        for round in 0..32u32 {
            let count = 1 + (lcg(&mut state) % 40);
            let sources: Vec<AtlasSource> = (0..count)
                .map(|i| {
                    let w = 1 + lcg(&mut state) % 512;
                    let h = 1 + lcg(&mut state) % 512;
                    source(u64::from(round) * 1000 + u64::from(i), w, h)
                })
                .collect();
            let plan = AtlasPlan::pack(&sources);
            assert_eq!(plan.entries().len(), sources.len(), "round {round}");

            let boxes = footprints(&plan);
            for b in &boxes {
                assert!(b.3 <= plan.page() && b.4 <= plan.page(), "round {round}");
                assert!(b.0 < plan.layers(), "round {round}");
            }
            for (i, a) in boxes.iter().enumerate() {
                for b in &boxes[i + 1..] {
                    let disjoint =
                        a.0 != b.0 || a.3 <= b.1 || b.3 <= a.1 || a.4 <= b.2 || b.4 <= a.2;
                    assert!(disjoint, "round {round}: {a:?} overlaps {b:?}");
                }
            }
        }
    }

    #[test]
    fn the_arrangement_does_not_depend_on_the_order_the_sources_arrive_in() {
        // Two cooks producing the same textures must produce the same atlas,
        // or a descriptor computed against one is wrong against the other.
        let forward: Vec<AtlasSource> = (0..24)
            .map(|i| source(i, 17 + (i as u32 % 7) * 31, 13 + (i as u32 % 5) * 29))
            .collect();
        let mut backward = forward.clone();
        backward.reverse();

        let a = AtlasPlan::pack(&forward);
        let b = AtlasPlan::pack(&backward);
        assert_eq!(a.page(), b.page());
        assert_eq!(a.layers(), b.layers());
        assert_eq!(a.entries(), b.entries());
    }

    #[test]
    fn one_image_tiled_and_clamped_is_two_rectangles() {
        // The guard ring differs between the two, so they cannot share one.
        let tiled = AtlasSource {
            key: TextureKey {
                hash: 7,
                wrap_s: AtlasWrap::Repeat,
                wrap_t: AtlasWrap::Repeat,
            },
            width: 64,
            height: 64,
        };
        let clamped = AtlasSource {
            key: TextureKey {
                hash: 7,
                wrap_s: AtlasWrap::Clamp,
                wrap_t: AtlasWrap::Clamp,
            },
            ..tiled
        };
        let plan = AtlasPlan::pack(&[tiled, clamped, tiled]);
        // Three requests, two identities: the duplicate collapses, the wrap
        // difference does not.
        assert_eq!(plan.entries().len(), 2);
    }

    #[test]
    fn the_page_chosen_is_the_one_that_allocates_least() {
        // Four 256-square textures pad to 258, which does not tile into a 512
        // page two to a side. The search has to notice that 1024 in one layer
        // beats 512 in four.
        let sources: Vec<AtlasSource> = (0..4).map(|i| source(i, 256, 256)).collect();
        let plan = AtlasPlan::pack(&sources);
        assert_eq!(plan.layers(), 1);
        assert_eq!(plan.page(), 1024);
        // Exactly the allocation four 512 pages would have cost, in one layer
        // instead of four, which is what the layer tiebreak is for.
        assert_eq!(plan.bytes(), 1024 * 1024 * 4);
    }

    #[test]
    fn a_scene_of_small_textures_does_not_allocate_a_large_page() {
        let sources: Vec<AtlasSource> = (0..8).map(|i| source(i, 32, 32)).collect();
        let plan = AtlasPlan::pack(&sources);
        assert_eq!(plan.page(), super::PAGE_MIN);
        assert_eq!(plan.layers(), 1);
    }

    #[test]
    fn a_texture_too_large_for_a_page_is_halved_and_counted() {
        let plan = AtlasPlan::pack(&[source(1, 8192, 8192)]);
        assert_eq!(plan.halved(), 1);
        assert_eq!(plan.dropped(), 0);
        let entry = plan.entry(key(1)).expect("packed");
        // 8192 pads past the cap, 4096 still does, 2048 fits.
        assert_eq!((entry.width, entry.height), (2048, 2048));
        assert!(padded(entry.width) <= PAGE_MAX);
    }

    #[test]
    fn exhausting_the_layer_budget_drops_and_counts_rather_than_panicking() {
        // One 4096 texture per page, one page per layer, more textures than
        // layers. Nothing here may panic and nothing may be placed off-atlas.
        let sources: Vec<AtlasSource> = (0..u64::from(MAX_LAYERS + 4))
            .map(|i| source(i, 4094, 4094))
            .collect();
        let plan = AtlasPlan::pack(&sources);
        assert_eq!(plan.layers(), MAX_LAYERS);
        assert_eq!(plan.dropped(), 4);
        assert_eq!(plan.entries().len(), MAX_LAYERS as usize);
        for entry in plan.entries() {
            assert!(entry.layer < MAX_LAYERS);
        }
    }

    #[test]
    fn a_dropped_texture_has_no_rectangle_and_reads_as_unused() {
        let sources: Vec<AtlasSource> = (0..u64::from(MAX_LAYERS + 1))
            .map(|i| source(i, 4094, 4094))
            .collect();
        let plan = AtlasPlan::pack(&sources);
        let missing = (0..u64::from(MAX_LAYERS + 1))
            .find(|i| plan.entry(key(*i)).is_none())
            .expect("one was dropped");
        assert_eq!(plan.rect(key(missing)), None);
        assert_eq!(
            plan.descriptor(key(missing), 0, AtlasFilter::Linear, true),
            TEXTURE_UNUSED
        );
    }

    #[test]
    fn a_rectangle_lands_strictly_inside_its_guard_ring() {
        let plan = AtlasPlan::pack(&[source(1, 100, 60)]);
        let rect = plan.rect(key(1)).expect("packed");
        let page = plan.page() as f32;
        // The mapped span must start at least one texel in and end at least one
        // texel short of the far edge, which is the whole point of the ring.
        assert!(rect[2] >= 1.0 / page);
        assert!(rect[3] >= 1.0 / page);
        assert!(rect[0] + rect[2] <= 1.0 - 1.0 / page);
        assert!(rect[1] + rect[3] <= 1.0 - 1.0 / page);
    }

    #[test]
    fn every_descriptor_field_round_trips_bit_exactly() {
        let mut checked = 0;
        for layer in [0u32, 1, 127, 255] {
            for uv_channel in 0..8u32 {
                for wrap_s in [AtlasWrap::Repeat, AtlasWrap::Clamp, AtlasWrap::Mirror] {
                    for wrap_t in [AtlasWrap::Repeat, AtlasWrap::Clamp, AtlasWrap::Mirror] {
                        for filter in [AtlasFilter::Nearest, AtlasFilter::Linear] {
                            for srgb in [false, true] {
                                let d = TextureDescriptor {
                                    layer,
                                    uv_channel,
                                    wrap_s,
                                    wrap_t,
                                    filter,
                                    srgb,
                                };
                                let bits = d.pack();
                                assert_eq!(bits & TEXTURE_UNUSED, 0);
                                assert_eq!(TextureDescriptor::unpack(bits), Some(d));
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(checked, 4 * 8 * 3 * 3 * 2 * 2);
    }

    #[test]
    fn the_sign_bit_is_the_empty_slot_and_zero_is_not() {
        // A zeroed word is a legal descriptor naming layer zero. That is why an
        // empty slot has to be written rather than left default, and why this
        // test exists at all.
        assert_eq!(TextureDescriptor::unpack(TextureDescriptor::none()), None);
        assert!(TextureDescriptor::unpack(0).is_some());
        // Every unused-flag word is empty regardless of what else it carries,
        // so a caller cannot half-set it.
        assert_eq!(TextureDescriptor::unpack(TEXTURE_UNUSED | 0x7FFF), None);
    }

    #[test]
    fn a_reserved_wrap_code_does_not_unpack() {
        let bad = 0b11 << 11;
        assert_eq!(TextureDescriptor::unpack(bad), None);
    }

    #[test]
    fn an_empty_plan_is_a_one_texel_page() {
        let plan = AtlasPlan::pack(&[]);
        assert!(plan.is_empty());
        assert_eq!(plan.page(), 1);
        assert_eq!(plan.layers(), 1);
    }

    #[test]
    fn a_zero_sized_source_is_not_packed() {
        let plan = AtlasPlan::pack(&[source(1, 0, 64), source(2, 64, 0), source(3, 64, 64)]);
        assert_eq!(plan.entries().len(), 1);
        assert!(plan.entry(key(3)).is_some());
    }

    /// An image whose red channel is the column and green channel is the row,
    /// so every texel names its own position and a misplaced one is obvious.
    fn ramp(width: u32, height: u32) -> RawImageData {
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                pixels.extend_from_slice(&[x as u8, y as u8, 0, 255]);
            }
        }
        RawImageData::new(pixels, width, height)
    }

    fn texel(block: &[u8], stride: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * stride + x) * 4) as usize;
        [block[i], block[i + 1], block[i + 2], block[i + 3]]
    }

    fn compose_with(wrap: AtlasWrap, image: &RawImageData) -> (Vec<u8>, u32) {
        let key = TextureKey {
            hash: image.hash,
            wrap_s: wrap,
            wrap_t: wrap,
        };
        let plan = AtlasPlan::pack(&[AtlasSource {
            key,
            width: image.width,
            height: image.height,
        }]);
        let entry = plan.entry(key).expect("packed");
        (super::compose(entry, image), image.width + 2)
    }

    #[test]
    fn a_tiling_texture_gets_the_opposite_edge_in_its_border() {
        // The whole reason the wrap mode is part of the packing identity. The
        // left border must be the rightmost column, so a bilinear tap at u = 0
        // blends the two columns a hardware Repeat would have blended.
        let image = ramp(8, 8);
        let (block, stride) = compose_with(AtlasWrap::Repeat, &image);
        assert_eq!(texel(&block, stride, 0, 1)[0], 7, "left border");
        assert_eq!(texel(&block, stride, 9, 1)[0], 0, "right border");
        assert_eq!(texel(&block, stride, 1, 0)[1], 7, "top border");
        assert_eq!(texel(&block, stride, 1, 9)[1], 0, "bottom border");
        // A corner takes the diagonally opposite texel, which is what both
        // axes wrapping at once means.
        assert_eq!(texel(&block, stride, 0, 0), [7, 7, 0, 255]);
        assert_eq!(texel(&block, stride, 9, 9), [0, 0, 0, 255]);
    }

    #[test]
    fn a_clamped_texture_gets_its_own_edge_in_its_border() {
        let image = ramp(8, 8);
        let (block, stride) = compose_with(AtlasWrap::Clamp, &image);
        assert_eq!(texel(&block, stride, 0, 1)[0], 0, "left border");
        assert_eq!(texel(&block, stride, 9, 1)[0], 7, "right border");
        assert_eq!(texel(&block, stride, 0, 0), [0, 0, 0, 255]);
    }

    #[test]
    fn a_mirrored_texture_borders_like_a_clamped_one() {
        // A mirror reflected at the boundary repeats the edge texel, so the
        // two share a fill. Asserted rather than assumed, because sharing the
        // arm is only correct while that is true.
        let image = ramp(8, 8);
        let (mirrored, stride) = compose_with(AtlasWrap::Mirror, &image);
        let (clamped, _) = compose_with(AtlasWrap::Clamp, &image);
        assert_eq!(mirrored, clamped);
        assert_eq!(texel(&mirrored, stride, 9, 1)[0], 7);
    }

    #[test]
    fn the_image_itself_is_copied_unchanged_inside_the_ring() {
        let image = ramp(8, 5);
        let (block, stride) = compose_with(AtlasWrap::Repeat, &image);
        for y in 0..5u32 {
            for x in 0..8u32 {
                assert_eq!(
                    texel(&block, stride, x + 1, y + 1),
                    [x as u8, y as u8, 0, 255]
                );
            }
        }
    }

    #[test]
    fn an_oversize_texture_is_halved_into_its_packed_size() {
        let image = ramp(64, 64);
        let entry = AtlasEntry {
            key: key(1),
            layer: 0,
            x: 1,
            y: 1,
            width: 16,
            height: 16,
            halvings: 2,
        };
        let block = super::compose(&entry, &image);
        assert_eq!(block.len(), (18 * 18 * 4) as usize);
        // Two halvings average 4x4 source blocks, so the first output texel is
        // the mean of columns 0..3, which is 1 (integer division twice).
        assert_eq!(texel(&block, 18, 1, 1)[0], 1);
    }

    #[test]
    fn an_image_shorter_than_its_header_claims_composes_rather_than_panicking() {
        let mut image = ramp(8, 8);
        image.pixels.truncate(16);
        let (block, stride) = compose_with(AtlasWrap::Repeat, &image);
        assert_eq!(block.len(), (stride * stride * 4) as usize);
    }
}
