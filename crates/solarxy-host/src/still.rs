//! The still render: a picture larger than the device will draw in one pass.
//!
//! # Why tiles at all
//!
//! A single dispatch or draw over sixty-seven megapixels is not something a
//! browser's GPU process reliably survives, and the failure is not a slow frame
//! but a lost device with no recovery on the web. So the job bounds every pass:
//! the image is cut into tiles, each inside the four-megapixel budget the gate
//! measured thirty consecutive dispatches at, and the tiles are rendered one at
//! a time with the queue drained between them.
//!
//! That bound is also the pacing. One sample chunk per frame keeps the page
//! responsive during a render that takes minutes, and it is the same mechanism:
//! nothing the job asks of the device is ever larger than one tile of one
//! chunk.
//!
//! # One driver, two engines
//!
//! The job holds a [`RenderBackend`] and does not care which. What it does care
//! about is the *engine the author chose*, because the two tile differently:
//! the rasterizer needs an asymmetric frustum so its tile is a window on the
//! same shot, and the path tracer needs a dispatch offset so its rays are the
//! rays that pixel would have cast anyway. Both arrive as
//! [`solarxy_renderer::backend::ImageWindow`] on the frame, and each backend
//! reads it its own way.
//!
//! The engine is therefore authored rather than sniffed. A render node carries
//! the choice, the job carries it forward, and nothing here asks a backend what
//! it is.
//!
//! # Aprons
//!
//! Path tracing is not a screen-space effect: a pixel's value depends on the
//! scene, not on its neighbours, so traced tiles butt together seamlessly with
//! no overlap at all. Bloom is a screen-space effect, and a tile that blooms
//! only from what is inside it has a visible discontinuity at its edge. So the
//! apron is not per engine but per post pass: bloom on means every tile renders
//! a margin it then discards.

use std::collections::VecDeque;

use solarxy_core::preferences::{InspectionMode, ResolvedBackground};
use solarxy_core::view_config::{DisplaySettings, PaneDisplaySettings};
use solarxy_core::AABB;
use solarxy_renderer::backend::{FrameCtx, FrameOutcome, ImageWindow, PaneContent, RenderBackend};
use solarxy_renderer::camera_state::CameraState;
use solarxy_renderer::capture::{CaptureTarget, CapturePoll, PendingCapture};
use solarxy_renderer::composite::CompositeLook;
use solarxy_renderer::environment::SceneEnvironment;
use solarxy_renderer::frame::Renderer;

/// The largest single pass the job will ask a device for.
///
/// Four megapixels, which is the budget gate G5 ran thirty consecutive
/// dispatches at without losing a device on either shell, and the same number
/// the screenshot path has been capped at since the web app shipped.
pub const TILE_BUDGET_PIXELS: u32 = 4 * 1024 * 1024;

/// The tile budget a surface showing the picture asks for.
///
/// An edge of 256 pixels, so an ordinary still comes in tens of tiles rather
/// than one and a reader watching it sees it arrive. Named here rather than
/// chosen at each call site, because it is one judgement about how often a
/// person wants to see something new and it should be made once.
///
/// What it costs is one target resize and one readback per extra tile. What it
/// buys is the whole value of a surface that shows the picture, and the output
/// is unchanged either way.
///
/// It sits beside [`TILE_BUDGET_PIXELS`] rather than in the headless crate,
/// where only a native caller could see it.
pub const PREVIEW_TILE_BUDGET: u32 = 256 * 256;

/// The margin a tile renders and discards when a screen-space post pass is on.
///
/// A hundred and twenty-eight pixels, which is comfortably wider than the bloom
/// blur's reach. Zero when nothing screen-space runs, which is the ordinary
/// case for a traced still and is what makes it seam-free by construction
/// rather than by a wide enough guess.
pub const TILE_APRON_PIXELS: u32 = 128;

/// The largest image the job will render, per edge.
pub const MAX_STILL_EDGE: u32 = 8192;

/// What a finished tile is read back as.
///
/// Three answers to one question, because a still goes to two very different
/// places. A screen wants eight bits in the display's own space; a compositing
/// package wants floating point, and then wants to know whether the look has
/// already been applied to it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum StillReadback {
    /// Eight bits per channel, through the finishing chain. The ordinary case,
    /// and what every caller that does not ask for something else gets.
    #[default]
    Display8,
    /// Floating point, through the finishing chain: the same image the display
    /// path produces, without the quantization.
    DisplayFloat,
    /// Floating point, before the finishing chain. Scene-referred light with no
    /// exposure, tone map or grade applied, which is what a compositing package
    /// expects to be handed and what lets it apply a look of its own.
    SceneLinear,
}

impl StillReadback {
    /// The texture format a tile is read out of.
    #[must_use]
    pub fn source_format(self, surface: wgpu::TextureFormat) -> wgpu::TextureFormat {
        match self {
            Self::Display8 => surface,
            Self::DisplayFloat => solarxy_renderer::pipelines::FLOAT_COMPOSITE_FORMAT,
            Self::SceneLinear => solarxy_renderer::texture::Texture::HDR_FORMAT,
        }
    }

    /// Bytes per pixel in a finished tile.
    ///
    /// Sixteen for both float modes rather than eight for one of them: a
    /// half-float source is widened on the way out, because the width it was
    /// stored at is a decision the renderer made and not one a consumer should
    /// have to undo.
    #[must_use]
    pub fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Display8 => 4,
            Self::DisplayFloat | Self::SceneLinear => 16,
        }
    }

    /// Whether the finishing chain runs at all.
    #[must_use]
    pub fn composites(self) -> bool {
        !matches!(self, Self::SceneLinear)
    }
}

/// The readback a format and a space name.
///
/// Takes the words rather than an enum because the callers are boundaries: a
/// dialog and a command line both hand over what a person chose. Anything that
/// is not a float image is the eight-bit display path, and a float one is
/// scene-referred unless it asks for the finished look, which is the command
/// line's default and so the browser's too. One vocabulary for one idea.
#[must_use]
pub fn readback_for(format: &str, space: &str) -> StillReadback {
    if !format.eq_ignore_ascii_case("exr") {
        return StillReadback::Display8;
    }
    if space.eq_ignore_ascii_case("display") {
        StillReadback::DisplayFloat
    } else {
        StillReadback::SceneLinear
    }
}

/// Clamps and sRGB-encodes a float image for a screen that cannot show it.
///
/// Every surface that previews a float render needs this and none of them
/// should invent their own: the browser's still dialog and the command line's
/// watch window are both showing a preview of a file judged elsewhere, and two
/// display transforms would make them disagree about a render neither of them
/// is authoritative about.
///
/// The clamp *is* the tone mapping for a scene-referred image, which is why
/// both surfaces say so rather than letting it look like the picture.
///
/// `bytes` is four `f32` a pixel; the result is four bytes a pixel, with the
/// alpha lane carried straight through. Coverage is a fraction rather than
/// light, so it quantizes linearly with no transfer curve; an opaque render's
/// lane is exactly one and lands as exactly 255, which is why honouring it
/// changed no existing byte.
#[must_use]
pub fn float_to_rgba8(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() / 4);
    for px in bytes.as_chunks::<16>().0 {
        for c in 0..3 {
            let i = c * 4;
            let v = f32::from_le_bytes([px[i], px[i + 1], px[i + 2], px[i + 3]]).clamp(0.0, 1.0);
            let encoded = if v <= 0.003_130_8 {
                v * 12.92
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            };
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            out.push((encoded * 255.0).round().clamp(0.0, 255.0) as u8);
        }
        let a = f32::from_le_bytes([px[12], px[13], px[14], px[15]]).clamp(0.0, 1.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        out.push((a * 255.0).round() as u8);
    }
    out
}

/// Which renderer draws the still.
///
/// Authored, not detected. See the module documentation.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum StillEngine {
    /// The viewport's rasterizer, tiled by asymmetric frustum.
    #[default]
    Raster,
    /// The path tracer, tiled by dispatch offset and accumulated to a sample
    /// count.
    PathTraced,
}

/// What to render.
#[derive(Clone, Copy, Debug)]
pub struct StillSpec {
    pub width: u32,
    pub height: u32,
    pub engine: StillEngine,
    /// Samples per pixel. Ignored by the raster engine, which draws each tile
    /// once.
    pub samples: u32,
    /// Whether a screen-space post pass runs, and so whether tiles need an
    /// apron. Read from the renderer's own post state by the shell.
    pub screen_space_post: bool,
    /// The largest single pass, in pixels.
    ///
    /// [`TILE_BUDGET_PIXELS`] in production. A field rather than a constant
    /// because it is genuinely a property of the device rather than of the
    /// picture: a weaker one wants smaller passes, and a test wants a grid out
    /// of an image small enough to render in a second.
    pub tile_budget: u32,
    /// What the finished tiles are read back as.
    pub readback: StillReadback,
    /// Whether to read the auxiliary channels back beside the colour.
    ///
    /// Two flags rather than a set of named passes, because albedo and normal
    /// are written by one store and read by one copy: what the job can fetch is
    /// two extra planes, and which of them a caller turns into which file is
    /// the caller's vocabulary rather than the job's.
    ///
    /// Costs a tile-sized float copy per tile, which is why it is opt-in. A
    /// backend that writes no auxiliary output simply returns nothing and the
    /// tile arrives without it.
    pub aux: bool,
    /// Whether to run a depth pass per tile and read it back.
    pub depth: bool,
    /// How often to publish the picture so far, in milliseconds. Zero never
    /// does.
    ///
    /// Opt-in, because a preview is only worth its composite and its readback
    /// to a caller that shows one. A surface with a window passes
    /// [`PREVIEW_INTERVAL_MS`]; the headless command passes zero and pays
    /// nothing, since its own reader is fed when a tile lands.
    pub preview_interval_ms: u64,
    /// Render with nothing behind the subject: the environment lights the
    /// scene but is not photographed into it, and the image leaves with a
    /// real matte in its alpha lane.
    ///
    /// The one field a shell sets. The job substitutes the transparent
    /// background into the frame it hands the backend, which the rasterizer
    /// reads as clear-to-zero-and-draw-no-background and the tracer ignores
    /// in favour of its own settings flag, and it tells the composite to
    /// carry the alpha lane instead of dropping it.
    pub transparent: bool,
}

impl Default for StillSpec {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            engine: StillEngine::default(),
            samples: 64,
            screen_space_post: false,
            tile_budget: TILE_BUDGET_PIXELS,
            readback: StillReadback::Display8,
            aux: false,
            depth: false,
            preview_interval_ms: 0,
            transparent: false,
        }
    }
}

impl StillSpec {
    /// Clamps the request to what the job will actually render.
    #[must_use]
    pub fn clamped(mut self) -> Self {
        self.width = self.width.clamp(16, MAX_STILL_EDGE);
        self.height = self.height.clamp(16, MAX_STILL_EDGE);
        self.samples = self.samples.max(1);
        // A budget under one whole tile of anything is a plan with no tiles.
        self.tile_budget = self.tile_budget.max(64 * 64);
        self
    }
}

/// A floating-point still being assembled from tiles.
///
/// Three channels for an opaque render, because that is what its encoder takes
/// and its alpha would be a constant one pretending to be a matte; dropping the
/// lane here rather than at encode time saves a quarter of the buffer at the
/// size where that matters. A transparent render keeps all four, colour
/// unassociated and alpha a plain coverage fraction, which is the one internal
/// convention both formats derive their own from: the floating-point encoder
/// multiplies on the way out and the eight-bit path does not.
///
/// Shared by both graphical shells rather than written twice, which is what
/// makes "the same scene saved from either produces the same values" a property
/// of the code instead of a thing to be checked and hoped for.
pub struct FloatImage {
    width: u32,
    height: u32,
    /// Scene-referred or display-referred, matching the readback that filled
    /// it. Carried so an encode cannot mislabel what it wrote.
    scene_linear: bool,
    /// Four when the still carries a matte, else three.
    channels: usize,
    /// `width * height * channels`, in image order.
    data: Vec<f32>,
}

impl FloatImage {
    /// The buffer a float readback needs, or `None` for the eight-bit one,
    /// which is assembled as bytes and needs nothing here.
    ///
    /// `transparent` keeps the alpha lane: the still carries a matte and the
    /// fourth channel is it.
    #[must_use]
    pub fn new(
        readback: StillReadback,
        width: u32,
        height: u32,
        transparent: bool,
    ) -> Option<Self> {
        if readback == StillReadback::Display8 {
            return None;
        }
        let channels = if transparent { 4 } else { 3 };
        Some(Self {
            width,
            height,
            scene_linear: readback == StillReadback::SceneLinear,
            channels,
            data: vec![0.0; (width as usize) * (height as usize) * channels],
        })
    }

    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Whether this holds scene-referred light rather than a finished look.
    #[must_use]
    pub fn is_scene_linear(&self) -> bool {
        self.scene_linear
    }

    /// Whether the fourth channel exists and is a matte.
    #[must_use]
    pub fn has_matte(&self) -> bool {
        self.channels == 4
    }

    /// The opaque image: three floats a pixel.
    #[must_use]
    pub fn rgb(&self) -> &[f32] {
        debug_assert_eq!(self.channels, 3, "a matte image is read through rgba()");
        &self.data
    }

    /// The matte image: four floats a pixel, colour unassociated, alpha the
    /// coverage fraction. What [`FloatImage::has_matte`] promises.
    #[must_use]
    pub fn rgba(&self) -> &[f32] {
        debug_assert_eq!(self.channels, 4, "an opaque image is read through rgb()");
        &self.data
    }

    /// Copies one finished tile's colour into its place in the image.
    ///
    /// The tile arrives as four `f32` a pixel. An opaque still lands as three,
    /// its alpha a constant one; a matte still keeps the lane, and when the
    /// readback is scene-referred the colour arrives coverage-weighted out of
    /// the accumulator and is divided out here, because everything on the CPU
    /// side of the readback speaks unassociated colour plus a fraction and the
    /// encoders own their formats' conventions. A display-referred readback
    /// arrives already unassociated: the composite divided before its
    /// nonlinear chain.
    pub fn place(&mut self, rect: TileRect, pixels: &[u8]) {
        let src = crate::passes::floats_of(pixels);
        for row in 0..rect.height {
            for col in 0..rect.width {
                let src_at = ((row * rect.width + col) as usize) * 4;
                let (x, y) = (rect.x + col, rect.y + row);
                if x >= self.width || y >= self.height {
                    continue;
                }
                let dst_at = ((y as usize) * (self.width as usize) + (x as usize)) * self.channels;
                if let (Some(p), Some(slot)) = (
                    src.get(src_at..src_at + self.channels),
                    self.data.get_mut(dst_at..dst_at + self.channels),
                ) {
                    slot.copy_from_slice(p);
                    if self.channels == 4 && self.scene_linear && slot[3] > 0.0 {
                        for c in 0..3 {
                            slot[c] /= slot[3];
                        }
                    }
                }
            }
        }
    }
}

/// A rectangle of the image, in pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TileRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl TileRect {
    #[must_use]
    pub fn area(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }
}

/// One tile: what it owns, what it renders, and where the first sits in the
/// second.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tile {
    /// The part of the final image this tile is responsible for. Every pixel of
    /// the image belongs to exactly one tile's `image`.
    pub image: TileRect,
    /// What is actually rendered: `image` grown by the apron and clipped to the
    /// picture, so an edge tile does not render outside it.
    pub render: TileRect,
}

impl Tile {
    /// Where `image` starts inside `render`, in pixels. Zero on every side when
    /// there is no apron.
    #[must_use]
    pub fn crop(&self) -> [u32; 2] {
        [self.image.x - self.render.x, self.image.y - self.render.y]
    }
}

/// Every tile of one image.
#[derive(Clone, Debug)]
pub struct TilePlan {
    pub tiles: Vec<Tile>,
    pub columns: u32,
    pub rows: u32,
}

impl TilePlan {
    /// Cuts `width` by `height` into tiles no larger than the budget.
    ///
    /// Square-ish rather than full-width strips: a strip of an eight-thousand
    /// pixel image is five hundred rows tall at the budget, which is a fine
    /// tile, but a square keeps the apron's waste proportional instead of
    /// paying it along the whole width of every row.
    #[must_use]
    pub fn new(width: u32, height: u32, budget: u32, apron: u32) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        // The rendered tile carries the apron on both sides, so the part it
        // owns has to be smaller by that much or the pass exceeds the budget.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let budget_edge = f64::from(budget).sqrt() as u32;
        let owned_edge = budget_edge.saturating_sub(2 * apron).max(1);

        let columns = width.div_ceil(owned_edge);
        let rows = height.div_ceil(owned_edge);
        let mut tiles = Vec::with_capacity((columns as usize) * (rows as usize));
        for row in 0..rows {
            for column in 0..columns {
                let x = column * owned_edge;
                let y = row * owned_edge;
                let image = TileRect {
                    x,
                    y,
                    width: owned_edge.min(width - x),
                    height: owned_edge.min(height - y),
                };
                // Grown by the apron and clipped to the picture: a tile at the
                // edge has nothing outside to bleed in from, so it renders
                // exactly what it owns there.
                let rx = image.x.saturating_sub(apron);
                let ry = image.y.saturating_sub(apron);
                let render = TileRect {
                    x: rx,
                    y: ry,
                    width: (image.x + image.width + apron).min(width) - rx,
                    height: (image.y + image.height + apron).min(height) - ry,
                };
                tiles.push(Tile { image, render });
            }
        }
        Self {
            tiles,
            columns,
            rows,
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }
}

/// One finished tile, ready for whatever assembles the picture.
pub struct StillTile {
    /// Where this belongs in the final image.
    pub rect: TileRect,
    /// The colour, tightly packed, apron already cropped away, at the width
    /// [`StillReadback::bytes_per_pixel`] states: four bytes for the eight-bit
    /// mode and sixteen for either float one.
    pub pixels: Vec<u8>,
    /// The auxiliary channels, when [`StillSpec::aux`] asked for them and the
    /// backend had them: four `f32` per pixel, albedo in the first three and
    /// the octahedrally packed world normal in the fourth.
    pub aux: Option<Vec<u8>>,
    /// The depth, when [`StillSpec::depth`] asked for it: one `f32` per pixel,
    /// measured along the camera's axis, `1e30` where the ray hit nothing.
    pub depth: Option<Vec<u8>>,
}

/// Four `f32`, which is what the auxiliary target is.
const AUX_BYTES_PER_PIXEL: usize = 16;
/// One `f32`, which is what a depth is.
const DEPTH_BYTES_PER_PIXEL: usize = 4;
/// Four bytes, which is what a preview always is.
const PREVIEW_BYTES_PER_PIXEL: usize = 4;

/// How often the job publishes the picture it has so far.
///
/// A quarter of a second, which is the same interval the command line's
/// dashboard already samples its throughput at, so the two readings a person
/// watches move at one rhythm rather than two.
///
/// Wall clock rather than a chunk count, and that is the whole point: a chunk
/// costs milliseconds in a simple scene and seconds in a heavy one, so a fixed
/// chunk interval would publish constantly in one and almost never in the
/// other. This is a statement about how often a person wants to see something
/// new, which is a property of the person and not of the scene.
pub const PREVIEW_INTERVAL_MS: u64 = 250;

/// The picture so far, for a surface that shows a render while it runs.
///
/// Always four bytes a pixel, whatever the render's own format is: a preview is
/// looked at rather than composited, and reading sixteen bytes to show four
/// would make the mechanism cost more the more precise the render.
///
/// The rect is the tile's own, apron already cut off, so a caller paints it
/// exactly where it paints a finished [`StillTile`] and needs no second path.
pub struct StillPreview {
    pub rect: TileRect,
    pub pixels: Vec<u8>,
}

/// One plane's readback: in flight, then landed.
///
/// Two states in one struct because a tile waits on up to three buffers and
/// they do not land together. A poll that has landed unmaps its buffer, so the
/// bytes have to be kept somewhere until the slowest plane arrives, and keeping
/// them here is what lets the collect step be called as many times as it takes.
struct PendingPlane {
    capture: Option<PendingCapture>,
    bytes: Option<Vec<u8>>,
    format: wgpu::TextureFormat,
    /// Whether to read it as floats. The colour plane's eight-bit mode is the
    /// only one that is not.
    floats: bool,
}

/// Where one plane's readback has got to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PlaneStep {
    Pending,
    Ready,
    Failed,
}

impl PendingPlane {
    fn arm(capture: PendingCapture, format: wgpu::TextureFormat, floats: bool) -> Self {
        Self {
            capture: Some(capture),
            bytes: None,
            format,
            floats,
        }
    }

    fn poll(&mut self, device: &wgpu::Device) -> PlaneStep {
        if self.bytes.is_some() {
            return PlaneStep::Ready;
        }
        let Some(capture) = self.capture.as_ref() else {
            return PlaneStep::Failed;
        };
        let polled = if self.floats {
            // Widened to full float on the way out and handed on as its bytes,
            // so a plane stays one shape whatever it holds and the caller reads
            // it back with the width its own kind states.
            match capture.poll_floats(device, self.format) {
                solarxy_renderer::capture::CaptureFloatPoll::Pending => CapturePoll::Pending,
                solarxy_renderer::capture::CaptureFloatPoll::Failed => CapturePoll::Failed,
                solarxy_renderer::capture::CaptureFloatPoll::Ready(floats) => {
                    CapturePoll::Ready(bytemuck::cast_slice(&floats).to_vec())
                }
            }
        } else {
            capture.poll(device, self.format)
        };
        match polled {
            CapturePoll::Pending => PlaneStep::Pending,
            CapturePoll::Failed => PlaneStep::Failed,
            CapturePoll::Ready(bytes) => {
                self.capture = None;
                self.bytes = Some(bytes);
                PlaneStep::Ready
            }
        }
    }
}

/// Every readback one tile is waiting on.
struct PendingTile {
    color: PendingPlane,
    aux: Option<PendingPlane>,
    depth: Option<PendingPlane>,
}

impl PendingTile {
    fn planes_mut(&mut self) -> impl Iterator<Item = &mut PendingPlane> {
        std::iter::once(&mut self.color)
            .chain(self.aux.iter_mut())
            .chain(self.depth.iter_mut())
    }

    /// The planes' bytes, once every one of them has landed. `None` if any is
    /// still missing, which after a ready poll it cannot be.
    fn into_planes(self) -> Option<TilePlanes> {
        Some(TilePlanes {
            color: self.color.bytes?,
            aux: match self.aux {
                Some(p) => Some(p.bytes?),
                None => None,
            },
            depth: match self.depth {
                Some(p) => Some(p.bytes?),
                None => None,
            },
        })
    }
}

/// What one tile's readbacks came back as, before the apron is cut off.
struct TilePlanes {
    color: Vec<u8>,
    aux: Option<Vec<u8>>,
    depth: Option<Vec<u8>>,
}

/// How far along a job is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StillProgress {
    pub tile: u32,
    pub tiles: u32,
    pub sample: u32,
    pub samples: u32,
    /// Pixel-samples drawn, and how many there are in the whole picture.
    ///
    /// **Weighted by area, which is the reason these are here rather than
    /// derived from the four counts above.** Tiles are not the same size: the
    /// plan fills row by row and the right-hand column and bottom row are
    /// whatever is left over, so counting tiles equally makes an estimate run
    /// long exactly at the end of a render, where a person is most likely to be
    /// reading it. The job owns the tile plan and is therefore the only thing
    /// that can weight them; nothing downstream of it can.
    ///
    /// Integers rather than a fraction because the progress events they ride on
    /// are compared for equality, and because the division is the estimator's
    /// business rather than the reporter's.
    pub drawn: u64,
    pub total: u64,
}

/// How much longer, from the rate so far, or nothing while there is not enough
/// to say.
///
/// Whole-run average rather than a recent rate: every tile of a given size
/// costs the same, so the average is the better predictor and does not lurch
/// when one tile happens to be sky. What the average got wrong before was not
/// its shape but its input, which counted a small edge tile as a whole one.
///
/// Takes a clock reading rather than reading one, which is what keeps it
/// testable and keeps it compiling for the browser.
#[must_use]
pub fn estimate_remaining_ms(drawn: u64, total: u64, elapsed_ms: u64) -> Option<u64> {
    if drawn == 0 || total == 0 || drawn >= total || elapsed_ms == 0 {
        return None;
    }
    // In milliseconds throughout: the largest render this job will accept is
    // about seven times ten to the tenth pixel-samples, which multiplied by any
    // plausible elapsed still fits, and the alternative is a float division
    // whose rounding a reader would see flicker in the last digit.
    let total_ms = (u128::from(elapsed_ms) * u128::from(total)) / u128::from(drawn);
    u64::try_from(total_ms.saturating_sub(u128::from(elapsed_ms))).ok()
}

/// A span as a person reads one.
///
/// One spelling for every surface that shows a time, which is the whole point:
/// the same render should not be "252.0s" in one window and "4m 12s" in
/// another. Seconds with a tenth below a minute, because that is the range
/// where a tenth means something; whole seconds above it, because past a minute
/// nobody is reading the fraction.
#[must_use]
pub fn format_duration_ms(ms: u64) -> String {
    let secs = ms / 1000;
    if secs < 60 {
        return format!("{:.1}s", ms as f64 / 1000.0);
    }
    if secs < 3600 {
        return format!("{}m {:02}s", secs / 60, secs % 60);
    }
    format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
}

/// What one [`StillRenderJob::advance`] did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StillStep {
    /// Still working. Call again next frame.
    Working,
    /// A tile finished; take it with [`StillRenderJob::take_tile`].
    Tile,
    /// The picture so far is ready; take it with
    /// [`StillRenderJob::take_preview`].
    ///
    /// Its own case rather than an overload of [`StillStep::Tile`], because the
    /// two mean different things to a caller that saves: a tile is the render's
    /// output and a preview is a look at it, and a shell that assembled a
    /// preview into the file it writes would be writing an unfinished picture.
    Preview,
    /// Every tile is done and taken.
    Done,
    /// A readback failed. The job is over and the picture is incomplete.
    Failed,
}

/// Everything encoding one tile needs that the job does not own.
///
/// Wide, because a pane is wide. It is the same bundle the shells already
/// assemble for an ordinary frame, minus the parts a capture fixes: no pane
/// index, no split, no selection rim.
pub struct StillCtx<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub renderer: &'a mut Renderer,
    pub camera: &'a mut CameraState,
    pub env: &'a SceneEnvironment,
    pub pds: &'a PaneDisplaySettings,
    pub display: &'a DisplaySettings,
    pub background: ResolvedBackground,
    pub bounds: Option<&'a AABB>,
    pub look: CompositeLook,
    /// The format the capture target is allocated in, which is the surface's.
    pub format: wgpu::TextureFormat,
    pub scene_present: bool,
    /// A monotonic reading in milliseconds, for the preview's throttle.
    ///
    /// Supplied by the caller rather than read here, because this crate
    /// compiles for the browser and there is no clock in it: the desktop reads
    /// an `Instant`, the browser reads the page's own timer, and the headless
    /// command reads the one it already started for its progress stream. On the
    /// bundle rather than an argument, so a caller that forgets it does not
    /// compile.
    pub now_ms: u64,
}

/// A still render in progress.
///
/// Owns its tile plan, its accumulation position, and one capture target it
/// reuses. Dropping it frees everything, which is what cancel does.
pub struct StillRenderJob {
    spec: StillSpec,
    plan: TilePlan,
    tile: usize,
    samples_done: u32,
    pending: Option<PendingTile>,
    ready: VecDeque<StillTile>,
    target: Option<CaptureTarget>,
    finished: bool,
    /// The preview's own readback, with its own lifetime.
    ///
    /// Deliberately not the `pending` slot above. That one gates sampling: the
    /// tile it belongs to is finished and the next cannot start until its buffer
    /// is back. A preview that borrowed it would halve the sample rate in order
    /// to show progress, which is the opposite of what it is for.
    preview: Option<PendingPlane>,
    preview_target: Option<CaptureTarget>,
    preview_ready: Option<StillPreview>,
    /// When the last preview was armed, so the throttle is a stated interval
    /// rather than however often the caller happens to advance.
    preview_armed_ms: Option<u64>,
}

impl StillRenderJob {
    /// Plans the render. Nothing is allocated on the GPU until the first
    /// [`StillRenderJob::advance`].
    #[must_use]
    pub fn new(spec: StillSpec) -> Self {
        let spec = spec.clamped();
        let apron = if spec.screen_space_post {
            TILE_APRON_PIXELS
        } else {
            0
        };
        let plan = TilePlan::new(spec.width, spec.height, spec.tile_budget, apron);
        Self {
            spec,
            plan,
            tile: 0,
            samples_done: 0,
            pending: None,
            ready: VecDeque::new(),
            target: None,
            finished: false,
            preview: None,
            preview_target: None,
            preview_ready: None,
            preview_armed_ms: None,
        }
    }

    #[must_use]
    pub fn spec(&self) -> StillSpec {
        self.spec
    }

    #[must_use]
    pub fn plan(&self) -> &TilePlan {
        &self.plan
    }

    #[must_use]
    pub fn progress(&self) -> StillProgress {
        let samples = u64::from(self.target_samples());
        // The area a tile owns rather than the area it renders: the apron is
        // drawn and thrown away, and counting it would make a render with one
        // look further along than the same render without.
        let total: u64 = self
            .plan
            .tiles
            .iter()
            .map(|t| t.image.area() * samples)
            .sum();
        let done: u64 = self
            .plan
            .tiles
            .iter()
            .take(self.tile)
            .map(|t| t.image.area() * samples)
            .sum();
        let current = self
            .plan
            .tiles
            .get(self.tile)
            .map_or(0, |t| t.image.area() * u64::from(self.samples_done));
        StillProgress {
            tile: u32::try_from(self.tile).unwrap_or(u32::MAX),
            tiles: u32::try_from(self.plan.len()).unwrap_or(u32::MAX),
            sample: self.samples_done,
            samples: self.target_samples(),
            drawn: done + current,
            total,
        }
    }

    /// A finished tile, if one is waiting.
    pub fn take_tile(&mut self) -> Option<StillTile> {
        self.ready.pop_front()
    }

    /// The picture so far, if one has landed since it was last taken.
    ///
    /// Safe to call after any [`StillRenderJob::advance`]; [`StillStep::Preview`]
    /// is the hint that there is something here rather than a precondition.
    pub fn take_preview(&mut self) -> Option<StillPreview> {
        self.preview_ready.take()
    }

    /// The tile currently being rendered, for a caller that wants to show where
    /// the job is.
    #[must_use]
    pub fn current(&self) -> Option<Tile> {
        self.plan.tiles.get(self.tile).copied()
    }

    fn target_samples(&self) -> u32 {
        match self.spec.engine {
            // One pass per tile: a rasterized pixel does not converge, it is
            // just drawn.
            StillEngine::Raster => 1,
            StillEngine::PathTraced => self.spec.samples,
        }
    }

    /// Renders one chunk of the current tile, or collects a readback that has
    /// landed. Call once per frame.
    ///
    /// **How much one call draws is the backend's setting, not an argument.**
    /// A path tracer's chunk is `TraceSettings::chunk`, which the shell sets
    /// when it starts the job; a rasterizer draws its tile once whatever anyone
    /// asks. Passing a chunk here would be the job telling a backend how to do
    /// its own pacing.
    ///
    /// # Preconditions
    ///
    /// The caller must have sized the shared render targets to
    /// [`StillRenderJob::current`]'s `render` rect before calling. The job does
    /// not do it itself because the two shells resize differently -- the
    /// desktop also marks the overlap statistics dirty -- and a third copy of
    /// that body here would be the one that stopped matching.
    pub fn advance(
        &mut self,
        ctx: &mut StillCtx<'_>,
        backend: &mut dyn RenderBackend,
    ) -> StillStep {
        if self.finished {
            return StillStep::Done;
        }
        // Polled before anything else and on every call, so a preview that
        // landed while the sampling carried on is picked up promptly and its
        // buffer is freed. A failure here is not the render's failure: the
        // picture is unaffected, so the slot is simply dropped and the next
        // interval tries again.
        let preview_landed = self.collect_preview(ctx.device);

        // A readback in flight is the only thing that matters: the tile it
        // belongs to is done being rendered and the next one cannot start until
        // its buffer is back.
        if self.pending.is_some() {
            let step = self.collect(ctx);
            if step == StillStep::Tile {
                // The accumulation belonged to the tile that just finished, and
                // the next tile is a different part of the picture. Nothing
                // else would tell the backend that, and a tracer that kept it
                // would report itself already converged and render the previous
                // tile again.
                backend.invalidate();
            }
            // A preview never displaces the step that says a tile arrived or
            // that the job ended: those decide what a caller saves, and this
            // only decides what it shows.
            if step == StillStep::Working && preview_landed {
                return StillStep::Preview;
            }
            return step;
        }
        let Some(tile) = self.plan.tiles.get(self.tile).copied() else {
            self.finished = true;
            return StillStep::Done;
        };

        // The shared targets follow the tile, and the caller has already sized
        // them. Constant across a whole job, so a shell's own resize
        // early-returns after the first tile of a given size -- which is why
        // the job owns the frame while it runs rather than sharing it with a
        // viewport that would resize them back every frame.
        debug_assert_eq!(
            (ctx.renderer.target_width, ctx.renderer.target_height),
            (tile.render.width, tile.render.height),
            "the render targets are not sized to this tile"
        );
        let target = self.capture_target(ctx, tile);

        let outcome = self.encode_tile(ctx, backend, tile, &target);
        match outcome {
            FrameOutcome::Converging { samples, .. } => {
                self.samples_done = samples;
                // Only a tile that is still converging has anything to preview,
                // which is also what keeps a rasterized still from paying for
                // the mechanism: it completes on its first encode and never
                // reaches here, so it arms nothing and allocates nothing.
                if self.wants_preview(ctx.now_ms) {
                    self.arm_preview(ctx, backend, tile);
                }
                if preview_landed {
                    return StillStep::Preview;
                }
                StillStep::Working
            }
            FrameOutcome::Complete => {
                self.samples_done = self.target_samples();
                self.arm_readback(ctx, backend, tile, &target);
                if preview_landed {
                    return StillStep::Preview;
                }
                StillStep::Working
            }
        }
    }

    /// Whether the interval has passed and no preview is already in flight.
    ///
    /// One outstanding at a time: a second would queue behind the first without
    /// making the picture any fresher, and would hold a second tile-sized buffer
    /// mapped while it waited.
    ///
    /// The first is armed immediately rather than after one interval, which is
    /// deliberate: the complaint this answers is that a render shows nothing at
    /// the start, and waiting a quarter of a second before even asking would
    /// reintroduce a smaller version of it.
    fn wants_preview(&self, now_ms: u64) -> bool {
        let interval = self.spec.preview_interval_ms;
        if interval == 0 || self.preview.is_some() {
            return false;
        }
        self.preview_armed_ms
            .is_none_or(|then| now_ms.saturating_sub(then) >= interval)
    }

    /// Polls the preview slot. `true` when one landed on this call.
    fn collect_preview(&mut self, device: &wgpu::Device) -> bool {
        let Some(plane) = self.preview.as_mut() else {
            return false;
        };
        match plane.poll(device) {
            PlaneStep::Pending => false,
            PlaneStep::Failed => {
                // The render is unaffected: this buffer held a copy of a
                // picture that is still in the target. Drop it and let the next
                // interval try again rather than ending the job.
                self.preview = None;
                false
            }
            PlaneStep::Ready => {
                let Some(plane) = self.preview.take() else {
                    return false;
                };
                let Some(bytes) = plane.bytes else {
                    return false;
                };
                let Some(tile) = self.plan.tiles.get(self.tile).copied() else {
                    return false;
                };
                self.preview_ready = Some(StillPreview {
                    rect: tile.image,
                    pixels: crop(&bytes, tile, PREVIEW_BYTES_PER_PIXEL),
                });
                true
            }
        }
    }

    /// Allocates or reuses the tile-sized capture target.
    fn capture_target(&mut self, ctx: &StillCtx<'_>, tile: Tile) -> CaptureTarget {
        let fits = self
            .target
            .as_ref()
            .is_some_and(|t| t.width == tile.render.width && t.height == tile.render.height);
        if !fits {
            self.target = Some(CaptureTarget::new(
                ctx.device,
                self.spec.readback.source_format(ctx.format),
                tile.render.width,
                tile.render.height,
            ));
        }
        // Cloned handles rather than a borrow: the caller needs `self` mutably
        // while this is alive, and every field of it is a refcounted handle.
        let t = self.target.as_ref().expect("just allocated");
        CaptureTarget {
            texture: t.texture.clone(),
            view: t.view.clone(),
            rect: t.rect,
            width: t.width,
            height: t.height,
        }
    }

    /// Allocates or reuses the tile-sized eight-bit preview target.
    ///
    /// Its own target rather than the capture one, for a reason the
    /// scene-referred mode makes plain: that mode never composites, so its
    /// capture target is never written and there would be nothing to preview
    /// from. One target in the surface's own format gives every mode the same
    /// path and the cheapest readback there is.
    fn preview_target(&mut self, ctx: &StillCtx<'_>, tile: Tile) -> CaptureTarget {
        let fits = self
            .preview_target
            .as_ref()
            .is_some_and(|t| t.width == tile.render.width && t.height == tile.render.height);
        if !fits {
            self.preview_target = Some(CaptureTarget::new(
                ctx.device,
                ctx.format,
                tile.render.width,
                tile.render.height,
            ));
        }
        let t = self.preview_target.as_ref().expect("just allocated");
        CaptureTarget {
            texture: t.texture.clone(),
            view: t.view.clone(),
            rect: t.rect,
            width: t.width,
            height: t.height,
        }
    }

    /// Composites what has accumulated so far and copies it out.
    ///
    /// The running mean is already in the shared target: the traced backend
    /// resolves into it on every encode rather than only at completion, so
    /// nothing has to be computed for a preview to exist and this only fetches
    /// it. What it adds is one composite pass, four times a second, which is
    /// what turns a scene-referred or floating-point render into something a
    /// screen can show.
    ///
    /// A scene-referred render is previewed through the display chain, which is
    /// the one place a preview and its finished tile differ: the file stays
    /// scene-referred and untouched, and a screen cannot show scene-referred
    /// light at all.
    fn arm_preview(&mut self, ctx: &mut StillCtx<'_>, backend: &mut dyn RenderBackend, tile: Tile) {
        let target = self.preview_target(ctx, tile);
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Still Preview Encoder"),
            });
        let bloom = ctx.renderer.post.bloom_enabled && ctx.scene_present;
        let ssao =
            ctx.renderer.post.ssao_enabled && ctx.scene_present && backend.caps().writes_occlusion;
        ctx.renderer.post.composite.write_params(
            ctx.queue,
            bloom,
            ssao,
            &ctx.look,
            &ctx.renderer.post.luts,
            InspectionMode::Shaded,
            self.spec.transparent,
        );
        let r = target.rect;
        ctx.renderer.post.composite.render(
            &mut encoder,
            &ctx.renderer.pipelines,
            &target.view,
            ssao,
            &ctx.renderer.post.ssao,
            Some([r.x, r.y, r.width, r.height]),
            true,
            // Never the float pipeline: a preview is eight bits by definition.
            None,
        );
        let capture = solarxy_renderer::capture::encode_capture(
            ctx.device,
            &mut encoder,
            &target.texture,
            (0, 0, target.width, target.height),
        );
        // Submitted before the buffer is mapped, for the same reason the tile's
        // readback is: a copy into a mapped buffer is rejected.
        ctx.queue.submit(std::iter::once(encoder.finish()));
        let (buffer, padded) = capture;
        self.preview = Some(PendingPlane::arm(
            PendingCapture::arm(buffer, padded, target.width, target.height),
            ctx.format,
            false,
        ));
        self.preview_armed_ms = Some(ctx.now_ms);
    }

    fn encode_tile(
        &self,
        ctx: &mut StillCtx<'_>,
        backend: &mut dyn RenderBackend,
        tile: Tile,
        target: &CaptureTarget,
    ) -> FrameOutcome {
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Still Tile Encoder"),
            });
        let cam_data = ctx.camera.camera;
        let hdr = ctx.renderer.targets.hdr_resolve_view.clone();
        #[allow(clippy::cast_precision_loss)]
        let rect = solarxy_renderer::panes::PaneRect {
            x: 0.0,
            y: 0.0,
            width: tile.render.width as f32,
            height: tile.render.height as f32,
        };
        let outcome = backend.encode(
            &mut FrameCtx {
                device: ctx.device,
                queue: ctx.queue,
                renderer: ctx.renderer,
                encoder: &mut encoder,
                // A still is one pane on its own, and it is the pane that
                // clears, exactly as a screenshot is.
                index: 0,
                rect,
                is_split: false,
                pds: ctx.pds,
                display: ctx.display,
                // The film back the render was authored with wins over the
                // viewer's background: this is what "transparent" means to the
                // rasterizer, and the tracer ignores the field either way, so
                // the substitution asks neither backend what it is.
                background: if self.spec.transparent {
                    solarxy_core::preferences::ResolvedBackground::TRANSPARENT
                } else {
                    ctx.background
                },
                camera: Some(ctx.camera),
                env: ctx.env,
                bounds: ctx.bounds,
                grid_plane: None,
                look: ctx.look,
                scene_present: ctx.scene_present,
                // A still never carries the selection rim.
                outline: false,
                window: Some(ImageWindow {
                    origin: [tile.render.x, tile.render.y],
                    full: [self.spec.width, self.spec.height],
                }),
                content: PaneContent::Scene {
                    extra: None,
                    selected: None,
                    cam_data,
                    shadow: true,
                },
            },
            &hdr,
        );

        // Composited here rather than through `composite_and_submit`: a capture
        // always clears, uses a full-rect viewport rather than a pane rect, and
        // carries no selection rim. Those three are the whole difference, and
        // they are the same three the web shell's screenshot path names.
        // Scene-referred output skips the chain entirely rather than running it
        // with a neutral look: "neutral" would still be the tone map's idea of
        // neutral, and the point of this mode is that nothing has been decided
        // yet.
        if !self.spec.readback.composites() {
            ctx.queue.submit(std::iter::once(encoder.finish()));
            return outcome;
        }
        // Cloned rather than borrowed: building it needs the pipeline set
        // mutably, and compositing needs it immutably a line later. The handle
        // is refcounted, so this is a pointer.
        let float_pipeline = (self.spec.readback == StillReadback::DisplayFloat).then(|| {
            ctx.renderer
                .pipelines
                .post
                .float_composite(ctx.device)
                .clone()
        });
        let bloom = ctx.renderer.post.bloom_enabled && ctx.scene_present;
        // Asked of the backend that just drew the tile, because the buffer the
        // chain would multiply by is only filled by one that runs the prepass.
        // Bloom needs no such question: it reads the colour.
        let ssao =
            ctx.renderer.post.ssao_enabled && ctx.scene_present && backend.caps().writes_occlusion;
        ctx.renderer.post.composite.write_params(
            ctx.queue,
            bloom,
            ssao,
            &ctx.look,
            &ctx.renderer.post.luts,
            InspectionMode::Shaded,
            self.spec.transparent,
        );
        let r = target.rect;
        ctx.renderer.post.composite.render(
            &mut encoder,
            &ctx.renderer.pipelines,
            &target.view,
            ssao,
            &ctx.renderer.post.ssao,
            Some([r.x, r.y, r.width, r.height]),
            true,
            float_pipeline.as_ref(),
        );
        ctx.queue.submit(std::iter::once(encoder.finish()));
        outcome
    }

    /// Copies the finished tile out, and whatever auxiliary planes were asked
    /// for beside it.
    ///
    /// Every copy goes into one encoder and one submission. That matters for
    /// the depth plane, which is a dispatch rather than a copy: its uniforms
    /// are written through the queue, and a queue write applies to the whole
    /// submission it lands in rather than at the point in the command stream
    /// where it was issued, so two tiles' depth parameters batched before one
    /// submit would both read the second tile's.
    fn arm_readback(
        &mut self,
        ctx: &StillCtx<'_>,
        backend: &mut dyn RenderBackend,
        tile: Tile,
        target: &CaptureTarget,
    ) {
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Still Tile Readback"),
            });
        // Scene-referred output is read out of the high-dynamic-range target
        // the passes wrote into, which is what "before the finishing chain"
        // literally means. Every other mode reads the capture target the chain
        // composited into.
        let source = if self.spec.readback.composites() {
            &target.texture
        } else {
            &ctx.renderer.targets.hdr_resolve_texture
        };
        let (buffer, padded) = solarxy_renderer::capture::encode_capture(
            ctx.device,
            &mut encoder,
            source,
            (0, 0, target.width, target.height),
        );
        let color = (buffer, padded);

        // The auxiliary first, because it only borrows the backend, and the
        // depth dispatch below needs it mutably.
        let mut aux = None;
        if self.spec.aux {
            // Pane zero: a still encodes one pane, and `encode_tile` says so.
            if let Some(sources) = backend.aov_sources(0) {
                aux = Some(solarxy_renderer::capture::encode_capture(
                    ctx.device,
                    &mut encoder,
                    sources.auxiliary,
                    (0, 0, target.width, target.height),
                ));
            }
        }

        let mut depth = None;
        if self.spec.depth {
            let window = Some(ImageWindow {
                origin: [tile.render.x, tile.render.y],
                full: [self.spec.width, self.spec.height],
            });
            if let Some(texture) = backend.encode_depth_aov(
                ctx.device,
                ctx.queue,
                &mut encoder,
                ctx.camera,
                [target.width, target.height],
                window,
            ) {
                depth = Some(solarxy_renderer::capture::encode_capture(
                    ctx.device,
                    &mut encoder,
                    texture,
                    (0, 0, target.width, target.height),
                ));
            }
        }

        // Submitted before anything is armed, and that order is the whole of
        // it: arming maps the staging buffer, and submitting a copy *into* a
        // mapped buffer is rejected. With one plane the two steps were adjacent
        // and the order looked like a formality.
        ctx.queue.submit(std::iter::once(encoder.finish()));
        let arm = |(buffer, padded): (wgpu::Buffer, u32)| {
            PendingCapture::arm(buffer, padded, target.width, target.height)
        };
        self.pending = Some(PendingTile {
            color: PendingPlane::arm(
                arm(color),
                self.spec.readback.source_format(ctx.format),
                self.spec.readback != StillReadback::Display8,
            ),
            aux: aux.map(|c| PendingPlane::arm(arm(c), wgpu::TextureFormat::Rgba32Float, true)),
            depth: depth.map(|c| PendingPlane::arm(arm(c), wgpu::TextureFormat::R32Float, true)),
        });
    }

    /// Polls every plane this tile is waiting on, and assembles the tile once
    /// the slowest of them has landed.
    fn collect(&mut self, ctx: &StillCtx<'_>) -> StillStep {
        let (mut waiting, mut failed) = (false, false);
        let Some(pending) = self.pending.as_mut() else {
            return StillStep::Working;
        };
        for plane in pending.planes_mut() {
            match plane.poll(ctx.device) {
                PlaneStep::Pending => waiting = true,
                PlaneStep::Ready => {}
                PlaneStep::Failed => failed = true,
            }
        }
        if failed {
            self.pending = None;
            self.finished = true;
            return StillStep::Failed;
        }
        if waiting {
            return StillStep::Working;
        }
        let Some(planes) = self.pending.take().and_then(PendingTile::into_planes) else {
            self.finished = true;
            return StillStep::Failed;
        };

        let tile = self.plan.tiles[self.tile];
        self.ready.push_back(StillTile {
            rect: tile.image,
            pixels: crop(&planes.color, tile, self.spec.readback.bytes_per_pixel()),
            aux: planes.aux.map(|a| crop(&a, tile, AUX_BYTES_PER_PIXEL)),
            depth: planes.depth.map(|d| crop(&d, tile, DEPTH_BYTES_PER_PIXEL)),
        });
        // Anything the preview slot holds describes the tile that just
        // finished, at fewer samples than the tile now in hand. Painting it
        // afterwards would undo the picture, and a preview still in flight
        // would land against the next tile's rect. Both are dropped here, which
        // also frees the buffer.
        self.preview = None;
        self.preview_ready = None;
        self.tile += 1;
        self.samples_done = 0;
        if self.tile >= self.plan.len() {
            self.finished = true;
        }
        StillStep::Tile
    }
}

/// Cuts the apron off a rendered tile, leaving the part it owns.
///
/// A copy rather than a view, because the result crosses a boundary that does
/// not carry strides.
fn crop(pixels: &[u8], tile: Tile, bytes_per_pixel: usize) -> Vec<u8> {
    if tile.render == tile.image {
        return pixels.to_vec();
    }
    let [ox, oy] = tile.crop();
    let stride = tile.render.width as usize * bytes_per_pixel;
    let row = tile.image.width as usize * bytes_per_pixel;
    let mut out = Vec::with_capacity(row * tile.image.height as usize);
    for y in 0..tile.image.height as usize {
        let start = (oy as usize + y) * stride + ox as usize * bytes_per_pixel;
        out.extend_from_slice(&pixels[start..start + row]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(width: u32, height: u32, apron: u32) -> TilePlan {
        TilePlan::new(width, height, TILE_BUDGET_PIXELS, apron)
    }

    /// The property an assembled picture depends on: every pixel belongs to
    /// exactly one tile.
    ///
    /// A gap is a black band and an overlap is a double-composited seam, and
    /// both are the kind of thing that looks like a rendering bug rather than
    /// an arithmetic one.
    #[test]
    fn the_tiles_cover_the_image_exactly_once() {
        for (w, h) in [
            (4096, 2304),
            (8192, 8192),
            (1920, 1080),
            (16, 16),
            // Deliberately not a multiple of anything.
            (3001, 1777),
        ] {
            for apron in [0, TILE_APRON_PIXELS] {
                let p = plan(w, h, apron);
                let mut covered = 0u64;
                for tile in &p.tiles {
                    covered += tile.image.area();
                    assert!(
                        tile.render.area() <= u64::from(TILE_BUDGET_PIXELS),
                        "{w}x{h} apron {apron}: a tile renders {} pixels, over the budget",
                        tile.render.area()
                    );
                    assert!(
                        tile.image.x + tile.image.width <= w
                            && tile.image.y + tile.image.height <= h,
                        "a tile runs outside the image"
                    );
                }
                assert_eq!(
                    covered,
                    u64::from(w) * u64::from(h),
                    "{w}x{h} apron {apron}: the tiles cover {covered} of {} pixels",
                    u64::from(w) * u64::from(h)
                );
            }
        }
    }

    #[test]
    fn a_small_image_is_one_tile_with_no_apron_to_crop() {
        let p = plan(800, 600, TILE_APRON_PIXELS);
        assert_eq!(p.len(), 1);
        let tile = p.tiles[0];
        // Nothing outside the picture to bleed in from, so the apron collapses
        // and the render rect is the image rect.
        assert_eq!(tile.render, tile.image);
        assert_eq!(tile.crop(), [0, 0]);
    }

    #[test]
    fn an_interior_tile_renders_its_apron_and_crops_it_back_off() {
        let p = plan(8192, 8192, TILE_APRON_PIXELS);
        let interior = p
            .tiles
            .iter()
            .find(|t| t.image.x > 0 && t.image.y > 0)
            .expect("an 8192 square image has interior tiles");
        assert_eq!(interior.crop(), [TILE_APRON_PIXELS, TILE_APRON_PIXELS]);
        assert!(interior.render.width > interior.image.width);
        assert!(interior.render.height > interior.image.height);
    }

    #[test]
    fn cropping_recovers_the_owned_rectangle() {
        // A tile whose rendered pixels are a known ramp, so a crop that took
        // the wrong window would produce visibly wrong values rather than the
        // right count of wrong ones.
        let tile = Tile {
            image: TileRect {
                x: 10,
                y: 10,
                width: 2,
                height: 2,
            },
            render: TileRect {
                x: 8,
                y: 8,
                width: 6,
                height: 6,
            },
        };
        let mut pixels = Vec::new();
        for y in 0..6u8 {
            for x in 0..6u8 {
                pixels.extend_from_slice(&[x, y, 0, 255]);
            }
        }
        let out = crop(&pixels, tile, 4);
        assert_eq!(out.len(), 2 * 2 * 4);
        // The owned rect starts two in and two down.
        assert_eq!(&out[0..4], &[2, 2, 0, 255]);
        assert_eq!(&out[4..8], &[3, 2, 0, 255]);
        assert_eq!(&out[8..12], &[2, 3, 0, 255]);
    }

    #[test]
    fn a_request_larger_than_the_cap_is_clamped_rather_than_refused() {
        let spec = StillSpec {
            width: 20000,
            height: 4,
            engine: StillEngine::PathTraced,
            samples: 0,
            ..StillSpec::default()
        }
        .clamped();
        assert_eq!(spec.width, MAX_STILL_EDGE);
        assert_eq!(spec.height, 16);
        // Zero samples would be a render of nothing at all.
        assert_eq!(spec.samples, 1);
    }

    #[test]
    fn the_raster_engine_converges_in_one_pass() {
        let job = StillRenderJob::new(StillSpec {
            width: 640,
            height: 480,
            engine: StillEngine::Raster,
            samples: 256,
            ..StillSpec::default()
        });
        // The sample count is a path tracer's control. A rasterized pixel is
        // drawn once, and reporting 256 would leave a progress bar at one part
        // in 256 forever.
        assert_eq!(job.progress().samples, 1);
    }

    /// An estimate appears only when there is something to estimate from, and
    /// says nothing rather than something wrong.
    #[test]
    fn an_estimate_waits_until_it_has_a_rate() {
        assert_eq!(estimate_remaining_ms(0, 100, 1000), None, "nothing drawn");
        assert_eq!(estimate_remaining_ms(50, 100, 0), None, "no time reported");
        assert_eq!(estimate_remaining_ms(100, 100, 1000), None, "finished");
        assert_eq!(estimate_remaining_ms(150, 100, 1000), None, "past the end");
        assert_eq!(estimate_remaining_ms(50, 0, 1000), None, "nothing to draw");

        // A quarter drawn in one second has three seconds left.
        assert_eq!(estimate_remaining_ms(25, 100, 1000), Some(3000));
        // Half drawn in ten seconds has ten left.
        assert_eq!(estimate_remaining_ms(1, 2, 10_000), Some(10_000));
    }

    /// The estimate is weighted by area, so a render whose last tiles are
    /// smaller does not report time it will not take.
    ///
    /// The arithmetic is the same either way for equal tiles; what this pins is
    /// that the weights are pixel-samples rather than tile counts, which is the
    /// difference at exactly the moment a person is watching the end.
    #[test]
    fn the_estimate_weights_a_tile_by_its_area() {
        // Three tiles: two whole ones and a narrow remainder. Counting tiles
        // equally would call this two thirds done; by area it is more.
        let big = 100 * 100u64;
        let small = 20 * 100u64;
        let total = big * 2 + small;
        let drawn = big * 2;
        let by_area = estimate_remaining_ms(drawn, total, 2000).expect("an estimate");

        // Two of three tiles in two seconds reads as one more second.
        let by_tile_count = estimate_remaining_ms(2, 3, 2000).expect("an estimate");
        assert!(
            by_area < by_tile_count,
            "an area-weighted estimate should be shorter than a tile-counted one \
             when the tile left over is the small one: {by_area}ms vs {by_tile_count}ms"
        );
        // Two hundred of twenty-two thousand pixel-samples remain, at ten
        // thousand a second.
        assert_eq!(by_area, 200);
    }

    /// One spelling of a span, whatever surface shows it.
    #[test]
    fn a_span_reads_the_same_way_everywhere() {
        assert_eq!(format_duration_ms(0), "0.0s");
        assert_eq!(format_duration_ms(1500), "1.5s");
        assert_eq!(format_duration_ms(59_900), "59.9s");
        assert_eq!(format_duration_ms(60_000), "1m 00s");
        assert_eq!(format_duration_ms(252_000), "4m 12s");
        assert_eq!(format_duration_ms(3_599_000), "59m 59s");
        assert_eq!(format_duration_ms(3_600_000), "1h 00m");
        assert_eq!(format_duration_ms(3_840_000), "1h 04m");
    }

    #[test]
    fn a_format_and_a_space_name_one_readback() {
        assert_eq!(
            super::readback_for("png", "sceneLinear"),
            StillReadback::Display8
        );
        // Space is meaningless for an eight-bit image and is ignored rather
        // than refused, because the caller is a dialog that always has one.
        assert_eq!(
            super::readback_for("png", "display"),
            StillReadback::Display8
        );
        assert_eq!(
            super::readback_for("exr", "sceneLinear"),
            StillReadback::SceneLinear
        );
        assert_eq!(
            super::readback_for("exr", "display"),
            StillReadback::DisplayFloat
        );
        // Scene-referred is the default a float image falls back to, matching
        // the command line, so an unknown word cannot silently apply a look.
        assert_eq!(
            super::readback_for("EXR", "nonsense"),
            StillReadback::SceneLinear
        );
    }

    #[test]
    fn a_float_preview_clamps_and_encodes_the_way_the_watch_window_does() {
        // Black, mid grey, white and an over-range value, as four pixels.
        let mut bytes = Vec::new();
        for px in [
            [0.0f32, 0.0, 0.0, 1.0],
            [0.5, 0.5, 0.5, 1.0],
            [1.0, 1.0, 1.0, 1.0],
            [8.0, 8.0, 8.0, 1.0],
        ] {
            for c in px {
                bytes.extend_from_slice(&c.to_le_bytes());
            }
        }
        let out = super::float_to_rgba8(&bytes);
        assert_eq!(out.len(), 16, "four bytes a pixel, opaque");
        assert_eq!(&out[0..4], &[0, 0, 0, 255]);
        // 0.5 linear is 188 through the sRGB transfer, not 128. A preview that
        // wrote 128 would be showing the image twice as dark as its file.
        assert_eq!(&out[4..8], &[188, 188, 188, 255]);
        assert_eq!(&out[8..12], &[255, 255, 255, 255]);
        // Over range clamps rather than wrapping: the clamp is the whole of
        // the tone mapping here, which is what the surfaces tell the reader.
        assert_eq!(&out[12..16], &[255, 255, 255, 255]);
    }

    #[test]
    fn the_float_preview_is_the_low_end_of_the_curve_too() {
        // The linear segment below the knee, which a pure power curve gets
        // visibly wrong in the darks.
        let mut bytes = Vec::new();
        for c in [0.001_f32, 0.001, 0.001, 1.0] {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
        let out = super::float_to_rgba8(&bytes);
        assert_eq!(out[0], 3, "0.001 linear is 3, through the linear segment");
    }

    /// The preview carries a fractional matte straight through: coverage is a
    /// fraction rather than light, so it quantizes linearly, with no transfer
    /// curve.
    #[test]
    fn the_float_preview_carries_the_matte_linearly() {
        let mut bytes = Vec::new();
        for c in [0.5_f32, 0.5, 0.5, 0.5] {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
        let out = super::float_to_rgba8(&bytes);
        assert_eq!(out[3], 128, "half coverage is 128, not 188");
    }

    /// A tile's floats, as the readback delivers them.
    fn tile_bytes(pixels: &[[f32; 4]]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for px in pixels {
            for c in px {
                bytes.extend_from_slice(&c.to_le_bytes());
            }
        }
        bytes
    }

    /// A display-referred matte tile lands as it arrived: the composite
    /// already divided the coverage out before its nonlinear chain.
    #[test]
    fn a_display_referred_matte_tile_lands_unassociated_as_it_arrived() {
        let mut image = FloatImage::new(StillReadback::DisplayFloat, 2, 1, true).expect("float");
        assert!(image.has_matte());
        let tile = TileRect {
            x: 0,
            y: 0,
            width: 2,
            height: 1,
        };
        image.place(
            tile,
            &tile_bytes(&[[0.8, 0.6, 0.4, 0.5], [0.2, 0.2, 0.2, 0.0]]),
        );
        assert_eq!(image.rgba(), &[0.8, 0.6, 0.4, 0.5, 0.2, 0.2, 0.2, 0.0]);
    }

    /// A scene-referred matte tile arrives coverage-weighted out of the
    /// accumulator and is divided out on landing, so everything on this side
    /// of the readback speaks unassociated colour plus a fraction; the guard
    /// leaves an uncovered pixel's lanes alone, and its zero matte is what
    /// clips them.
    #[test]
    fn a_scene_referred_matte_tile_is_unassociated_on_landing() {
        let mut image = FloatImage::new(StillReadback::SceneLinear, 2, 1, true).expect("float");
        let tile = TileRect {
            x: 0,
            y: 0,
            width: 2,
            height: 1,
        };
        image.place(
            tile,
            &tile_bytes(&[[0.4, 0.3, 0.2, 0.5], [0.1, 0.1, 0.1, 0.0]]),
        );
        let got = image.rgba();
        for (i, want) in [0.8, 0.6, 0.4, 0.5].iter().enumerate() {
            assert!((got[i] - want).abs() < 1e-6, "channel {i}");
        }
        assert_eq!(&got[4..8], &[0.1, 0.1, 0.1, 0.0]);
    }

    /// The opaque image is unchanged by the matte's existence: three channels,
    /// the constant lane dropped on landing.
    #[test]
    fn an_opaque_float_image_stays_three_channels() {
        let mut image = FloatImage::new(StillReadback::SceneLinear, 2, 1, false).expect("float");
        assert!(!image.has_matte());
        let tile = TileRect {
            x: 0,
            y: 0,
            width: 2,
            height: 1,
        };
        image.place(
            tile,
            &tile_bytes(&[[0.4, 0.3, 0.2, 1.0], [0.1, 0.2, 0.3, 1.0]]),
        );
        assert_eq!(image.rgb(), &[0.4, 0.3, 0.2, 0.1, 0.2, 0.3]);
    }
}
