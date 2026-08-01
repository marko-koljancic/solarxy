//! Headless golden-image capture + comparison for renderer regression
//! testing. Made possible by the winit decoupling: `Renderer::new` builds
//! the full renderer without a window or surface.
//!
//! Capture a baseline before a rendering refactor, re-capture after, and
//! compare:
//!
//! ```bash
//! cargo run -p solarxy-renderer --example golden -- \
//!     capture --model res/models/xyzrgb_dragon.obj --out .goldens/baseline
//! cargo run -p solarxy-renderer --example golden -- \
//!     compare .goldens/baseline .goldens/after --tolerance 0
//! ```
//!
//! Blocking readback is deliberate here: this is a native-only dev tool,
//! not part of the app render path, which reads back asynchronously.

use anyhow::{Context, bail};

use solarxy_core::AABB;
use solarxy_core::preferences::{
    BackgroundMode, IblMode, InspectionMode, LineWeight, MaterialOverride, NormalsMode, PaneMode,
    ToneMode, UvMapBackground, UvMode, ViewMode,
};
use solarxy_core::view_config::{BoundsMode, PaneDisplaySettings};
use solarxy_renderer::camera::{Camera, CameraUniform};
use solarxy_renderer::camera_state::CameraState;
use solarxy_renderer::composite::CompositeLook;
use solarxy_renderer::lut::LutSlot;
use solarxy_renderer::frame::{Renderer, RendererInit};
use solarxy_renderer::ibl::BrdfLut;
use solarxy_renderer::scene::{BackgroundModeExt, ModelScene};

const WIDTH: u32 = 1024;
const HEIGHT: u32 = 768;
const MSAA: u32 = 4;
const SHADOW_MAP_SIZE: u32 = 2048;

/// The captured mode set: name + the pane settings that produce it.
fn modes() -> Vec<(&'static str, PaneDisplaySettings)> {
    let base = PaneDisplaySettings {
        view_mode: ViewMode::Shaded,
        prev_non_ghosted_mode: ViewMode::Shaded,
        ghosted_wireframe: false,
        normals_mode: NormalsMode::Off,
        background_mode: BackgroundMode::GRADIENT,
        uv_mode: UvMode::Off,
        bounds_mode: BoundsMode::Off,
        line_weight: LineWeight::Medium,
        show_grid: true,
        show_axis_gizmo: false,
        show_local_axes: false,
        inspection_mode: InspectionMode::Shaded,
        material_override: MaterialOverride::None,
        texel_density_target: 1.0,
        pane_mode: PaneMode::Scene3D,
        uv_bg: UvMapBackground::Dark,
        uv_offset: [0.0, 0.0],
        uv_zoom: 1.0,
        show_uv_overlap: false,
        show_validation: false,
        turntable_active: false,
    };
    vec![
        ("shaded", base),
        (
            "wireframe",
            PaneDisplaySettings {
                view_mode: ViewMode::WireframeOnly,
                ..base
            },
        ),
        (
            "material_id",
            PaneDisplaySettings {
                inspection_mode: InspectionMode::MaterialId,
                ..base
            },
        ),
        (
            "depth",
            PaneDisplaySettings {
                inspection_mode: InspectionMode::Depth,
                ..base
            },
        ),
        (
            "validation",
            PaneDisplaySettings {
                show_validation: true,
                ..base
            },
        ),
        // Clay routes the direct-light loop through `lambert_direct` and
        // overrides the shading normal, a branch the other five modes all
        // miss because they run `material_override: None`. Added for the
        // 0.8.1 world-space hoist, which rewrites exactly that
        // override, and kept afterwards so the branch stays gated.
        (
            "clay",
            PaneDisplaySettings {
                material_override: MaterialOverride::Clay,
                ..base
            },
        ),
    ]
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("capture") => capture(&args[1..]),
        Some("compare") => compare(&args[1..]),
        _ => {
            eprintln!(
                "usage:\n  golden capture --model <path> --out <dir>\n  golden compare <dir_a> <dir_b> [--tolerance N]"
            );
            bail!("missing or unknown subcommand");
        }
    }
}

fn arg_value(args: &[String], key: &str) -> Option<String> {
    args.iter()
        .position(|a| a == key)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn capture(args: &[String]) -> anyhow::Result<()> {
    let model_path = arg_value(args, "--model").context("--model <path> is required")?;
    let out_dir = arg_value(args, "--out").context("--out <dir> is required")?;
    std::fs::create_dir_all(&out_dir)?;

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: None,
        required_features: wgpu::Features::empty(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))?;

    // No window: fabricate the surface configuration the renderer sizes
    // its targets and composite pipeline against.
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: wgpu::TextureFormat::Bgra8UnormSrgb,
        width: WIDTH,
        height: HEIGHT,
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode: wgpu::CompositeAlphaMode::Opaque,
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };

    let background = BackgroundMode::GRADIENT.resolve(&[]);
    let (sky_top, sky_bottom) = background.sky_colors();
    let init = RendererInit {
        msaa_sample_count: MSAA,
        gradient_top: [0.35, 0.41, 0.47, 1.0],
        gradient_bottom: [0.66, 0.70, 0.72, 1.0],
        sky_top,
        sky_bottom,
        wireframe_color: background.wireframe_color(),
        wireframe_line_width: LineWeight::Medium.width_px(),
        bloom_enabled: false,
        ssao_enabled: false,
        tone_mode: ToneMode::AcesFilmic,
        exposure: 1.0,
        ibl_mode: IblMode::Full,
        uv_checker_png: include_bytes!("../../../res/textures/uv-checker_1k.png"),
    };
    let mut renderer = Renderer::new(&device, &queue, &config, &init)
        .map_err(|e| anyhow::anyhow!("Renderer::new: {e}"))?;

    let brdf_placeholder = BrdfLut::fallback(&device, &queue);
    let scene = ModelScene::new(
        model_path.clone(),
        &device,
        &queue,
        &renderer.layouts,
        &config,
        background.grid_color(),
        &brdf_placeholder,
        &renderer.ibl_res.ltc,
        SHADOW_MAP_SIZE,
    )
    .map_err(|e| anyhow::anyhow!("ModelScene::new: {e}"))?;

    let mut cam = CameraState::new(
        &device,
        &renderer.layouts.camera,
        &scene.model.bounds,
        WIDTH as f32 / HEIGHT as f32,
    );
    cam.update(&queue, 1.0 / 60.0);

    // Offscreen "surface" texture the composite pass tone-maps into.
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Golden Target"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: config.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());

    for (name, pds) in modes() {
        write_inspection_block(&queue, &cam, &pds, &scene.model.bounds);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Golden Encoder"),
        });
        let objects = [scene.draw_object()];
        renderer.render_shadow_pass(&mut encoder, &scene.env, &objects);
        renderer.render_main_pass(
            &mut encoder,
            &scene.env,
            &objects,
            &cam.bind_group,
            &cam.camera,
            &pds,
            background,
        );
        renderer.post.composite.write_params(
            &queue,
            false,
            false,
            &CompositeLook::from_tone(ToneMode::AcesFilmic, 1.0),
            &renderer.post.luts,
            pds.inspection_mode,
        );
        renderer.post.composite.render(
            &mut encoder,
            &renderer.pipelines,
            &target_view,
            false,
            &renderer.post.ssao,
            Some([0.0, 0.0, WIDTH as f32, HEIGHT as f32]),
            true,
        );
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = read_target(&device, &queue, &target)?;
        let path = format!("{out_dir}/{name}.png");
        image::RgbaImage::from_raw(WIDTH, HEIGHT, pixels)
            .context("malformed pixel buffer")?
            .save_with_format(&path, image::ImageFormat::Png)?;
        println!("GOLDEN wrote {path}");
    }

    // The principled surface proof.
    //
    // Every other capture here renders a material at its defaults, which is
    // exactly what makes them useful: the principled properties are all
    // identities at rest, so those captures must not move when the lobes
    // are added, and a diff in them means a regression. That leaves the
    // opposite question unanswered, and it is the one that matters just as
    // much: does any of this new code run at all? A capture that is
    // pixel-identical because the shader ignores the parameters looks
    // exactly like one that is pixel-identical because the defaults are
    // neutral.
    //
    // So: turn every principled property on at once and capture that. It is
    // not a physically sensible material, deliberately, because the job here
    // is coverage rather than beauty. Compare it against `shaded.png` and
    // the two must differ; compare it across commits and it gates every
    // lobe at once.
    {
        for mat in &scene.model.materials {
            let mut uniform = mat.uniform;
            uniform.ior = 1.7;
            uniform.transmission = 0.4;
            uniform.thickness = 0.5;
            uniform.attenuation_color = [0.8, 0.2, 0.1];
            uniform.attenuation_distance = 1.0;
            uniform.clearcoat = 0.8;
            uniform.clearcoat_roughness = 0.2;
            uniform.anisotropy = 0.6;
            uniform.anisotropy_rotation = 0.7;
            uniform.sheen_color = [0.3, 0.2, 0.5];
            uniform.sheen_roughness = 0.4;
            uniform.specular_color = [0.9, 0.8, 0.7];
            uniform.specular_intensity = 0.6;
            uniform.iridescence = 0.7;
            uniform.iridescence_ior = 1.8;
            uniform.iridescence_thickness_max = 550.0;
            queue.write_buffer(&mat.uniform_buffer, 0, bytemuck::cast_slice(&[uniform]));
        }

        let pds = modes()[0].1;
        write_inspection_block(&queue, &cam, &pds, &scene.model.bounds);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Golden Principled Encoder"),
        });
        let objects = [scene.draw_object()];
        renderer.render_shadow_pass(&mut encoder, &scene.env, &objects);
        renderer.render_main_pass(
            &mut encoder,
            &scene.env,
            &objects,
            &cam.bind_group,
            &cam.camera,
            &pds,
            background,
        );
        renderer.post.composite.write_params(
            &queue,
            false,
            false,
            &CompositeLook::from_tone(ToneMode::AcesFilmic, 1.0),
            &renderer.post.luts,
            pds.inspection_mode,
        );
        renderer.post.composite.render(
            &mut encoder,
            &renderer.pipelines,
            &target_view,
            false,
            &renderer.post.ssao,
            Some([0.0, 0.0, WIDTH as f32, HEIGHT as f32]),
            true,
        );
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = read_target(&device, &queue, &target)?;
        let path = format!("{out_dir}/principled.png");
        image::RgbaImage::from_raw(WIDTH, HEIGHT, pixels)
            .context("malformed pixel buffer")?
            .save_with_format(&path, image::ImageFormat::Png)?;
        println!("GOLDEN wrote {path} (every principled lobe at once)");

        // Put the materials back, so anything captured after this sees the
        // scene it expects rather than the stress material.
        for mat in &scene.model.materials {
            queue.write_buffer(&mat.uniform_buffer, 0, bytemuck::cast_slice(&[mat.uniform]));
        }
    }

    // The colour-grading proof, and it exists for exactly the reason the
    // principled capture above does.
    //
    // Every capture in the compare set renders at a neutral look, which is
    // what makes them useful: grading is an identity at rest, so none of
    // them may move when it lands. That leaves "does the grading code run
    // at all?" unanswered, and an inert feature and a missing feature
    // produce the same pixels.
    //
    // So: both slots loaded and a heavy grade, all at once. Not a look
    // anyone would ship, deliberately, because the job is coverage.
    {
        // A table nothing could produce by accident: channels rotated, so
        // a frame that came through it is unmistakable. Built here rather
        // than committed, because a 33-cubed .cube is most of a megabyte
        // of text and this is three lines.
        let n = 17u32;
        let last = f32::from(u16::try_from(n - 1).unwrap_or(1));
        let mut swapped = Vec::with_capacity((n as usize).pow(3) * 3);
        for b in 0..n {
            for g in 0..n {
                for r in 0..n {
                    swapped.push(f32::from(u16::try_from(b).unwrap_or(0)) / last);
                    swapped.push(f32::from(u16::try_from(r).unwrap_or(0)) / last);
                    swapped.push(f32::from(u16::try_from(g).unwrap_or(0)) / last);
                }
            }
        }
        let rotate = solarxy_core::LutCube::new(n, swapped, [0.0; 3], [1.0; 3]);
        renderer.set_lut(&device, &queue, LutSlot::A, Some(&rotate));
        renderer.set_lut(&device, &queue, LutSlot::B, Some(&rotate));

        let look = CompositeLook {
            tone_mode: ToneMode::AcesFilmic,
            exposure: 1.3,
            lift: [0.02, 0.0, -0.01],
            gamma: [1.1, 1.0, 0.9],
            gain: [1.0, 1.05, 1.2],
            lut_a_strength: 0.5,
            lut_b_strength: 0.75,
        };

        let pds = modes()[0].1;
        write_inspection_block(&queue, &cam, &pds, &scene.model.bounds);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Golden Graded Encoder"),
        });
        let objects = [scene.draw_object()];
        renderer.render_shadow_pass(&mut encoder, &scene.env, &objects);
        renderer.render_main_pass(
            &mut encoder,
            &scene.env,
            &objects,
            &cam.bind_group,
            &cam.camera,
            &pds,
            background,
        );
        renderer.post.composite.write_params(
            &queue,
            false,
            false,
            &look,
            &renderer.post.luts,
            pds.inspection_mode,
        );
        renderer.post.composite.render(
            &mut encoder,
            &renderer.pipelines,
            &target_view,
            false,
            &renderer.post.ssao,
            Some([0.0, 0.0, WIDTH as f32, HEIGHT as f32]),
            true,
        );
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = read_target(&device, &queue, &target)?;
        let path = format!("{out_dir}/graded.png");
        image::RgbaImage::from_raw(WIDTH, HEIGHT, pixels)
            .context("malformed pixel buffer")?
            .save_with_format(&path, image::ImageFormat::Png)?;
        println!("GOLDEN wrote {path} (both LUT slots plus lift/gamma/gain)");

        // Clear the slots, so anything captured after this sees the neutral
        // look the rest of the suite is built on.
        renderer.set_lut(&device, &queue, LutSlot::A, None);
        renderer.set_lut(&device, &queue, LutSlot::B, None);
    }

    // Extra (not part of the compare set): the multi-object proof — two
    // extra objects with independent transforms drawn through the
    // SceneObjects delta path beside the loaded model.
    {
        use solarxy_core::scene::{SceneDelta, SceneObjectId, SceneOp};
        use solarxy_renderer::scene_objects::{SceneObjects, cooked_from_parts};

        let d = scene.model.bounds.diagonal();
        let c = scene.model.bounds.center();
        let cube = |s: f32| {
            cooked_from_parts(
                "golden_cube",
                vec![
                    [-s, -s, -s],
                    [s, -s, -s],
                    [s, s, -s],
                    [-s, s, -s],
                    [-s, -s, s],
                    [s, -s, s],
                    [s, s, s],
                    [-s, s, s],
                ],
                vec![
                    0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 3, 7, 6, 3, 6, 2, 0, 4,
                    7, 0, 7, 3, 1, 2, 6, 1, 6, 5,
                ],
                None,
            )
        };
        let place = |dx: f32, dy: f32| -> [[f32; 4]; 4] {
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [c.x + dx, c.y + dy, c.z, 1.0],
            ]
        };

        let mut extra = SceneObjects::new();
        extra
            .apply(
                &device,
                &queue,
                &renderer.layouts,
                &SceneDelta {
                    ops: vec![
                        SceneOp::UpsertGeometry {
                            id: SceneObjectId(1),
                            geometry: std::sync::Arc::new(cube(d * 0.06)),
                        },
                        SceneOp::SetTransform {
                            id: SceneObjectId(1),
                            transform: place(-d * 0.45, d * 0.15),
                        },
                        SceneOp::UpsertGeometry {
                            id: SceneObjectId(2),
                            geometry: std::sync::Arc::new(cube(d * 0.035)),
                        },
                        SceneOp::SetTransform {
                            id: SceneObjectId(2),
                            transform: place(d * 0.45, d * 0.3),
                        },
                    ],
                },
            )
            .map_err(|e| anyhow::anyhow!("SceneObjects::apply: {e}"))?;

        let (name, pds) = &modes()[0];
        let _ = name;
        write_inspection_block(&queue, &cam, pds, &scene.model.bounds);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Golden Two-Object Encoder"),
        });
        let mut objects = vec![scene.draw_object()];
        objects.extend(extra.draw_objects());
        renderer.render_shadow_pass(&mut encoder, &scene.env, &objects);
        renderer.render_main_pass(
            &mut encoder,
            &scene.env,
            &objects,
            &cam.bind_group,
            &cam.camera,
            pds,
            background,
        );
        renderer.post.composite.write_params(
            &queue,
            false,
            false,
            &CompositeLook::from_tone(ToneMode::AcesFilmic, 1.0),
            &renderer.post.luts,
            pds.inspection_mode,
        );
        renderer.post.composite.render(
            &mut encoder,
            &renderer.pipelines,
            &target_view,
            false,
            &renderer.post.ssao,
            Some([0.0, 0.0, WIDTH as f32, HEIGHT as f32]),
            true,
        );
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = read_target(&device, &queue, &target)?;
        let path = format!("{out_dir}/two_objects.png");
        image::RgbaImage::from_raw(WIDTH, HEIGHT, pixels)
            .context("malformed pixel buffer")?
            .save_with_format(&path, image::ImageFormat::Png)?;
        println!("GOLDEN wrote {path} (exit-criterion proof, not compared)");
    }

    // 0.8.0 goldens growth: the non-triangle topologies and vertex colors
    // (point quads, a line list, a vertex-colored quad through the PBR
    // colored path) via the SceneObjects delta path. Compared once both
    // sides of a diff carry it (see `grown_captures`).
    {
        use std::sync::Arc;

        use solarxy_core::MeshTopology;
        use solarxy_core::geometry::compute_bounds;
        use solarxy_core::scene::{CookedGeometry, CookedMesh, SceneDelta, SceneObjectId, SceneOp};
        use solarxy_renderer::scene_objects::SceneObjects;

        let d = scene.model.bounds.diagonal();
        let c = scene.model.bounds.center();

        // A rainbow 8x8 point grid (the scatter-output stand-in).
        let mut grid_pos = Vec::new();
        let mut grid_col = Vec::new();
        for i in 0..8u32 {
            for j in 0..8u32 {
                let (fi, fj) = (i as f32 / 7.0, j as f32 / 7.0);
                grid_pos.push([(fi - 0.5) * d * 0.5, fj * d * 0.35, 0.0]);
                grid_col.push([fi, fj, 1.0 - fi, 1.0]);
            }
        }
        let cloud = CookedMesh {
            name: "golden_cloud".to_string(),
            positions: Arc::new(grid_pos),
            normals: None,
            tex_coords: None,
            indices: Arc::new(Vec::new()),
            material_index: None,
            topology: MeshTopology::Points,
            colors: Some(Arc::new(grid_col)),
            instances: None,
        };

        // A colored zigzag polyline.
        let mut wire_pos = Vec::new();
        let mut wire_col = Vec::new();
        let mut wire_idx = Vec::new();
        for i in 0..16u32 {
            let t = i as f32 / 15.0;
            let y = if i % 2 == 0 { 0.0 } else { d * 0.06 };
            wire_pos.push([(t - 0.5) * d * 0.6, y - d * 0.12, d * 0.05]);
            wire_col.push([1.0 - t, t, 0.5, 1.0]);
            if i > 0 {
                wire_idx.push(i - 1);
                wire_idx.push(i);
            }
        }
        let wire = CookedMesh {
            name: "golden_wire".to_string(),
            positions: Arc::new(wire_pos),
            normals: None,
            tex_coords: None,
            indices: Arc::new(wire_idx),
            material_index: None,
            topology: MeshTopology::Lines,
            colors: Some(Arc::new(wire_col)),
            instances: None,
        };

        // A vertex-colored quad through the colored PBR pipeline.
        let q = d * 0.12;
        let quad = CookedMesh {
            name: "golden_quad".to_string(),
            positions: Arc::new(vec![
                [-q, -q - d * 0.3, 0.0],
                [q, -q - d * 0.3, 0.0],
                [q, q - d * 0.3, 0.0],
                [-q, q - d * 0.3, 0.0],
            ]),
            normals: Some(Arc::new(vec![[0.0, 0.0, 1.0]; 4])),
            tex_coords: None,
            indices: Arc::new(vec![0, 1, 2, 0, 2, 3]),
            material_index: None,
            topology: MeshTopology::Triangles,
            colors: Some(Arc::new(vec![
                [1.0, 0.0, 0.0, 1.0],
                [0.0, 1.0, 0.0, 1.0],
                [0.0, 0.0, 1.0, 1.0],
                [1.0, 1.0, 1.0, 1.0],
            ])),
            instances: None,
        };

        let all_positions: Vec<[f32; 3]> = cloud
            .positions
            .iter()
            .chain(wire.positions.iter())
            .chain(quad.positions.iter())
            .copied()
            .collect();
        let geometry = CookedGeometry {
            bounds: compute_bounds(&all_positions),
            meshes: vec![cloud, wire, quad],
            materials: Vec::new(),
        };

        let mut extra = SceneObjects::new();
        extra
            .apply(
                &device,
                &queue,
                &renderer.layouts,
                &SceneDelta {
                    ops: vec![
                        SceneOp::UpsertGeometry {
                            id: SceneObjectId(1),
                            geometry: Arc::new(geometry),
                        },
                        SceneOp::SetTransform {
                            id: SceneObjectId(1),
                            transform: [
                                [1.0, 0.0, 0.0, 0.0],
                                [0.0, 1.0, 0.0, 0.0],
                                [0.0, 0.0, 1.0, 0.0],
                                [c.x, c.y + d * 0.05, c.z + d * 0.3, 1.0],
                            ],
                        },
                    ],
                },
            )
            .map_err(|e| anyhow::anyhow!("SceneObjects::apply: {e}"))?;

        let (_, pds) = &modes()[0];
        write_inspection_block(&queue, &cam, pds, &scene.model.bounds);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Golden Topology Encoder"),
        });
        let mut objects = vec![scene.draw_object()];
        objects.extend(extra.draw_objects());
        renderer.render_shadow_pass(&mut encoder, &scene.env, &objects);
        renderer.render_main_pass(
            &mut encoder,
            &scene.env,
            &objects,
            &cam.bind_group,
            &cam.camera,
            pds,
            background,
        );
        renderer.post.composite.write_params(
            &queue,
            false,
            false,
            &CompositeLook::from_tone(ToneMode::AcesFilmic, 1.0),
            &renderer.post.luts,
            pds.inspection_mode,
        );
        renderer.post.composite.render(
            &mut encoder,
            &renderer.pipelines,
            &target_view,
            false,
            &renderer.post.ssao,
            Some([0.0, 0.0, WIDTH as f32, HEIGHT as f32]),
            true,
        );
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = read_target(&device, &queue, &target)?;
        let path = format!("{out_dir}/topology.png");
        image::RgbaImage::from_raw(WIDTH, HEIGHT, pixels)
            .context("malformed pixel buffer")?
            .save_with_format(&path, image::ImageFormat::Png)?;
        println!("GOLDEN wrote {path} (points/lines/vertex colors)");
    }

    println!(
        "GOLDEN capture complete: {} modes in {out_dir}",
        modes().len()
    );
    Ok(())
}

/// Captures added after the original mode set (the goldens-growth seam).
/// Compared only when BOTH sides of a diff carry them: the base side of
/// the commit that introduces one cannot have captured it, and a one-sided
/// absence is a notice rather than a failure.
///
/// A name may also be a [`modes`] entry (`clay` is): listing it here is
/// what makes it *captured* like any other mode but *compared* leniently,
/// until the base side of a diff has it too. Drop the name from this list
/// once every branch in flight carries the capture, and it becomes a hard
/// gate.
fn grown_captures() -> &'static [&'static str] {
    &["topology", "clay", "principled", "graded"]
}

/// Replicates the app's per-pane partial camera-uniform write (inspection
/// mode, texel density, overrides, depth bounds) for deterministic modes.
fn write_inspection_block(
    queue: &wgpu::Queue,
    cam: &CameraState,
    pds: &PaneDisplaySettings,
    bounds: &AABB,
) {
    let (near, far) = depth_bounds(&cam.camera, bounds);
    let data: [u32; 8] = [
        pds.inspection_mode.as_u32(),
        pds.texel_density_target.to_bits(),
        pds.material_override.as_u32(),
        near.to_bits(),
        far.to_bits(),
        1.0f32.to_bits(),
        1.0f32.to_bits(),
        0.0f32.to_bits(),
    ];
    queue.write_buffer(
        &cam.buffer,
        CameraUniform::INSPECTION_OFFSET,
        bytemuck::cast_slice(&data),
    );
}

/// Same math as the app's `compute_depth_bounds`.
fn depth_bounds(camera: &Camera, bounds: &AABB) -> (f32, f32) {
    let view = camera.build_view_matrix();
    let mut z_min = f32::INFINITY;
    let mut z_max = f32::NEG_INFINITY;
    for corner in &bounds.corners() {
        let vp = view * corner.to_homogeneous();
        let z = -vp.z;
        z_min = z_min.min(z);
        z_max = z_max.max(z);
    }
    z_min = z_min.max(0.001);
    if z_max <= z_min {
        z_max = z_min + 1.0;
    }
    (z_min, z_max)
}

/// Blocking BGRA readback of the offscreen target, returned as RGBA rows.
fn read_target(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &wgpu::Texture,
) -> anyhow::Result<Vec<u8>> {
    let bytes_per_pixel = 4u32;
    let unpadded = WIDTH * bytes_per_pixel;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded = unpadded.div_ceil(align) * align;

    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Golden Readback"),
        size: u64::from(padded * HEIGHT),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Golden Copy Encoder"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(HEIGHT),
            },
        },
        wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(std::iter::once(encoder.finish()));

    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    match rx.recv() {
        Ok(Ok(())) => {}
        other => bail!("map_async failed: {other:?}"),
    }

    let data = slice.get_mapped_range();
    let mut pixels = Vec::with_capacity((unpadded * HEIGHT) as usize);
    for row in 0..HEIGHT {
        let start = (row * padded) as usize;
        pixels.extend_from_slice(&data[start..start + unpadded as usize]);
    }
    drop(data);
    buffer.unmap();

    // BGRA -> RGBA.
    for chunk in pixels.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }
    Ok(pixels)
}

fn compare(args: &[String]) -> anyhow::Result<()> {
    let (Some(dir_a), Some(dir_b)) = (args.first(), args.get(1)) else {
        bail!("compare needs <dir_a> <dir_b>");
    };
    let tolerance: u8 = arg_value(args, "--tolerance")
        .map(|t| t.parse())
        .transpose()?
        .unwrap_or(0);

    // Required set = every mode EXCEPT those still flagged as grown; a
    // grown name is re-added below under the present-on-both-sides rule,
    // so a newly added mode cannot fail a diff whose base predates it.
    let mut names: Vec<String> = modes()
        .iter()
        .map(|(n, _)| (*n).to_string())
        .filter(|n| !grown_captures().contains(&n.as_str()))
        .collect();
    for extra in grown_captures() {
        let pa = format!("{dir_a}/{extra}.png");
        let pb = format!("{dir_b}/{extra}.png");
        if std::path::Path::new(&pa).exists() && std::path::Path::new(&pb).exists() {
            names.push((*extra).to_string());
        } else {
            println!("GOLDEN {extra}: skipped (grown capture absent on one side)");
        }
    }

    let mut failures = 0usize;
    for name in names {
        let pa = format!("{dir_a}/{name}.png");
        let pb = format!("{dir_b}/{name}.png");
        let a = image::open(&pa).with_context(|| pa.clone())?.to_rgba8();
        let b = image::open(&pb).with_context(|| pb.clone())?.to_rgba8();
        if a.dimensions() != b.dimensions() {
            println!(
                "GOLDEN {name}: DIMENSION MISMATCH {:?} vs {:?}",
                a.dimensions(),
                b.dimensions()
            );
            failures += 1;
            continue;
        }
        let mut max_delta = 0u8;
        let mut differing = 0usize;
        for (pa, pb) in a.pixels().zip(b.pixels()) {
            let mut pixel_differs = false;
            for c in 0..4 {
                let d = pa.0[c].abs_diff(pb.0[c]);
                max_delta = max_delta.max(d);
                if d > tolerance {
                    pixel_differs = true;
                }
            }
            if pixel_differs {
                differing += 1;
            }
        }
        let total = (a.width() * a.height()) as usize;
        let status = if differing == 0 { "OK" } else { "DIFF" };
        println!(
            "GOLDEN {name}: {status} max_channel_delta={max_delta} differing_pixels={differing}/{total}"
        );
        if differing > 0 {
            failures += 1;
        }
    }
    if failures > 0 {
        bail!("{failures} golden(s) differ beyond tolerance {tolerance}");
    }
    println!("GOLDEN compare: all modes match (tolerance {tolerance})");
    Ok(())
}
