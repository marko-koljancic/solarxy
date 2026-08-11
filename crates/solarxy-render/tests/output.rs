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
    solarxy_render::run_render(&model(), opts, &mut |_| {})
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
    let outcome = solarxy_render::run_render(Path::new("no-such-scene.slxy"), &opts, &mut |_| {});
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
