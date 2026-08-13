//! A camera's aperture has to reach the image.
//!
//! This test exists because the thing it checks is invisible to every other
//! test in the workspace. The sampling that turns an aperture into defocus is
//! covered at the kernel level, and covered well, but every one of those cases
//! passes an aperture of zero: they prove the math given a radius, not that a
//! radius authored on a camera node ever arrives. For one release it did not.
//! The parameter resolved into the scene contract, the tracer read the field
//! it was meant to arrive in, and nothing joined the two, so every path-traced
//! image was a pinhole render and no test noticed.
//!
//! So the assertion here is deliberately crude and end to end: two scenes,
//! identical but for the f-number, rendered through the real entry point must
//! not produce the same file. A subtler test of the same thing would be a test
//! of the sampler again.
//!
//! Needs a real adapter and skips loudly without one, like every other GPU
//! test here.

use std::path::PathBuf;

use solarxy_graph::document::GraphContext;
use solarxy_graph::engine::{Command, Engine, EngineEvent, PortRefDto, SceneSidecar};
use solarxy_graph::params::{ParamSource, ParamValue};
use solarxy_render::{Output, RenderEngine, RenderError, RenderOptions};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("solarxy-render-lens").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// A scene with something near the camera, something far behind it, and a
/// camera focused on the near thing at `f_stop`.
///
/// Depth is the whole point: an aperture changes nothing in a picture whose
/// content is all at the focus distance, so a scene that could not tell the
/// two apart would make this test pass for the wrong reason.
fn scene_at(f_stop: f64) -> Vec<u8> {
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

    let geo = add(&mut engine, root, "geo", [0.0, 0.0]);
    let g = GraphContext::Subflow(geo);
    let near = add(&mut engine, g, "sphere", [0.0, 0.0]);
    let near_x = add(&mut engine, g, "transform", [160.0, 0.0]);
    let far = add(&mut engine, g, "box", [0.0, 120.0]);
    let far_x = add(&mut engine, g, "transform", [160.0, 120.0]);
    let merge = add(&mut engine, g, "merge", [320.0, 60.0]);

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

    set(&mut engine, g, near, "radius", ParamValue::Float(0.35));
    set(
        &mut engine,
        g,
        near_x,
        "translate",
        ParamValue::Vec3([0.0, 0.0, 1.5]),
    );
    set(&mut engine, g, far, "width", ParamValue::Float(3.0));
    set(&mut engine, g, far, "height", ParamValue::Float(3.0));
    set(&mut engine, g, far, "depth", ParamValue::Float(0.1));
    set(
        &mut engine,
        g,
        far_x,
        "translate",
        ParamValue::Vec3([0.0, 0.0, -4.0]),
    );

    let connect = |engine: &mut Engine, ctx, from: (_, &str), to: (_, &str)| {
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
    connect(&mut engine, g, (near, "geometry"), (near_x, "geometry"));
    connect(&mut engine, g, (far, "geometry"), (far_x, "geometry"));
    connect(&mut engine, g, (near_x, "geometry"), (merge, "inputs"));
    connect(&mut engine, g, (far_x, "geometry"), (merge, "inputs"));
    engine
        .apply(Command::SetActiveOutput {
            ctx: g,
            node: Some(merge),
        })
        .expect("display flag");

    let light = add(&mut engine, root, "point_light", [0.0, 240.0]);
    set(
        &mut engine,
        root,
        light,
        "position",
        ParamValue::Vec3([2.0, 3.0, 4.0]),
    );

    let camera = add(&mut engine, root, "camera", [320.0, 240.0]);
    set(
        &mut engine,
        root,
        camera,
        "position",
        ParamValue::Vec3([0.0, 0.0, 4.0]),
    );
    set(
        &mut engine,
        root,
        camera,
        "target",
        ParamValue::Vec3([0.0, 0.0, 1.5]),
    );
    set(
        &mut engine,
        root,
        camera,
        "f_stop",
        ParamValue::Float(f_stop),
    );
    // Explicit rather than inherited from the target, so the two scenes differ
    // in exactly one value.
    set(
        &mut engine,
        root,
        camera,
        "focus_distance",
        ParamValue::Float(2.5),
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

    for _ in 0..8 {
        if engine.cook(&mut || true).is_empty() {
            break;
        }
    }
    engine
        .save_slxy(&SceneSidecar::default())
        .expect("save the scene")
}

/// Renders `scene` and returns the file, or `None` where there is no adapter.
fn render_scene(dir: &std::path::Path, name: &str, f_stop: f64) -> Option<Vec<u8>> {
    let scene_path = dir.join(format!("{name}.slxy"));
    std::fs::write(&scene_path, scene_at(f_stop)).expect("write the scene");
    let out = dir.join(format!("{name}.png"));
    let opts = RenderOptions {
        output: Some(Output::File(out.clone())),
        engine: Some(RenderEngine::PathTraced),
        width: Some(128),
        height: Some(96),
        // Enough for the two images to differ structurally rather than by
        // noise, and few enough to stay a test.
        samples: Some(24),
        // Fixed, so the only thing that can differ between the two runs is the
        // aperture. Without this the assertion would hold whatever the fix.
        seed: Some(11),
        ..RenderOptions::default()
    };
    match solarxy_render::run_render(&scene_path, &opts, &mut solarxy_render::Silent) {
        Ok(_) => Some(std::fs::read(&out).expect("the image")),
        Err(RenderError::NoAdapter) => {
            eprintln!("skipping: no GPU adapter");
            None
        }
        Err(e) => panic!("render failed: {e}"),
    }
}

#[test]
fn an_open_aperture_renders_a_different_image_from_a_pinhole() {
    let dir = scratch("aperture");
    let Some(pinhole) = render_scene(&dir, "pinhole", 0.0) else {
        return;
    };
    let Some(open) = render_scene(&dir, "open", 1.2) else {
        return;
    };
    assert_ne!(
        pinhole, open,
        "a camera at f/1.2 rendered the same image as a pinhole, so its \
         aperture is not reaching the tracer"
    );
}

#[test]
fn the_same_aperture_renders_the_same_image_twice() {
    // The control for the test above. If renders were not reproducible at a
    // fixed seed, that test would pass with the wiring removed again.
    let dir = scratch("aperture-control");
    let Some(first) = render_scene(&dir, "first", 1.2) else {
        return;
    };
    let Some(second) = render_scene(&dir, "second", 1.2) else {
        return;
    };
    assert_eq!(
        first, second,
        "two renders of one scene at one aperture and one seed differ, so the \
         aperture comparison beside this proves nothing"
    );
}
