//! The sampling controls authored on a render node have to reach the image.
//!
//! A sibling of the aperture test beside this, written for the same reason and
//! against the same failure. That one exists because a camera's aperture
//! resolved correctly out of a document, was read by the tracer from the field
//! it was meant to arrive in, and nothing joined the two, so for a whole
//! release every path-traced image was a pinhole render and no test noticed:
//! the kernel-level cases all passed an aperture of zero, which is the value
//! the broken path produced anyway.
//!
//! An exact sample count, an indirect clamp and a seed are three more values
//! threaded that same distance, so each one gets an assertion that fails if the
//! wiring is removed rather than one that holds because the default happened to
//! arrive. The clamp's is the strongest of the three and the most worth having:
//! it checks that the image gets darker, which is the documented cost of
//! clamping, rather than only that something changed.
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
        for px in image.pixels.chunks_exact(4) {
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
