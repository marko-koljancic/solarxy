//! Which auxiliary pass a surface shows, and how each becomes display pixels.
//!
//! A selector never touches the render: it chooses among planes a preview
//! already carries, so switching is a replay rather than anything a running job
//! notices. What lives here is only the display mapping, which a file
//! deliberately does not have, plus the rule for whether a pass can be shown at
//! all.
//!
//! # Why this is in the shared crate
//!
//! The same argument [`crate::still::float_to_rgba8`] already makes: every
//! surface that shows a pass needs these mappings, and two of them would make
//! the surfaces disagree about a render none of them is authoritative about.
//! This is the only crate all three shells reach, and the only one that is
//! wasm-clean by construction, so it is the only place the browser and the
//! terminal can share one answer.
//!
//! # Why the rule asks about capability rather than engine
//!
//! [`PassSelector`] is told whether the render writes auxiliary passes, never
//! which backend drew it. That is the rule the backend contract already states
//! for [`solarxy_renderer::backend::BackendCaps`], and here it is also the only
//! shape available: the engine enum lives in the graph crate, which this crate
//! must not depend on. A caller that holds an engine resolves it to a
//! capability on its own side of that line.

/// An auxiliary pass a render can write beside the image.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AovKind {
    /// The surface colour, before any light reached it.
    Albedo,
    /// The world-space surface normal.
    Normal,
    /// How far away the surface is, along the camera's axis.
    Depth,
}

impl AovKind {
    /// The word that names it, on the command line and in the file name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Albedo => "albedo",
            Self::Normal => "normal",
            Self::Depth => "depth",
        }
    }

    /// Whether this pass is read out of the accumulated auxiliary target
    /// rather than out of its own.
    ///
    /// Albedo and normal share one store, so asking for either fetches both.
    /// Depth is its own dispatch, because a depth is not a quantity whose mean
    /// is the answer.
    #[must_use]
    pub fn from_auxiliary(self) -> bool {
        matches!(self, Self::Albedo | Self::Normal)
    }
}

/// A pass a surface can show, including the beauty the others sit beside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassKind {
    Beauty,
    Albedo,
    Normal,
    Depth,
}

impl PassKind {
    /// Every pass, in the order a selector lists them.
    pub const ALL: [Self; 4] = [Self::Beauty, Self::Albedo, Self::Normal, Self::Depth];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Beauty => "Beauty",
            Self::Albedo => "Albedo",
            Self::Normal => "Normal",
            Self::Depth => "Depth",
        }
    }

    /// The auxiliary pass this shows, or nothing for the beauty.
    #[must_use]
    pub fn aov(self) -> Option<AovKind> {
        match self {
            Self::Beauty => None,
            Self::Albedo => Some(AovKind::Albedo),
            Self::Normal => Some(AovKind::Normal),
            Self::Depth => Some(AovKind::Depth),
        }
    }
}

/// What the selector knows: what the run asked for, whether the render writes
/// passes at all, and what the reader chose.
pub struct PassSelector {
    selected: PassKind,
    requested: Vec<AovKind>,
    writes_aovs: Option<bool>,
}

impl PassSelector {
    #[must_use]
    pub fn new(requested: &[AovKind]) -> Self {
        Self {
            selected: PassKind::Beauty,
            requested: requested.to_vec(),
            writes_aovs: None,
        }
    }

    /// Told by every preview, decided by the first: a render that writes no
    /// passes has none to show, so whatever was chosen before that was known
    /// falls back to the beauty.
    pub fn saw_capability(&mut self, writes_aovs: bool) {
        self.writes_aovs = Some(writes_aovs);
        if !self.available(self.selected) {
            self.selected = PassKind::Beauty;
        }
    }

    /// Whether this run writes no auxiliary passes at all, in which case the
    /// selector shows the beauty alone rather than three disabled rows for
    /// passes no flag could have produced.
    #[must_use]
    pub fn beauty_only(&self) -> bool {
        self.writes_aovs == Some(false)
    }

    /// Whether this run can show the pass.
    #[must_use]
    pub fn available(&self, kind: PassKind) -> bool {
        match kind.aov() {
            None => true,
            Some(aov) => !self.beauty_only() && self.requested.contains(&aov),
        }
    }

    #[must_use]
    pub fn selected(&self) -> PassKind {
        self.selected
    }

    /// Choose, if this run can show it. Never touches the render.
    pub fn choose(&mut self, kind: PassKind) {
        if self.available(kind) {
            self.selected = kind;
        }
    }
}

/// Reads the first three of every four floats: the albedo the kernel merged.
#[must_use]
pub fn albedo_from_auxiliary(aux: &[f32]) -> Vec<f32> {
    aux.as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect()
}

/// Reads the fourth of every four floats and unpacks the world normal from it.
///
/// A pixel that never described a surface decodes to `+Z`, which is the
/// sentinel the packing defines rather than a direction anything faced.
#[must_use]
pub fn normal_from_auxiliary(aux: &[f32]) -> Vec<f32> {
    aux.as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| solarxy_renderer::pathtrace::unpack_aov_normal(p[3]))
        .collect()
}

/// Reinterprets a float plane's bytes.
///
/// The still job hands every float plane back as its bytes so a tile is one
/// shape whatever it holds; this is the other end of that.
#[must_use]
pub fn floats_of(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}

/// One display value as a byte.
fn byte_of(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// The albedo lanes of the auxiliary plane, through the same transfer curve
/// the beauty preview uses, so the two read as one family of pictures.
#[must_use]
pub fn albedo_rgba8(aux_bytes: &[u8]) -> Vec<u8> {
    let rgb = albedo_from_auxiliary(&floats_of(aux_bytes));
    let mut rgba = Vec::with_capacity(rgb.len() / 3 * 16);
    for p in rgb.as_chunks::<3>().0 {
        for v in [p[0], p[1], p[2], 1.0f32] {
            rgba.extend_from_slice(&v.to_le_bytes());
        }
    }
    crate::still::float_to_rgba8(&rgba)
}

/// The world normal, remapped from its signed range into the visible one.
///
/// Written as raw display bytes: an sRGB texture sampled onto an sRGB
/// surface decodes and re-encodes, so the bytes uploaded are the bytes
/// shown, and a normal map should look like a normal map rather than a
/// gamma-corrected guess at one.
#[must_use]
pub fn normal_rgba8(aux_bytes: &[u8]) -> Vec<u8> {
    let n = normal_from_auxiliary(&floats_of(aux_bytes));
    let mut out = Vec::with_capacity(n.len() / 3 * 4);
    for p in n.as_chunks::<3>().0 {
        for v in [p[0], p[1], p[2]] {
            out.push(byte_of(v * 0.5 + 0.5));
        }
        out.push(255);
    }
    out
}

/// Where a ray found nothing, per the still job's depth contract.
const DEPTH_MISS_FLOOR: f32 = 1e29;

/// The nearest surface's grey, and the floor the farthest one is held at,
/// so the far end of a scene never reads as the misses beside it.
const DEPTH_NEAR: f32 = 1.0;
const DEPTH_FAR: f32 = 0.1;

/// The grey a depth plane with no range at all shows: distinct from both
/// the misses and the floor, so a flat plane reads as a surface.
const DEPTH_FLAT: f32 = 0.55;

/// The depth plane, normalized over its finite range: near is bright, far
/// is held at a floor, and a miss is black, which keeps the three states
/// tellable apart at a glance.
#[must_use]
pub fn depth_rgba8(depth_bytes: &[u8]) -> Vec<u8> {
    let depth = floats_of(depth_bytes);
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for &v in &depth {
        if v < DEPTH_MISS_FLOOR {
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    let span = hi - lo;
    let mut out = Vec::with_capacity(depth.len() * 4);
    for &v in &depth {
        let grey = if v >= DEPTH_MISS_FLOOR {
            0.0
        } else if span <= f32::EPSILON {
            DEPTH_FLAT
        } else {
            DEPTH_NEAR - (v - lo) / span * (DEPTH_NEAR - DEPTH_FAR)
        };
        let b = byte_of(grey);
        out.extend_from_slice(&[b, b, b, 255]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aux_bytes(pixels: &[[f32; 4]]) -> Vec<u8> {
        pixels
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect()
    }

    /// A render that writes no passes shows the beauty alone, whatever was
    /// requested or chosen before that was known.
    #[test]
    fn a_render_that_writes_no_passes_is_beauty_only() {
        let mut selector = PassSelector::new(&[AovKind::Albedo]);
        selector.choose(PassKind::Albedo);
        assert_eq!(selector.selected(), PassKind::Albedo);

        selector.saw_capability(false);
        assert!(selector.beauty_only());
        assert_eq!(
            selector.selected(),
            PassKind::Beauty,
            "a choice made before the capability was known survived it"
        );
        assert!(!selector.available(PassKind::Albedo));
        selector.choose(PassKind::Albedo);
        assert_eq!(selector.selected(), PassKind::Beauty);
    }

    /// A render that writes passes offers exactly what was requested, and
    /// refuses the rest.
    #[test]
    fn a_render_that_writes_passes_offers_the_requested_ones() {
        let mut selector = PassSelector::new(&[AovKind::Normal]);
        selector.saw_capability(true);
        assert!(!selector.beauty_only());
        assert!(selector.available(PassKind::Beauty));
        assert!(!selector.available(PassKind::Albedo));
        assert!(selector.available(PassKind::Normal));
        assert!(!selector.available(PassKind::Depth));

        selector.choose(PassKind::Depth);
        assert_eq!(
            selector.selected(),
            PassKind::Beauty,
            "an unrequested pass was chosen"
        );
        selector.choose(PassKind::Normal);
        assert_eq!(selector.selected(), PassKind::Normal);
    }

    /// The albedo display agrees with the shared float transfer on the rgb
    /// lanes, which pins the repacking between the two.
    #[test]
    fn the_albedo_display_matches_the_shared_transfer() {
        let aux = aux_bytes(&[[0.5, 0.25, 1.5, 0.0], [0.0, 1.0, 0.125, 123.0]]);
        let shown = albedo_rgba8(&aux);

        let mut repacked = Vec::new();
        for p in [[0.5f32, 0.25, 1.5, 1.0], [0.0, 1.0, 0.125, 1.0]] {
            for v in p {
                repacked.extend_from_slice(&v.to_le_bytes());
            }
        }
        let expected = crate::still::float_to_rgba8(&repacked);
        assert_eq!(shown, expected);
    }

    /// The normal display is the shared unpack remapped into visible range,
    /// opaque. The unpack itself is pinned in the renderer; what this pins
    /// is the remap and the packing around it.
    #[test]
    fn the_normal_display_remaps_the_shared_unpack() {
        let packed = [0.0f32, 123_456.0, 8_000_000.0];
        let aux = aux_bytes(&[
            [0.0, 0.0, 0.0, packed[0]],
            [0.0, 0.0, 0.0, packed[1]],
            [0.0, 0.0, 0.0, packed[2]],
        ]);
        let shown = normal_rgba8(&aux);

        let unpacked = normal_from_auxiliary(&floats_of(&aux));
        for (pixel, n) in shown
            .as_chunks::<4>()
            .0
            .iter()
            .zip(unpacked.as_chunks::<3>().0)
        {
            for lane in 0..3 {
                let expected = byte_of(n[lane] * 0.5 + 0.5);
                assert_eq!(pixel[lane], expected);
            }
            assert_eq!(pixel[3], 255, "a normal pixel is not opaque");
        }
    }

    /// Near is bright, far is floored, a miss is black, and the three are
    /// tellable apart.
    #[test]
    fn the_depth_display_ramps_and_marks_misses() {
        let depth: Vec<u8> = [1.0f32, 2.0, 1e30, 1.5]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let shown = depth_rgba8(&depth);
        let greys: Vec<u8> = shown.as_chunks::<4>().0.iter().map(|p| p[0]).collect();
        assert_eq!(greys[0], 255, "the nearest surface is not the brightest");
        assert_eq!(greys[1], byte_of(DEPTH_FAR), "the farthest is not floored");
        assert_eq!(greys[2], 0, "a miss is not black");
        assert!(
            greys[3] > greys[1] && greys[3] < greys[0],
            "the middle did not land between the ends: {greys:?}"
        );
        for p in shown.as_chunks::<4>().0 {
            assert_eq!(p[0], p[1]);
            assert_eq!(p[1], p[2]);
            assert_eq!(p[3], 255);
        }
    }

    /// Degenerate planes stay defined: all misses are all black, and a flat
    /// plane is a mid grey rather than a division by zero.
    #[test]
    fn degenerate_depth_planes_stay_defined() {
        let all_miss: Vec<u8> = [1e30f32, 1e30]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let shown = depth_rgba8(&all_miss);
        assert!(
            shown.as_chunks::<4>().0.iter().all(|p| p[0] == 0),
            "{shown:?}"
        );

        let flat: Vec<u8> = [3.0f32, 3.0].iter().flat_map(|v| v.to_le_bytes()).collect();
        let shown = depth_rgba8(&flat);
        assert!(
            shown
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[0] == byte_of(DEPTH_FLAT)),
            "{shown:?}"
        );
    }
}
