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
/// `bytes` is four `f32` a pixel; the result is four bytes a pixel, opaque.
#[must_use]
pub fn float_to_rgba8(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() / 4);
    for px in bytes.chunks_exact(16) {
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
        out.push(255);
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
}

/// What one [`StillRenderJob::advance`] did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StillStep {
    /// Still working. Call again next frame.
    Working,
    /// A tile finished; take it with [`StillRenderJob::take_tile`].
    Tile,
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
        StillProgress {
            tile: u32::try_from(self.tile).unwrap_or(u32::MAX),
            tiles: u32::try_from(self.plan.len()).unwrap_or(u32::MAX),
            sample: self.samples_done,
            samples: self.target_samples(),
        }
    }

    /// A finished tile, if one is waiting.
    pub fn take_tile(&mut self) -> Option<StillTile> {
        self.ready.pop_front()
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
                StillStep::Working
            }
            FrameOutcome::Complete => {
                self.samples_done = self.target_samples();
                self.arm_readback(ctx, backend, tile, &target);
                StillStep::Working
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
                background: ctx.background,
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
}
