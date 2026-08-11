//! Rendering a Solarxy scene with no browser and no window.
//!
//! # Why this is a crate and not command-line code
//!
//! The same reason the validation crate is one: the capability should be
//! reachable by another tool without spawning a subprocess and parsing its
//! output. The command-line binary above it is an argument parser and an exit
//! code, and nothing else.
//!
//! # One render path
//!
//! A scene file and a bare model both become a cooked document (see [`input`]),
//! and from there this crate does what a shell does, in the same order, through
//! the same shared pieces: bring a renderer up with no surface, ingest the
//! scene delta into a backend, build a camera, and drive the tiled still job.
//! There is deliberately no headless-only rendering code, because a second
//! implementation of any of that is a second thing to keep true.
//!
//! # What a still is
//!
//! A photograph of the scene, not a screenshot of a viewport: no grid, no
//! gizmo, no overlays. That view has a single shared definition
//! ([`PaneDisplaySettings::for_still`]) which the browser's still dialog uses
//! too, so the two surfaces produce the same image rather than two images that
//! happen to look similar.
//!
//! [`PaneDisplaySettings::for_still`]: solarxy_core::view_config::PaneDisplaySettings::for_still

#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod error;
pub mod input;
pub mod report;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use solarxy_core::preferences::BackgroundMode;
use solarxy_core::scene::CameraDef;
use solarxy_core::view_config::{DisplaySettings, PaneDisplaySettings, PaneLook, ViewLayout};
use solarxy_graph::document::GraphContext;
use solarxy_graph::nodes::{RenderEngine, RenderSettings};
use solarxy_host::headless::HeadlessHost;
use solarxy_host::raster::RasterBackend;
use solarxy_host::still::{StillCtx, StillEngine, StillRenderJob, StillSpec, StillStep};
use solarxy_renderer::backend::RenderBackend;
use solarxy_renderer::camera_state::CameraState;
use solarxy_renderer::pathtrace::backend::PathBackend;
use solarxy_renderer::scene::BackgroundModeExt;

pub use error::RenderError;
pub use report::{RENDER_REPORT_SCHEMA_VERSION, RenderReport};

/// Where the encoded image goes.
///
/// Bytes rather than text, which is the one place this differs from the
/// validation crate's sink: an image is not a string, and a PNG through a
/// `String` is a corrupted PNG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    Stdout,
    File(PathBuf),
}

impl Output {
    /// `-` means stdout, following the convention every tool that reads from a
    /// pipe already uses.
    #[must_use]
    pub fn from_path(path: &Path) -> Self {
        if path.as_os_str() == "-" {
            Self::Stdout
        } else {
            Self::File(path.to_path_buf())
        }
    }

    /// # Errors
    /// The write failing.
    pub fn write(&self, bytes: &[u8]) -> Result<(), RenderError> {
        use std::io::Write;
        match self {
            Self::Stdout => {
                std::io::stdout()
                    .write_all(bytes)
                    .map_err(|source| RenderError::OutputUnwritable {
                        path: PathBuf::from("-"),
                        source,
                    })
            }
            Self::File(path) => {
                std::fs::write(path, bytes).map_err(|source| RenderError::OutputUnwritable {
                    path: path.clone(),
                    source,
                })
            }
        }
    }

    /// What the report should name, absolute where it can be.
    #[must_use]
    pub fn display(&self) -> String {
        match self {
            Self::Stdout => "-".to_string(),
            Self::File(p) => std::fs::canonicalize(p)
                .unwrap_or_else(|_| p.clone())
                .display()
                .to_string(),
        }
    }
}

/// What the caller asked for, over what the scene says.
///
/// Every override is optional and every `None` means "whatever the render node
/// says", which is what keeps the node authoritative and the flags a
/// convenience rather than a second source of truth.
#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    pub output: Option<Output>,
    /// Which render node to use, when a scene has more than one.
    pub render_node: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub samples: Option<u32>,
    pub bounces: Option<u32>,
    pub denoise: Option<bool>,
    pub engine: Option<RenderEngine>,
    pub seed: Option<u32>,
}

impl RenderOptions {
    fn apply_to(&self, mut settings: RenderSettings) -> RenderSettings {
        if let Some(v) = self.width {
            settings.width = v.max(16);
        }
        if let Some(v) = self.height {
            settings.height = v.max(16);
        }
        if let Some(v) = self.samples {
            settings.samples = v.max(1);
        }
        if let Some(v) = self.bounces {
            settings.bounces = v.max(1);
        }
        if let Some(v) = self.denoise {
            settings.denoise = v;
        }
        if let Some(v) = self.engine {
            settings.engine = v;
        }
        settings
    }
}

/// A finished render.
pub struct RenderOutcome {
    pub report: RenderReport,
}

/// Loads, cooks, renders, and writes.
///
/// # Errors
/// Every way that can fail, as [`RenderError`], which the caller maps onto its
/// own exit taxonomy.
pub fn run_render(input: &Path, opts: &RenderOptions) -> Result<RenderOutcome, RenderError> {
    let started = Instant::now();
    let loaded = input::load(input)?;
    let mut warnings = loaded.warnings;
    let engine = loaded.engine;

    let settings = opts.apply_to(resolve_settings(&engine, opts, &mut warnings)?);
    let delta = {
        let mut e = engine;
        e.take_scene_delta()
    };

    let (device, queue) = request_device()?;
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let mut host = HeadlessHost::new(&device, &queue, format, 64, 64)
        .map_err(|e| RenderError::Device(e.to_string()))?;

    let background_mode = BackgroundMode::GRADIENT;
    let background = background_mode.resolve(&[]);
    let mut backend = build_backend(
        &device,
        &queue,
        &host,
        &settings,
        opts.seed,
        background.sky_colors(),
    );
    backend.apply(&device, &queue, &delta);

    // A raster backend knows the scene's extent; a traced one is asked the same
    // question through the raster ingest that ran beside it, so both frame the
    // same subject. Failing that, the placeholder the shells seed from.
    let raster_probe = RasterBackend::new(Arc::clone(&host.renderer.layouts));
    let mut probe = raster_probe;
    probe.apply(&device, &queue, &delta);
    if let Some(b) = probe.scene().visible_bounds() {
        host.bounds = b;
    }

    let (mut camera, look) =
        build_camera(&device, &queue, &mut host, &probe, &settings, &mut warnings);

    let pds = PaneDisplaySettings::for_still(background_mode);
    let display = still_display_settings();

    let spec = StillSpec {
        width: settings.width,
        height: settings.height,
        engine: match settings.engine {
            RenderEngine::PathTraced => StillEngine::PathTraced,
            RenderEngine::Raster => StillEngine::Raster,
        },
        samples: settings.samples,
        // Both screen-space post passes are off in a headless bring-up, so the
        // apron would be a margin around nothing.
        screen_space_post: false,
        tile_budget: solarxy_host::still::TILE_BUDGET_PIXELS,
    };
    let mut job = StillRenderJob::new(spec);
    let spec = job.spec();
    let tiles = job.plan().len();

    let scene_present = probe.scene().draw_objects().next().is_some();
    let image = drive(
        &device,
        &queue,
        &mut host,
        &mut camera,
        &mut job,
        backend.as_mut(),
        &StillView {
            pds,
            display,
            background,
            look,
            format,
            scene_present,
        },
    )?;

    let output = opts
        .output
        .clone()
        .unwrap_or_else(|| Output::File(PathBuf::from("render.png")));
    let encoded = solarxy_formats::export::encode_png_bytes(&solarxy_core::RawImageData::new(
        image,
        spec.width,
        spec.height,
    ))?;
    output.write(&encoded)?;

    #[allow(clippy::cast_possible_truncation)]
    Ok(RenderOutcome {
        report: RenderReport {
            schema_version: RENDER_REPORT_SCHEMA_VERSION,
            solarxy_version: env!("CARGO_PKG_VERSION"),
            output: output.display(),
            width: spec.width,
            height: spec.height,
            engine: match settings.engine {
                RenderEngine::PathTraced => "pathTraced",
                RenderEngine::Raster => "raster",
            },
            samples: match settings.engine {
                RenderEngine::PathTraced => spec.samples,
                RenderEngine::Raster => 1,
            },
            tiles: tiles as u32,
            elapsed_ms: started.elapsed().as_millis() as u64,
            warnings,
        },
    })
}

/// The render node's settings, or the defaults when there is no render node.
///
/// A bare model has none, and demanding one would mean the simplest possible
/// invocation could not work. The defaults are the node's own, so the two
/// answers agree.
fn resolve_settings(
    engine: &solarxy_graph::engine::Engine,
    opts: &RenderOptions,
    warnings: &mut Vec<String>,
) -> Result<RenderSettings, RenderError> {
    let graph = engine
        .document()
        .graph(GraphContext::Root)
        .map_err(|e| RenderError::RenderNode(e.to_string()))?;
    let render_nodes: Vec<_> = graph
        .nodes()
        .filter(|n| n.type_id == "render")
        .map(|n| n.id)
        .collect();

    let chosen = match (&opts.render_node, render_nodes.len()) {
        // Named by the node's own `name` param, which is what a reader sees on
        // the canvas and therefore the only name they could type.
        (Some(name), _) => *render_nodes
            .iter()
            .find(|id| {
                matches!(
                    engine.resolved_param(GraphContext::Root, **id, "name"),
                    Ok(solarxy_graph::params::ParamValue::Text(ref t)) if t == name
                )
            })
            .ok_or(RenderError::NoRenderNode)?,
        (None, 0) => {
            warnings.push("the scene has no render node; rendering at the defaults".into());
            return Ok(default_settings());
        }
        (None, 1) => render_nodes[0],
        (None, n) => return Err(RenderError::AmbiguousRenderNode(n)),
    };
    engine
        .render_settings(GraphContext::Root, chosen)
        .map_err(RenderError::RenderNode)
}

/// What a document with no render node renders at.
fn default_settings() -> RenderSettings {
    RenderSettings {
        camera: None,
        width: 1920,
        height: 1080,
        engine: RenderEngine::Raster,
        samples: 64,
        bounces: 6,
        transmissive_bounces: 4,
        denoise: false,
    }
}

/// The camera the settings name, if the cooked scene carries it.
fn named_camera(
    scene: &RasterBackend,
    camera: Option<solarxy_graph::document::NodeId>,
) -> Option<CameraDef> {
    let id = camera?;
    scene
        .scene()
        .cameras()?
        .iter()
        .find(|c| c.id == solarxy_core::scene::SceneObjectId(id.0))
        .cloned()
}

/// The global half of the view a still is drawn with.
///
/// Not shared with the browser, unlike the per-pane half: every field here is
/// either a session concern a headless render does not have, like the layout,
/// or a scene value it takes from the document.
fn still_display_settings() -> DisplaySettings {
    DisplaySettings {
        turntable_active: false,
        turntable_rpm: 6.0,
        lights_locked: false,
        layout: ViewLayout::Single,
        split_ratio: 0.5,
        roughness_scale: 1.0,
        metallic_scale: 1.0,
        hdri_rotation: 0.0,
        hdri_intensity: 1.0,
        point_size: 4.0,
    }
}

/// Copies one finished tile into the assembled image.
fn blit(image: &mut [u8], image_width: u32, tile: &solarxy_host::still::StillTile) {
    let stride = image_width as usize * 4;
    let row = tile.rect.width as usize * 4;
    for y in 0..tile.rect.height as usize {
        let src = y * row;
        let dst = (tile.rect.y as usize + y) * stride + tile.rect.x as usize * 4;
        if dst + row <= image.len() && src + row <= tile.pixels.len() {
            image[dst..dst + row].copy_from_slice(&tile.pixels[src..src + row]);
        }
    }
}

/// A device with no surface, asking for exactly what both shells ask for.
///
/// Requesting more would mean an image the shipped app cannot reproduce, which
/// defeats the point of rendering the same scene here.
fn request_device() -> Result<(wgpu::Device, wgpu::Queue), RenderError> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .map_err(|_| RenderError::NoAdapter)?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("solarxy-render"),
        required_features: wgpu::Features::empty(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .map_err(|e| RenderError::Device(e.to_string()))
}

/// Everything a tile's encode needs that does not change between tiles.
///
/// Bundled because the driver takes eight references already and a ninth
/// argument list is not clearer than a name.
struct StillView {
    pds: PaneDisplaySettings,
    display: DisplaySettings,
    background: solarxy_core::preferences::ResolvedBackground,
    look: solarxy_renderer::composite::CompositeLook,
    format: wgpu::TextureFormat,
    scene_present: bool,
}

/// Runs the job to completion, resizing the shared targets per tile, and
/// returns the assembled image.
///
/// The resize is the caller's job by the still job's own contract: the two
/// shells resize with different policy around one body, so the job asserts
/// rather than resizes.
fn drive(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    host: &mut HeadlessHost,
    camera: &mut CameraState,
    job: &mut StillRenderJob,
    backend: &mut dyn RenderBackend,
    view: &StillView,
) -> Result<Vec<u8>, RenderError> {
    let spec = job.spec();
    let mut image = vec![0u8; (spec.width as usize) * (spec.height as usize) * 4];
    while let Some(tile) = job.current() {
        host.renderer
            .resize_targets(device, tile.render.width, tile.render.height);
        let step = {
            let mut ctx = StillCtx {
                device,
                queue,
                renderer: &mut host.renderer,
                camera,
                env: &host.env,
                pds: &view.pds,
                display: &view.display,
                background: view.background,
                bounds: Some(&host.bounds),
                look: view.look,
                format: view.format,
                scene_present: view.scene_present,
            };
            job.advance(&mut ctx, backend)
        };
        match step {
            StillStep::Working => {}
            StillStep::Tile => {
                while let Some(t) = job.take_tile() {
                    blit(&mut image, spec.width, &t);
                }
            }
            StillStep::Done => break,
            StillStep::Failed => return Err(RenderError::DeviceLost),
        }
    }
    Ok(image)
}

/// The backend the settings ask for, configured from them.
///
/// The tracer's chunk is set here rather than by the job, because the job takes
/// the chunk from the backend by design: a browser paces one sample per frame to
/// stay responsive and a terminal has no frame to pace against.
fn build_backend(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    host: &HeadlessHost,
    settings: &RenderSettings,
    seed: Option<u32>,
    sky: ([f32; 3], [f32; 3]),
) -> Box<dyn RenderBackend> {
    match settings.engine {
        RenderEngine::Raster => Box::new(RasterBackend::new(Arc::clone(&host.renderer.layouts))),
        RenderEngine::PathTraced => {
            let mut t = PathBackend::new(device, queue);
            let mut trace = t.settings();
            trace.samples = settings.samples;
            trace.bounces = settings.bounces;
            trace.transmissive_bounces = settings.transmissive_bounces;
            trace.denoise = settings.denoise;
            trace.chunk = 8.min(settings.samples.max(1));
            if let Some(seed) = seed {
                trace.seed = seed;
            }
            t.set_settings(trace);
            // The constant sky the kernel falls back to when the scene carries
            // no environment image, taken from the same background the raster
            // path resolves so the two agree rather than coincide. Without it
            // the tracer integrates against its own near-black default and a
            // scene with no environment renders almost unlit.
            t.set_sky(sky.0, sky.1);
            Box::new(t)
        }
    }
}

/// The camera the shot is taken through, and the look it carries.
///
/// Framing the scene's bounds when no camera is named is what makes the
/// simplest invocation produce a picture rather than an error, and the warning
/// beside it is what keeps a composition nobody chose from looking authored.
///
/// The viewer rig is applied on the same condition both shells use: only when
/// the scene authored no lights of its own. Applying it unconditionally would
/// overwrite an authored lighting setup with a camera-relative one.
fn build_camera(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    host: &mut HeadlessHost,
    probe: &RasterBackend,
    settings: &RenderSettings,
    warnings: &mut Vec<String>,
) -> (CameraState, solarxy_renderer::composite::CompositeLook) {
    let camera_def = named_camera(probe, settings.camera);
    if camera_def.is_none() {
        warnings.push(if settings.camera.is_some() {
            "the named camera is not in the cooked scene; framing the scene instead".into()
        } else {
            "the scene names no camera; framing its bounds".into()
        });
    }

    #[allow(clippy::cast_precision_loss)]
    let aspect = settings.width as f32 / settings.height.max(1) as f32;
    let mut camera = CameraState::new(device, &host.renderer.layouts.camera, &host.bounds, aspect);
    if let Some(def) = camera_def.as_ref() {
        solarxy_host::cameras::apply_camera_def(&mut camera.camera, def);
        // After the definition, not before: the shot's aspect comes from the
        // image being rendered rather than from whatever the camera was authored
        // against.
        camera.camera.aspect = aspect;
    }

    if probe.scene().lights().is_none() {
        let cam_data = camera.camera;
        solarxy_host::setup_pane_lighting(
            queue,
            &mut host.env,
            &cam_data,
            &host.bounds,
            host.renderer.ibl_res.ibl.irradiance_average,
        );
    }

    let look = camera_def
        .as_ref()
        .map(|d| solarxy_renderer::composite::resolve_look(Some(&d.look), &PaneLook::default()))
        .unwrap_or_default();
    (camera, look)
}
