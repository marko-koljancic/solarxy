//! The render controls authored on a render node have to reach the image.
//!
//! A sibling of the aperture test beside this, written for the same reason and
//! against the same failure. That one exists because a camera's aperture
//! resolved correctly out of a document, was read by the tracer from the field
//! it was meant to arrive in, and nothing joined the two, so for a whole
//! release every path-traced image was a pinhole render and no test noticed:
//! the kernel-level cases all passed an aperture of zero, which is the value
//! the broken path produced anyway.
//!
//! An exact sample count, an indirect clamp, a seed, and the denoiser's
//! strength and stopping point are threaded that same distance, so each gets an
//! assertion that fails when the wiring is removed rather than one that holds
//! because the default happened to arrive. Two are worth singling out. The
//! clamp's checks that the image gets *darker*, which is the documented cost of
//! clamping, rather than only that something changed. The denoiser's compares
//! two filtered renders rather than a filtered one against an unfiltered one,
//! because turning the filter on has always worked: what had no caller anywhere
//! in the workspace was the setter that steers it.
//!
//! Every assertion here was checked by deleting the wiring and watching it
//! fail. One did not, at first, and was rewritten; its own comment says so.
//!
//! Needs a real adapter and skips loudly without one, like every other GPU test
//! here.

use std::path::PathBuf;

use solarxy_graph::document::{GraphContext, NodeId};
use solarxy_graph::engine::{Command, Engine, EngineEvent, PortRefDto, SceneSidecar};
use solarxy_graph::params::{ParamSource, ParamValue};
use solarxy_render::{
    Output, Preview, PreviewFormat, RenderEngine, RenderError, RenderOptions, RenderProgress,
    RenderSink,
};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("solarxy-render-quality")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// What the render node is asked for, so each test varies exactly one thing.
///
/// Every field absent means the node keeps its own default, which is what makes
/// the comparisons below single-variable.
#[derive(Clone, Copy, Default)]
struct Authored {
    samples: Option<i64>,
    firefly_clamp: Option<f64>,
    seed: Option<i64>,
    denoise: Option<bool>,
    denoise_strength: Option<f64>,
    denoise_until_samples: Option<i64>,
    resolution_preset: Option<&'static str>,
    orientation: Option<&'static str>,
    transparent_background: Option<bool>,
}

/// A scene lit mostly by bounce, with the render node authored from `a`.
///
/// The ground plane is what makes this a test of the clamp rather than of
/// nothing: the clamp only ever acts on a sample's contribution *after* it has
/// bounced, so a scene lit entirely head-on would carry almost no energy for it
/// to remove and would pass whatever the wiring did. Here the light is low and
/// close to a large bright plane, so a good part of what reaches the sphere
/// arrives off that plane.
fn scene_with(a: Authored) -> Vec<u8> {
    let root = GraphContext::Root;
    let mut engine = Engine::new().expect("builtin registry");

    let add = |engine: &mut Engine, ctx, ty: &str, pos: [f32; 2]| {
        let batch = engine
            .apply(Command::AddNode {
                ctx,
                node_type: ty.to_string(),
                position: pos,
            })
            .unwrap_or_else(|e| panic!("add {ty}: {e}"));
        batch
            .events
            .iter()
            .find_map(|ev| match ev {
                EngineEvent::NodeAdded { node, .. } => Some(node.id),
                _ => None,
            })
            .unwrap_or_else(|| panic!("add {ty}: no NodeAdded"))
    };
    let set = |engine: &mut Engine, ctx, node, key: &str, value: ParamValue| {
        engine
            .apply(Command::SetParam {
                ctx,
                node,
                key: key.to_string(),
                value: ParamSource::Literal(value),
            })
            .unwrap_or_else(|e| panic!("set {key}: {e}"));
    };
    let connect = |engine: &mut Engine, ctx, from: (NodeId, &str), to: (NodeId, &str)| {
        engine
            .apply(Command::Connect {
                ctx,
                from: PortRefDto {
                    node: from.0,
                    port: from.1.to_string(),
                },
                to: PortRefDto {
                    node: to.0,
                    port: to.1.to_string(),
                },
            })
            .expect("connect");
    };

    let geo = add(&mut engine, root, "geo", [0.0, 0.0]);
    let g = GraphContext::Subflow(geo);
    let ball = add(&mut engine, g, "sphere", [0.0, 0.0]);
    let ball_x = add(&mut engine, g, "transform", [160.0, 0.0]);
    let ground = add(&mut engine, g, "box", [0.0, 120.0]);
    let ground_x = add(&mut engine, g, "transform", [160.0, 120.0]);
    let merge = add(&mut engine, g, "merge", [320.0, 60.0]);

    set(&mut engine, g, ball, "radius", ParamValue::Float(0.8));
    set(
        &mut engine,
        g,
        ball_x,
        "translate",
        ParamValue::Vec3([0.0, 0.8, 0.0]),
    );
    set(&mut engine, g, ground, "width", ParamValue::Float(12.0));
    set(&mut engine, g, ground, "height", ParamValue::Float(0.2));
    set(&mut engine, g, ground, "depth", ParamValue::Float(12.0));
    set(
        &mut engine,
        g,
        ground_x,
        "translate",
        ParamValue::Vec3([0.0, -0.1, 0.0]),
    );
    connect(&mut engine, g, (ball, "geometry"), (ball_x, "geometry"));
    connect(&mut engine, g, (ground, "geometry"), (ground_x, "geometry"));
    connect(&mut engine, g, (ball_x, "geometry"), (merge, "inputs"));
    connect(&mut engine, g, (ground_x, "geometry"), (merge, "inputs"));
    engine
        .apply(Command::SetActiveOutput {
            ctx: g,
            node: Some(merge),
        })
        .expect("display flag");

    // Low, close and bright, so the plane is doing a lot of the lighting.
    let light = add(&mut engine, root, "point_light", [0.0, 240.0]);
    set(
        &mut engine,
        root,
        light,
        "position",
        ParamValue::Vec3([1.5, 1.2, 1.5]),
    );
    set(
        &mut engine,
        root,
        light,
        "intensity",
        ParamValue::Float(40.0),
    );

    let camera = add(&mut engine, root, "camera", [320.0, 240.0]);
    set(
        &mut engine,
        root,
        camera,
        "position",
        ParamValue::Vec3([0.0, 1.6, 4.5]),
    );
    set(
        &mut engine,
        root,
        camera,
        "target",
        ParamValue::Vec3([0.0, 0.7, 0.0]),
    );

    let render = add(&mut engine, root, "render", [480.0, 240.0]);
    set(
        &mut engine,
        root,
        render,
        "camera_path",
        ParamValue::NodeRef(Some(camera)),
    );
    set(
        &mut engine,
        root,
        render,
        "engine",
        ParamValue::Enum("traced".into()),
    );
    if let Some(samples) = a.samples {
        set(
            &mut engine,
            root,
            render,
            "quality",
            ParamValue::Enum("custom".into()),
        );
        set(
            &mut engine,
            root,
            render,
            "samples",
            ParamValue::Int(samples),
        );
    }
    if let Some(clamp) = a.firefly_clamp {
        set(
            &mut engine,
            root,
            render,
            "firefly_clamp",
            ParamValue::Float(clamp),
        );
    }
    if let Some(seed) = a.seed {
        set(&mut engine, root, render, "seed", ParamValue::Int(seed));
    }
    if let Some(denoise) = a.denoise {
        set(
            &mut engine,
            root,
            render,
            "denoise",
            ParamValue::Bool(denoise),
        );
    }
    if let Some(strength) = a.denoise_strength {
        set(
            &mut engine,
            root,
            render,
            "denoise_strength",
            ParamValue::Float(strength),
        );
    }
    if let Some(until) = a.denoise_until_samples {
        set(
            &mut engine,
            root,
            render,
            "denoise_until_samples",
            ParamValue::Int(until),
        );
    }
    if let Some(preset) = a.resolution_preset {
        set(
            &mut engine,
            root,
            render,
            "resolution_preset",
            ParamValue::Enum(preset.into()),
        );
    }
    if let Some(orientation) = a.orientation {
        set(
            &mut engine,
            root,
            render,
            "orientation",
            ParamValue::Enum(orientation.into()),
        );
    }
    if let Some(transparent) = a.transparent_background {
        set(
            &mut engine,
            root,
            render,
            "transparent_background",
            ParamValue::Bool(transparent),
        );
    }

    for _ in 0..8 {
        if engine.cook(&mut || true).is_empty() {
            break;
        }
    }
    engine
        .save_slxy(&SceneSidecar::default())
        .expect("save the scene")
}

/// Keeps the mean brightness of the last picture the render handed over.
///
/// The whole picture arrives rather than the rectangle that changed, so the
/// last call is the finished image and each one can replace the last. Reading
/// it here rather than decoding the written file is what keeps this test from
/// needing an image decoder it would otherwise have to take a dependency for.
#[derive(Default)]
struct MeanBrightness {
    mean: f64,
}

impl RenderSink for MeanBrightness {
    fn report(&mut self, _progress: &RenderProgress) {}

    fn preview(&mut self, image: &Preview<'_>) {
        assert!(
            matches!(image.format, PreviewFormat::Rgba8),
            "this test reads eight-bit pixels"
        );
        let mut sum = 0.0;
        let mut count = 0u64;
        for px in image.pixels.as_chunks::<4>().0 {
            sum += f64::from(px[0]) + f64::from(px[1]) + f64::from(px[2]);
            count += 3;
        }
        self.mean = if count == 0 { 0.0 } else { sum / count as f64 };
    }
}

/// Renders `a` and returns the written file and the picture's mean brightness,
/// or `None` where there is no adapter.
fn render(dir: &std::path::Path, name: &str, a: Authored) -> Option<(Vec<u8>, f64)> {
    let scene_path = dir.join(format!("{name}.slxy"));
    std::fs::write(&scene_path, scene_with(a)).expect("write the scene");
    let out = dir.join(format!("{name}.png"));
    let opts = RenderOptions {
        output: Some(Output::File(out.clone())),
        engine: Some(RenderEngine::PathTraced),
        width: Some(128),
        height: Some(96),
        // Deliberately no `samples` and no `seed` here. Both have a flag that
        // would override the node, and a test that set either would be checking
        // the flag rather than the document.
        ..RenderOptions::default()
    };
    let mut sink = MeanBrightness::default();
    match solarxy_render::run_render(&scene_path, &opts, &mut sink) {
        Ok(_) => Some((std::fs::read(&out).expect("the image"), sink.mean)),
        Err(RenderError::NoAdapter) => {
            eprintln!("skipping: no GPU adapter");
            None
        }
        Err(e) => panic!("render failed: {e}"),
    }
}

/// A tight clamp removes energy, which is the documented cost of clamping.
///
/// Asserted as "darker" rather than as "different" on purpose. A difference
/// would prove only that the value reached something; the direction is what
/// says it reached the ceiling on indirect light rather than some other field
/// of the same width.
#[test]
fn a_tight_clamp_authored_on_the_node_darkens_the_image() {
    let dir = scratch("clamp");
    let base = Authored {
        samples: Some(24),
        seed: Some(11),
        ..Authored::default()
    };
    let Some((_, open)) = render(
        &dir,
        "open",
        Authored {
            firefly_clamp: Some(1000.0),
            ..base
        },
    ) else {
        return;
    };
    let Some((_, tight)) = render(
        &dir,
        "tight",
        Authored {
            firefly_clamp: Some(0.01),
            ..base
        },
    ) else {
        return;
    };
    assert!(
        tight < open,
        "a clamp of 0.01 rendered a picture no darker than one of 1000 \
         (mean {tight:.3} against {open:.3}), so the clamp authored on the \
         node is not reaching the tracer"
    );
}

/// The seed on the node changes the grain, and repeats when it does not change.
///
/// Both halves matter. Without the first the value is not arriving; without the
/// second the first would pass whatever the wiring did, because two renders
/// that never agree cannot be told apart by a seed.
#[test]
fn the_seed_authored_on_the_node_reaches_the_sampler() {
    let dir = scratch("seed");
    let base = Authored {
        samples: Some(16),
        ..Authored::default()
    };
    let Some((first, _)) = render(
        &dir,
        "first",
        Authored {
            seed: Some(11),
            ..base
        },
    ) else {
        return;
    };
    let Some((again, _)) = render(
        &dir,
        "again",
        Authored {
            seed: Some(11),
            ..base
        },
    ) else {
        return;
    };
    assert_eq!(
        first, again,
        "one scene at one seed rendered twice produced two different files, so \
         the comparison below proves nothing"
    );

    let Some((other, _)) = render(
        &dir,
        "other",
        Authored {
            seed: Some(9_999),
            ..base
        },
    ) else {
        return;
    };
    assert_ne!(
        first, other,
        "two seeds rendered the same image, so the seed authored on the node \
         is not reaching the sampler"
    );
}

/// An exact count on the node is used, and is the same count the flag means.
///
/// Two assertions, because either alone is satisfied by a break the other
/// catches. Comparing the node against the flag says the new path resolves to
/// the same number rather than to some number, but both paths share one mapping
/// into the tracer, so a count dropped in that shared code would leave the two
/// agreeing on the wrong answer. Comparing against the preset default is what
/// notices that: nine samples and the Good preset's sixty-four cannot render
/// the same picture. This was checked by deleting the wiring and watching which
/// assertion failed, and at first only the second one did.
#[test]
fn an_exact_count_on_the_node_is_used_and_matches_the_flag() {
    let dir = scratch("samples");
    let Some((authored, _)) = render(
        &dir,
        "authored",
        Authored {
            samples: Some(9),
            seed: Some(11),
            ..Authored::default()
        },
    ) else {
        return;
    };

    // Nothing authored, so the Good preset's sixty-four.
    let Some((preset, _)) = render(
        &dir,
        "preset",
        Authored {
            seed: Some(11),
            ..Authored::default()
        },
    ) else {
        return;
    };
    assert_ne!(
        authored, preset,
        "nine samples rendered the same image as the Good preset's sixty-four, \
         so the exact count is not being used at all"
    );

    // The same scene with nothing authored, driven by the flags instead.
    let scene_path = dir.join("flagged.slxy");
    std::fs::write(
        &scene_path,
        scene_with(Authored {
            seed: Some(11),
            ..Authored::default()
        }),
    )
    .expect("write the scene");
    let out = dir.join("flagged.png");
    let opts = RenderOptions {
        output: Some(Output::File(out.clone())),
        engine: Some(RenderEngine::PathTraced),
        width: Some(128),
        height: Some(96),
        samples: Some(9),
        ..RenderOptions::default()
    };
    let flagged = match solarxy_render::run_render(&scene_path, &opts, &mut solarxy_render::Silent)
    {
        Ok(_) => std::fs::read(&out).expect("the image"),
        Err(RenderError::NoAdapter) => return,
        Err(e) => panic!("render failed: {e}"),
    };

    assert_eq!(
        authored, flagged,
        "nine samples authored on the node rendered a different image from \
         nine on the flag, so the exact count is not the count being used"
    );
}

/// The denoiser's strength authored on the node changes the picture.
///
/// The setter these reach had no callers anywhere in the workspace before this
/// release, so every render on every surface ran the filter at its measured
/// defaults whatever anyone asked for. That is the state this asserts is over,
/// and it is why the comparison is against a second filtered render rather than
/// against an unfiltered one: turning the filter on has always worked, and a
/// test that only checked that would have passed throughout.
#[test]
fn the_denoise_strength_authored_on_the_node_changes_the_picture() {
    let dir = scratch("strength");
    let base = Authored {
        samples: Some(16),
        seed: Some(11),
        denoise: Some(true),
        ..Authored::default()
    };
    let Some((gentle, _)) = render(
        &dir,
        "gentle",
        Authored {
            denoise_strength: Some(0.25),
            ..base
        },
    ) else {
        return;
    };
    let Some((heavy, _)) = render(
        &dir,
        "heavy",
        Authored {
            denoise_strength: Some(4.0),
            ..base
        },
    ) else {
        return;
    };
    assert_ne!(
        gentle, heavy,
        "a filter at quarter strength produced the same image as one at four \
         times, so the strength authored on the node is not reaching the filter"
    );
}

/// A threshold below the sample count stops the filter, and above it does not.
///
/// The pair is what makes this a test of the threshold rather than of the
/// toggle. Stopping after one sample on a sixteen-sample render has to land on
/// the unfiltered image, and stopping after a thousand has to land on the
/// filtered one, so a threshold read as "always" or "never" fails one of them
/// whichever way it is wrong.
#[test]
fn the_denoise_threshold_decides_whether_the_filter_ran() {
    let dir = scratch("threshold");
    let base = Authored {
        samples: Some(16),
        seed: Some(11),
        ..Authored::default()
    };
    let Some((unfiltered, _)) = render(
        &dir,
        "off",
        Authored {
            denoise: Some(false),
            ..base
        },
    ) else {
        return;
    };
    let Some((filtered, _)) = render(
        &dir,
        "on",
        Authored {
            denoise: Some(true),
            ..base
        },
    ) else {
        return;
    };
    assert_ne!(
        unfiltered, filtered,
        "the filter changed nothing, so neither comparison below means \
         anything"
    );

    let Some((stopped, _)) = render(
        &dir,
        "stopped",
        Authored {
            denoise: Some(true),
            denoise_until_samples: Some(1),
            ..base
        },
    ) else {
        return;
    };
    assert_eq!(
        stopped, unfiltered,
        "a filter told to stop after one sample still filtered a sixteen \
         sample render"
    );

    let Some((never_stopped, _)) = render(
        &dir,
        "never-stopped",
        Authored {
            denoise: Some(true),
            denoise_until_samples: Some(1000),
            ..base
        },
    ) else {
        return;
    };
    assert_eq!(
        never_stopped, filtered,
        "a threshold well above the sample count stopped the filter anyway"
    );
}

/// A PNG's own idea of its size, read out of the header.
///
/// The first chunk of a PNG is always IHDR and always starts with the two
/// dimensions, big endian, so this needs no decoder and takes no dependency to
/// ask the file rather than the renderer how large it is. Asking the renderer
/// would be asking the thing under test.
fn png_size(bytes: &[u8]) -> (u32, u32) {
    assert!(bytes.len() > 24, "too short to be a PNG");
    assert_eq!(&bytes[12..16], b"IHDR", "not a PNG, or not IHDR first");
    let at = |o: usize| u32::from_be_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
    (at(16), at(20))
}

/// Renders at whatever size the node resolves to, and returns the file's size.
///
/// No size flags, deliberately: those override the node, so a test that passed
/// them would be checking the flags. Rasterized, because size resolution has
/// nothing to do with which engine draws and one pass a tile is a great deal
/// faster than a converging one.
fn rendered_size(dir: &std::path::Path, name: &str, a: Authored) -> Option<(u32, u32)> {
    let scene_path = dir.join(format!("{name}.slxy"));
    std::fs::write(&scene_path, scene_with(a)).expect("write the scene");
    let out = dir.join(format!("{name}.png"));
    let opts = RenderOptions {
        output: Some(Output::File(out.clone())),
        engine: Some(RenderEngine::Raster),
        ..RenderOptions::default()
    };
    match solarxy_render::run_render(&scene_path, &opts, &mut solarxy_render::Silent) {
        Ok(_) => Some(png_size(&std::fs::read(&out).expect("the image"))),
        Err(RenderError::NoAdapter) => {
            eprintln!("skipping: no GPU adapter");
            None
        }
        Err(e) => panic!("render failed: {e}"),
    }
}

/// A named size sets the size, and orientation turns it, all the way to a file.
///
/// The engine's own suite checks that a preset resolves to the right pair. This
/// checks that the pair survives everything between the node and the written
/// image, which is the difference between a preset that sets the size and one
/// that only describes it. A preset that merely described would leave every
/// render at the node's authored width and height and pass every test upstream
/// of this one.
#[test]
fn a_named_output_size_reaches_the_written_file() {
    let dir = scratch("output-size");

    // Nothing chosen: the node's own width and height, which this scene leaves
    // at their defaults. The control for the two below.
    let Some(custom) = rendered_size(&dir, "custom", Authored::default()) else {
        return;
    };
    assert_eq!(custom, (1920, 1080), "the node's own default size");

    let Some(square) = rendered_size(
        &dir,
        "square",
        Authored {
            resolution_preset: Some("square"),
            ..Authored::default()
        },
    ) else {
        return;
    };
    assert_eq!(
        square,
        (1080, 1080),
        "choosing a named size did not change what was rendered, so the preset \
         describes a size rather than setting one"
    );

    // High definition turned on its side is the story and reel size, which is
    // the whole reason the list is stated wide edge first rather than carrying
    // a second entry for every vertical delivery.
    let Some(story) = rendered_size(
        &dir,
        "story",
        Authored {
            resolution_preset: Some("hd"),
            orientation: Some("portrait"),
            ..Authored::default()
        },
    ) else {
        return;
    };
    assert_eq!(
        story,
        (1080, 1920),
        "portrait did not turn the preset on its way to the file"
    );
}

/// Keeps the last picture whole, alpha included, for the matte assertions.
#[derive(Default)]
struct PixelGrab {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

impl RenderSink for PixelGrab {
    fn report(&mut self, _progress: &RenderProgress) {}

    fn preview(&mut self, image: &Preview<'_>) {
        assert!(
            matches!(image.format, PreviewFormat::Rgba8),
            "this test reads eight-bit pixels"
        );
        self.pixels = image.pixels.to_vec();
        self.width = image.width;
        self.height = image.height;
    }
}

/// The first channel name in an EXR's header.
///
/// The channels attribute is `channels\0chlist\0`, a four-byte size, then the
/// channel list with each name null-terminated, and the format requires the
/// list sorted alphabetically. So the first name is `A` exactly when the file
/// carries a matte and `B` (of B, G, R) exactly when it does not, and reading
/// one byte here is what keeps this test from taking a decoder dependency for
/// a question the header answers.
fn first_channel(bytes: &[u8]) -> u8 {
    let marker = b"chlist\0";
    let at = bytes
        .windows(marker.len())
        .position(|w| w == marker)
        .expect("an exr header names its channel list type");
    bytes[at + marker.len() + 4]
}

/// The matte authored on the node reaches both files the command writes, and
/// an opaque render keeps writing three channels.
#[test]
fn the_transparent_background_reaches_both_files() {
    let dir = scratch("transparent");
    let authored = Authored {
        samples: Some(4),
        seed: Some(11),
        transparent_background: Some(true),
        ..Authored::default()
    };
    let scene_path = dir.join("matte.slxy");
    std::fs::write(&scene_path, scene_with(authored)).expect("write the scene");

    // The eight-bit file, checked at the values: the sink is fed the same
    // assembled pixels the encoder writes, and the encoder is RGBA through
    // and through.
    let png = dir.join("matte.png");
    let opts = RenderOptions {
        output: Some(Output::File(png.clone())),
        width: Some(128),
        height: Some(96),
        ..RenderOptions::default()
    };
    let mut sink = PixelGrab::default();
    match solarxy_render::run_render(&scene_path, &opts, &mut sink) {
        Ok(_) => {}
        Err(RenderError::NoAdapter) => {
            eprintln!("skipping: no GPU adapter");
            return;
        }
        Err(e) => panic!("render failed: {e}"),
    }
    assert!(png.exists(), "the image was written");
    let at = |x: u32, y: u32| -> &[u8] {
        let i = ((y * sink.width + x) * 4) as usize;
        &sink.pixels[i..i + 4]
    };
    assert_eq!(at(2, 2)[3], 0, "the sky is not covered");
    assert_eq!(at(2, 2)[..3], [0, 0, 0], "an uncovered pixel holds nothing");
    assert_eq!(
        at(sink.width / 2, sink.height / 2)[3],
        255,
        "the subject is fully covered"
    );
    assert!(
        sink.pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[3] > 0 && p[3] < 255),
        "a silhouette pixel is fractional"
    );

    // The floating-point file: an alpha channel exists exactly when the matte
    // was asked for. The premultiplication itself is pinned where the encoder
    // lives, in solarxy-formats, against the exr crate's own reader.
    let exr = dir.join("matte.exr");
    let opts = RenderOptions {
        output: Some(Output::File(exr.clone())),
        width: Some(64),
        height: Some(48),
        ..RenderOptions::default()
    };
    match solarxy_render::run_render(&scene_path, &opts, &mut solarxy_render::Silent) {
        Ok(_) => {}
        Err(RenderError::NoAdapter) => return,
        Err(e) => panic!("render failed: {e}"),
    }
    assert_eq!(
        first_channel(&std::fs::read(&exr).expect("the file")),
        b'A',
        "a matte render's float file carries an alpha channel"
    );

    // And the behaviour the reasoning still stands for: an opaque render's
    // float file keeps its three channels, no constant fourth.
    let opaque_scene = dir.join("opaque.slxy");
    std::fs::write(
        &opaque_scene,
        scene_with(Authored {
            samples: Some(4),
            seed: Some(11),
            ..Authored::default()
        }),
    )
    .expect("write the scene");
    let opaque_exr = dir.join("opaque.exr");
    let opts = RenderOptions {
        output: Some(Output::File(opaque_exr.clone())),
        width: Some(64),
        height: Some(48),
        ..RenderOptions::default()
    };
    match solarxy_render::run_render(&opaque_scene, &opts, &mut solarxy_render::Silent) {
        Ok(_) => {}
        Err(RenderError::NoAdapter) => return,
        Err(e) => panic!("render failed: {e}"),
    }
    assert_eq!(
        first_channel(&std::fs::read(&opaque_exr).expect("the file")),
        b'B',
        "an opaque render's float file stays three channels"
    );
}
