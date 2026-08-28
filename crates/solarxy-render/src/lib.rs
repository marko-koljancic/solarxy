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
pub mod files;
pub mod input;
pub mod report;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use solarxy_core::preferences::BackgroundMode;
use solarxy_core::scene::{BackgroundKind, CameraDef};
use solarxy_core::view_config::{DisplaySettings, PaneDisplaySettings, PaneLook, ViewLayout};
use solarxy_graph::document::GraphContext;
use solarxy_graph::nodes::RenderSettings;
/// Re-exported so a caller can name the engine without taking a dependency on
/// the graph crate for one enum.
pub use solarxy_graph::nodes::RenderEngine;
/// Re-exported for the same reason, and for one more: the command line's watch
/// window and the browser's still dialog both show a float render through this,
/// and a second copy of it would let the two surfaces disagree about a render
/// neither of them is authoritative about.
pub use solarxy_host::still::float_to_rgba8;
use solarxy_host::headless::{EnvironmentRequest, HeadlessHost};
use solarxy_host::raster::RasterBackend;
use solarxy_host::still::{StillCtx, StillEngine, StillRenderJob, StillSpec, StillStep};
use solarxy_renderer::backend::RenderBackend;
use solarxy_renderer::camera_state::CameraState;
use solarxy_renderer::pathtrace::backend::PathBackend;
use solarxy_renderer::pathtrace::environment::TraceEnvironment;

pub use error::RenderError;
// The pass extraction helpers travel with the preview: a sink that shows a
// pass must read the planes exactly the way the file writer reads them, or
// the window and the sibling file would disagree about one buffer.
pub use files::{AovKind, ExrSpace, albedo_from_auxiliary, floats_of, normal_from_auxiliary};
pub use report::{RENDER_REPORT_SCHEMA_VERSION, RenderReport};

/// How far along a render is.
///
/// One stream with one definition of progress, emitted through a callback that
/// [`run_render`] calls between the steps it is made of. A callback rather than
/// a channel because there are no threads here: the render loop is the only
/// thing running, and it calls the sink between cook passes and between tiles,
/// which is where it already reads the cancel flag.
///
/// # What is not in it
///
/// A denoising stage. The filter is not a phase: it runs inside each tile's
/// dispatch, before the resolve, so a line saying "denoising" would describe a
/// span of time that does not exist. What it costs is already inside
/// [`RenderProgress::Sampling`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderProgress {
    /// Reading the input and turning it into a document.
    Loading,
    /// Cooking it. Bounded by passes rather than measured in nodes, because a
    /// pass is what the engine's own loop counts and inventing a percentage
    /// from anything else would be a number nobody could act on.
    Cooking {
        pass: u32,
        passes: u32,
    },
    /// Building the ray hierarchy, which for a large model is the longest step
    /// before any pixel is drawn.
    BuildingHierarchy {
        triangles: u64,
    },
    /// Drawing. `sample` and `samples` describe the tile, not the image: each
    /// tile converges on its own before the next one starts.
    ///
    /// `columns` and `rows` are the shape the tiles are laid out in, which a
    /// surface drawing a grid of them cannot work out from a count: the plan
    /// is filled row by row and the last row may be short. Carried on this
    /// event rather than announced by one of its own, so a log that collapses
    /// repeated steps is unaffected.
    Sampling {
        tile: u32,
        tiles: u32,
        columns: u32,
        rows: u32,
        sample: u32,
        samples: u32,
        elapsed_ms: u64,
    },
    /// Encoding and writing the result.
    Writing {
        output: String,
    },
    Done {
        elapsed_ms: u64,
    },
    /// The step that failed, named so a sink can end its line honestly rather
    /// than leaving the last one hanging.
    Failed {
        stage: &'static str,
    },
}

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
    /// Auxiliary passes to write beside the image. Empty is the ordinary case.
    pub aovs: Vec<AovKind>,
    /// Which space a float beauty is written in. Meaningless, and refused, for
    /// an output that is not floating point.
    pub exr_space: Option<ExrSpace>,
    /// Set from outside to stop the render.
    ///
    /// A flag rather than a callback because the thing that sets it is a signal
    /// handler, which cannot call into a borrow of anything. The render reads it
    /// between cook passes and between tiles, which is often enough that an
    /// interrupt feels immediate and rare enough that it costs nothing.
    ///
    /// Nothing partial is left behind when it fires: the image is encoded and
    /// written in one call after the last tile, so a run that stops early never
    /// creates the file at all.
    pub cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// The largest single pass, in pixels, or the job's own default.
    ///
    /// The reason to lower it is that pixels only reach a sink when a tile
    /// finishes: an image inside the default budget is one tile, so a surface
    /// showing the picture as it converges would show nothing at all until the
    /// render ended. A smaller budget cuts the same picture into more pieces
    /// and it arrives in pieces. The output is unchanged, which
    /// `two_tile_budgets_render_the_same_image` asserts rather than assumes.
    pub tile_budget: Option<u32>,
}

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
pub const PREVIEW_TILE_BUDGET: u32 = 256 * 256;

/// The picture so far, as tiles land.
///
/// Whole rather than the rectangle that just arrived, because every consumer
/// so far uploads or redraws all of it anyway and a rect would be an
/// optimisation with no caller.
pub struct Preview<'a> {
    pub width: u32,
    pub height: u32,
    pub pixels: &'a [u8],
    pub format: PreviewFormat,
    /// The auxiliary plane, present exactly when the run asked for albedo or
    /// normal: four `f32` a pixel, albedo in the first three lanes and the
    /// packed world normal in the fourth, as the still job hands them over.
    pub aux: Option<&'a [u8]>,
    /// The depth plane, present exactly when the run asked for it: one `f32`
    /// a pixel, measured along the camera's axis, `1e30` where the ray hit
    /// nothing.
    pub depth: Option<&'a [u8]>,
    /// The engine that drew the picture, so a surface can say which passes
    /// could exist rather than guessing from which ones do.
    pub engine: RenderEngine,
}

/// How to read [`Preview::pixels`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewFormat {
    /// Four bytes a pixel, display-referred. The ordinary case.
    Rgba8,
    /// Sixteen bytes a pixel. Display-referred for a float image, and
    /// scene-referred for one written in scene-linear, which a consumer that
    /// wants to show it has to tone map itself.
    Rgba32F,
}

/// Where a render says what it is doing.
///
/// Two methods, one required. Reporting is what every sink is for; pixels are
/// wanted only by a surface that shows the picture, and a trait with a
/// defaulted second method lets the plain line stay four lines long. The same
/// shape the auxiliary passes took on the backend, for the same reason.
pub trait RenderSink {
    /// One event.
    fn report(&mut self, progress: &RenderProgress);

    /// The picture so far. Called when a tile lands, never between.
    fn preview(&mut self, _image: &Preview<'_>) {}
}

/// A sink that says nothing, for a caller that only wants the picture.
///
/// A named type rather than a blanket implementation over closures. That was
/// tried: a closure written `&mut |_| {}` is not higher-ranked over the
/// borrow, so every no-op call site failed with a lifetime error about
/// `FnMut` rather than about anything the caller had done. One unit struct
/// costs a word at the call site and nothing to understand.
pub struct Silent;

impl RenderSink for Silent {
    fn report(&mut self, _progress: &RenderProgress) {}
}

/// What a backend of a given kind can do, without building one.
///
/// The capability is read off the backend's own declaration, so a third
/// backend that writes auxiliary passes becomes usable here by saying so
/// rather than by being added to a list of engines that may.
///
/// A constant rather than [`RenderBackend::caps`] because both constructors
/// need a device, and a caller deciding whether an option it was handed can
/// take effect should not have to start a GPU to find out. That is not a
/// detail: it is the difference between a mistyped command exiting one
/// immediately and exiting four on a machine with no adapter.
fn caps_of(engine: RenderEngine) -> solarxy_renderer::backend::BackendCaps {
    match engine {
        RenderEngine::Raster => RasterBackend::CAPS,
        RenderEngine::PathTraced => PathBackend::CAPS,
    }
}

/// Refuses an option that cannot take effect, before anything is read.
///
/// A flag that silently does nothing is worse than a refusal: the run
/// succeeds, the file is there, and the pass the pipeline was waiting for is
/// not. `engine` is `None` before the document has been read, which is when
/// only the flag-against-flag cases can be judged.
fn check_options(opts: &RenderOptions, engine: Option<RenderEngine>) -> Result<(), RenderError> {
    let output = opts.output.as_ref();
    if let Some(engine) = engine {
        let caps = caps_of(engine);
        let named = match engine {
            RenderEngine::Raster => "raster",
            RenderEngine::PathTraced => "path-traced",
        };
        if !opts.aovs.is_empty() && !caps.writes_aovs {
            return Err(RenderError::OptionIneffective(format!(
                "the {named} engine writes no auxiliary passes, so --aov cannot take effect"
            )));
        }
        // Sample count, bounce budget and denoising all describe an image
        // built by accumulating frames. A backend that draws once has nowhere
        // to put them, and silently ignoring them is exactly what the doc
        // comment above refuses: the run succeeds and the setting the caller
        // asked for did nothing. Judged on the capability rather than on which
        // backend it is, so a future accumulating backend needs no change here.
        if !caps.progressive {
            for (asked, flag) in [
                (opts.samples.is_some(), "--spp"),
                (opts.bounces.is_some(), "--bounces"),
                (opts.denoise.is_some(), "--denoise/--no-denoise"),
            ] {
                if asked {
                    return Err(RenderError::OptionIneffective(format!(
                        "the {named} engine draws each pixel once rather than accumulating \
                         samples, so {flag} cannot take effect"
                    )));
                }
            }
        }
        return Ok(());
    }
    if !opts.aovs.is_empty() && matches!(output, Some(Output::Stdout)) {
        return Err(RenderError::OptionIneffective(
            "--aov writes files beside the image, and standard output has no beside".into(),
        ));
    }
    if opts.exr_space.is_some() {
        let is_exr = match output {
            Some(Output::File(p)) => files::is_exr(p),
            // Nothing is written to a path, so nothing carries a space.
            Some(Output::Stdout) | None => false,
        };
        if !is_exr {
            return Err(RenderError::OptionIneffective(
                "--exr-space applies to an .exr output; this one is not".into(),
            ));
        }
    }
    Ok(())
}

impl RenderOptions {
    /// Whether the caller has asked for the render to stop.
    #[must_use]
    pub fn cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(|f| f.load(std::sync::atomic::Ordering::Relaxed))
    }
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
/// `progress` is called between steps; pass `&mut |_| {}` to ignore it.
///
/// # Errors
/// Every way that can fail, as [`RenderError`], which the caller maps onto its
/// own exit taxonomy.
pub fn run_render(
    input: &Path,
    opts: &RenderOptions,
    sink: &mut dyn RenderSink,
) -> Result<RenderOutcome, RenderError> {
    // Wrapped so the last thing a sink hears is always either a completion or
    // the name of the step that ended it, whichever way the body left. A sink
    // that has been drawing over one line needs to know to stop.
    match run(input, opts, sink) {
        Ok(outcome) => {
            sink.report(&RenderProgress::Done {
                elapsed_ms: outcome.report.elapsed_ms,
            });
            Ok(outcome)
        }
        Err(e) => {
            sink.report(&RenderProgress::Failed { stage: e.stage() });
            Err(e)
        }
    }
}

fn run(
    input: &Path,
    opts: &RenderOptions,
    sink: &mut dyn RenderSink,
) -> Result<RenderOutcome, RenderError> {
    let started = Instant::now();
    // Before the file is opened: a request that contradicts itself is a
    // mistake in the invocation, and a build system should get it back in the
    // time it takes to parse rather than after a cook.
    check_options(opts, None)?;
    if let Some(engine) = opts.engine {
        check_options(opts, Some(engine))?;
    }
    let loaded = input::load(input, opts.cancel.as_ref(), sink)?;
    let mut warnings = loaded.warnings;
    let engine = loaded.engine;

    let settings = opts.apply_to(resolve_settings(&engine, opts, &mut warnings)?);
    // Again, now that the document has had its say: the engine can come from
    // the render node rather than from a flag, and the answer has to be the
    // same either way. Still before any device exists.
    check_options(opts, Some(settings.engine))?;
    let delta = {
        let mut e = engine;
        e.take_scene_delta()
    };

    let (device, queue) = request_device()?;
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let mut host = HeadlessHost::new(&device, &queue, format, 64, 64)
        .map_err(|e| RenderError::Device(e.to_string()))?;

    // The scene's own environment, before anything is built from it. Until
    // this existed the delta carried it and the one consumer with no window
    // dropped it, so a document lit by an image rendered from a terminal
    // against a constant and from a browser against the image.
    let environment = host.apply_scene_environment(&device, &queue, &delta);

    // A background mode is a viewing preference and a headless render has no
    // viewer whose preference it could be, so only the scene gets to move it:
    // a document that asked to be shot against its own sky is, and everything
    // else keeps the gradient the graphical shells default a pane to, which is
    // what makes the same document agree across the three surfaces.
    let background_mode = if environment.image && environment.background == BackgroundKind::HdriSky
    {
        BackgroundMode::HDRI_SKY
    } else {
        BackgroundMode::GRADIENT
    };
    let background = background_mode.resolve(&[]);
    let mut backend = build_backend(&device, &queue, &host, &settings, opts.seed, environment);
    // The ingest is where a traced render builds its ray hierarchy, and for a
    // large model that is the longest step before any pixel is drawn. Reported
    // before it rather than after, because a sink saying nothing for a minute is
    // indistinguishable from a sink that has hung.
    if settings.engine == RenderEngine::PathTraced {
        sink.report(&RenderProgress::BuildingHierarchy {
            triangles: triangle_count(&delta),
        });
    }
    backend.apply(&device, &queue, &delta);
    // Asked right after the ingest, which is where the count is made. The
    // tracer intersects triangles, so a scene carrying curves or point clouds
    // renders its triangles and drops the rest; saying so is the difference
    // between a limitation and a hole somebody finds later.
    warnings.extend(backend.skipped_primitives_warning());

    // A raster backend knows the scene's extent; a traced one is asked the same
    // question through the raster ingest that ran beside it, so both frame the
    // same subject. Failing that, the placeholder the shells seed from.
    let raster_probe = RasterBackend::new(Arc::clone(&host.renderer.layouts));
    let mut probe = raster_probe;
    probe.apply(&device, &queue, &delta);
    if let Some(b) = probe.scene().visible_bounds() {
        host.bounds = b;
    }

    let (mut camera, look) = build_camera(
        &device,
        &queue,
        &mut host,
        &probe,
        backend.as_mut(),
        &settings,
        &mut warnings,
    );

    let pds = PaneDisplaySettings::for_still(background_mode);
    let display = still_display_settings();

    let output = opts
        .output
        .clone()
        .unwrap_or_else(|| Output::File(PathBuf::from("render.png")));

    let mut job = StillRenderJob::new(still_spec(&settings, &output, opts));
    let spec = job.spec();
    let tiles = job.plan().len();

    let scene_present = probe.scene().draw_objects().next().is_some();
    let assembled = drive(
        &mut Drive {
            device: &device,
            queue: &queue,
            host: &mut host,
            camera: &mut camera,
            job: &mut job,
            backend: backend.as_mut(),
            sink,
        },
        &StillView {
            cancel: opts,
            pds,
            display,
            background,
            look,
            format,
            scene_present,
            engine: settings.engine,
        },
        started,
    )?;

    // One event, naming the image. The passes are named in the report instead:
    // the plain sink collapses repeated steps, so four `Writing` lines would
    // show as one anyway, and a machine reading the result wants the list.
    sink.report(&RenderProgress::Writing {
        output: output.display(),
    });
    let aovs = write_all(&output, opts, assembled, spec, &mut warnings)?;

    Ok(RenderOutcome {
        report: report(&output, spec, &settings, tiles, started, warnings, aovs),
    })
}

/// Encodes the picture, writes it, and writes every pass beside it.
///
/// The image first, because it is what was asked for and a pass without one is
/// of no use to anybody.
fn write_all(
    output: &Output,
    opts: &RenderOptions,
    assembled: Assembled,
    spec: StillSpec,
    warnings: &mut Vec<String>,
) -> Result<Vec<String>, RenderError> {
    // Taken apart rather than borrowed, so the colour moves into the encoder
    // instead of being copied for it.
    let Assembled { color, aux, depth } = assembled;
    let encoded = if spec.readback == solarxy_host::still::StillReadback::Display8 {
        solarxy_formats::export::encode_png_bytes(&solarxy_core::RawImageData::new(
            color,
            spec.width,
            spec.height,
        ))?
    } else {
        solarxy_formats::export::encode_exr_rgb_bytes(&solarxy_core::RawImageHdr::new(
            files::rgb_from_rgba(&files::floats_of(&color)),
            spec.width,
            spec.height,
        ))?
    };
    output.write(&encoded)?;
    write_passes(
        output,
        opts,
        aux.as_deref(),
        depth.as_deref(),
        spec,
        warnings,
    )
}

/// Writes every requested pass beside the image, and names what it wrote.
fn write_passes(
    output: &Output,
    opts: &RenderOptions,
    aux: Option<&[u8]>,
    depth: Option<&[u8]>,
    spec: StillSpec,
    warnings: &mut Vec<String>,
) -> Result<Vec<String>, RenderError> {
    let Output::File(image_path) = output else {
        return Ok(Vec::new());
    };
    let mut written = Vec::new();
    for kind in &opts.aovs {
        let plane = match kind {
            AovKind::Albedo => aux.map(|a| files::albedo_from_auxiliary(&files::floats_of(a))),
            AovKind::Normal => aux.map(|a| files::normal_from_auxiliary(&files::floats_of(a))),
            AovKind::Depth => depth.map(files::floats_of),
        };
        let Some(plane) = plane else {
            // Refused before the render for every case that can be judged
            // ahead of time, so reaching here means the backend declared the
            // pass and then had none. Said out loud rather than written as a
            // black file.
            warnings.push(format!(
                "the renderer produced no {} pass; it was not written",
                kind.as_str()
            ));
            continue;
        };
        let path = files::sibling(image_path, *kind);
        files::write_pass(&path, *kind, &plane, spec.width, spec.height)?;
        written.push(
            std::fs::canonicalize(&path)
                .unwrap_or(path)
                .display()
                .to_string(),
        );
    }
    Ok(written)
}

/// What the run produced, as the machine-readable result.
#[allow(clippy::cast_possible_truncation)]
fn report(
    output: &Output,
    spec: StillSpec,
    settings: &RenderSettings,
    tiles: usize,
    started: Instant,
    warnings: Vec<String>,
    aovs: Vec<String>,
) -> RenderReport {
    RenderReport {
        schema_version: RENDER_REPORT_SCHEMA_VERSION,
        solarxy_version: env!("CARGO_PKG_VERSION"),
        output: output.display(),
        width: spec.width,
        height: spec.height,
        engine: match settings.engine {
            RenderEngine::PathTraced => "pathTraced",
            RenderEngine::Raster => "raster",
        },
        // A rasterized pixel does not converge, it is drawn, so the count that
        // means something for one engine means nothing for the other.
        samples: match settings.engine {
            RenderEngine::PathTraced => spec.samples,
            RenderEngine::Raster => 1,
        },
        tiles: tiles as u32,
        elapsed_ms: started.elapsed().as_millis() as u64,
        warnings,
        aovs,
    }
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

/// How many triangles a delta puts in front of the hierarchy builder.
///
/// Read off the upserts rather than off the backend, because the point of
/// reporting it is to say how much work is about to start, and by the time a
/// backend could answer the work is done.
fn triangle_count(delta: &solarxy_core::scene::SceneDelta) -> u64 {
    delta
        .ops
        .iter()
        .filter_map(|op| match op {
            solarxy_core::scene::SceneOp::UpsertGeometry { geometry, .. } => Some(geometry),
            _ => None,
        })
        .flat_map(|g| g.meshes.iter())
        .map(|m| (m.indices.len() / 3) as u64)
        .sum()
}

/// The still the settings describe, written where the caller asked.
///
/// The output's extension chooses the depth, because the file format is the
/// only place the choice is visible: asking for eight bits in a container that
/// holds floats would throw the render away, and asking for floats in a PNG
/// cannot be honoured. `--exr-space` then chooses which floats, and its default
/// is scene-referred, because a compositing package has not decided the look
/// yet and a tone-mapped float is a decision already taken.
fn still_spec(settings: &RenderSettings, output: &Output, opts: &RenderOptions) -> StillSpec {
    use solarxy_host::still::StillReadback;
    let readback = match output {
        Output::File(path) if files::is_exr(path) => match opts.exr_space.unwrap_or_default() {
            ExrSpace::SceneLinear => StillReadback::SceneLinear,
            ExrSpace::Display => StillReadback::DisplayFloat,
        },
        Output::File(_) | Output::Stdout => StillReadback::Display8,
    };
    StillSpec {
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
        tile_budget: opts
            .tile_budget
            .unwrap_or(solarxy_host::still::TILE_BUDGET_PIXELS),
        readback,
        // Albedo and normal come out of one store, so either of them asks for
        // the same copy.
        aux: opts.aovs.iter().any(|k| k.from_auxiliary()),
        depth: opts.aovs.contains(&AovKind::Depth),
    }
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

/// Copies one finished plane of one tile into the assembled image.
///
/// Takes its pixel width rather than assuming four, because the same body
/// assembles an eight-bit colour, a four-float colour, a four-float auxiliary
/// and a one-float depth.
fn blit(
    image: &mut [u8],
    image_width: u32,
    rect: solarxy_host::still::TileRect,
    plane: &[u8],
    bytes_per_pixel: usize,
) {
    let stride = image_width as usize * bytes_per_pixel;
    let row = rect.width as usize * bytes_per_pixel;
    for y in 0..rect.height as usize {
        let src = y * row;
        let dst = (rect.y as usize + y) * stride + rect.x as usize * bytes_per_pixel;
        if dst + row <= image.len() && src + row <= plane.len() {
            image[dst..dst + row].copy_from_slice(&plane[src..src + row]);
        }
    }
}

/// The whole picture, and whatever passes were asked for beside it.
///
/// Every plane is bytes rather than typed samples for the reason the tiles are:
/// one assembly body serves all of them, and the reader at the far end knows
/// what it asked for.
struct Assembled {
    color: Vec<u8>,
    aux: Option<Vec<u8>>,
    depth: Option<Vec<u8>>,
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
/// Bundled for the reason the still job's own context is bundled: the driver
/// takes a handful of references already and a longer argument list is not
/// clearer than a name. [`Drive`] below is the mutable half of the same idea.
struct StillView<'a> {
    /// Read between tiles, which is where an interrupt takes effect.
    cancel: &'a RenderOptions,
    pds: PaneDisplaySettings,
    display: DisplaySettings,
    background: solarxy_core::preferences::ResolvedBackground,
    look: solarxy_renderer::composite::CompositeLook,
    format: wgpu::TextureFormat,
    scene_present: bool,
    /// Carried from the resolved settings rather than mapped back from the
    /// job's spec, so the still-engine translation stays written once.
    engine: RenderEngine,
}

/// Runs the job to completion, resizing the shared targets per tile, and
/// returns the assembled image.
///
/// The resize is the caller's job by the still job's own contract: the two
/// shells resize with different policy around one body, so the job asserts
/// rather than resizes.
struct Drive<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    host: &'a mut HeadlessHost,
    camera: &'a mut CameraState,
    job: &'a mut StillRenderJob,
    backend: &'a mut dyn RenderBackend,
    sink: &'a mut dyn RenderSink,
}

fn drive(
    d: &mut Drive<'_>,
    view: &StillView<'_>,
    started: Instant,
) -> Result<Assembled, RenderError> {
    let Drive {
        device,
        queue,
        host,
        camera,
        job,
        backend,
        sink,
    } = d;
    let spec = job.spec();
    let pixels = (spec.width as usize) * (spec.height as usize);
    let color_bpp = spec.readback.bytes_per_pixel();
    let mut out = Assembled {
        color: vec![0u8; pixels * color_bpp],
        // Allocated on the spec rather than on the first tile that carries one,
        // so a backend that goes quiet halfway leaves a hole rather than a
        // shorter picture.
        aux: spec.aux.then(|| vec![0u8; pixels * AUX_BYTES]),
        depth: spec.depth.then(|| vec![0u8; pixels * DEPTH_BYTES]),
    };
    // Read once: the shape of the plan is fixed when the job is built, and
    // asking inside the loop would borrow the job where it has to be advanced.
    let (columns, rows) = {
        let plan = job.plan();
        (plan.columns, plan.rows)
    };
    while let Some(tile) = job.current() {
        if view.cancel.cancelled() {
            return Err(RenderError::Cancelled);
        }
        let p = job.progress();
        #[allow(clippy::cast_possible_truncation)]
        sink.report(&RenderProgress::Sampling {
            tile: p.tile,
            tiles: p.tiles,
            columns,
            rows,
            sample: p.sample,
            samples: p.samples,
            elapsed_ms: started.elapsed().as_millis() as u64,
        });
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
            job.advance(&mut ctx, *backend)
        };
        match step {
            StillStep::Working => {}
            StillStep::Tile => {
                while let Some(t) = job.take_tile() {
                    blit(&mut out.color, spec.width, t.rect, &t.pixels, color_bpp);
                    if let (Some(image), Some(plane)) = (out.aux.as_mut(), t.aux.as_ref()) {
                        blit(image, spec.width, t.rect, plane, AUX_BYTES);
                    }
                    if let (Some(image), Some(plane)) = (out.depth.as_mut(), t.depth.as_ref()) {
                        blit(image, spec.width, t.rect, plane, DEPTH_BYTES);
                    }
                }
                // Here and nowhere else: this is the only moment the picture
                // has changed. A sink that shows it redraws once a tile, which
                // is why a surface that wants to see a render converge asks for
                // a smaller budget than one tile of the whole image.
                sink.preview(&Preview {
                    width: spec.width,
                    height: spec.height,
                    pixels: &out.color,
                    format: if spec.readback == solarxy_host::still::StillReadback::Display8 {
                        PreviewFormat::Rgba8
                    } else {
                        PreviewFormat::Rgba32F
                    },
                    aux: out.aux.as_deref(),
                    depth: out.depth.as_deref(),
                    engine: view.engine,
                });
            }
            StillStep::Done => break,
            StillStep::Failed => return Err(RenderError::DeviceLost),
        }
    }
    Ok(out)
}

/// Four `f32`: albedo and the packed normal, as the still job hands them over.
const AUX_BYTES: usize = 16;
/// One `f32`.
const DEPTH_BYTES: usize = 4;

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
    environment: EnvironmentRequest,
) -> Box<dyn RenderBackend> {
    match settings.engine {
        // Nothing to install: the raster path reads image-based lighting off
        // the host's renderer, which already has the scene's, and the sky it
        // draws is the background resolved beside it.
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
            // The traced scene cache drops the environment op by design, on
            // the reasoning that a host already holds the decoded and
            // convolved image and should build from that rather than keep a
            // second copy of the largest asset in a scene. This is the third
            // surface's half of that decision. Nothing is uploaded twice: the
            // equirect the sky pass retains and the equirect the kernel walks
            // are one texture, so only the two distribution tables are built.
            let ibl = &host.renderer.ibl_res.ibl;
            match (
                environment.image,
                ibl.equirect.as_ref(),
                ibl.distribution.as_ref(),
            ) {
                (true, Some(equirect), Some(distribution)) => {
                    let built = TraceEnvironment::from_shared_equirect(
                        device,
                        queue,
                        &equirect.view,
                        distribution,
                    );
                    t.set_environment(device, built, environment.intensity, environment.rotation);
                }
                // No image is no environment, and black is how the kernel is
                // told so: it drops an all-zero sky from the direct-lighting
                // estimator's choice entirely, rather than spending half of
                // every draw connecting to something that returns nothing. A
                // render is a property of the scene, so a document lit by
                // nothing but its own lights renders that way here, the same
                // as it does in a still from the browser. The raster arm above
                // keeps the resolved background instead, because there
                // image-based lighting is the ambient term rather than one arm
                // of an estimator, and a shell's raster still keeps its
                // background too.
                _ => t.set_sky([0.0; 3], [0.0; 3]),
            }
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
///
/// Both halves of the rig are applied here, from the shot's camera: the
/// rasterizer's, which is a uniform, and the tracer's, which is scene data,
/// because a tracer binds no lights uniform and would otherwise render this
/// scene by the environment alone.
fn build_camera(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    host: &mut HeadlessHost,
    probe: &RasterBackend,
    backend: &mut dyn RenderBackend,
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
    // The lens rides with the camera it belongs to, and a shot with no camera
    // is a pinhole. Set unconditionally so a render that falls back to framing
    // the bounds cannot inherit an aperture from anywhere.
    backend.set_lens(
        camera_def
            .as_ref()
            .map(solarxy_host::cameras::lens_for)
            .unwrap_or_default(),
    );
    if let Some(def) = camera_def.as_ref() {
        solarxy_host::cameras::apply_camera_def(&mut camera.camera, def);
        // After the definition, not before: the shot's aspect comes from the
        // image being rendered rather than from whatever the camera was authored
        // against.
        camera.camera.aspect = aspect;
    }

    if probe.scene().authored_lights().is_none() {
        let cam_data = camera.camera;
        solarxy_host::setup_pane_lighting(
            queue,
            &mut host.env,
            &cam_data,
            &host.bounds,
            host.renderer.ibl_res.ibl.irradiance_average,
        );
        solarxy_host::apply_viewer_rig(device, queue, backend, probe.scene(), &cam_data);
    }

    let look = camera_def
        .as_ref()
        .map(|d| solarxy_renderer::composite::resolve_look(Some(&d.look), &PaneLook::default()))
        .unwrap_or_default();
    (camera, look)
}
