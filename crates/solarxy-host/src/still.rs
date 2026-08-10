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
    /// Tightly packed RGBA8, `rect.width * rect.height * 4` bytes, apron
    /// already cropped away.
    pub pixels: Vec<u8>,
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
    pending: Option<PendingCapture>,
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
                self.arm_readback(ctx, &target);
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
                ctx.format,
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
        let bloom = ctx.renderer.post.bloom_enabled && ctx.scene_present;
        let ssao = ctx.renderer.post.ssao_enabled && ctx.scene_present;
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
        );
        ctx.queue.submit(std::iter::once(encoder.finish()));
        outcome
    }

    fn arm_readback(&mut self, ctx: &StillCtx<'_>, target: &CaptureTarget) {
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Still Tile Readback"),
            });
        let (buffer, padded) = solarxy_renderer::capture::encode_capture(
            ctx.device,
            &mut encoder,
            &target.texture,
            (0, 0, target.width, target.height),
        );
        ctx.queue.submit(std::iter::once(encoder.finish()));
        self.pending = Some(PendingCapture::arm(
            buffer,
            padded,
            target.width,
            target.height,
        ));
    }

    fn collect(&mut self, ctx: &StillCtx<'_>) -> StillStep {
        let Some(pending) = self.pending.as_ref() else {
            return StillStep::Working;
        };
        match pending.poll(ctx.device, ctx.format) {
            CapturePoll::Pending => StillStep::Working,
            CapturePoll::Failed => {
                self.pending = None;
                self.finished = true;
                StillStep::Failed
            }
            CapturePoll::Ready(pixels) => {
                self.pending = None;
                let tile = self.plan.tiles[self.tile];
                self.ready.push_back(StillTile {
                    rect: tile.image,
                    pixels: crop(&pixels, tile),
                });
                self.tile += 1;
                self.samples_done = 0;
                if self.tile >= self.plan.len() {
                    self.finished = true;
                }
                StillStep::Tile
            }
        }
    }
}

/// Cuts the apron off a rendered tile, leaving the part it owns.
///
/// A copy rather than a view, because the result crosses a boundary that does
/// not carry strides.
fn crop(pixels: &[u8], tile: Tile) -> Vec<u8> {
    if tile.render == tile.image {
        return pixels.to_vec();
    }
    let [ox, oy] = tile.crop();
    let stride = tile.render.width as usize * 4;
    let row = tile.image.width as usize * 4;
    let mut out = Vec::with_capacity(row * tile.image.height as usize);
    for y in 0..tile.image.height as usize {
        let start = (oy as usize + y) * stride + ox as usize * 4;
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
        let out = crop(&pixels, tile);
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
}
