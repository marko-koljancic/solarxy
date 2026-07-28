//! Acceptance chain A (0.8.1 milestone), the half a machine can check.
//!
//! Chain A is the milestone's end-to-end proof that F1 through F4 work
//! *together* rather than merely passing their own suites: a `ch()`-driven
//! parameter, a `$T`-driven wrangle, playback, a rename that rewrites the
//! expression pointing at it, and finally a web bundle served statically.
//!
//! The last two steps need eyes on a browser. Everything before them is
//! measurable, so this builds the scene, asserts the engine half, and writes
//! the `.slxy` for a human to carry through the rest. Running it is
//! therefore both a verification and the fixture generator.
//!
//! ```text
//! cargo run -p solarxy-graph --example chain_a_scene -- /tmp/chain-a.slxy
//! ```

use solarxy_graph::document::GraphContext;
use solarxy_graph::engine::{PortRefDto, SceneSidecar};
use solarxy_graph::params::{ParamSource, ParamValue};
use solarxy_graph::runtime::LoopMode;
use solarxy_graph::{Command, Engine, EngineEvent};

fn add(
    e: &mut Engine,
    ctx: GraphContext,
    ty: &str,
    pos: [f32; 2],
) -> solarxy_graph::document::NodeId {
    let batch = e
        .apply(Command::AddNode {
            ctx,
            node_type: ty.to_string(),
            position: pos,
        })
        .expect("add");
    batch
        .events
        .iter()
        .find_map(|ev| match ev {
            EngineEvent::NodeAdded { node, .. } => Some(node.id),
            _ => None,
        })
        .expect("AddNode emits NodeAdded")
}

fn set(
    e: &mut Engine,
    ctx: GraphContext,
    node: solarxy_graph::document::NodeId,
    key: &str,
    v: ParamSource,
) {
    e.apply(Command::SetParam {
        ctx,
        node,
        key: key.to_string(),
        value: v,
    })
    .unwrap_or_else(|err| panic!("set {key}: {err}"));
}

fn expr(s: &str) -> ParamSource {
    ParamSource::Expression {
        expr: s.to_string(),
    }
}

fn text(s: &str) -> ParamSource {
    ParamSource::Literal(ParamValue::Text(s.to_string()))
}

/// Vertex count and the summed Y of the displayed geometry: enough to tell
/// "the wrangle moved the surface" from "nothing happened".
fn probe(
    e: &mut Engine,
    _ctx: GraphContext,
    node: solarxy_graph::document::NodeId,
) -> (usize, f64) {
    e.cook(&mut || true);
    let set = e
        .geometry_output(node)
        .unwrap_or_else(|| panic!("node {node:?} has no cooked geometry"));
    let mut count = 0usize;
    let mut sum = 0.0f64;
    for mesh in &set.meshes {
        count += mesh.positions.len();
        for p in mesh.positions.iter() {
            sum += f64::from(p[1]);
        }
    }
    (count, sum)
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/chain-a.slxy".to_string());
    let mut e = Engine::new().expect("the builtin registry is valid");

    // A geo container, and the geometry chain inside its subflow.
    let geo = add(&mut e, GraphContext::Root, "geo", [0.0, 0.0]);
    let ctx = GraphContext::Subflow(geo);

    // The control: a box nobody displays, whose width is the single number
    // the rest of the scene is driven from.
    let control = add(&mut e, ctx, "box", [-320.0, -160.0]);
    set(&mut e, ctx, control, "name", text("control"));
    set(
        &mut e,
        ctx,
        control,
        "width",
        ParamSource::Literal(ParamValue::Float(2.0)),
    );

    // The subject: a dense plane whose size READS the control by name.
    let plane = add(&mut e, ctx, "plane", [-320.0, 0.0]);
    set(&mut e, ctx, plane, "name", text("subject"));
    set(
        &mut e,
        ctx,
        plane,
        "width",
        expr(r#"ch("control/width") * 2"#),
    );
    set(
        &mut e,
        ctx,
        plane,
        "height",
        expr(r#"ch("control/width") * 2"#),
    );
    set(
        &mut e,
        ctx,
        plane,
        "width_segments",
        ParamSource::Literal(ParamValue::Int(48)),
    );
    set(
        &mut e,
        ctx,
        plane,
        "height_segments",
        ParamSource::Literal(ParamValue::Int(48)),
    );

    // The wrangle: ripples the surface from the clock, and colours by height
    // so the motion is unmistakable even in a still.
    let wrangle = add(&mut e, ctx, "attribute_wrangle", [-40.0, 0.0]);
    set(&mut e, ctx, wrangle, "name", text("ripple"));
    set(
        &mut e,
        ctx,
        wrangle,
        "program",
        text(
            "float d = length(set(@P.x, 0, @P.z));\n\
             float h = sin(d * 3 - $T * 3) * 0.25;\n\
             @P = set(@P.x, @P.y + h, @P.z);\n\
             @Cd = set(0.5 + h * 2, 0.6, 0.5 - h * 2);",
        ),
    );
    e.apply(Command::Connect {
        ctx,
        from: PortRefDto {
            node: plane,
            port: "geometry".into(),
        },
        to: PortRefDto {
            node: wrangle,
            port: "geometry".into(),
        },
    })
    .expect("connect");
    e.apply(Command::SetActiveOutput {
        ctx,
        node: Some(wrangle),
    })
    .expect("display");

    // A light, so the published bundle is not a silhouette.
    let light = add(
        &mut e,
        GraphContext::Root,
        "rect_area_light",
        [320.0, -160.0],
    );
    set(
        &mut e,
        GraphContext::Root,
        light,
        "width",
        ParamSource::Literal(ParamValue::Float(4.0)),
    );
    set(
        &mut e,
        GraphContext::Root,
        light,
        "height",
        ParamSource::Literal(ParamValue::Float(4.0)),
    );
    set(
        &mut e,
        GraphContext::Root,
        light,
        "translate",
        ParamSource::Literal(ParamValue::Vec3([0.0, 5.0, 2.0])),
    );
    set(
        &mut e,
        GraphContext::Root,
        light,
        "intensity",
        ParamSource::Literal(ParamValue::Float(40.0)),
    );

    let mut checks = 0;
    let fail = |what: &str| {
        eprintln!("  FAIL {what}");
        std::process::exit(1);
    };

    // ---- 1. ch() drives the subject from the control -------------------
    let (verts_before, _) = probe(&mut e, ctx, wrangle);
    set(
        &mut e,
        ctx,
        control,
        "width",
        ParamSource::Literal(ParamValue::Float(4.0)),
    );
    let bbox_grew = {
        e.cook(&mut || true);
        let g = e.geometry_output(wrangle).expect("geometry");
        g.meshes
            .iter()
            .flat_map(|m| m.positions.iter())
            .fold(0.0f32, |a, p| a.max(p[0].abs()))
    };
    if bbox_grew <= 2.0 {
        fail("ch() did not drive the subject: widening the control changed nothing");
    }
    println!("  ok   ch(\"control/width\") drives the subject (half-extent now {bbox_grew:.2})");
    checks += 1;

    // ---- 2. renaming the control rewrites the expression ---------------
    set(&mut e, ctx, control, "name", text("master"));
    let src = e
        .document()
        .graph(ctx)
        .expect("subflow")
        .node(plane)
        .expect("subject")
        .params
        .get("width")
        .cloned()
        .expect("the subject still has a width source");
    match &src {
        ParamSource::Expression { expr } if expr.contains("master/width") => {
            println!("  ok   rename rewrote the expression: {expr}");
            checks += 1;
        }
        other => fail(&format!("rename did not rewrite the expression: {other:?}")),
    }

    // The rewritten expression must still RESOLVE, not merely read right.
    let (verts_after, _) = probe(&mut e, ctx, wrangle);
    if verts_after == 0 || verts_after != verts_before {
        fail("the subject stopped cooking after the rename");
    }
    println!("  ok   the subject still cooks after the rename ({verts_after} points)");
    checks += 1;

    // ---- 3. the wrangle actually moves with the clock ------------------
    e.apply(Command::SetFrameRange { start: 1, end: 120 })
        .expect("range");
    e.apply(Command::SetFps { fps: 24.0 }).expect("fps");
    e.apply(Command::SetLoopMode {
        mode: LoopMode::Loop,
    })
    .expect("loop");
    e.apply(Command::SetAutoplay { autoplay: true })
        .expect("autoplay");

    e.apply(Command::SetFrame { frame: 1 }).expect("frame 1");
    let (_, y_at_1) = probe(&mut e, ctx, wrangle);
    e.apply(Command::SetFrame { frame: 30 }).expect("frame 30");
    let (_, y_at_30) = probe(&mut e, ctx, wrangle);
    e.apply(Command::SetFrame { frame: 1 }).expect("back to 1");
    let (_, y_again) = probe(&mut e, ctx, wrangle);

    if (y_at_1 - y_at_30).abs() < 1e-6 {
        fail("the wrangle did not move between frame 1 and frame 30");
    }
    println!("  ok   the surface moves with the clock (sum Y {y_at_1:.4} -> {y_at_30:.4})");
    checks += 1;

    if (y_at_1 - y_again).abs() > 1e-9 {
        fail("returning to frame 1 gave a different result: the clock is not reproducible");
    }
    println!("  ok   frame 1 is reproducible after visiting frame 30 (fixed step holds)");
    checks += 1;

    // ---- 4. it saves, and the clock settings survive -------------------
    let bytes = e.save_slxy(&SceneSidecar::default()).expect("save");
    let mut reloaded = Engine::new().expect("registry");
    reloaded.load_slxy(&bytes).expect("reload");
    let clock = reloaded.clock();
    if clock.effective_range() != (1, 120) || !clock.autoplay || clock.playing || clock.frame != 1 {
        fail(&format!(
            "the runtime section did not round-trip: {clock:?}"
        ));
    }
    println!("  ok   runtime round-trips, and reloads stopped at the range start");
    checks += 1;

    std::fs::write(&out, &bytes).expect("write the scene");
    println!("\n{checks} engine-side checks passed.");
    println!("Scene written to {out} ({} KB)", bytes.len() / 1024);
    println!("\nWhat a human still has to confirm, in a VISIBLE browser tab:");
    println!("  1. open the scene, press Space, and watch the surface ripple");
    println!("  2. File > Export web bundle, then serve the unzipped folder");
    println!("  3. the published page plays on its own (autoplay is on)");
}
