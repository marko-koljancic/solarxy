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
use solarxy_renderer::pathtrace::backend::{PathBackend, TraceSettings};
use solarxy_renderer::pathtrace::denoise::DenoiseSettings;
use solarxy_renderer::pathtrace::environment::TraceEnvironment;

pub use error::RenderError;
// The pass extraction helpers travel with the preview: a sink that shows a
// pass must read the planes exactly the way the file writer reads them, or
// the window and the sibling file would disagree about one buffer.
pub use files::{AovKind, ExrSpace, albedo_from_auxiliary, floats_of, normal_from_auxiliary};
/// The pass selector and the display mappings, re-exported for the same reason
/// as `float_to_rgba8` above: they live in the shared crate so the browser can
/// reach them, and a shell that already depends on this one should not have to
/// take a second dependency to name a pass.
pub use solarxy_host::passes::{PassKind, PassSelector, albedo_rgba8, depth_rgba8, normal_rgba8};
/// The remaining-time estimate and the one spelling of a span, re-exported for
/// the same reason: every surface that shows a render's progress reads them, so
/// there is one answer to how long is left and one way of writing it down.
pub use solarxy_host::still::{estimate_remaining_ms, format_duration_ms};
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
        /// How far along the whole picture is, in pixel-samples, weighted by
        /// the area each tile owns.
        ///
        /// Carried rather than derived from the counts above, because only the
        /// job knows how big each tile is: the plan's last column and bottom
        /// row are whatever is left over, and a reader that treated every tile
        /// as equal would report an estimate that runs long at the end. A sink
        /// decides nothing about how far along a render is; it is told.
        drawn: u64,
        total: u64,
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
/// Defined beside the job's own budget in the shared crate, so a surface that
/// cannot reach this one can still ask for it.
pub use solarxy_host::still::PREVIEW_TILE_BUDGET;

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
pub fn caps_of(engine: RenderEngine) -> solarxy_renderer::backend::BackendCaps {
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
        // The seed reached the backend directly until the render node carried
        // one, which made it the only render setting a document could not
        // state. It overrides here now, like every other flag, so a script that
        // pins a seed still pins it and a document that names one is finally
        // read.
        if let Some(v) = self.seed {
            settings.seed = v;
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
    let mut backend = build_backend(&device, &queue, &host, &settings, environment);
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
/// What a cooked document renders at, before any command-line override.
///
/// Public so the desktop shell's own resolution can be pinned against it: the
/// two shells read one document and must read it the same way, and the only
/// thing that could prove that was a comparison neither of them could make
/// alone.
pub fn resolve_settings(
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
        // No mid-tile previews. A sink here is fed a whole picture when a tile
        // lands, and a surface that wants to watch one arrive asks for smaller
        // tiles instead; paying for a composite and a readback four times a
        // second in a render nobody is watching would be a tax on every
        // scripted run.
        preview_interval_ms: 0,
    }
}

/// What a document with no render node renders at.
///
/// The node type's own descriptor defaults, read through the engine rather than
/// restated here. This was a hand-written literal until the render node's third
/// version, kept in step with the desktop's copy by a test comparing fields one
/// at a time; a field added to the settings and forgotten in one of them would
/// have rendered differently depending on which surface you asked from.
fn default_settings() -> RenderSettings {
    RenderSettings::defaults()
}

/// The tracer configured the way the render node asks for.
///
/// Destructured exhaustively on purpose, and that is what the function is for
/// rather than a style choice: a value added to `RenderSettings` stops this
/// compiling until this surface says what happens to it. Earlier in this
/// release the camera's aperture resolved correctly out of a document and then
/// reached no renderer, and every test passed because they all used an aperture
/// of zero. A test can catch a value wired to the wrong field; only the
/// compiler catches one wired nowhere.
///
/// The three surfaces cannot share this. `solarxy-host` is where shared host
/// behaviour goes and it deliberately has no `solarxy-graph` dependency, while
/// the engine must not see the renderer, so the boundary refuses a common home
/// in both directions.
fn trace_settings_for(settings: &RenderSettings) -> TraceSettings {
    let RenderSettings {
        // The shot itself, read by the still spec and the job's camera.
        camera: _,
        width: _,
        height: _,
        engine: _,
        samples,
        bounces,
        transmissive_bounces,
        firefly_clamp,
        seed,
        denoise,
        denoise_until_samples,
        // The four that steer the filter are configured by their own setter
        // rather than on here. See `denoise_settings_for` below.
        denoise_strength: _,
        denoise_sigma_color: _,
        denoise_normal_power: _,
        denoise_sigma_albedo: _,
        denoise_level_falloff: _,
        // The film back and what is written beside the picture: the still
        // spec's business and the encoder's, not the tracer's.
        transparent_background: _,
        aov_albedo: _,
        aov_normal: _,
        aov_depth: _,
    } = *settings;
    let samples = samples.max(1);
    TraceSettings {
        samples,
        bounces,
        transmissive_bounces,
        firefly_clamp,
        seed,
        denoise,
        denoise_until_samples,
        // The job takes its chunk from the backend by design: a browser paces
        // one sample per frame to stay responsive and a terminal has no frame
        // to pace against.
        chunk: 8.min(samples),
        // The lens is installed separately, from the camera the shot names.
        ..TraceSettings::default()
    }
}

/// How the render node asks for the filter to be steered.
///
/// Strength multiplies the colour tolerance rather than being a fifth
/// independent number, because that tolerance is the value that most changes
/// the outcome: any other expression of it would leave the advanced controls
/// holding a value the everyday one could contradict.
fn denoise_settings_for(settings: &RenderSettings) -> DenoiseSettings {
    DenoiseSettings {
        sigma_color: settings.denoise_sigma_color * settings.denoise_strength,
        normal_power: settings.denoise_normal_power,
        sigma_albedo: settings.denoise_sigma_albedo,
        level_falloff: settings.denoise_level_falloff,
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

/// A device with no surface, asking for what the GPU shells ask for.
///
/// The core WebGPU defaults are the floor, and the two buffer size limits are
/// raised off the adapter exactly as the browser and the desktop raise them,
/// through the same helper. This surface carried the identical 256 MiB ceiling
/// and would have refused a large model from the command line for the same
/// reason the browser did.
///
/// It does not make an image this renderer could not otherwise produce. A limit
/// governs what fits, not what is drawn, so the picture is unchanged and only
/// the size of scene that reaches the GPU moves.
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
        required_limits: solarxy_renderer::limits::required_limits(&adapter.limits()),
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
            drawn: p.drawn,
            total: p.total,
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
                // The clock this crate already started for its progress
                // stream, so the preview's throttle and the elapsed a reader
                // sees are measured against one reading.
                now_ms: started.elapsed().as_millis() as u64,
            };
            job.advance(&mut ctx, *backend)
        };
        match step {
            // Unreachable: this crate asks for no previews, because its reader
            // is fed when a tile lands and an ordinary headless render has
            // nobody watching it converge.
            StillStep::Working | StillStep::Preview => {}
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
    environment: EnvironmentRequest,
) -> Box<dyn RenderBackend> {
    match settings.engine {
        // Nothing to install: the raster path reads image-based lighting off
        // the host's renderer, which already has the scene's, and the sky it
        // draws is the background resolved beside it.
        RenderEngine::Raster => Box::new(RasterBackend::new(Arc::clone(&host.renderer.layouts))),
        RenderEngine::PathTraced => {
            let mut t = PathBackend::new(device, queue);
            t.set_settings(trace_settings_for(settings));
            t.set_denoise_settings(denoise_settings_for(settings));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every value the render node authors for the walk reaches the tracer.
    ///
    /// The values are deliberately nothing like the defaults. A test written
    /// with the defaults passes with the assignment deleted, which is exactly
    /// how the camera's aperture reached no renderer for a whole release while
    /// every test stayed green.
    #[test]
    fn every_authored_value_reaches_the_tracer() {
        let mut s = RenderSettings::defaults();
        s.samples = 91;
        s.bounces = 13;
        s.transmissive_bounces = 7;
        s.firefly_clamp = 3.5;
        s.seed = 4242;
        s.denoise = true;

        let t = trace_settings_for(&s);
        assert_eq!(t.samples, 91);
        assert_eq!(t.bounces, 13);
        assert_eq!(t.transmissive_bounces, 7);
        assert!((t.firefly_clamp - 3.5).abs() < f32::EPSILON);
        assert_eq!(t.seed, 4242);
        assert!(t.denoise);
        assert_eq!(t.chunk, 8, "a terminal has no frame to pace against");
    }

    /// The four steering values reach the filter, and strength multiplies the
    /// one it is documented to multiply.
    ///
    /// Deliberately distinct values, so a field wired to its neighbour fails
    /// rather than passing on a coincidence.
    #[test]
    fn the_steering_values_reach_the_filter() {
        let mut s = RenderSettings::defaults();
        s.denoise_sigma_color = 2.0;
        s.denoise_normal_power = 33.0;
        s.denoise_sigma_albedo = 0.5;
        s.denoise_level_falloff = 3.0;
        s.denoise_strength = 1.5;

        let d = denoise_settings_for(&s);
        assert!(
            (d.sigma_color - 3.0).abs() < f32::EPSILON,
            "strength multiplies the colour tolerance: 2.0 at 1.5 is 3.0"
        );
        assert!((d.normal_power - 33.0).abs() < f32::EPSILON);
        assert!((d.sigma_albedo - 0.5).abs() < f32::EPSILON);
        assert!((d.level_falloff - 3.0).abs() < f32::EPSILON);
    }

    /// At the default strength the filter runs at exactly its measured values.
    ///
    /// The multiplier is what makes this worth asserting: a strength that
    /// defaulted to anything but one would silently retune every existing
    /// render the moment the control shipped.
    #[test]
    fn the_defaults_are_the_measured_values_untouched() {
        let d = denoise_settings_for(&RenderSettings::defaults());
        assert_eq!(d, DenoiseSettings::default());
    }

    /// The threshold reaches the walk's settings, where the gate reads it.
    #[test]
    fn the_denoise_threshold_reaches_the_tracer() {
        let mut s = RenderSettings::defaults();
        s.denoise = true;
        s.denoise_until_samples = 40;
        let t = trace_settings_for(&s);
        assert_eq!(t.denoise_until_samples, 40);
        assert!(t.filtering_at(40));
        assert!(!t.filtering_at(41));
    }

    /// A render shorter than a chunk submits the render, not the chunk.
    #[test]
    fn the_chunk_never_outruns_the_render() {
        let mut s = RenderSettings::defaults();
        s.samples = 3;
        assert_eq!(trace_settings_for(&s).chunk, 3);

        s.samples = 0;
        let t = trace_settings_for(&s);
        assert_eq!(
            (t.samples, t.chunk),
            (1, 1),
            "a render of no samples still draws one, rather than looping on a \
             chunk of zero"
        );
    }

    /// The document supplies the seed when no flag does.
    ///
    /// The seed used to bypass the settings entirely and reach the backend on
    /// its own, which made it the only render setting a document could state
    /// and not be read for. This is the half of the precedence that regressed
    /// when it moved.
    #[test]
    fn the_node_supplies_the_seed_when_no_flag_does() {
        let mut settings = RenderSettings::defaults();
        settings.seed = 1234;
        let opts = RenderOptions::default();
        assert_eq!(opts.seed, None, "the flag is what this test is without");
        assert_eq!(opts.apply_to(settings).seed, 1234);
    }

    /// And a flag still overrides it, which is the other half.
    ///
    /// A flag that could not override the document would be useless in a build
    /// system, which is the rule every other override on here follows.
    #[test]
    fn a_seed_flag_overrides_the_node() {
        let mut settings = RenderSettings::defaults();
        settings.seed = 1234;
        let opts = RenderOptions {
            seed: Some(99),
            ..RenderOptions::default()
        };
        assert_eq!(opts.apply_to(settings).seed, 99);
    }

    /// An exact sample count and a clamp survive the flags that do not name
    /// them.
    ///
    /// `apply_to` overwrites field by field, so a value with no flag beside it
    /// has to come through untouched rather than being reset to a default on
    /// the way past.
    #[test]
    fn values_with_no_flag_of_their_own_survive_the_overrides() {
        let mut settings = RenderSettings::defaults();
        settings.samples = 90;
        settings.firefly_clamp = 2.5;
        settings.seed = 1234;

        // A run that overrides something else entirely.
        let opts = RenderOptions {
            width: Some(640),
            ..RenderOptions::default()
        };
        let out = opts.apply_to(settings);
        assert_eq!(out.width, 640);
        assert_eq!(out.samples, 90, "the exact count did not survive");
        assert!(
            (out.firefly_clamp - 2.5).abs() < f32::EPSILON,
            "the clamp did not survive; it has no flag, so nothing else would \
             have restored it"
        );
        assert_eq!(out.seed, 1234);
    }
}

/// The render node's size limits and the still job's are the same numbers.
///
/// They are two copies: the node declares a hard range on its width and height,
/// and [`StillSpec`] clamps to [`solarxy_host::still::MAX_STILL_EDGE`]. Nothing
/// tied them together, and they cannot be tied by construction, because the
/// engine does not depend on the host and must not. This crate depends on both,
/// so it is the first place the two are visible at once and the only place the
/// comparison can be made at all.
///
/// What it prevents is a size preset that is offered and then refused. A node
/// that let somebody choose eight thousand pixels while the job silently
/// clamped to four would render a picture at a size nobody asked for, and the
/// dialog would have said the size it did not get.
#[cfg(test)]
mod size_limits {
    use solarxy_host::still::MAX_STILL_EDGE;

    /// The node's own hard range on an edge.
    fn node_edge_range(key: &str) -> (f64, f64) {
        let engine = solarxy_graph::Engine::new().expect("builtin registry");
        let desc = engine
            .registry()
            .get("render")
            .expect("the render node is registered");
        desc.param(key)
            .and_then(|p| p.range)
            .unwrap_or_else(|| panic!("{key} declares a hard range"))
            .hard
    }

    #[test]
    fn the_node_offers_exactly_the_sizes_the_job_will_render() {
        for key in ["width", "height"] {
            let (min, max) = node_edge_range(key);
            assert!(
                (max - f64::from(MAX_STILL_EDGE)).abs() < f64::EPSILON,
                "the render node offers {key} up to {max} while the still job \
                 clamps to {MAX_STILL_EDGE}, so a size in between is offered \
                 and then quietly changed"
            );
            // The floor matters for the same reason and is the same number on
            // both sides today.
            assert!(
                min >= 16.0,
                "the node's {key} floor of {min} is under the job's, so a small \
                 render would be silently enlarged"
            );
        }
    }

    /// And every named size fits inside it.
    ///
    /// Derived from the registry rather than restated, so a preset added later
    /// is checked without this test being touched. The engine's own suite
    /// checks each preset against the node's declared range; this checks the
    /// same presets against what the renderer will actually accept, which is
    /// the number the criterion is really about.
    #[test]
    fn every_named_size_is_one_the_job_will_render() {
        use solarxy_graph::registry::param_spec::ParamType;

        let engine = solarxy_graph::Engine::new().expect("builtin registry");
        let desc = engine
            .registry()
            .get("render")
            .expect("the render node is registered");
        let spec = desc
            .param("resolution_preset")
            .expect("the render node declares an output size preset");
        let ParamType::Enum { variants } = &spec.ty else {
            panic!("the output size preset stopped being an enum");
        };

        let mut checked = 0;
        for variant in variants {
            let Some((w, h)) = solarxy_graph::nodes::resolution_preset_size(&variant.key) else {
                continue;
            };
            checked += 1;
            assert!(
                w <= MAX_STILL_EDGE && h <= MAX_STILL_EDGE,
                "the preset {:?} is {w} by {h}, which the still job would clamp \
                 to {MAX_STILL_EDGE}: offer a size the renderer accepts rather \
                 than one it refuses",
                variant.key
            );
            assert!(
                w >= 16 && h >= 16,
                "the preset {:?} is smaller than the job's floor",
                variant.key
            );
        }
        assert!(checked > 0, "no preset resolved to a size to check");
    }
}
