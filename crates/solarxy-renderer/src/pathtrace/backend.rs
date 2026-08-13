//! The path tracer behind the render backend contract.
//!
//! The first thing in this release that a host can hold and drive without
//! knowing it is holding a tracer. Everything under it -- the arena, the atlas,
//! the hierarchy cache, the kernel -- existed before this and had no caller;
//! this is the caller.
//!
//! # What a progressive backend has to do differently
//!
//! One [`RenderBackend::encode`] does not produce a finished pane. It draws a
//! chunk of samples, folds them into a running mean and reports
//! [`FrameOutcome::Converging`] until the mean has all the samples it was asked
//! for. A host reads that and keeps scheduling frames; it does not have to know
//! why.
//!
//! The other half of the contract is [`RenderBackend::invalidate`], which is
//! how everything a mean is no longer valid over -- a moved camera, an edited
//! parameter, a cooked scene -- reaches an accumulator that has no idea any of
//! those things exist.
//!
//! # What it deliberately does not draw
//!
//! No background, no grid, no axis gizmo, no bounds, no validation overlay.
//! Those are raster passes over a depth buffer this path does not produce, and
//! a traced image is what the tracer integrates: the environment where a ray
//! left, and nothing where a viewport would have drawn furniture. The pane's
//! background mode does not apply to it, which is a product decision recorded
//! in the render node's help rather than an omission.
//!
//! # Per pane, inside the backend
//!
//! A host holds one of these however many panes it shows, because the scene is
//! per session. The accumulator is not: it is keyed by [`FrameCtx::index`],
//! which is the arrangement the trait's documentation sets out and the reason
//! `encode` takes a pane index at all.

use solarxy_core::scene::SceneDelta;

use crate::backend::{BackendCaps, FrameCtx, FrameOutcome, PaneContent, RenderBackend, TopologyMask};
use crate::bind_groups::PathtraceLayouts;
use crate::pathtrace::denoise::{DenoiseSettings, Denoiser};
use crate::pathtrace::depth::{DepthPass, DepthTarget};
use crate::pathtrace::environment::TraceEnvironment;
use crate::pathtrace::resolve::TraceResolve;
use crate::pathtrace::scene::TraceSceneCache;
use crate::pathtrace::{
    EnvParams, PathEstimator, PathKernel, PathUniforms, TraceAtlas, TraceParams, TraceScene,
    TraceTarget, TraceUniforms,
};

/// The most panes a layout can show at once, and so the width of any per-pane
/// array a backend keeps. Mirrors the rasterizer's.
const PANE_SLOTS: usize = 4;

/// What a render is, as opposed to what the scene is.
///
/// Everything here is authored: it comes from the render node in a document or
/// from a shell's defaults, and none of it is derived from the geometry. Kept
/// as one struct so a host sets a render up in one call and so the fields a
/// still and a preview disagree about are visible side by side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TraceSettings {
    /// Samples per pixel the accumulation converges to.
    pub samples: u32,
    /// How many of them one [`RenderBackend::encode`] draws.
    ///
    /// The pacing control, and the only thing standing between a large image
    /// and a lost device: every dispatch is bounded by this times the pane's
    /// pixels, whatever the sample count asks for.
    pub chunk: u32,
    /// Scattering events a path may have.
    pub bounces: u32,
    /// How many of those may additionally be transmissive.
    pub transmissive_bounces: u32,
    /// The luminance one sample's indirect contribution may reach. See
    /// [`TraceParams::firefly_clamp`].
    pub firefly_clamp: f32,
    /// Fixed, so two runs of the same scene produce the same image.
    pub seed: u32,
    /// The aperture's radius in world units, already resolved out of the
    /// camera's f-number. Zero is a pinhole.
    pub aperture_radius: f32,
    /// How far in front of the camera is sharp, in world units.
    pub focus_distance: f32,
    /// Aperture blades. Zero, one and two are circular.
    pub aperture_blades: u32,
    /// Whether the edge-aware filter runs before the resolve.
    ///
    /// **Off by default, which is the still's default.** A converged still
    /// does not need it and a filter can only take detail out of one. The
    /// interactive preview (the per-pane traced display mode) turns it on,
    /// because a one-sample frame is unusable without it.
    pub denoise: bool,
    /// The accumulator's size as a fraction of the pane's, `(0, 1]`.
    ///
    /// One for a still, which must land every pixel it was asked for. The
    /// interactive preview trades resolution for convergence: at one half,
    /// a sample costs a quarter of the rays and the resolve upscales the
    /// mean into the pane-sized target. Never applied to a windowed
    /// render, whose tile is a window on a picture whose size is authored.
    pub resolution_scale: f32,
}

impl Default for TraceSettings {
    fn default() -> Self {
        Self {
            samples: 64,
            chunk: 1,
            bounces: 6,
            transmissive_bounces: 4,
            firefly_clamp: DEFAULT_FIREFLY_CLAMP,
            seed: 0x9E37_79B9,
            aperture_radius: 0.0,
            focus_distance: 0.0,
            aperture_blades: 0,
            denoise: false,
            resolution_scale: 1.0,
        }
    }
}

/// The luminance one sample's indirect contribution may reach before it is
/// scaled back to it.
///
/// Sixteen, which is well above anything a surface returns under an authored
/// environment of unit brightness and well below what a near-specular scatter
/// onto a small bright source charges. It is not authored: a control whose
/// effect is "how much energy would you like removed from the parts of the
/// image you cannot predict" is not one a person can reason about, and the
/// render node carries no parameter for it.
pub const DEFAULT_FIREFLY_CLAMP: f32 = 16.0;

/// One pane's accumulation.
struct PaneAccumulator {
    target: TraceTarget,
    uniforms: PathUniforms,
    /// How many samples the mean already averages. Zero means the read slot
    /// holds nothing.
    samples: u32,
    width: u32,
    height: u32,
    /// The camera buffer the uniforms bind, so a pane whose camera slot is
    /// replaced rebinds rather than writing into a buffer nobody reads.
    ///
    /// A handle rather than a flag, because `wgpu::Buffer` compares by the
    /// allocation it names rather than by its description: two cameras of the
    /// same size are two buffers, and a bind group built over the wrong one
    /// keeps it alive and shows its view.
    camera: wgpu::Buffer,
}

/// The path tracer, as a host drives it.
pub struct PathBackend {
    layouts: PathtraceLayouts,
    kernel: PathKernel,
    resolve: TraceResolve,
    /// Allocates nothing until the filter is switched on, so a still with it
    /// off pays neither the scratch nor the dispatches.
    denoiser: Denoiser,
    /// The CPU half: what the document says, hierarchies included.
    cache: TraceSceneCache,
    /// The GPU half: the arena the kernel binds.
    scene: TraceScene,
    /// The sampled group: the texture atlas and the environment.
    atlas: TraceAtlas,
    panes: [Option<PaneAccumulator>; PANE_SLOTS],
    /// The depth pass and the uniforms it dispatches against, built the first
    /// time one is asked for.
    ///
    /// Lazy because a render that wants no depth should pay nothing for it, and
    /// what it would otherwise pay is a shader module and a pipeline at every
    /// session's first frame. The uniforms are its own rather than a pane's:
    /// the parameters differ, since a depth ray has no aperture and no jitter,
    /// and writing them into a pane's buffer would leave that pane's next
    /// dispatch reading them.
    depth: Option<(DepthPass, TraceUniforms, wgpu::Buffer)>,
    /// The target [`RenderBackend::encode_depth_aov`] writes into, reallocated
    /// when the size changes.
    ///
    /// One, not one per pane, because the only caller renders a single pane at
    /// a time and a depth is not accumulated: there is nothing in it to keep
    /// between calls. What is genuinely per pane is the mean, and that lives in
    /// [`Self::panes`].
    depth_target: Option<DepthTarget>,
    settings: TraceSettings,
    /// The sky the kernel integrates against when no environment image is
    /// installed. Looking up, then looking down.
    sky: ([f32; 3], [f32; 3]),
    env_intensity: f32,
    env_rotation: f32,
}

impl PathBackend {
    /// What this backend can do, as a constant.
    ///
    /// A constant rather than only a method for the reason the rasterizer's is:
    /// the answer reads no GPU state, and a caller checking whether an option it
    /// was handed can take effect should not have to create a device to find
    /// out. The method returns this.
    pub const CAPS: BackendCaps = BackendCaps {
        // Repeated frames of an unchanged pane keep improving it, which is
        // the whole difference between this backend and the other one.
        progressive: true,
        // A count rather than a uniform's capacity. The rasterizer's eight
        // is the size of a struct; this reads a storage array and is bound
        // by nothing a user will reach.
        max_lights: None,
        supports_instancing: true,
        // Points and lines are not surfaces and the traversal has no
        // primitive for them. A host states that rather than letting a
        // point cloud silently vanish.
        supports_topology: TopologyMask::TRIANGLES,
        writes_aovs: true,
        // Occlusion is already in the image: a path that leaves a crevice is
        // one the traversal blocked. There is no prepass and nothing fills
        // the buffer, so the finishing chain must not reach for it.
        writes_occlusion: false,
    };

    /// Builds every pipeline and an empty scene.
    ///
    /// Fails only if the device rejects a module, which on the web is the
    /// moment the browser's WGSL front end sees the whole composition for the
    /// first time.
    #[must_use]
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let layouts = PathtraceLayouts::new(device);
        // A throwaway camera buffer, only so the kernel's pipeline layout can
        // be built before any pane has a camera. Each pane binds its own.
        let seed_camera = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Pathtrace Layout Seed Camera"),
            size: std::mem::size_of::<crate::camera::CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let seed_uniforms = PathUniforms::new(device, &seed_camera);
        let kernel = PathKernel::new(device, &layouts, &seed_uniforms);
        let resolve = TraceResolve::new(device);
        let denoiser = Denoiser::new(device);
        let scene = TraceScene::new(device, &layouts);
        let atlas = TraceAtlas::new(device, queue, &layouts);
        Self {
            layouts,
            kernel,
            resolve,
            denoiser,
            cache: TraceSceneCache::new(),
            scene,
            atlas,
            panes: [None, None, None, None],
            depth: None,
            depth_target: None,
            settings: TraceSettings::default(),
            sky: ([0.05, 0.06, 0.08], [0.02, 0.02, 0.02]),
            env_intensity: 1.0,
            env_rotation: 0.0,
        }
    }

    /// A depth target this backend's layouts can bind.
    ///
    /// Here rather than on [`DepthTarget`] itself so a caller does not have to
    /// hold the layouts to allocate one.
    #[must_use]
    pub fn depth_target(&self, device: &wgpu::Device, width: u32, height: u32) -> DepthTarget {
        DepthTarget::new(device, &self.layouts, width, height)
    }

    /// Traces one primary ray per pixel and writes how far away the surface it
    /// found is, measured along the camera's axis.
    ///
    /// Nothing about it is accumulated: it is a single sample at each pixel's
    /// centre with no jitter and no aperture, which is what a compositing
    /// package expects a depth pass to be and the only thing a silhouette pixel
    /// can honestly report. See [`crate::pathtrace::depth`].
    ///
    /// `window` places the tile in a larger image, exactly as it does for a
    /// colour dispatch; `None` renders the whole of `target`.
    pub fn encode_depth(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        camera: &crate::camera_state::CameraState,
        target: &DepthTarget,
        window: Option<crate::backend::ImageWindow>,
    ) {
        let [width, height] = target.size();
        // Rebuilt when the camera buffer changes, which is the same staleness
        // test the accumulator applies for the same reason: a bind group holds
        // the buffer it was built over.
        let stale = self
            .depth
            .as_ref()
            .is_none_or(|(_, _, bound)| *bound != camera.buffer);
        if stale {
            self.depth = Some((
                DepthPass::new(device, &self.layouts),
                TraceUniforms::new(device, &self.layouts, &camera.buffer),
                camera.buffer.clone(),
            ));
        }
        let Some((pass, uniforms, _)) = self.depth.as_ref() else {
            return;
        };

        let (tile_offset, resolution) = match window {
            Some(w) => (w.origin, w.full),
            None => ([0, 0], [width, height]),
        };
        uniforms.write(
            queue,
            &TraceParams {
                tile_offset,
                tile_size: [width, height],
                resolution,
                // A depth ray scatters nowhere and is drawn once, so every
                // field the bounce loop and the sampler read stays at its zero.
                // The aperture is the one worth naming: zero is what makes
                // `camera_ray` return a pinhole ray and ignore the lens pair
                // this pass hands it.
                aperture_radius: 0.0,
                ..TraceParams::default()
            },
        );
        pass.encode(encoder, &self.scene, target, uniforms, [width, height]);
    }

    /// What the next render uses. Changing any of it drops every accumulation,
    /// because a mean over two settings is a mean over neither.
    pub fn set_settings(&mut self, settings: TraceSettings) {
        if self.settings != settings {
            self.settings = settings;
            self.invalidate();
        }
    }

    #[must_use]
    pub fn settings(&self) -> TraceSettings {
        self.settings
    }

    /// How the filter is steered, when [`TraceSettings::denoise`] turns it on.
    ///
    /// Separate from [`TraceSettings`] because it is not authored: the render
    /// node carries a toggle and not three sigmas, and a control whose effect
    /// is "how much detail would you like removed" is not one a person can
    /// reason about.
    pub fn set_denoise_settings(&mut self, settings: DenoiseSettings) {
        self.denoiser.set_settings(settings);
    }

    #[must_use]
    pub fn denoise_settings(&self) -> DenoiseSettings {
        self.denoiser.settings()
    }

    /// Installs a prepared environment image and the look the environment node
    /// authored over it.
    ///
    /// The tracer's equivalent of the rasterizer's lighting chokepoint: one
    /// place an environment reaches the GPU, so a caller cannot install half of
    /// one.
    pub fn set_environment(
        &mut self,
        device: &wgpu::Device,
        environment: TraceEnvironment,
        intensity: f32,
        rotation: f32,
    ) {
        self.atlas
            .set_environment(device, &self.layouts, environment);
        self.env_intensity = intensity;
        self.env_rotation = rotation;
        self.invalidate();
    }

    /// The two scalars that scale and turn the environment, without rebuilding
    /// it.
    ///
    /// Separate from [`Self::set_environment`] because a rotation slider is
    /// dragged: rebuilding through that entry point would upload the
    /// distribution once per frame of the drag, for two values the kernel
    /// reads out of a uniform.
    pub fn set_environment_params(&mut self, intensity: f32, rotation: f32) {
        if (self.env_intensity, self.env_rotation) != (intensity, rotation) {
            self.env_intensity = intensity;
            self.env_rotation = rotation;
            self.invalidate();
        }
    }

    /// The two colours the kernel blends by the world up axis when there is no
    /// environment image, which is the ordinary case for a scene that has never
    /// been given one.
    pub fn set_sky(&mut self, up: [f32; 3], down: [f32; 3]) {
        if self.sky != (up, down) {
            self.sky = (up, down);
            self.invalidate();
        }
    }

    /// The scene the tracer holds, for the hierarchy jobs a host pumps and for
    /// the counts a progress readout wants.
    #[must_use]
    pub fn scene_cache(&self) -> &TraceSceneCache {
        &self.cache
    }

    /// The same, mutably, which is how a completed hierarchy build is handed
    /// back under [`crate::pathtrace::scene::BuildPolicy::Deferred`].
    pub fn scene_cache_mut(&mut self) -> &mut TraceSceneCache {
        &mut self.cache
    }

    /// How many samples the given pane's mean averages, and how many it is
    /// converging to.
    #[must_use]
    pub fn progress(&self, pane: usize) -> (u32, u32) {
        let done = self
            .panes
            .get(pane)
            .and_then(Option::as_ref)
            .map_or(0, |p| p.samples);
        (done, self.settings.samples.max(1))
    }

    /// Brings the GPU side up to date with whatever the cache holds.
    ///
    /// Separate from [`RenderBackend::apply`] because a hierarchy that finished
    /// building in a worker changes the scene without any delta arriving, and
    /// that path has to reach the same three uploads.
    pub fn sync(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let Some(arena) = self.cache.repack() else {
            return;
        };
        self.scene.sync(device, queue, &self.layouts, arena);
        self.atlas.sync(
            device,
            queue,
            &self.layouts,
            self.cache.atlas(),
            self.cache.atlas_textures(),
        );
        self.invalidate();
    }

    /// The snapshot twin of [`RenderBackend::apply`], for a tracer that may
    /// already hold a scene. A snapshot carries no removals, so the cache
    /// reconciles what it holds against what the snapshot names before the
    /// ops run; unchanged geometry stays a hierarchy-cache hit. A shell
    /// starts every still through this rather than snapshotting once at
    /// construction, which was the defect where every still after the first
    /// rendered the first scene.
    pub fn apply_snapshot(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        delta: &SceneDelta,
    ) {
        self.cache.apply_snapshot(delta);
        self.sync(device, queue);
    }

    /// The environment uniform this frame: the image when one is installed, the
    /// two-colour sky when not.
    fn environment(&self) -> EnvParams {
        if self.atlas.environment().size() == [0, 0] {
            EnvParams::constant(self.sky.0, self.sky.1)
        } else {
            EnvParams::image(
                self.atlas.environment(),
                self.env_rotation,
                self.env_intensity,
            )
        }
    }
}

impl PathBackend {
    /// What the last ingest made of the scene.
    ///
    /// Exposed so a shell can tell somebody what was left out. The tracer
    /// intersects triangles, so a scene carrying point clouds or poly-lines
    /// renders its triangles and drops the rest; the count is kept during
    /// ingest and was, until 0.9.0, kept and never shown, which made a missing
    /// curve something you noticed rather than something you were told.
    #[must_use]
    pub fn scene_stats(&self) -> crate::pathtrace::scene::TraceSceneStats {
        self.cache.stats()
    }
}

impl RenderBackend for PathBackend {
    fn apply(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, delta: &SceneDelta) {
        self.cache.apply(delta);
        self.sync(device, queue);
    }

    /// Store the lens, and drop any accumulation taken through a different
    /// one.
    ///
    /// The invalidate is the point: an aperture is integrated over, so a mean
    /// gathered at one f-number is not a partial result of another. Skipped
    /// when nothing moved, because a host that calls this every frame with an
    /// unchanged lens would otherwise never converge.
    fn skipped_primitives_warning(&self) -> Option<String> {
        let stats = self.cache.stats();
        skipped_primitives_message(stats.skipped_points, stats.skipped_lines)
    }

    fn set_lens(&mut self, lens: solarxy_core::scene::CameraLens) {
        if (self.settings.aperture_radius - lens.aperture_radius).abs() < f32::EPSILON
            && (self.settings.focus_distance - lens.focus_distance).abs() < f32::EPSILON
            && self.settings.aperture_blades == lens.blades
        {
            return;
        }
        self.settings.aperture_radius = lens.aperture_radius.max(0.0);
        self.settings.focus_distance = lens.focus_distance.max(0.0);
        self.settings.aperture_blades = lens.blades;
        self.invalidate();
    }

    /// Draw one chunk of samples into this pane's mean and resolve it into
    /// `target`.
    ///
    /// `target` is used, unlike the rasterizer's, and that asymmetry is the
    /// whole reason the parameter exists: this backend has its own float
    /// accumulation and has to put a linear half-float image somewhere the
    /// shared composite can read it.
    fn encode(&mut self, ctx: &mut FrameCtx<'_>, target: &wgpu::TextureView) -> FrameOutcome {
        let PaneContent::Scene { .. } = &ctx.content else {
            // No camera, or a UV layout, which this backend does not draw.
            return FrameOutcome::Complete;
        };
        // The arm's `cam_data` is deliberately unread. It is the camera as it
        // stood *before* the aspect write, which the raster main pass takes as
        // a value; this path reads the camera's uniform buffer through a bind
        // group, so what it needs is the write below to have happened.
        let Some(camera) = ctx.camera.as_deref_mut() else {
            return FrameOutcome::Complete;
        };

        // Read before the destructure below, because both borrow `self` and
        // the accumulator's is mutable.
        let settings = self.settings;
        let environment = self.environment();

        // Destructured rather than reached through `self.`, for the reason the
        // shared pane body destructures its context: the accumulator is
        // borrowed mutably while the scene, the atlas and the pipelines are
        // borrowed immutably, and only field-level bindings let the compiler
        // see that those are disjoint.
        let Self {
            layouts,
            kernel,
            resolve,
            denoiser,
            scene,
            atlas,
            panes,
            ..
        } = self;
        let Some(slot) = panes.get_mut(ctx.index) else {
            return FrameOutcome::Complete;
        };

        // The pane's own pixels, not the pane's rect in CSS units: the
        // accumulator is sized in the same texels the shared target is.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (full_width, full_height) = (
            (ctx.rect.width.max(1.0)) as u32,
            (ctx.rect.height.max(1.0)) as u32,
        );
        // The preview's resolution scale shrinks the accumulator, the
        // kernel's grid and the filter with it; the resolve upscales into
        // the pane-sized viewport of the target. Only the whole-pane path
        // scales: a tile is a window on a still whose size is authored,
        // and the still job reads the target back at exactly the size it
        // asked for.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (width, height) = if ctx.window.is_none() {
            let scale = settings.resolution_scale.clamp(0.1, 1.0);
            (
                ((full_width as f32 * scale) as u32).max(1),
                ((full_height as f32 * scale) as u32).max(1),
            )
        } else {
            (full_width, full_height)
        };

        // Where this pane sits in the picture. An ordinary frame is the whole
        // of a one-pane image; a still render's tile is a window on a larger
        // one, and the difference reaches the kernel as a dispatch offset.
        //
        // The camera is **not** windowed here, unlike the raster path's. The
        // kernel builds its ray from the pixel's coordinate in the whole image,
        // so it needs the whole image's camera; windowing it as well would
        // apply the offset twice.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (tile_offset, resolution) = match ctx.window {
            Some(w) => (w.origin, w.full),
            None => ([0, 0], [width, height]),
        };

        // The aspect write has to land before the camera buffer is read, the
        // same way the raster path does it, or the first traced frame after a
        // resize frames the previous shape.
        #[allow(clippy::cast_precision_loss)]
        let aspect = match ctx.window {
            Some(w) => w.full[0] as f32 / (w.full[1].max(1) as f32),
            None => ctx.rect.width / ctx.rect.height.max(1.0),
        };
        camera.write_with_aspect(ctx.queue, aspect);

        let stale = slot
            .as_ref()
            .is_none_or(|p| p.width != width || p.height != height || p.camera != camera.buffer);
        if stale {
            *slot = Some(PaneAccumulator {
                target: TraceTarget::new(ctx.device, layouts, width, height),
                uniforms: PathUniforms::new(ctx.device, &camera.buffer),
                samples: 0,
                width,
                height,
                camera: camera.buffer.clone(),
            });
        }
        let Some(pane) = slot.as_mut() else {
            return FrameOutcome::Complete;
        };

        let total = settings.samples.max(1);
        if pane.samples >= total {
            // Converged, and the mean is still in the target, so the pane is
            // re-resolved rather than re-traced. That is what makes a finished
            // still cost nothing to keep on screen. The filter re-runs, because
            // its output is scratch rather than a cached image and it is off by
            // default on exactly the renders that reach this branch.
            let source = if settings.denoise {
                denoiser.encode(
                    ctx.device,
                    ctx.queue,
                    ctx.encoder,
                    &pane.target,
                    pane.samples,
                )
            } else {
                pane.target.color_view()
            };
            resolve.encode(
                ctx.device,
                ctx.encoder,
                source,
                target,
                (full_width, full_height),
            );
            return FrameOutcome::Complete;
        }

        // Swapped **before** the dispatch, not after it, and the difference is
        // not cosmetic. Everything that reads the accumulator reads the write
        // slot, so a swap after the last dispatch of a run would leave the
        // converged branch above resolving the slot from the dispatch before
        // it -- or, after a single dispatch, one nothing has ever written.
        //
        // The first dispatch of a run does not swap, which is what pairs with
        // `sample_base` of zero: it writes one slot and never reads the other.
        if pane.samples > 0 {
            pane.target.swap();
        }

        let chunk = settings.chunk.max(1).min(total - pane.samples);
        let params = TraceParams {
            tile_offset,
            tile_size: [width, height],
            resolution,
            bounces: settings.bounces,
            transmissive_bounces: settings.transmissive_bounces,
            samples: total,
            seed: settings.seed,
            light_count: scene.light_count(),
            aperture_radius: settings.aperture_radius,
            focus_distance: settings.focus_distance,
            aperture_blades: settings.aperture_blades,
            chunk,
            sample_base: pane.samples,
            firefly_clamp: settings.firefly_clamp,
            ..TraceParams::default()
        };
        pane.uniforms.write(ctx.queue, &params, &environment);
        kernel.encode(
            ctx.encoder,
            // The estimator is not a setting. All three converge to the same
            // image and the other two exist so a test can prove it; a render
            // uses both techniques.
            PathEstimator::Mis,
            scene,
            atlas,
            &pane.target,
            &pane.uniforms,
            [width, height],
        );
        pane.samples += chunk;
        // Filtered before the resolve rather than after it, so the whole look
        // chain runs on the filtered image the way it runs on the raw one.
        // Filtering after tone mapping would smooth a display-referred picture
        // and put the grain back the moment the exposure moved.
        let source = if settings.denoise {
            denoiser.encode(
                ctx.device,
                ctx.queue,
                ctx.encoder,
                &pane.target,
                pane.samples,
            )
        } else {
            pane.target.color_view()
        };
        resolve.encode(
            ctx.device,
            ctx.encoder,
            source,
            target,
            (full_width, full_height),
        );

        if pane.samples >= total {
            FrameOutcome::Complete
        } else {
            FrameOutcome::Converging {
                samples: pane.samples,
                target_samples: total,
            }
        }
    }

    fn caps(&self) -> BackendCaps {
        Self::CAPS
    }

    /// The pane's auxiliary target, which holds finished means rather than
    /// sums: the kernel merges each chunk in by the count of samples that found
    /// a surface, so a reader divides by nothing.
    ///
    /// The write slot, because the swap runs before a dispatch and never after
    /// one, so the write slot is what the last dispatch left.
    fn aov_sources(&self, pane: usize) -> Option<crate::backend::AovSources<'_>> {
        let slot = self.panes.get(pane)?.as_ref()?;
        Some(crate::backend::AovSources {
            auxiliary: slot.target.auxiliary_texture(),
        })
    }

    fn encode_depth_aov(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        camera: &crate::camera_state::CameraState,
        size: [u32; 2],
        window: Option<crate::backend::ImageWindow>,
    ) -> Option<&wgpu::Texture> {
        if self
            .depth_target
            .as_ref()
            .is_none_or(|t| t.size() != [size[0].max(1), size[1].max(1)])
        {
            self.depth_target = Some(DepthTarget::new(
                device,
                &self.layouts,
                size[0].max(1),
                size[1].max(1),
            ));
        }
        // Lifted out and put back, because the pass needs this backend mutably
        // while the target it writes is borrowed from the same backend. The
        // move is a handful of handles.
        let target = self.depth_target.take()?;
        self.encode_depth(device, queue, encoder, camera, &target, window);
        self.depth_target = Some(target);
        self.depth_target.as_ref().map(DepthTarget::texture)
    }

    /// Drop every pane's accumulation.
    ///
    /// The counter, not the textures: a mean with no samples behind it does not
    /// read its history, so zeroing the count is the whole reset and the
    /// allocation survives to be written over.
    fn invalidate(&mut self) {
        for pane in self.panes.iter_mut().flatten() {
            pane.samples = 0;
        }
    }
}

impl PathBackend {
    /// Drop one pane's accumulation, leaving the others converging.
    ///
    /// The trait's [`RenderBackend::invalidate`] is the whole-scene reset a
    /// delta wants; this is the narrower one a single pane's camera move
    /// wants. Inherent rather than on the contract, because the contract
    /// has no pane vocabulary beyond `FrameCtx::index` and the rasterizer
    /// accumulates nothing there is to drop.
    pub fn invalidate_pane(&mut self, index: usize) {
        if let Some(pane) = self.panes.get_mut(index).and_then(Option::as_mut) {
            pane.samples = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_describe_a_progressive_backend_without_naming_it() {
        // The real constant, not a literal rebuilt here, which would agree
        // with itself whatever the backend later declared.
        let caps = PathBackend::CAPS;
        assert!(caps.progressive);
        // Unbounded is not a large number, which is the whole point of the
        // `Option`: a host says "every light in the scene" rather than "8192".
        assert!(caps.max_lights.is_none());
        assert!(!caps.supports_topology.contains(TopologyMask::POINTS));
        assert!(!caps.supports_topology.contains(TopologyMask::LINES));
        assert!(caps.writes_aovs);
        // Occlusion is in the image already. Nothing here fills the buffer the
        // finishing chain would multiply by, so a host must not reach for it,
        // and this is the only thing that says so.
        assert!(!caps.writes_occlusion);
    }

    #[test]
    fn the_default_settings_pace_themselves() {
        let s = TraceSettings::default();
        // One sample per encode by default, which is what keeps a dispatch
        // bounded no matter what sample count is asked for.
        assert_eq!(s.chunk, 1);
        assert!(s.samples >= s.chunk);
        // On by default, because the speckle it removes is the one stage-four
        // limitation this stage closes.
        assert!(s.firefly_clamp > 0.0);
    }
}

/// The sentence a shell shows when a traced render leaves geometry out.
///
/// Free-standing so it can be tested without a device, and shared so the
/// browser, the desktop and the command line say the same thing rather than
/// three things that drift.
///
/// Both counts are meshes rather than primitives, which is what the ingest
/// counts and what a person can act on: "one of your objects is missing" is
/// more useful than a vertex total.
#[must_use]
pub fn skipped_primitives_message(points: u32, lines: u32) -> Option<String> {
    if points == 0 && lines == 0 {
        return None;
    }
    let part = |n: u32, one: &str, many: &str| match n {
        0 => None,
        1 => Some(format!("1 {one}")),
        _ => Some(format!("{n} {many}")),
    };
    let listed: Vec<String> = [
        part(points, "point cloud", "point clouds"),
        part(lines, "poly-line object", "poly-line objects"),
    ]
    .into_iter()
    .flatten()
    .collect();
    Some(format!(
        "the path tracer skipped {} because it intersects triangles only; \
         render with the rasterizer if they are the subject",
        listed.join(" and ")
    ))
}

#[cfg(test)]
mod skipped_tests {
    use super::skipped_primitives_message;

    #[test]
    fn a_scene_of_triangles_says_nothing() {
        assert!(skipped_primitives_message(0, 0).is_none());
    }

    #[test]
    fn one_of_each_is_singular_and_both_are_named() {
        let m = skipped_primitives_message(1, 1).expect("a message");
        assert!(m.contains("1 point cloud "), "{m}");
        assert!(m.contains("1 poly-line object"), "{m}");
        assert!(!m.contains("clouds"), "singular, not plural: {m}");
    }

    #[test]
    fn only_what_was_skipped_is_named() {
        // A scene with curves and no point clouds must not be told about
        // point clouds it does not have.
        let m = skipped_primitives_message(0, 4).expect("a message");
        assert!(m.contains("4 poly-line objects"), "{m}");
        assert!(!m.contains("point"), "{m}");
        assert!(!m.contains(" and "), "nothing to join: {m}");
    }

    #[test]
    fn the_message_says_what_to_do_instead() {
        let m = skipped_primitives_message(3, 0).expect("a message");
        assert!(m.contains("3 point clouds"), "{m}");
        assert!(m.contains("rasterizer"), "a limitation with no remedy: {m}");
    }
}
