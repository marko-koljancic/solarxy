//! A headless render is lit by the scene's environment.
//!
//! The gap this covers was never a rendering bug: the cook produced the
//! environment, the delta carried it, and the one consumer with no window
//! dropped it, so the same document rendered from a terminal against a
//! constant and from a browser against its image. Nothing caught that, because
//! no bundled sample authors an environment at all and the only headless test
//! rendered a bare model, which cannot.
//!
//! So these assert the environment *arrived*, not that the render succeeded: a
//! run that silently fell back to the constant sky would still write a file and
//! still exit zero, which is exactly what it did before.
//!
//! Both engines, because they install it differently. A tracer binds it and
//! samples it directly; the raster path reads image-based lighting off the
//! renderer the host already brought up, and draws the sky in a separate pass.

use std::path::{Path, PathBuf};

use solarxy_graph::document::{GraphContext, NodeId};
use solarxy_graph::engine::{Command, Engine, EngineEvent, PortRefDto, SceneSidecar};
use solarxy_render::{Output, RenderEngine, RenderOptions};

const ROOT: GraphContext = GraphContext::Root;

/// A colour no fallback in this codebase could produce by accident: green far
/// below both red and blue. Every constant sky here is a grey or a grey-blue,
/// where green sits between the two, so a magenta backdrop cannot be one.
const SKY_RGBE: [u8; 4] = [230, 40, 200, 129];

/// A flat Radiance image of a single colour.
///
/// Generated rather than committed. A fixture would be a binary asset in a
/// public repository for a file whose whole content is one repeated pixel, and
/// the bytes are more legible written out than they would be in a blob.
///
/// Old-style flat scanlines, not run-length encoded: a decoder reads a
/// new-style scanline only when a pixel begins `2, 2`, and this one does not.
fn radiance_hdr(width: u32, height: u32, rgbe: [u8; 4]) -> Vec<u8> {
    assert!(
        !(rgbe[0] == 2 && rgbe[1] == 2),
        "this pixel would be read as a run-length header"
    );
    let mut bytes = Vec::from(*b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n");
    bytes.extend_from_slice(format!("-Y {height} +X {width}\n").as_bytes());
    for _ in 0..(width as usize * height as usize) {
        bytes.extend_from_slice(&rgbe);
    }
    bytes
}

struct Build {
    engine: Engine,
}

impl Build {
    fn new() -> Self {
        Self {
            engine: Engine::new().expect("builtin registry"),
        }
    }

    fn add(&mut self, ctx: GraphContext, ty: &str) -> NodeId {
        let batch = self
            .engine
            .apply(Command::AddNode {
                ctx,
                node_type: ty.to_string(),
                position: [0.0, 0.0],
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

    fn set(
        &mut self,
        ctx: GraphContext,
        node: NodeId,
        key: &str,
        value: solarxy_graph::params::ParamValue,
    ) {
        self.engine
            .apply(Command::SetParam {
                ctx,
                node,
                key: key.to_string(),
                value: solarxy_graph::params::ParamSource::Literal(value),
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
            .unwrap_or_else(|e| panic!("connect {}: {e}", from.1));
    }

    fn display(&mut self, ctx: GraphContext, node: NodeId) {
        self.engine
            .apply(Command::SetActiveOutput {
                ctx,
                node: Some(node),
            })
            .expect("display flag");
    }

    fn finish(mut self, path: &Path) {
        for _ in 0..8 {
            if self.engine.cook(&mut || true).is_empty() {
                break;
            }
        }
        let bytes = self
            .engine
            .save_slxy(&SceneSidecar::default())
            .expect("save .slxy");
        std::fs::write(path, bytes).expect("write the scene");
    }
}

/// A sphere, a light, a render node, and optionally an environment.
///
/// The light is there so the *control* is lit: without one both shells
/// synthesize a viewer rig, and a scene lit by a rig no matter what would hide
/// the very difference being measured.
fn scene(path: &Path, environment: bool, hdri_sky: bool) {
    use solarxy_graph::params::ParamValue as V;

    let mut b = Build::new();

    // Geometry lives in a geo container: the root context takes lights,
    // cameras, the environment and the render node, and nothing that cooks.
    let geo = b.add(ROOT, "geo");
    let g = GraphContext::Subflow(geo);
    let sphere = b.add(g, "sphere");
    b.set(g, sphere, "radius", V::Float(1.0));
    let material = b.add(g, "material");
    b.set(g, material, "base_color", V::Color([0.8, 0.8, 0.8, 1.0]));
    b.set(g, material, "roughness", V::Float(0.4));
    b.connect(g, (sphere, "geometry"), (material, "geometry"));
    b.display(g, material);

    let light = b.add(ROOT, "point_light");
    b.set(ROOT, light, "intensity", V::Float(4.0));

    if environment {
        let asset = b.engine.stage_asset(
            "sky.hdr",
            "image/vnd.radiance",
            radiance_hdr(16, 8, SKY_RGBE),
        );
        let env = b.add(ROOT, "environment");
        b.set(ROOT, env, "hdri", V::Asset(asset));
        if hdri_sky {
            // The variant name as the node declares it. Spelled out because
            // the constant lives in a crate-private module.
            b.set(ROOT, env, "background", V::Enum("hdri_sky".to_string()));
        }
    }

    // Named rather than left to the bounds-framing fallback, so the shot is a
    // property of the document. Two surfaces can only be compared against each
    // other if they are looking from the same place.
    let camera = b.add(ROOT, "camera");
    b.set(ROOT, camera, "position", V::Vec3([0.0, 0.6, 3.4]));
    b.set(ROOT, camera, "target", V::Vec3([0.0, 0.0, 0.0]));
    b.set(ROOT, camera, "fov_y", V::Float(45.0));

    let render = b.add(ROOT, "render");
    b.set(ROOT, render, "camera_path", V::NodeRef(Some(camera)));
    // Authored rather than left to the caller's flag, so the document alone
    // decides the shot. A scene that says what it is renders the same wherever
    // it is opened, which is the whole subject here.
    b.set(ROOT, render, "engine", V::Enum("traced".to_string()));
    b.set(ROOT, render, "width", V::Int(64));
    b.set(ROOT, render, "height", V::Int(64));
    b.finish(path);
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("solarxy-render-environment")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// The rendered image as float pixels, or `None` where the machine has no
/// adapter, which is how every other GPU test here skips itself.
fn render_floats(scene_path: &Path, out: &Path, engine: RenderEngine) -> Option<Vec<[f32; 3]>> {
    let opts = RenderOptions {
        output: Some(Output::File(out.to_path_buf())),
        engine: Some(engine),
        width: Some(64),
        height: Some(64),
        // Only the accumulating engine takes these three; the rasterizer
        // refuses them rather than ignoring them, so this helper asks for them
        // only where they can act. The seed is shared, because both engines
        // read it.
        samples: matches!(engine, RenderEngine::PathTraced).then_some(8),
        denoise: matches!(engine, RenderEngine::PathTraced).then_some(false),
        seed: Some(1),
        ..RenderOptions::default()
    };
    match solarxy_render::run_render(scene_path, &opts, &mut solarxy_render::Silent) {
        Ok(_) => {}
        Err(solarxy_render::RenderError::NoAdapter) => return None,
        Err(e) => panic!("render failed: {e}"),
    }
    let bytes = std::fs::read(out).expect("the image");
    let image = solarxy_formats::hdr::decode_exr_bytes(&bytes).expect("decode the render");
    Some(image.pixels.as_chunks::<3>().0.to_vec())
}

/// The mean of the four corners, which is background wherever a centred sphere
/// is the only thing in the scene.
fn corners(pixels: &[[f32; 3]], width: usize, height: usize) -> [f32; 3] {
    let at = |x: usize, y: usize| pixels[y * width + x];
    let picks = [
        at(0, 0),
        at(width - 1, 0),
        at(0, height - 1),
        at(width - 1, height - 1),
    ];
    let mut sum = [0.0; 3];
    for p in picks {
        for c in 0..3 {
            sum[c] += p[c];
        }
    }
    sum.map(|v| v / 4.0)
}

/// The mean over a centred block, which is the sphere rather than the sky.
///
/// The whole-image mean will not do for the raster path: a small subject on an
/// unchanged backdrop moves it by a few percent whatever the lighting does, so
/// the measurement has to look at the thing being lit.
fn subject(pixels: &[[f32; 3]], width: usize, half: usize) -> f32 {
    let mid = width / 2;
    let mut sum = 0.0;
    let mut n = 0.0;
    for y in (mid - half)..(mid + half) {
        for x in (mid - half)..(mid + half) {
            let p = pixels[y * width + x];
            sum += p[0] + p[1] + p[2];
            n += 3.0;
        }
    }
    sum / n
}

fn mean(pixels: &[[f32; 3]]) -> f32 {
    let total: f32 = pixels.iter().map(|p| p[0] + p[1] + p[2]).sum();
    total / (pixels.len() as f32 * 3.0)
}

/// Green below both red and blue, which is the authored sky and nothing this
/// codebase falls back to.
fn reads_as_the_authored_sky(c: [f32; 3]) -> bool {
    c[1] < c[0] * 0.5 && c[1] < c[2] * 0.5 && c[0] > 0.01 && c[2] > 0.01
}

/// A tracer that reached the environment shows it where the rays escape.
///
/// The strongest form the assertion takes: the backdrop of a traced image *is*
/// the environment, so the corners carry the authored colour rather than a
/// resemblance to it. The control renders the identical document with the
/// environment node removed and gets no sky at all.
#[test]
fn a_traced_render_integrates_against_the_scene_environment() {
    let dir = scratch("traced");
    let lit = dir.join("lit.slxy");
    let dark = dir.join("dark.slxy");
    scene(&lit, true, false);
    scene(&dark, false, false);

    let Some(with) = render_floats(&lit, &dir.join("with.exr"), RenderEngine::PathTraced) else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let without =
        render_floats(&dark, &dir.join("without.exr"), RenderEngine::PathTraced).expect("adapter");

    let sky = corners(&with, 64, 64);
    assert!(
        reads_as_the_authored_sky(sky),
        "the traced backdrop is not the authored environment: {sky:?}"
    );
    let none = corners(&without, 64, 64);
    assert!(
        none.iter().all(|c| *c < 1e-4),
        "a scene authoring no environment was rendered against something: {none:?}"
    );
    assert!(
        mean(&with) > mean(&without) * 1.5,
        "the environment lit nothing: {} against {}",
        mean(&with),
        mean(&without)
    );
}

/// The raster path reads image-based lighting off the renderer, so the proof is
/// the ambient term rather than the backdrop: the same document, the same
/// light, and a materially brighter image with the environment installed.
#[test]
fn a_rasterized_render_is_lit_by_the_scene_environment() {
    let dir = scratch("raster");
    let lit = dir.join("lit.slxy");
    let dark = dir.join("dark.slxy");
    scene(&lit, true, false);
    scene(&dark, false, false);

    let Some(with) = render_floats(&lit, &dir.join("with.exr"), RenderEngine::Raster) else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let without =
        render_floats(&dark, &dir.join("without.exr"), RenderEngine::Raster).expect("adapter");

    let lit_subject = subject(&with, 64, 12);
    let dark_subject = subject(&without, 64, 12);
    assert!(
        lit_subject > dark_subject * 1.2,
        "image-based lighting did not reach the raster path: {lit_subject} against {dark_subject}"
    );
}

/// A backdrop is scene data when the document asks for it.
///
/// A background mode is otherwise a viewing preference, and a headless render
/// has no viewer whose preference it could be, so this is the one thing allowed
/// to move it: the environment node's own `background` parameter.
#[test]
fn a_scene_that_asks_to_be_shot_against_its_sky_is() {
    let dir = scratch("hdri-sky");
    let asked = dir.join("asked.slxy");
    let kept = dir.join("kept.slxy");
    scene(&asked, true, true);
    scene(&kept, true, false);

    let Some(shot) = render_floats(&asked, &dir.join("asked.exr"), RenderEngine::Raster) else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let default =
        render_floats(&kept, &dir.join("kept.exr"), RenderEngine::Raster).expect("adapter");

    let sky = corners(&shot, 64, 64);
    assert!(
        reads_as_the_authored_sky(sky),
        "the raster backdrop is not the authored sky: {sky:?}"
    );
    let gradient = corners(&default, 64, 64);
    assert!(
        !reads_as_the_authored_sky(gradient),
        "a scene that did not ask for its sky was shot against it anyway: {gradient:?}"
    );
}
