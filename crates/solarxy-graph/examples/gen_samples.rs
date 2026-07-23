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
        self.engine
            .apply(Command::SetParam {
                ctx,
                node,
                key: key.to_string(),
                value: ParamSource::Literal(value),
            })
            .unwrap_or_else(|e| panic!("set {key}: {e}"));
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
            .unwrap_or_else(|e| panic!("connect {}:{} -> {}:{}: {e}", from.0.0, from.1, to.0.0, to.1));
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
    b.set(g, move_sphere, "translate", ParamValue::Vec3([-0.75, 0.0, 0.0]));
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
    b.set(g, bind, "base_color", ParamValue::Color([0.83, 0.62, 0.21, 1.0]));
    b.set(g, bind, "metallic", ParamValue::Float(0.85));
    b.set(g, bind, "roughness", ParamValue::Float(0.3));
    b.set(g, bind, "material_name", ParamValue::Text("gold".into()));
    b.connect(g, (knot, "geometry"), (bind, "geometry"));
    b.display(g, bind);

    let key = b.add(ROOT, "directional_light", [340.0, -60.0]);
    b.set(ROOT, key, "position", ParamValue::Vec3([3.0, 4.0, 2.5]));
    b.set(ROOT, key, "intensity", ParamValue::Float(1.6));
    let fill = b.add(ROOT, "hemisphere_light", [340.0, 40.0]);
    b.set(ROOT, fill, "intensity", ParamValue::Float(0.9));
    let camera = b.add(ROOT, "camera", [340.0, 140.0]);
    b.set(ROOT, camera, "position", ParamValue::Vec3([3.2, 2.1, 3.6]));
    b.set(ROOT, camera, "target", ParamValue::Vec3([0.0, 0.0, 0.0]));
    let render = b.add(ROOT, "render", [560.0, 140.0]);
    b.set(ROOT, render, "camera_path", ParamValue::NodeRef(Some(camera)));

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

fn main() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web/public/samples");
    std::fs::create_dir_all(&dir).expect("samples dir");
    modeling_basics().save(&dir, "modeling-basics.slxy");
    copy_and_scatter().save(&dir, "copy-and-scatter.slxy");
    attributes_and_displace().save(&dir, "attributes-and-displace.slxy");
    texture_to_material().save(&dir, "texture-to-material.slxy");
    lights_camera_review().save(&dir, "lights-camera-review.slxy");
}
