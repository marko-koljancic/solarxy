//! Generates the bundled sample scenes the web app serves under
//! File > Sample Scenes (web/public/samples/*.slxy).
//!
//!     cargo run -p solarxy-graph --example gen_samples
//!
//! Each scene is authored through the same Command API the frontend
//! dispatches, so regenerating after a node change is one command; the
//! fixture test (tests/sample_scenes.rs) then cooks whatever is
//! committed. Every scene is fully parametric (no imported assets) and
//! carries note nodes teaching the workflow in place.

use solarxy_graph::engine::{Command, Engine, EngineEvent, PortRefDto, SceneSidecar};
use solarxy_graph::document::{GraphContext, NodeId};
use solarxy_graph::params::{ParamSource, ParamValue};
use solarxy_graph::runtime::LoopMode;
use solarxy_graph::review::{ReviewAnchor, ReviewCategory};

struct Builder {
    engine: Engine,
}

impl Builder {
    fn new() -> Self {
        Self {
            engine: Engine::new().expect("builtin registry"),
        }
    }

    fn add(&mut self, ctx: GraphContext, ty: &str, pos: [f32; 2]) -> NodeId {
        let batch = self
            .engine
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
            .unwrap_or_else(|| panic!("add {ty}: no NodeAdded event"))
    }

    fn set(&mut self, ctx: GraphContext, node: NodeId, key: &str, value: ParamValue) {
        self.set_source(ctx, node, key, ParamSource::Literal(value));
    }

    /// Sets a param from any source, which is what lets a sample carry an
    /// expression. `set` above is the literal shorthand; every scene
    /// written before 0.8.1 uses only that one.
    fn set_source(&mut self, ctx: GraphContext, node: NodeId, key: &str, value: ParamSource) {
        self.engine
            .apply(Command::SetParam {
                ctx,
                node,
                key: key.to_string(),
                value,
            })
            .unwrap_or_else(|e| panic!("set {key}: {e}"));
    }

    /// Drives a param with an expression.
    fn expr(&mut self, ctx: GraphContext, node: NodeId, key: &str, src: &str) {
        self.set_source(
            ctx,
            node,
            key,
            ParamSource::Expression {
                expr: src.to_string(),
            },
        );
    }

    /// The document's runtime settings: frame range, rate, loop mode, and
    /// whether a published player starts playing on load.
    ///
    /// Saved in `.slxy` (the session half -- playing, current frame -- is
    /// not), so a sample opens with the range its animation was authored
    /// for rather than the 1-240 default.
    fn runtime(&mut self, start: i64, end: i64, fps: f64, loop_mode: LoopMode, autoplay: bool) {
        for cmd in [
            Command::SetFrameRange { start, end },
            Command::SetFps { fps },
            Command::SetLoopMode { mode: loop_mode },
            Command::SetAutoplay { autoplay },
        ] {
            self.engine
                .apply(cmd)
                .unwrap_or_else(|e| panic!("runtime: {e}"));
        }
    }

    fn connect(&mut self, ctx: GraphContext, from: (NodeId, &str), to: (NodeId, &str)) {
        self.engine
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
            .unwrap_or_else(|e| {
                panic!(
                    "connect {}:{} -> {}:{}: {e}",
                    from.0.0, from.1, to.0.0, to.1
                )
            });
    }

    fn display(&mut self, ctx: GraphContext, node: NodeId) {
        self.engine
            .apply(Command::SetActiveOutput {
                ctx,
                node: Some(node),
            })
            .unwrap_or_else(|e| panic!("display flag: {e}"));
    }

    fn note(&mut self, ctx: GraphContext, pos: [f32; 2], size: [f32; 2], text: &str) {
        let id = self.add(ctx, "note", pos);
        self.set(ctx, id, "text", ParamValue::Text(text.to_string()));
        self.set(ctx, id, "width", ParamValue::Float(f64::from(size[0])));
        self.set(ctx, id, "height", ParamValue::Float(f64::from(size[1])));
    }

    fn rename(&mut self, ctx: GraphContext, node: NodeId, name: &str) {
        self.set(ctx, node, "name", ParamValue::Text(name.to_string()));
    }

    fn cook(&mut self) {
        for _ in 0..8 {
            if self.engine.cook(&mut || true).is_empty() {
                break;
            }
        }
    }

    fn save(mut self, dir: &std::path::Path, file: &str) {
        // Cook before saving so annotation hashes and stats are honest;
        // the file itself stores only the parametric document.
        self.cook();
        let bytes = self
            .engine
            .save_slxy(&SceneSidecar::default())
            .unwrap_or_else(|e| panic!("save {file}: {e}"));
        let path = dir.join(file);
        std::fs::write(&path, bytes).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
        println!("wrote {}", path.display());
    }
}

const ROOT: GraphContext = GraphContext::Root;

fn sub(id: NodeId) -> GraphContext {
    GraphContext::Subflow(id)
}

/// Modeling basics: generators into modifiers into a merge, one tidy
/// chain with the display flag on the result.
fn modeling_basics() -> Builder {
    let mut b = Builder::new();
    b.note(
        ROOT,
        [40.0, -160.0],
        [420.0, 96.0],
        "Welcome to Solarxy. This scene is a lesson: double-click the basics \
         container to step inside and see the graph that builds the model. \
         Every sample opens read-to-edit; nothing here is baked.",
    );
    let geo = b.add(ROOT, "geo", [120.0, 0.0]);
    b.rename(ROOT, geo, "basics");
    let g = sub(geo);

    b.note(
        g,
        [-320.0, -40.0],
        [280.0, 150.0],
        "The chain cooks top to bottom: a box is subdivided, moved aside, \
         and merged with a sphere. Select any node and scrub its \
         parameters; everything downstream recooks. The blue dot marks the \
         display flag, the node the viewport shows.",
    );
    let box_id = b.add(g, "box", [0.0, -120.0]);
    b.set(g, box_id, "width_segments", ParamValue::Int(2));
    b.set(g, box_id, "height_segments", ParamValue::Int(2));
    b.set(g, box_id, "depth_segments", ParamValue::Int(2));
    let subdiv = b.add(g, "subdivide", [0.0, 0.0]);
    b.set(g, subdiv, "iterations", ParamValue::Int(2));
    let move_box = b.add(g, "transform", [0.0, 120.0]);
    b.set(g, move_box, "translate", ParamValue::Vec3([0.8, 0.0, 0.0]));
    let sphere = b.add(g, "sphere", [260.0, 0.0]);
    b.set(g, sphere, "radius", ParamValue::Float(0.55));
    let move_sphere = b.add(g, "transform", [260.0, 120.0]);
    b.set(
        g,
        move_sphere,
        "translate",
        ParamValue::Vec3([-0.75, 0.0, 0.0]),
    );
    let merge = b.add(g, "merge", [130.0, 240.0]);
    b.connect(g, (box_id, "geometry"), (subdiv, "geometry"));
    b.connect(g, (subdiv, "geometry"), (move_box, "geometry"));
    b.connect(g, (move_box, "geometry"), (merge, "inputs"));
    b.connect(g, (sphere, "geometry"), (move_sphere, "geometry"));
    b.connect(g, (move_sphere, "geometry"), (merge, "inputs"));
    b.display(g, merge);
    b.note(
        g,
        [420.0, 200.0],
        [280.0, 120.0],
        "Try it: raise Subdivide's iterations, or swap the sphere for an \
         import node (File > Import Model) and wire it into the same merge. \
         The rest of the chain does not care where geometry comes from.",
    );
    b
}

/// Copy and scatter: points from a surface, a template instanced onto
/// them, oriented by the normal lane.
fn copy_and_scatter() -> Builder {
    let mut b = Builder::new();
    b.note(
        ROOT,
        [40.0, -160.0],
        [420.0, 82.0],
        "Instancing: scatter points over a surface, then copy a template \
         onto every point. Step into scatterfield for the graph.",
    );
    let geo = b.add(ROOT, "geo", [120.0, 0.0]);
    b.rename(ROOT, geo, "scatterfield");
    let g = sub(geo);

    let sphere = b.add(g, "sphere", [0.0, -120.0]);
    b.set(g, sphere, "radius", ParamValue::Float(1.0));
    b.set(g, sphere, "width_segments", ParamValue::Int(32));
    b.set(g, sphere, "height_segments", ParamValue::Int(20));
    let scatter = b.add(g, "scatter", [0.0, 0.0]);
    b.set(g, scatter, "count", ParamValue::Int(220));
    b.set(g, scatter, "seed", ParamValue::Int(7));
    let cone = b.add(g, "cone", [260.0, -120.0]);
    b.set(g, cone, "radius", ParamValue::Float(0.05));
    b.set(g, cone, "height", ParamValue::Float(0.16));
    b.set(g, cone, "radial_segments", ParamValue::Int(6));
    let copy = b.add(g, "copy_to_points", [130.0, 120.0]);
    b.set(g, copy, "scale_variance", ParamValue::Float(0.35));
    b.set(g, copy, "seed", ParamValue::Int(3));
    let merge = b.add(g, "merge", [130.0, 240.0]);
    b.connect(g, (sphere, "geometry"), (scatter, "geometry"));
    b.connect(g, (scatter, "geometry"), (copy, "points"));
    b.connect(g, (cone, "geometry"), (copy, "template"));
    b.connect(g, (sphere, "geometry"), (merge, "inputs"));
    b.connect(g, (copy, "geometry"), (merge, "inputs"));
    b.display(g, merge);
    b.note(
        g,
        [-320.0, 0.0],
        [280.0, 150.0],
        "Scatter writes an N attribute on its points, and Copy to Points \
         orients each cone along it, which is why they stand on the \
         surface. Scrub the counts and seeds, then open the Attributes \
         panel on the scatter node to see the point data itself.",
    );
    b.note(
        g,
        [420.0, 120.0],
        [280.0, 96.0],
        "Try it: replace the cone template with any geometry, even an \
         imported model. Scale Variance keeps the copies from looking \
         stamped.",
    );
    b
}

/// Attributes and displace: an image sampled into lanes, one driving
/// displacement, one driving vertex color.
fn attributes_and_displace() -> Builder {
    let mut b = Builder::new();
    b.note(
        ROOT,
        [40.0, -160.0],
        [430.0, 96.0],
        "Attributes driving shape: a texture network builds a noise map, \
         terrain samples it into point lanes, and a displace node reads \
         one lane as height. Step into maps first, then terrain.",
    );
    let tex = b.add(ROOT, "texnet", [120.0, 0.0]);
    b.rename(ROOT, tex, "maps");
    let t = sub(tex);
    let noise = b.add(t, "noise", [0.0, -100.0]);
    b.set(t, noise, "scale", ParamValue::Float(5.0));
    b.set(t, noise, "seed", ParamValue::Int(11));
    let levels = b.add(t, "levels", [0.0, 20.0]);
    b.set(t, levels, "gamma", ParamValue::Float(1.25));
    b.connect(t, (noise, "image"), (levels, "image"));
    b.display(t, levels);
    b.note(
        t,
        [220.0, -60.0],
        [260.0, 110.0],
        "This network cooks an image. The display flag picks which node \
         the tex_ref nodes elsewhere resolve, so inserting another adjust \
         node here reshapes the terrain live.",
    );

    let geo = b.add(ROOT, "geo", [320.0, 0.0]);
    b.rename(ROOT, geo, "terrain");
    let g = sub(geo);
    let plane = b.add(g, "plane", [0.0, -220.0]);
    b.set(g, plane, "width", ParamValue::Float(3.0));
    b.set(g, plane, "height", ParamValue::Float(3.0));
    b.set(g, plane, "width_segments", ParamValue::Int(96));
    b.set(g, plane, "height_segments", ParamValue::Int(96));
    let tex_ref = b.add(g, "tex_ref", [260.0, -220.0]);
    b.set(g, tex_ref, "texture_path", ParamValue::NodeRef(Some(tex)));
    let height = b.add(g, "attribute_from_image", [0.0, -100.0]);
    b.set(g, height, "attr_name", ParamValue::Text("height".into()));
    b.set(g, height, "channels", ParamValue::Enum("luminance".into()));
    b.set(g, height, "srgb", ParamValue::Bool(false));
    let color = b.add(g, "attribute_from_image", [0.0, 20.0]);
    let displace = b.add(g, "displace", [0.0, 140.0]);
    b.set(g, displace, "amplitude", ParamValue::Float(0.45));
    b.set(g, displace, "amp_attr", ParamValue::Text("height".into()));
    let normals = b.add(g, "compute_normals", [0.0, 260.0]);
    let lay_flat = b.add(g, "transform", [0.0, 380.0]);
    b.set(g, lay_flat, "rotate", ParamValue::Vec3([-90.0, 0.0, 0.0]));
    b.connect(g, (plane, "geometry"), (height, "geometry"));
    b.connect(g, (tex_ref, "image"), (height, "image"));
    b.connect(g, (height, "geometry"), (color, "geometry"));
    b.connect(g, (tex_ref, "image"), (color, "image"));
    b.connect(g, (color, "geometry"), (displace, "geometry"));
    b.connect(g, (displace, "geometry"), (normals, "geometry"));
    b.connect(g, (normals, "geometry"), (lay_flat, "geometry"));
    b.display(g, lay_flat);
    b.note(
        g,
        [-330.0, -100.0],
        [290.0, 170.0],
        "Two attribute_from_image nodes sample the same map: one writes a \
         height float lane, the other writes color (vertex color displays \
         immediately). Displace moves each point along its normal, \
         amplitude times height. Open the Attributes panel to watch the \
         lanes, and pick height in the viewport's right strip to see it \
         as arrows or labels.",
    );
    b.note(
        g,
        [260.0, 140.0],
        [270.0, 110.0],
        "Try it: scrub Displace's amplitude, change the noise seed in \
         maps, or promote the height lane to primitives with \
         attribute_promote and inspect the Primitive tab.",
    );
    b
}

/// Texture to material: a texture network feeding a principled surface,
/// bound to geometry by path reference.
fn texture_to_material() -> Builder {
    let mut b = Builder::new();
    b.note(
        ROOT,
        [40.0, -160.0],
        [430.0, 96.0],
        "The material pipeline: maps cooks an image, materials wraps it \
         in a principled surface, and the geometry binds that material by \
         path. Each container is its own small network.",
    );
    let tex = b.add(ROOT, "texnet", [120.0, 0.0]);
    b.rename(ROOT, tex, "maps");
    let t = sub(tex);
    let noise = b.add(t, "noise", [0.0, -100.0]);
    b.set(t, noise, "scale", ParamValue::Float(9.0));
    b.set(t, noise, "seed", ParamValue::Int(4));
    let levels = b.add(t, "levels", [0.0, 20.0]);
    b.set(t, levels, "in_black", ParamValue::Float(0.15));
    b.set(t, levels, "gamma", ParamValue::Float(0.9));
    b.connect(t, (noise, "image"), (levels, "image"));
    b.display(t, levels);

    let mat = b.add(ROOT, "matnet", [320.0, 0.0]);
    b.rename(ROOT, mat, "materials");
    let m = sub(mat);
    let map = b.add(m, "tex_ref", [0.0, -100.0]);
    b.set(m, map, "texture_path", ParamValue::NodeRef(Some(tex)));
    let principled = b.add(m, "principled", [0.0, 20.0]);
    b.set(
        m,
        principled,
        "base_color",
        ParamValue::Color([0.82, 0.78, 0.7, 1.0]),
    );
    b.set(m, principled, "roughness", ParamValue::Float(0.45));
    b.set(m, principled, "metallic", ParamValue::Float(0.15));
    b.set(
        m,
        principled,
        "material_name",
        ParamValue::Text("weathered".into()),
    );
    b.connect(m, (map, "image"), (principled, "base_color_map"));
    b.display(m, principled);
    b.note(
        m,
        [220.0, -40.0],
        [270.0, 120.0],
        "tex_ref pulls the maps network's display output into this \
         material as its base color. The other map ports (normal, \
         metallic-roughness, occlusion, emissive) work the same way.",
    );

    let geo = b.add(ROOT, "geo", [520.0, 0.0]);
    b.rename(ROOT, geo, "shaded");
    let g = sub(geo);
    let sphere = b.add(g, "sphere", [0.0, -120.0]);
    b.set(g, sphere, "width_segments", ParamValue::Int(48));
    b.set(g, sphere, "height_segments", ParamValue::Int(32));
    let bind = b.add(g, "material", [0.0, 0.0]);
    b.set(g, bind, "mode", ParamValue::Enum("reference".into()));
    b.set(g, bind, "material_path", ParamValue::NodeRef(Some(mat)));
    b.connect(g, (sphere, "geometry"), (bind, "geometry"));
    b.display(g, bind);
    b.note(
        g,
        [240.0, -60.0],
        [270.0, 120.0],
        "The material node in reference mode points at the materials \
         container. Edit the principled surface or the noise map and this \
         sphere follows; nothing is copied, only referenced.",
    );
    b
}

/// Lights, camera, review: a lit hero object, a framed camera, a render
/// node, and a pre-placed review thread.
fn lights_camera_review() -> Builder {
    let mut b = Builder::new();
    b.note(
        ROOT,
        [40.0, -180.0],
        [440.0, 110.0],
        "Presentation and feedback: lights and a camera are root nodes \
         like any other. Look through the camera from a pane's Camera \
         menu, press Render on the render node for a high-quality still, \
         and toggle Review Mode (Shift+R) to read the pinned note on the \
         knot.",
    );
    let geo = b.add(ROOT, "geo", [120.0, 0.0]);
    b.rename(ROOT, geo, "hero");
    let g = sub(geo);
    let knot = b.add(g, "torus_knot", [0.0, -100.0]);
    b.set(g, knot, "tubular_segments", ParamValue::Int(200));
    b.set(g, knot, "radial_segments", ParamValue::Int(24));
    let bind = b.add(g, "material", [0.0, 20.0]);
    b.set(
        g,
        bind,
        "base_color",
        ParamValue::Color([0.83, 0.62, 0.21, 1.0]),
    );
    b.set(g, bind, "metallic", ParamValue::Float(0.85));
    b.set(g, bind, "roughness", ParamValue::Float(0.3));
    b.set(g, bind, "material_name", ParamValue::Text("gold".into()));
    b.connect(g, (knot, "geometry"), (bind, "geometry"));
    b.display(g, bind);

    let key = b.add(ROOT, "directional_light", [340.0, -60.0]);
    b.set(ROOT, key, "position", ParamValue::Vec3([3.0, 4.0, 2.5]));
    // 4.8 is the physical-units value: the committed pre-rescale file said
    // 1.6 and the load-time migration tripled it, so authoring 4.8 keeps
    // the scene's brightness identical across a regeneration.
    b.set(ROOT, key, "intensity", ParamValue::Float(4.8));
    let fill = b.add(ROOT, "hemisphere_light", [340.0, 40.0]);
    b.set(ROOT, fill, "intensity", ParamValue::Float(0.9));
    let camera = b.add(ROOT, "camera", [340.0, 140.0]);
    b.set(ROOT, camera, "position", ParamValue::Vec3([3.2, 2.1, 3.6]));
    b.set(ROOT, camera, "target", ParamValue::Vec3([0.0, 0.0, 0.0]));
    let render = b.add(ROOT, "render", [560.0, 140.0]);
    b.set(
        ROOT,
        render,
        "camera_path",
        ParamValue::NodeRef(Some(camera)),
    );

    // The pre-placed review thread needs cooked geometry (the engine
    // stamps the anchor with the display output's hash on add).
    b.cook();
    let anchor = ReviewAnchor {
        ctx: ROOT,
        node: geo,
        mesh: Some(0),
        face: Some(0),
        barycentric: Some([0.34, 0.33, 0.33]),
        world_fallback: Some([1.2, 0.35, 0.4]),
        geometry_hash: None,
    };
    b.engine
        .apply(Command::AddAnnotation {
            anchor: anchor.clone(),
            text: "Review notes pin to geometry like this one. Reply from the \
                   Review panel, or click the surface in review mode to add \
                   your own."
                .into(),
            category: ReviewCategory::Info,
            author: Some("Solarxy Samples".into()),
            created_at: "2026-07-23T12:00:00Z".into(),
            reply_to: None,
        })
        .expect("annotation");
    b
}

/// Animated field: the 0.8.1 story end to end. A wrangle drives `@P` and
/// `@Cd` from `$T`, a `ch()` expression ties one node's parameter to
/// another's, and the camera and key light are themselves expression-driven,
/// so pressing Play moves the geometry, the shading and the shot at once.
fn animated_field() -> Builder {
    let mut b = Builder::new();
    b.note(
        ROOT,
        [40.0, -200.0],
        [460.0, 130.0],
        "Press Play (Space). Everything moving here is an expression or a \
         wrangle reading the scene clock: no keyframes exist yet. $T is \
         scene seconds, $F the frame. Scrub the timeline under the viewport \
         to step through it by hand.",
    );

    let geo = b.add(ROOT, "geo", [120.0, 0.0]);
    b.rename(ROOT, geo, "field");
    let g = sub(geo);

    // The control node: never displayed, exists only to be read. This is
    // what `ch()` is for, and having a visible source makes the mechanism
    // legible instead of magic.
    let control = b.add(g, "box", [-260.0, -180.0]);
    b.rename(g, control, "control");
    b.set(g, control, "width", ParamValue::Float(6.0));
    b.note(
        g,
        [-260.0, -300.0],
        [250.0, 100.0],
        "A control node. Nothing displays it; the plane below reads its \
         Width through ch(\"control/width\"). Change it and the field \
         resizes. Rename it and the expression follows.",
    );

    let plane = b.add(g, "plane", [0.0, -180.0]);
    b.rename(g, plane, "subject");
    b.expr(g, plane, "width", "ch(\"control/width\")");
    b.expr(g, plane, "height", "ch(\"control/width\")");
    b.set(g, plane, "width_segments", ParamValue::Int(72));
    b.set(g, plane, "height_segments", ParamValue::Int(72));

    // Lay the plane flat FIRST. The generator authors it in XY, and the
    // wrangle below reads @P.x / @P.z and displaces along @P.y, which is
    // only the right thing once the surface is in the ground plane.
    // Rotating afterwards instead would tip the ripple on its side.
    let lay = b.add(g, "transform", [0.0, -120.0]);
    b.set(g, lay, "rotate", ParamValue::Vec3([-90.0, 0.0, 0.0]));
    b.connect(g, (plane, "geometry"), (lay, "geometry"));

    // The ripple. Points only: @P is a point attribute, so a primitive
    // wrangle could not move the surface.
    let ripple = b.add(g, "attribute_wrangle", [0.0, -60.0]);
    b.rename(g, ripple, "ripple");
    b.set(
        g,
        ripple,
        "program",
        ParamValue::Text(
            "float d = length(set(@P.x, 0, @P.z));\n\
             float wave = sin(d * 2.2 - $T * 2.5);\n\
             float falloff = 1 - clamp(d / 3.5, 0, 1);\n\
             float h = wave * falloff * 0.45;\n\
             @P = set(@P.x, @P.y + h, @P.z);\n\
             @Cd = set(0.55 + h, 0.45, 0.75 - h);"
                .into(),
        ),
    );
    b.connect(g, (lay, "geometry"), (ripple, "geometry"));

    let normals = b.add(g, "compute_normals", [0.0, 40.0]);
    b.connect(g, (ripple, "geometry"), (normals, "geometry"));
    b.display(g, normals);
    b.note(
        g,
        [240.0, -80.0],
        [280.0, 190.0],
        "The wrangle runs once per point. It reads $T, so the engine \
         re-cooks it every frame while the clock runs and leaves it alone \
         when stopped. @Cd is the reserved colour lane the viewport already \
         displays, which is why the ripple is coloured without a material. \
         compute_normals after it re-lights the moved surface.",
    );

    // An orbiting camera: the expression engine on something other than
    // geometry, and the reason the scene reads as a shot rather than a
    // turntable.
    let camera = b.add(ROOT, "camera", [420.0, -40.0]);
    b.rename(ROOT, camera, "orbit");
    b.expr(
        ROOT,
        camera,
        "position",
        "set(sin($T * 0.35) * 6.5, 3.4 + sin($T * 0.7) * 0.8, cos($T * 0.35) * 6.5)",
    );
    b.set(ROOT, camera, "target", ParamValue::Vec3([0.0, 0.0, 0.0]));

    let key = b.add(ROOT, "directional_light", [420.0, 60.0]);
    b.set(ROOT, key, "position", ParamValue::Vec3([4.0, 5.0, 2.0]));
    // Every term tripled against the original 1.5 + sin * 0.45: the light
    // migration cannot rewrite an expression, so the committed file was
    // hand-corrected to physical units and the source has to match it.
    b.expr(ROOT, key, "intensity", "4.5 + sin($T * 1.1) * 1.35");
    b.set(ROOT, key, "cast_shadow", ParamValue::Bool(true));
    let fill = b.add(ROOT, "hemisphere_light", [420.0, 160.0]);
    b.set(ROOT, fill, "intensity", ParamValue::Float(0.7));
    b.note(
        ROOT,
        [620.0, -40.0],
        [280.0, 160.0],
        "The camera orbits and the key light breathes, both from plain \
         expressions on ordinary parameters. Bind a pane to the camera from \
         its Camera menu to ride along. Any numeric parameter in the scene \
         can be driven this way: click the = beside its label.",
    );

    // Ten seconds at 24fps, looping. Autoplay is on so a published scene
    // moves the moment somebody opens it; the editor deliberately ignores
    // the flag and opens stopped.
    b.runtime(1, 240, 24.0, LoopMode::Loop, true);
    b
}

/// Procedural look-dev: texture network into a material network onto
/// geometry, lit by an area light. Static on purpose -- it is about how a
/// surface is built, and an animation would only get in the way.
fn procedural_lookdev() -> Builder {
    let mut b = Builder::new();
    b.note(
        ROOT,
        [40.0, -200.0],
        [470.0, 120.0],
        "Three networks, one surface. The texture container builds maps \
         procedurally, the material container consumes them, and the \
         geometry container references the material by path. Dive into any \
         of them by double-clicking.",
    );

    // --- maps -------------------------------------------------------
    let tex = b.add(ROOT, "texnet", [120.0, 0.0]);
    b.rename(ROOT, tex, "maps");
    let t = sub(tex);
    let cells = b.add(t, "voronoi", [0.0, -160.0]);
    b.set(t, cells, "scale", ParamValue::Float(7.0));
    b.set(t, cells, "seed", ParamValue::Int(4));
    b.set(t, cells, "jitter", ParamValue::Float(0.85));
    let grain = b.add(t, "noise", [-220.0, -160.0]);
    b.set(t, grain, "scale", ParamValue::Float(24.0));
    b.set(t, grain, "seed", ParamValue::Int(11));
    let blended = b.add(t, "mix", [0.0, -40.0]);
    b.set(t, blended, "factor", ParamValue::Float(0.35));
    b.connect(t, (cells, "image"), (blended, "image"));
    b.connect(t, (grain, "image"), (blended, "blend"));
    let shaped = b.add(t, "levels", [0.0, 60.0]);
    b.set(t, shaped, "gamma", ParamValue::Float(1.35));
    b.set(t, shaped, "in_black", ParamValue::Float(0.08));
    b.connect(t, (blended, "image"), (shaped, "image"));
    b.display(t, shaped);
    b.note(
        t,
        [240.0, -160.0],
        [280.0, 160.0],
        "Voronoi cells for the large structure, fine noise on top, mixed \
         and then shaped with levels. Every map here is generated: the \
         scene carries no image files, so it is a few kilobytes and looks \
         the same on any machine.",
    );

    // --- material ---------------------------------------------------
    let mat = b.add(ROOT, "matnet", [320.0, 0.0]);
    b.rename(ROOT, mat, "surface");
    let m = sub(mat);
    let map = b.add(m, "tex_ref", [0.0, -120.0]);
    b.set(m, map, "texture_path", ParamValue::NodeRef(Some(tex)));
    let surface = b.add(m, "principled", [0.0, 20.0]);
    b.set(
        m,
        surface,
        "base_color",
        ParamValue::Color([0.72, 0.44, 0.30, 1.0]),
    );
    b.set(m, surface, "roughness", ParamValue::Float(0.55));
    b.set(m, surface, "metallic", ParamValue::Float(0.15));
    b.set(
        m,
        surface,
        "material_name",
        ParamValue::Text("fired clay".into()),
    );
    b.connect(m, (map, "image"), (surface, "base_color_map"));
    b.display(m, surface);

    // --- geometry ---------------------------------------------------
    let geo = b.add(ROOT, "geo", [520.0, 0.0]);
    b.rename(ROOT, geo, "subject");
    let g = sub(geo);
    let knot = b.add(g, "torus_knot", [0.0, -220.0]);
    b.set(g, knot, "radius", ParamValue::Float(1.1));
    b.set(g, knot, "tube", ParamValue::Float(0.34));
    b.set(g, knot, "tubular_segments", ParamValue::Int(240));
    b.set(g, knot, "radial_segments", ParamValue::Int(32));
    let uvs = b.add(g, "uv_project", [0.0, -100.0]);
    b.set(g, uvs, "scale", ParamValue::Vec2([3.0, 1.0]));
    b.connect(g, (knot, "geometry"), (uvs, "geometry"));
    let bind = b.add(g, "material", [0.0, 20.0]);
    b.set(g, bind, "mode", ParamValue::Enum("reference".into()));
    b.set(g, bind, "material_path", ParamValue::NodeRef(Some(mat)));
    b.connect(g, (uvs, "geometry"), (bind, "geometry"));
    b.display(g, bind);
    b.note(
        g,
        [240.0, -140.0],
        [280.0, 170.0],
        "uv_project gives the knot somewhere to put the map before the \
         material is bound; without UVs a textured surface has nothing to \
         look up. Open the UV pane (3) to see the layout.",
    );

    // --- lighting and camera ---------------------------------------
    let area = b.add(ROOT, "rect_area_light", [720.0, -80.0]);
    b.set(ROOT, area, "translate", ParamValue::Vec3([2.4, 3.2, 2.0]));
    b.set(ROOT, area, "rotate", ParamValue::Vec3([-48.0, 32.0, 0.0]));
    b.set(ROOT, area, "width", ParamValue::Float(4.0));
    b.set(ROOT, area, "height", ParamValue::Float(2.5));
    b.set(ROOT, area, "intensity", ParamValue::Float(14.0));
    let bounce = b.add(ROOT, "hemisphere_light", [720.0, 30.0]);
    b.set(ROOT, bounce, "intensity", ParamValue::Float(0.55));
    let camera = b.add(ROOT, "camera", [720.0, 130.0]);
    b.set(ROOT, camera, "position", ParamValue::Vec3([2.6, 1.7, 3.4]));
    b.set(ROOT, camera, "target", ParamValue::Vec3([0.0, 0.0, 0.0]));
    b.set(ROOT, camera, "fov_y", ParamValue::Float(38.0));
    b.note(
        ROOT,
        [920.0, -80.0],
        [280.0, 170.0],
        "A rectangular area light: its Width and Height are real, so \
         widening it broadens the highlight and softens the shadow edge \
         the way a larger softbox would. Rotate it and the shading \
         follows.",
    );
    b
}

/// The Orrery: the flagship sample. A miniature solar system composing
/// every major capability in one scene: parametric geometry across four
/// containers, clock and frame expressions with `ch()` and a geometry
/// query, an instanced asteroid belt, a point wrangle writing colour,
/// procedural textures feeding materials through more than one map slot,
/// an emissive sun, a switch whose index an expression drives, the
/// attribute nodes, and the runtime on an autoplaying loop.
fn orrery() -> Builder {
    let mut b = Builder::new();
    b.note(
        ROOT,
        [40.0, -220.0],
        [480.0, 124.0],
        "The Orrery: everything in one scene. Planets orbit on expressions, \
         an instanced asteroid belt streams around them, procedural maps \
         shade the gas giant, and at frame 121 a switch flips the whole \
         scene to a wireframe schematic. Press Play (Space), then dive into \
         any container.",
    );

    // --- bands: the gas giant's colour map ---------------------------
    let bands_net = b.add(ROOT, "texnet", [120.0, 0.0]);
    b.rename(ROOT, bands_net, "bands");
    let t = sub(bands_net);
    let ramp = b.add(t, "ramp", [0.0, -160.0]);
    b.set(
        t,
        ramp,
        "color_a",
        ParamValue::Color([0.86, 0.64, 0.4, 1.0]),
    );
    b.set(
        t,
        ramp,
        "color_b",
        ParamValue::Color([0.4, 0.26, 0.42, 1.0]),
    );
    let turb = b.add(t, "noise", [-220.0, -160.0]);
    b.set(t, turb, "scale", ParamValue::Float(6.0));
    b.set(t, turb, "seed", ParamValue::Int(9));
    let blended = b.add(t, "mix", [0.0, -40.0]);
    b.set(t, blended, "factor", ParamValue::Float(0.4));
    b.connect(t, (ramp, "image"), (blended, "image"));
    b.connect(t, (turb, "image"), (blended, "blend"));
    let shaped = b.add(t, "levels", [0.0, 60.0]);
    b.set(t, shaped, "gamma", ParamValue::Float(1.1));
    b.connect(t, (blended, "image"), (shaped, "image"));
    b.display(t, shaped);
    b.note(
        t,
        [220.0, -120.0],
        [270.0, 110.0],
        "A ramp for the bands, noise for the turbulence, mixed and shaped. \
         The display flag is what tex_ref nodes elsewhere resolve, so an \
         adjust node inserted here restripes the planet live.",
    );

    // --- relief: the same idea cooking a normal map ------------------
    let relief_net = b.add(ROOT, "texnet", [320.0, 0.0]);
    b.rename(ROOT, relief_net, "relief");
    let r = sub(relief_net);
    let bump = b.add(r, "noise", [0.0, -100.0]);
    b.set(r, bump, "scale", ParamValue::Float(14.0));
    b.set(r, bump, "seed", ParamValue::Int(21));
    let nrm = b.add(r, "height_to_normal", [0.0, 20.0]);
    b.connect(r, (bump, "image"), (nrm, "image"));
    b.display(r, nrm);
    b.note(
        r,
        [220.0, -60.0],
        [260.0, 96.0],
        "height_to_normal reads the noise as a height field and cooks a \
         tangent-space normal map, purple the way normal maps are. The \
         gas giant consumes it in its Normal Map slot.",
    );

    // --- materials: two surfaces mixed, for the rocky planet ---------
    let mat = b.add(ROOT, "matnet", [520.0, 0.0]);
    b.rename(ROOT, mat, "materials");
    let m = sub(mat);
    let rock_map = b.add(m, "tex_ref", [0.0, -120.0]);
    b.set(
        m,
        rock_map,
        "texture_path",
        ParamValue::NodeRef(Some(bands_net)),
    );
    let rock = b.add(m, "principled", [0.0, 0.0]);
    b.set(
        m,
        rock,
        "base_color",
        ParamValue::Color([0.5, 0.42, 0.36, 1.0]),
    );
    b.set(m, rock, "roughness", ParamValue::Float(0.62));
    b.set(
        m,
        rock,
        "material_name",
        ParamValue::Text("banded rock".into()),
    );
    b.connect(m, (rock_map, "image"), (rock, "base_color_map"));
    let ice = b.add(m, "principled", [220.0, 0.0]);
    b.set(
        m,
        ice,
        "base_color",
        ParamValue::Color([0.76, 0.83, 0.9, 1.0]),
    );
    b.set(m, ice, "roughness", ParamValue::Float(0.28));
    b.set(
        m,
        ice,
        "material_name",
        ParamValue::Text("polar ice".into()),
    );
    let blend_mat = b.add(m, "mix_material", [110.0, 120.0]);
    b.set(m, blend_mat, "factor", ParamValue::Float(0.3));
    b.connect(m, (rock, "material"), (blend_mat, "a"));
    b.connect(m, (ice, "material"), (blend_mat, "b"));
    b.display(m, blend_mat);
    b.note(
        m,
        [340.0, 100.0],
        [270.0, 110.0],
        "mix_material blends two principled surfaces; the rocky planet \
         references this network by path, so scrubbing Factor re-shades it \
         from across the scene.",
    );

    // --- the orrery itself -------------------------------------------
    let geo = b.add(ROOT, "geo", [720.0, 0.0]);
    b.rename(ROOT, geo, "orrery");
    let g = sub(geo);

    // The control: one number the guide ring, the orbit and nothing else
    // all read through ch().
    let control = b.add(g, "box", [-300.0, -420.0]);
    b.rename(g, control, "control");
    b.set(g, control, "width", ParamValue::Float(2.2));
    b.note(
        g,
        [-300.0, -540.0],
        [260.0, 100.0],
        "The control node again: the rocky planet's orbit radius AND its \
         guide ring both read ch(\"control/width\"), so one scrub moves \
         the planet and the track it rides together.",
    );

    // The sun: emissive, so it reads as the light source it sits on.
    let sun = b.add(g, "sphere", [-60.0, -420.0]);
    b.rename(g, sun, "sun");
    b.set(g, sun, "radius", ParamValue::Float(0.55));
    b.set(g, sun, "width_segments", ParamValue::Int(48));
    b.set(g, sun, "height_segments", ParamValue::Int(32));
    let sun_mat = b.add(g, "material", [-60.0, -320.0]);
    b.set(
        g,
        sun_mat,
        "base_color",
        ParamValue::Color([1.0, 0.62, 0.2, 1.0]),
    );
    b.set(
        g,
        sun_mat,
        "emissive",
        ParamValue::Color([1.0, 0.45, 0.12, 1.0]),
    );
    b.set(g, sun_mat, "emissive_strength", ParamValue::Float(4.0));
    b.set(
        g,
        sun_mat,
        "material_name",
        ParamValue::Text("sunfire".into()),
    );
    b.connect(g, (sun, "geometry"), (sun_mat, "geometry"));

    // The rocky planet and its moon. The moon orbits the planet FIRST,
    // then the pair orbits the sun: composition by merge-then-transform.
    let terra = b.add(g, "sphere", [160.0, -420.0]);
    b.rename(g, terra, "terra");
    b.set(g, terra, "radius", ParamValue::Float(0.2));
    let terra_mat = b.add(g, "material", [160.0, -320.0]);
    b.set(g, terra_mat, "mode", ParamValue::Enum("reference".into()));
    b.set(
        g,
        terra_mat,
        "material_path",
        ParamValue::NodeRef(Some(mat)),
    );
    b.connect(g, (terra, "geometry"), (terra_mat, "geometry"));
    let moon = b.add(g, "sphere", [340.0, -420.0]);
    b.rename(g, moon, "moon");
    b.set(g, moon, "radius", ParamValue::Float(0.055));
    let moon_orbit = b.add(g, "transform", [340.0, -320.0]);
    b.rename(g, moon_orbit, "moonorbit");
    b.expr(
        g,
        moon_orbit,
        "translate",
        "set(sin($T * 2.2) * 0.5, 0.12, cos($T * 2.2) * 0.5)",
    );
    b.connect(g, (moon, "geometry"), (moon_orbit, "geometry"));
    let pair = b.add(g, "merge", [250.0, -220.0]);
    b.connect(g, (terra_mat, "geometry"), (pair, "inputs"));
    b.connect(g, (moon_orbit, "geometry"), (pair, "inputs"));
    let orbit_a = b.add(g, "transform", [250.0, -120.0]);
    b.rename(g, orbit_a, "orbit_terra");
    b.expr(
        g,
        orbit_a,
        "translate",
        "set(sin($T * 0.45) * ch(\"control/width\"), 0, cos($T * 0.45) * ch(\"control/width\"))",
    );
    b.connect(g, (pair, "geometry"), (orbit_a, "geometry"));
    b.note(
        g,
        [480.0, -280.0],
        [280.0, 130.0],
        "The moon orbits terra, then the merged pair orbits the sun: \
         merge first, transform after, and the child ride-alongs come \
         free. moonorbit runs on $T seconds; the outer orbit mixes $T \
         with the control read.",
    );

    // The gas giant: an inline material consuming two map slots, fed by
    // two texture networks.
    let gas = b.add(g, "sphere", [660.0, -420.0]);
    b.rename(g, gas, "gaseous");
    b.set(g, gas, "radius", ParamValue::Float(0.34));
    b.set(g, gas, "width_segments", ParamValue::Int(48));
    b.set(g, gas, "height_segments", ParamValue::Int(32));
    let gas_bands = b.add(g, "tex_ref", [840.0, -420.0]);
    b.set(
        g,
        gas_bands,
        "texture_path",
        ParamValue::NodeRef(Some(bands_net)),
    );
    let gas_relief = b.add(g, "tex_ref", [840.0, -320.0]);
    b.set(
        g,
        gas_relief,
        "texture_path",
        ParamValue::NodeRef(Some(relief_net)),
    );
    let gas_mat = b.add(g, "material", [660.0, -320.0]);
    b.set(g, gas_mat, "roughness", ParamValue::Float(0.5));
    b.set(
        g,
        gas_mat,
        "material_name",
        ParamValue::Text("gaseous shell".into()),
    );
    b.connect(g, (gas, "geometry"), (gas_mat, "geometry"));
    b.connect(g, (gas_bands, "image"), (gas_mat, "base_color_map"));
    b.connect(g, (gas_relief, "image"), (gas_mat, "normal_map"));
    let orbit_b = b.add(g, "transform", [660.0, -220.0]);
    b.rename(g, orbit_b, "orbit_gaseous");
    b.expr(
        g,
        orbit_b,
        "translate",
        "set(sin($F * 0.006) * 3.3, 0, cos($F * 0.006) * 3.3)",
    );
    b.connect(g, (gas_mat, "geometry"), (orbit_b, "geometry"));

    // The asteroid belt: scatter on a flattened torus, the attribute
    // nodes and a wrangle preparing the points, one box instanced onto
    // all of them.
    let belt_ring = b.add(g, "torus", [1060.0, -420.0]);
    b.rename(g, belt_ring, "beltring");
    b.set(g, belt_ring, "radius", ParamValue::Float(2.6));
    b.set(g, belt_ring, "tube", ParamValue::Float(0.14));
    b.set(g, belt_ring, "tubular_segments", ParamValue::Int(96));
    let belt_flat = b.add(g, "transform", [1060.0, -320.0]);
    b.set(g, belt_flat, "rotate", ParamValue::Vec3([-90.0, 0.0, 0.0]));
    b.connect(g, (belt_ring, "geometry"), (belt_flat, "geometry"));
    let belt_pts = b.add(g, "scatter", [1060.0, -220.0]);
    b.set(g, belt_pts, "count", ParamValue::Int(420));
    b.set(g, belt_pts, "seed", ParamValue::Int(5));
    b.connect(g, (belt_flat, "geometry"), (belt_pts, "geometry"));
    let belt_band = b.add(g, "attribute_create", [1060.0, -120.0]);
    b.set(g, belt_band, "attr_name", ParamValue::Text("band".into()));
    b.set(g, belt_band, "value_float", ParamValue::Float(1.0));
    b.connect(g, (belt_pts, "geometry"), (belt_band, "geometry"));
    let belt_scale = b.add(g, "attribute_randomize", [1060.0, -20.0]);
    b.set(
        g,
        belt_scale,
        "attr_name",
        ParamValue::Text("pscale".into()),
    );
    b.set(g, belt_scale, "min_float", ParamValue::Float(0.35));
    b.set(g, belt_scale, "max_float", ParamValue::Float(1.5));
    b.set(g, belt_scale, "seed", ParamValue::Int(12));
    b.connect(g, (belt_band, "geometry"), (belt_scale, "geometry"));
    // The spin turns the POINTS, before the copy: a transform after the
    // copy would bake the placements into real triangles, which is the
    // one thing an asteroid belt of four hundred copies must never do.
    let belt_spin = b.add(g, "transform", [1060.0, 80.0]);
    b.rename(g, belt_spin, "beltspin");
    b.expr(g, belt_spin, "rotate", "set(0, $T * 3.5, 0)");
    b.connect(g, (belt_scale, "geometry"), (belt_spin, "geometry"));
    let rock_tpl = b.add(g, "box", [1280.0, -220.0]);
    b.rename(g, rock_tpl, "rocktemplate");
    b.set(g, rock_tpl, "width", ParamValue::Float(0.06));
    b.set(g, rock_tpl, "height", ParamValue::Float(0.04));
    b.set(g, rock_tpl, "depth", ParamValue::Float(0.05));
    let belt_copy = b.add(g, "copy_to_points", [1170.0, 180.0]);
    b.set(g, belt_copy, "scale_variance", ParamValue::Float(0.25));
    b.expr(g, belt_copy, "seed", "npoints()");
    b.connect(g, (belt_spin, "geometry"), (belt_copy, "points"));
    b.connect(g, (rock_tpl, "geometry"), (belt_copy, "template"));
    b.note(
        g,
        [1360.0, -80.0],
        [290.0, 190.0],
        "The belt is four hundred instanced copies of one box: \
         attribute_randomize writes a pscale lane Copy to Points reads \
         per point, beltspin turns the point cloud BEFORE the copy (a \
         transform after it would bake the placements), and the copy \
         seed is the expression npoints(), so changing Scatter's count \
         reshuffles the whole belt. The status line reports placements, \
         not baked triangles.",
    );

    // Guide rings: the same geometry the planets ride, made visible; the
    // terra ring reads the same control as the orbit.
    let path_a = b.add(g, "torus", [-60.0, -220.0]);
    b.rename(g, path_a, "terra_ring");
    b.expr(g, path_a, "radius", "ch(\"control/width\")");
    b.set(g, path_a, "tube", ParamValue::Float(0.008));
    b.set(g, path_a, "tubular_segments", ParamValue::Int(128));
    let path_b = b.add(g, "torus", [100.0, -220.0]);
    b.rename(g, path_b, "gaseous_ring");
    b.set(g, path_b, "radius", ParamValue::Float(3.3));
    b.set(g, path_b, "tube", ParamValue::Float(0.008));
    b.set(g, path_b, "tubular_segments", ParamValue::Int(160));
    let guides = b.add(g, "merge", [20.0, -120.0]);
    b.connect(g, (path_a, "geometry"), (guides, "inputs"));
    b.connect(g, (path_b, "geometry"), (guides, "inputs"));
    let guides_flat = b.add(g, "transform", [20.0, -20.0]);
    b.set(
        g,
        guides_flat,
        "rotate",
        ParamValue::Vec3([-90.0, 0.0, 0.0]),
    );
    b.connect(g, (guides, "geometry"), (guides_flat, "geometry"));
    // The wrangle: a point program painting the guide rings by distance
    // from the sun, and the sample's proof that vertex colour runs end to
    // end (an instanced copy carries transforms, not lanes, so the paint
    // lives on real geometry).
    let ring_paint = b.add(g, "attribute_wrangle", [20.0, 80.0]);
    b.rename(g, ring_paint, "ringpaint");
    b.set(
        g,
        ring_paint,
        "program",
        ParamValue::Text(
            "float t = clamp((length(set(@P.x, 0, @P.z)) - 2.0) / 1.5, 0, 1);\n\
             @Cd = set(0.85 - t * 0.35, 0.6, 0.4 + t * 0.45);"
                .into(),
        ),
    );
    b.connect(g, (guides_flat, "geometry"), (ring_paint, "geometry"));
    b.note(
        g,
        [-260.0, 40.0],
        [270.0, 120.0],
        "ringpaint runs once per point on the guide rings, colouring them \
         by distance from the sun through @Cd, the reserved lane the \
         viewport displays without any material. The same program box \
         drives the animated field sample's whole ripple.",
    );

    // Everything together, then the look switch: slot 0 is the full
    // scene, slot 1 the same scene as extracted edges, and the index is
    // an expression, so the schematic cuts in mid-loop on its own.
    let all = b.add(g, "merge", [560.0, 120.0]);
    b.connect(g, (sun_mat, "geometry"), (all, "inputs"));
    b.connect(g, (orbit_a, "geometry"), (all, "inputs"));
    b.connect(g, (orbit_b, "geometry"), (all, "inputs"));
    b.connect(g, (belt_copy, "geometry"), (all, "inputs"));
    b.connect(g, (ring_paint, "geometry"), (all, "inputs"));
    let schematic = b.add(g, "edges_to_geo", [700.0, 220.0]);
    b.connect(g, (all, "geometry"), (schematic, "geometry"));
    let look = b.add(g, "switch", [560.0, 320.0]);
    b.rename(g, look, "lookswitch");
    b.expr(g, look, "index", "$F < 121 ? 0 : 1");
    b.connect(g, (all, "geometry"), (look, "inputs"));
    b.connect(g, (schematic, "geometry"), (look, "inputs"));
    b.display(g, look);
    b.note(
        g,
        [760.0, 340.0],
        [290.0, 130.0],
        "The switch holds two whole looks: the shaded scene and its \
         wireframe schematic. Its Index is the expression $F < 121 ? 0 : \
         1, so the flip happens mid-loop by itself; set it to a literal 0 \
         or 1 to pin a look instead.",
    );

    // --- camera, light, runtime --------------------------------------
    let camera = b.add(ROOT, "camera", [940.0, -60.0]);
    b.rename(ROOT, camera, "orbitcam");
    b.expr(
        ROOT,
        camera,
        "position",
        "set(sin($T * 0.22) * 7.5, 4.6 + sin($T * 0.4) * 0.6, cos($T * 0.22) * 7.5)",
    );
    b.set(ROOT, camera, "target", ParamValue::Vec3([0.0, 0.0, 0.0]));
    b.set(ROOT, camera, "fov_y", ParamValue::Float(36.0));
    let glow = b.add(ROOT, "point_light", [940.0, 40.0]);
    b.rename(ROOT, glow, "sunglow");
    b.set(ROOT, glow, "position", ParamValue::Vec3([0.0, 0.4, 0.0]));
    b.set(ROOT, glow, "intensity", ParamValue::Float(6.0));
    let space = b.add(ROOT, "hemisphere_light", [940.0, 140.0]);
    b.rename(ROOT, space, "spacefill");
    b.set(ROOT, space, "intensity", ParamValue::Float(0.35));
    let render = b.add(ROOT, "render", [940.0, 240.0]);
    b.set(
        ROOT,
        render,
        "camera_path",
        ParamValue::NodeRef(Some(camera)),
    );
    b.note(
        ROOT,
        [1140.0, 40.0],
        [290.0, 130.0],
        "Try it: scrub control/width inside the orrery and watch the \
         planet and its ring move together; raise Scatter's count and the \
         belt reshuffles; look through orbitcam from a pane's Camera menu \
         to ride the shot.",
    );

    // Ten seconds at 24 fps, looping, autoplaying when published.
    b.runtime(1, 240, 24.0, LoopMode::Loop, true);
    b
}

fn main() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web/public/samples");
    std::fs::create_dir_all(&dir).expect("samples dir");
    modeling_basics().save(&dir, "modeling-basics.slxy");
    copy_and_scatter().save(&dir, "copy-and-scatter.slxy");
    attributes_and_displace().save(&dir, "attributes-and-displace.slxy");
    texture_to_material().save(&dir, "texture-to-material.slxy");
    lights_camera_review().save(&dir, "lights-camera-review.slxy");
    animated_field().save(&dir, "animated-field.slxy");
    procedural_lookdev().save(&dir, "procedural-lookdev.slxy");
    orrery().save(&dir, "the-orrery.slxy");
}
