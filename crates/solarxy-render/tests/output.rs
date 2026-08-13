//! What a render writes, and that it writes the same thing twice.
//!
//! Two halves with different needs. The refusals need no adapter at all, by
//! design: an option that cannot take effect is decided before the file is
//! opened and before a device is requested, so a machine with no GPU still
//! runs those. The reproducibility check needs a real one, and skips where
//! there is none, the same way every other GPU test in this workspace does.

use std::path::{Path, PathBuf};

use solarxy_render::{AovKind, ExrSpace, Output, RenderEngine, RenderError, RenderOptions};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels below the workspace root")
        .to_path_buf()
}

fn model() -> PathBuf {
    repo_root().join("res/models/armadillo.obj")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("solarxy-render-output")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

fn render(opts: &RenderOptions) -> Result<solarxy_render::RenderOutcome, RenderError> {
    solarxy_render::run_render(&model(), opts, &mut solarxy_render::Silent)
}

/// The message a refusal carries, or a panic naming what happened instead.
fn refusal(opts: &RenderOptions) -> String {
    match render(opts) {
        Err(RenderError::OptionIneffective(message)) => message,
        Err(other) => panic!("expected a refusal, got: {other}"),
        Ok(_) => panic!("expected a refusal, and it rendered"),
    }
}

#[test]
fn a_rasterized_render_refuses_auxiliary_passes_rather_than_dropping_them() {
    let message = refusal(&RenderOptions {
        output: Some(Output::File(PathBuf::from("/dev/null.exr"))),
        engine: Some(RenderEngine::Raster),
        aovs: vec![AovKind::Albedo],
        ..RenderOptions::default()
    });
    assert!(message.contains("--aov"), "unhelpful message: {message}");
}

/// And it decides that without a GPU, which is the point of reading the
/// capability off a constant rather than off an instance.
///
/// A refusal that needed a device would report "no GPU adapter" on the machines
/// most likely to be running a render command by mistake, and would be
/// untestable on the continuous-integration runner.
#[test]
fn the_refusal_arrives_before_anything_is_loaded_or_started() {
    let opts = RenderOptions {
        output: Some(Output::File(PathBuf::from("/dev/null.exr"))),
        engine: Some(RenderEngine::Raster),
        aovs: vec![AovKind::Depth],
        ..RenderOptions::default()
    };
    // An input that does not exist. Reaching the loader would report that
    // instead, and reaching a device would report the adapter.
    let outcome = solarxy_render::run_render(
        Path::new("no-such-scene.slxy"),
        &opts,
        &mut solarxy_render::Silent,
    );
    assert!(
        matches!(outcome, Err(RenderError::OptionIneffective(_))),
        "the request was checked too late"
    );
}

#[test]
fn a_display_space_is_refused_where_no_float_file_can_carry_it() {
    let message = refusal(&RenderOptions {
        output: Some(Output::File(PathBuf::from("/dev/null.png"))),
        exr_space: Some(ExrSpace::Display),
        ..RenderOptions::default()
    });
    assert!(
        message.contains("--exr-space"),
        "unhelpful message: {message}"
    );
}

#[test]
fn passes_are_refused_when_there_is_nowhere_beside_the_image_to_put_them() {
    let message = refusal(&RenderOptions {
        output: Some(Output::Stdout),
        engine: Some(RenderEngine::PathTraced),
        aovs: vec![AovKind::Normal],
        ..RenderOptions::default()
    });
    assert!(message.contains("--aov"), "unhelpful message: {message}");
}

/// Two runs of one seed, compared byte for byte, beauty and every pass.
///
/// The acceptance criterion this release owes a pipeline: without it a render
/// check in continuous integration cannot exist, because there is nothing to
/// diff against.
#[test]
fn the_same_seed_writes_the_same_bytes() {
    let dir = scratch("reproducible");
    let mut wrote = Vec::new();
    for run in 0..2 {
        let out = dir.join(format!("run{run}.exr"));
        let opts = RenderOptions {
            output: Some(Output::File(out.clone())),
            engine: Some(RenderEngine::PathTraced),
            width: Some(96),
            height: Some(72),
            samples: Some(4),
            seed: Some(7),
            aovs: vec![AovKind::Albedo, AovKind::Normal, AovKind::Depth],
            ..RenderOptions::default()
        };
        match render(&opts) {
            Ok(outcome) => {
                assert_eq!(
                    outcome.report.aovs.len(),
                    3,
                    "the report did not name every pass it wrote"
                );
                let mut files = vec![std::fs::read(&out).expect("the image")];
                for pass in ["albedo", "normal", "depth"] {
                    let sibling = dir.join(format!("run{run}.{pass}.exr"));
                    assert!(sibling.exists(), "no {pass} pass beside the image");
                    files.push(std::fs::read(&sibling).expect("the pass"));
                }
                wrote.push(files);
            }
            Err(RenderError::NoAdapter) => {
                assert!(
                    std::env::var("SOLARXY_REQUIRE_GPU").as_deref() != Ok("1"),
                    "SOLARXY_REQUIRE_GPU=1 but no usable GPU adapter"
                );
                eprintln!("skipping: no GPU adapter available");
                return;
            }
            Err(other) => panic!("the render failed: {other}"),
        }
    }

    let (first, second) = (&wrote[0], &wrote[1]);
    for (i, name) in ["image", "albedo", "normal", "depth"].iter().enumerate() {
        assert_eq!(
            first[i].len(),
            second[i].len(),
            "the {name} differed in length between two runs of one seed"
        );
        assert!(
            first[i] == second[i],
            "the {name} differed between two runs of one seed"
        );
    }
}

/// Notes what the last preview claimed about itself.
///
/// The watch window picks a pass from what the run requested and says which
/// engine drew the picture; both facts ride the preview, so they are pinned
/// at the seam a surface actually reads rather than inferred from the files.
#[derive(Default)]
struct PlaneWitness {
    saw: bool,
    aux: bool,
    depth: bool,
    engine: Option<RenderEngine>,
}

impl solarxy_render::RenderSink for PlaneWitness {
    fn report(&mut self, _progress: &solarxy_render::RenderProgress) {}

    fn preview(&mut self, image: &solarxy_render::Preview<'_>) {
        let pixels = (image.width as usize) * (image.height as usize);
        if let Some(plane) = image.aux {
            assert_eq!(plane.len(), pixels * 16, "the aux plane is not whole");
        }
        if let Some(plane) = image.depth {
            assert_eq!(plane.len(), pixels * 4, "the depth plane is not whole");
        }
        self.saw = true;
        self.aux = image.aux.is_some();
        self.depth = image.depth.is_some();
        self.engine = Some(image.engine);
    }
}

/// The preview carries a pass exactly when the run asked for it, and names
/// the engine that drew the picture.
#[test]
fn the_preview_carries_the_passes_the_run_asked_for() {
    let cases: [(RenderEngine, Vec<AovKind>, bool, bool); 3] = [
        (
            RenderEngine::PathTraced,
            vec![AovKind::Albedo, AovKind::Depth],
            true,
            true,
        ),
        (RenderEngine::PathTraced, vec![], false, false),
        (RenderEngine::Raster, vec![], false, false),
    ];
    for (engine, aovs, want_aux, want_depth) in cases {
        let dir = scratch(&format!("preview-planes-{engine:?}-{}", aovs.len()));
        let opts = RenderOptions {
            output: Some(Output::File(dir.join("out.png"))),
            engine: Some(engine),
            width: Some(64),
            height: Some(48),
            samples: Some(2),
            seed: Some(3),
            aovs,
            ..RenderOptions::default()
        };
        let mut sink = PlaneWitness::default();
        match solarxy_render::run_render(&model(), &opts, &mut sink) {
            Ok(_) => {
                assert!(sink.saw, "{engine:?}: no preview arrived at all");
                assert_eq!(
                    sink.aux, want_aux,
                    "{engine:?}: the aux plane did not match the request"
                );
                assert_eq!(
                    sink.depth, want_depth,
                    "{engine:?}: the depth plane did not match the request"
                );
                assert_eq!(
                    sink.engine,
                    Some(engine),
                    "the preview named the wrong engine"
                );
            }
            Err(RenderError::NoAdapter) => {
                assert!(
                    std::env::var("SOLARXY_REQUIRE_GPU").as_deref() != Ok("1"),
                    "SOLARXY_REQUIRE_GPU=1 but no usable GPU adapter"
                );
                eprintln!("skipping: no GPU adapter available");
                return;
            }
            Err(other) => panic!("the render failed: {other}"),
        }
    }
}

/// Keeps the picture the render last handed over.
///
/// Comparing the written files would compare compressed bytes, where one
/// changed pixel moves the whole stream and the size of a difference cannot be
/// read at all. The preview seam hands over the assembled image itself, so this
/// exercises that seam and gets pixels to compare in the same move.
#[derive(Default)]
struct LastPicture {
    pixels: Vec<u8>,
    calls: usize,
}

impl solarxy_render::RenderSink for LastPicture {
    fn report(&mut self, _progress: &solarxy_render::RenderProgress) {}

    fn preview(&mut self, image: &solarxy_render::Preview<'_>) {
        assert_eq!(
            image.format,
            solarxy_render::PreviewFormat::Rgba8,
            "an eight-bit output previewed as something else"
        );
        assert_eq!(
            image.pixels.len(),
            (image.width as usize) * (image.height as usize) * 4,
            "the preview is not the whole picture"
        );
        self.pixels = image.pixels.to_vec();
        self.calls += 1;
    }
}

/// A finer tiling renders the same picture.
///
/// A surface that shows a render converging asks for one, because pixels only
/// reach a sink when a tile finishes and an ordinary image is a single tile.
/// The milestone measured a tiled render as identical to a single-pass one at
/// the default budget; this is the same claim at the budget those surfaces
/// actually use.
///
/// **Identical for one engine and not quite for the other, which is worth
/// knowing.** Measured at 160 by 120 across two tilings: the tracer comes out
/// at **zero differing pixels**, because it offsets a dispatch and its seed is
/// the whole-image coordinate. The rasterizer comes out at **two pixels of
/// 19,200, worst channel 13 of 255**, because it cuts an asymmetric frustum out
/// of the picture's own and the arithmetic deriving it differs in the last bits
/// between two tilings, so a pixel lying exactly on a triangle edge can take
/// coverage one way at one tiling and the other way at the other. The bound
/// below is loose enough for that and far tighter than a seam, which would put
/// a whole row or column out.
#[test]
fn two_tile_budgets_render_the_same_image() {
    /// A twentieth of a percent of the pixels. A seam along one boundary of
    /// this picture would be four times that on its own.
    const TOLERATED_FRACTION: f64 = 0.0005;
    /// And none of them by more than this, so "a few pixels" cannot become
    /// "a few pixels that are completely wrong".
    const TOLERATED_DELTA: u8 = 32;

    for engine in [RenderEngine::Raster, RenderEngine::PathTraced] {
        let dir = scratch(&format!("budgets-{engine:?}"));
        let mut runs = Vec::new();
        // 160 by 120 is two tiles at an edge of 128 and six at an edge of 64,
        // which is what makes this test able to fail at all.
        for (run, budget) in [(0, 128 * 128), (1, 64 * 64)] {
            let opts = RenderOptions {
                output: Some(Output::File(dir.join(format!("{run}.png")))),
                engine: Some(engine),
                width: Some(160),
                height: Some(120),
                samples: Some(4),
                seed: Some(11),
                tile_budget: Some(budget),
                ..RenderOptions::default()
            };
            let mut sink = LastPicture::default();
            match solarxy_render::run_render(&model(), &opts, &mut sink) {
                Ok(outcome) => {
                    assert_eq!(
                        sink.calls, outcome.report.tiles as usize,
                        "{engine:?}: the picture was offered once per tile and this was not that"
                    );
                    runs.push((outcome.report.tiles, sink.pixels));
                }
                Err(RenderError::NoAdapter) => {
                    assert!(
                        std::env::var("SOLARXY_REQUIRE_GPU").as_deref() != Ok("1"),
                        "SOLARXY_REQUIRE_GPU=1 but no usable GPU adapter"
                    );
                    eprintln!("skipping: no GPU adapter available");
                    return;
                }
                Err(other) => panic!("the render failed: {other}"),
            }
        }

        // Without this the comparison would hold for the reason nothing had
        // changed, which is the way this class of test usually fails.
        assert!(
            runs[1].0 > runs[0].0,
            "{engine:?}: the smaller budget did not cut more tiles ({} then {})",
            runs[0].0,
            runs[1].0
        );

        let (a, b) = (&runs[0].1, &runs[1].1);
        assert_eq!(
            a.len(),
            b.len(),
            "{engine:?}: two pictures of different size"
        );
        let mut differing = 0usize;
        let mut worst = 0u8;
        for (x, y) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
            let delta = x
                .iter()
                .zip(y)
                .map(|(p, q)| p.abs_diff(*q))
                .max()
                .unwrap_or(0);
            if delta > 0 {
                differing += 1;
                worst = worst.max(delta);
            }
        }
        #[allow(clippy::cast_precision_loss)]
        let fraction = differing as f64 / (a.len() / 4) as f64;
        assert!(
            fraction <= TOLERATED_FRACTION && worst <= TOLERATED_DELTA,
            "{engine:?}: two tile budgets rendered different pictures: \
             {differing} pixels ({:.4}%), worst channel {worst}",
            fraction * 100.0
        );
    }
}

/// The passes are not merely reproducible, they are the passes.
///
/// Bytes being equal says nothing about whether either file describes the
/// scene: two runs of a bug are also equal. The normal pass is the one that
/// can be checked without a reference image, because every one of its pixels
/// is a direction whether it found a surface or not.
#[test]
fn the_normal_pass_holds_directions() {
    let dir = scratch("normals");
    let out = dir.join("shot.png");
    let opts = RenderOptions {
        output: Some(Output::File(out)),
        engine: Some(RenderEngine::PathTraced),
        width: Some(64),
        height: Some(48),
        samples: Some(4),
        seed: Some(1),
        aovs: vec![AovKind::Normal],
        ..RenderOptions::default()
    };
    match render(&opts) {
        Ok(_) => {}
        Err(RenderError::NoAdapter) => {
            assert!(
                std::env::var("SOLARXY_REQUIRE_GPU").as_deref() != Ok("1"),
                "SOLARXY_REQUIRE_GPU=1 but no usable GPU adapter"
            );
            eprintln!("skipping: no GPU adapter available");
            return;
        }
        Err(other) => panic!("the render failed: {other}"),
    }

    // Written beside a PNG, because a pass is float whatever the beauty is.
    let bytes = std::fs::read(dir.join("shot.normal.exr")).expect("the normal pass");
    let image = solarxy_formats::hdr::decode_exr_bytes(&bytes).expect("it decodes as EXR");
    assert_eq!((image.width, image.height), (64, 48));
    for n in image.pixels.chunks_exact(3) {
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        assert!(
            (length - 1.0).abs() < 1e-3,
            "a normal that is not a direction: {n:?}"
        );
    }
}
