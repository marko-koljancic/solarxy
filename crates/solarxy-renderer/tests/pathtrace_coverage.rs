//! Coverage on a real device: does a transparent render's matte count what the
//! camera actually saw, exactly.
//!
//! The decisive case is the mirror against the sky. The colour target's alpha
//! lane counts samples that *described* a surface, which deliberately skips a
//! mirror, so a matte built from it would cut a hole where the camera plainly
//! saw metal. Coverage is its own count in its own buffer, and the first test
//! holds the two apart on the exact scene where they disagree.
//!
//! The count is asserted bit-exact between a chunked run and a one-shot run at
//! 8192 samples -- the largest count the render node offers, not a convenient
//! one -- because an integer sum in a `u32` has no reassociation to hide
//! behind: any difference means the two runs disagreed about which samples
//! they drew.

mod common;

use cgmath::SquareMatrix;
use solarxy_bvh::Bvh;
use solarxy_core::AABB;
use solarxy_core::geometry::RawMaterialData;
use solarxy_core::preferences::ProjectionMode;
use solarxy_renderer::camera::{Camera, CameraUniform};
use solarxy_renderer::pathtrace::arena::{ArenaMesh, ArenaPlacement, INSTANCE_VISIBLE, TraceArena};
use solarxy_renderer::pathtrace::material::TracedMaterial;
use solarxy_renderer::pathtrace::scene::MaterialTextures;
use solarxy_renderer::pathtrace::{
    CoveragePoll, EnvParams, PathEstimator, PathKernel, PathUniforms, ReadbackPoll, TraceAtlas,
    TraceParams, TraceScene, TraceTarget,
};

const WIDTH: u32 = 24;
const HEIGHT: u32 = 24;
const SEED: u32 = 0xC0FE_1234;

/// The sky every test renders against: bright enough that a suppressed
/// environment is a measurable absence, not a comparison of zeros.
fn sky() -> EnvParams {
    EnvParams::constant([0.1, 0.12, 0.16], [0.03, 0.03, 0.04])
}

struct Rig {
    scene: TraceScene,
    atlas: TraceAtlas,
    kernel: PathKernel,
    uniforms: PathUniforms,
    target: TraceTarget,
}

/// A unit sphere on nothing, wearing the given material, seen from far enough
/// back that the corners of the frame are sky.
fn rig(gpu: &common::Gpu, raw: &RawMaterialData) -> Rig {
    let (positions, indices) = solarxy_bvh::corpus::sphere(48, 24);
    let bvh = Bvh::build_triangles(&positions, &indices);
    let mesh = ArenaMesh {
        bvh: &bvh,
        positions: &positions,
        indices: &indices,
        normals: None,
        uv0: None,
    };
    let placement = ArenaPlacement {
        mesh: 0,
        world: cgmath::Matrix4::identity().into(),
        inv_world: cgmath::Matrix4::identity().into(),
        material_base: 0,
        flags: INSTANCE_VISIBLE,
    };
    let tlas = Bvh::build_tlas(&[AABB {
        min: [-1.0, -1.0, -1.0].into(),
        max: [1.0, 1.0, 1.0].into(),
    }]);
    let material = TracedMaterial::from_raw(raw, &MaterialTextures::default());
    let arena = TraceArena::build(&tlas, &[mesh], &[placement]).with_materials(vec![material]);

    let scene = TraceScene::upload(&gpu.device, &gpu.queue, &gpu.pathtrace, &arena);
    let atlas = TraceAtlas::new(&gpu.device, &gpu.queue, &gpu.pathtrace);

    let camera = Camera {
        eye: cgmath::Point3::new(0.0, 0.4, 3.4),
        target: cgmath::Point3::new(0.0, 0.0, 0.0),
        up: cgmath::Vector3::unit_y(),
        aspect: 1.0,
        fovy: 40.0,
        znear: 0.1,
        zfar: 100.0,
        projection: ProjectionMode::Perspective,
        ortho_scale: 1.0,
    };
    let mut camera_uniform = CameraUniform::new();
    camera_uniform.update_view_proj(&camera);
    let camera_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Coverage Camera"),
        size: std::mem::size_of::<CameraUniform>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    gpu.queue
        .write_buffer(&camera_buffer, 0, bytemuck::bytes_of(&camera_uniform));

    let uniforms = PathUniforms::new(&gpu.device, &camera_buffer);
    let kernel = PathKernel::new(&gpu.device, &gpu.pathtrace, &uniforms);
    let target = TraceTarget::new(&gpu.device, &gpu.pathtrace, WIDTH, HEIGHT);
    Rig {
        scene,
        atlas,
        kernel,
        uniforms,
        target,
    }
}

fn params(samples: u32, transparent: bool) -> TraceParams {
    TraceParams {
        tile_offset: [0, 0],
        tile_size: [WIDTH, HEIGHT],
        resolution: [WIDTH, HEIGHT],
        bounces: 4,
        transmissive_bounces: 4,
        samples,
        seed: SEED,
        light_count: 0,
        flags: if transparent {
            TraceParams::FLAG_TRANSPARENT
        } else {
            0
        },
        ..TraceParams::default()
    }
}

/// Runs `samples` in chunks of `chunk` (zero is the one-shot dispatch) and
/// reads back the colour and the coverage counts.
fn run(
    gpu: &common::Gpu,
    rig: &mut Rig,
    samples: u32,
    chunk: u32,
    transparent: bool,
) -> (Vec<f32>, Vec<u32>) {
    let environment = sky();
    let mut done = 0;
    loop {
        let step = if chunk == 0 {
            samples
        } else {
            chunk.min(samples - done)
        };
        let p = TraceParams {
            chunk: if chunk == 0 { 0 } else { step },
            sample_base: done,
            ..params(samples, transparent)
        };
        rig.uniforms.write(&gpu.queue, &p, &environment);
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Coverage Encoder"),
            });
        rig.kernel.encode(
            &mut encoder,
            PathEstimator::Mis,
            &rig.scene,
            &rig.atlas,
            &rig.target,
            &rig.uniforms,
            [WIDTH, HEIGHT],
        );
        gpu.queue.submit(Some(encoder.finish()));
        done += step;
        if done >= samples {
            break;
        }
        // Between dispatches, never after the last one: everything downstream
        // reads the write slot. The coverage buffer does not care -- it is one
        // buffer shared by both slots -- but the colour comparison does.
        rig.target.swap();
    }

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Coverage Readback Encoder"),
        });
    let mut color = rig.target.encode_readback(&gpu.device, &mut encoder);
    let mut coverage = rig
        .target
        .encode_coverage_readback(&gpu.device, &mut encoder);
    gpu.queue.submit(Some(encoder.finish()));

    let color = loop {
        match color.poll(&gpu.device) {
            ReadbackPoll::Ready(values) => break values,
            ReadbackPoll::Pending => std::thread::sleep(std::time::Duration::from_millis(1)),
            ReadbackPoll::Failed => panic!("colour readback failed"),
        }
    };
    let coverage = loop {
        match coverage.poll(&gpu.device) {
            CoveragePoll::Ready(values) => break values,
            CoveragePoll::Pending => std::thread::sleep(std::time::Duration::from_millis(1)),
            CoveragePoll::Failed => panic!("coverage readback failed"),
        }
    };
    (color, coverage)
}

fn at(x: u32, y: u32) -> usize {
    (y * WIDTH + x) as usize
}

fn rough() -> RawMaterialData {
    RawMaterialData {
        base_color_factor: [0.8, 0.8, 0.8, 1.0],
        roughness_factor: 0.35,
        metallic_factor: 0.0,
        ..RawMaterialData::default()
    }
}

/// A mirror is covered where it describes nothing: the case that separates
/// coverage from the described count, and the reason the matte could not
/// simply read the alpha lane.
#[test]
fn a_mirror_against_the_sky_is_covered_where_nothing_is_described() {
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };
    let mirror = RawMaterialData {
        base_color_factor: [0.9, 0.9, 0.9, 1.0],
        roughness_factor: 0.0,
        metallic_factor: 1.0,
        ..RawMaterialData::default()
    };
    let mut rig = rig(&gpu, &mirror);
    let samples = 32;
    let (color, coverage) = run(&gpu, &mut rig, samples, 0, true);

    let center = at(WIDTH / 2, HEIGHT / 2);
    assert_eq!(
        coverage[center], samples,
        "the camera saw metal on every sample"
    );
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let described = color[center * 4 + 3] as u32;
    assert_eq!(described, 0, "a mirror describes nothing");

    let corner = at(0, 0);
    assert_eq!(coverage[corner], 0, "the sky is not covered");
}

/// Chunked and one-shot runs land on the identical count, at the largest
/// sample count the render node offers rather than a convenient one, and a
/// silhouette pixel is genuinely fractional.
#[test]
fn a_silhouette_is_fractional_and_chunking_cannot_move_the_count() {
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };
    let samples = 8192;
    let mut one_shot = rig(&gpu, &rough());
    let (_, whole) = run(&gpu, &mut one_shot, samples, 0, true);
    let mut chunked = rig(&gpu, &rough());
    let (_, pieces) = run(&gpu, &mut chunked, samples, 512, true);

    assert_eq!(
        whole, pieces,
        "an integer count has no reassociation to drift by"
    );
    assert_eq!(whole[at(WIDTH / 2, HEIGHT / 2)], samples);
    assert_eq!(whole[at(0, 0)], 0);
    assert!(
        whole.iter().any(|&c| c > 0 && c < samples),
        "a silhouette pixel is covered by some samples and not others"
    );
}

/// Glass is a surface the camera found: a window mattes opaque, and what
/// refracted through it is in the colour.
#[test]
fn glass_is_covered_because_the_camera_found_a_surface() {
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };
    let glass = RawMaterialData {
        base_color_factor: [1.0, 1.0, 1.0, 1.0],
        roughness_factor: 0.0,
        metallic_factor: 0.0,
        transmission: 1.0,
        ..RawMaterialData::default()
    };
    let mut rig = rig(&gpu, &glass);
    let samples = 32;
    let (_, coverage) = run(&gpu, &mut rig, samples, 0, true);
    assert_eq!(
        coverage[at(WIDTH / 2, HEIGHT / 2)],
        samples,
        "a transmissive surface is covered"
    );
}

/// The environment still lights the scene: on a pixel the camera fully
/// covered, the transparent render is bit-identical to the opaque one, and on
/// a pixel it never covered, the transparent render is exactly nothing.
///
/// Bit-identical rather than close, because a covered sample's arithmetic is
/// untouched by the flag: the suppression branch only exists on the camera
/// segment's own miss, which a covered sample by definition never took.
#[test]
fn the_environment_lights_covered_pixels_identically() {
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };
    let mut rig = rig(&gpu, &rough());
    let samples = 64;
    let (opaque, _) = run(&gpu, &mut rig, samples, 0, false);
    let (transparent, coverage) = run(&gpu, &mut rig, samples, 0, true);

    let mut full = 0;
    let mut empty = 0;
    for (i, &covered) in coverage.iter().enumerate() {
        let o = &opaque[i * 4..i * 4 + 3];
        let t = &transparent[i * 4..i * 4 + 3];
        if covered == samples {
            assert_eq!(o, t, "a fully covered pixel is lit identically");
            full += 1;
        }
        if covered == 0 {
            assert_eq!(t, [0.0, 0.0, 0.0], "an uncovered pixel holds nothing");
            empty += 1;
        }
        // The invariant that holds the two counts in order: a sample that
        // described a surface necessarily found one.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let described = transparent[i * 4 + 3] as u32;
        assert!(
            described <= covered,
            "described samples are covered samples"
        );
    }
    assert!(full > 0, "the sphere covers pixels");
    assert!(empty > 0, "the sky leaves pixels uncovered");
}
