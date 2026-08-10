//! Accumulation on a real device: does a render split across dispatches reach
//! the same image as one, does the firefly clamp bound what it claims to, and
//! does the resolve hand the accumulator to the post chain unchanged.
//!
//! The first question is the one the whole stage rests on. A still render is
//! paced by chunking, and a chunked mean that drifts from a one-shot mean is a
//! bug that looks like noise: the picture is plausible, it is just not the one
//! the estimator converges to. Nothing else in the suite would notice.
//!
//! The comparison is tight rather than approximate on purpose. Both runs draw
//! the *same samples* -- the sampler's domain is the whole run and a chunk takes
//! a disjoint slice of it -- so the only thing that differs is the order the
//! sums are accumulated in. Anything beyond floating-point reassociation means
//! the two runs disagree about which samples they drew, which is the failure
//! this exists to catch.

mod common;

use cgmath::SquareMatrix;
use solarxy_bvh::Bvh;
use solarxy_core::AABB;
use solarxy_core::geometry::RawMaterialData;
use solarxy_core::preferences::ProjectionMode;
use solarxy_core::scene::{LightDef, LightKind};
use solarxy_renderer::camera::{Camera, CameraUniform};
use solarxy_renderer::pathtrace::arena::{ArenaMesh, ArenaPlacement, INSTANCE_VISIBLE, TraceArena};
use solarxy_renderer::pathtrace::light::TracedLight;
use solarxy_renderer::pathtrace::material::TracedMaterial;
use solarxy_renderer::pathtrace::resolve::TraceResolve;
use solarxy_renderer::pathtrace::scene::MaterialTextures;
use solarxy_renderer::pathtrace::{
    EnvParams, PathEstimator, PathKernel, PathUniforms, ReadbackPoll, TraceAtlas, TraceParams,
    TraceScene, TraceTarget,
};
use solarxy_renderer::texture::Texture;

const WIDTH: u32 = 48;
const HEIGHT: u32 = 48;
const SEED: u32 = 0x51ED_1234;

/// Everything one of these tests needs on the GPU, assembled once.
struct Rig {
    scene: TraceScene,
    atlas: TraceAtlas,
    kernel: PathKernel,
    uniforms: PathUniforms,
    target: TraceTarget,
    light_count: u32,
}

/// A rough sphere on nothing, optionally under one light.
///
/// The sphere rather than a plane because a curved surface spreads its normals
/// across the frame, so a mistake that depends on the direction a path left in
/// shows up somewhere rather than nowhere.
fn rig(gpu: &common::Gpu, light: Option<LightDef>) -> Rig {
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

    // Rough and not metallic, so the surface scatters broadly and the
    // auxiliary channels are recorded at the first hit rather than deferred
    // past a mirror.
    let raw = RawMaterialData {
        base_color_factor: [0.8, 0.8, 0.8, 1.0],
        roughness_factor: 0.35,
        metallic_factor: 0.0,
        ..RawMaterialData::default()
    };
    let material = TracedMaterial::from_raw(&raw, &MaterialTextures::default());

    let lights = light.map(|l| TracedLight::pool(&[l])).unwrap_or_default();
    let light_count = u32::try_from(lights.len()).unwrap_or(0);
    let arena = TraceArena::build(&tlas, &[mesh], &[placement])
        .with_materials(vec![material])
        .with_lights(lights);

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
        label: Some("Accumulate Camera"),
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
        light_count,
    }
}

fn base_params(rig: &Rig, samples: u32) -> TraceParams {
    TraceParams {
        tile_offset: [0, 0],
        tile_size: [WIDTH, HEIGHT],
        resolution: [WIDTH, HEIGHT],
        bounces: 4,
        transmissive_bounces: 0,
        samples,
        seed: SEED,
        light_count: rig.light_count,
        aperture_radius: 0.0,
        focus_distance: 0.0,
        aperture_blades: 0,
        ..TraceParams::default()
    }
}

fn drain(
    readback: &mut solarxy_renderer::pathtrace::FloatReadback,
    device: &wgpu::Device,
) -> Vec<f32> {
    loop {
        match readback.poll(device) {
            ReadbackPoll::Ready(values) => return values,
            ReadbackPoll::Pending => std::thread::sleep(std::time::Duration::from_millis(1)),
            ReadbackPoll::Failed => panic!("accumulation readback failed"),
        }
    }
}

/// Runs `samples` samples in chunks of `chunk` and reads the mean back.
///
/// `chunk` of zero is the one-shot dispatch every caller written before the
/// accumulator existed produces.
fn run(
    gpu: &common::Gpu,
    rig: &mut Rig,
    estimator: PathEstimator,
    environment: &EnvParams,
    samples: u32,
    chunk: u32,
    clamp: f32,
) -> (Vec<f32>, Vec<f32>) {
    let mut done = 0;
    loop {
        let step = if chunk == 0 {
            samples
        } else {
            chunk.min(samples - done)
        };
        let params = TraceParams {
            chunk: if chunk == 0 { 0 } else { step },
            sample_base: done,
            firefly_clamp: clamp,
            ..base_params(rig, samples)
        };
        rig.uniforms.write(&gpu.queue, &params, environment);
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Accumulate Encoder"),
            });
        rig.kernel.encode(
            &mut encoder,
            estimator,
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
        // reads the write slot.
        rig.target.swap();
    }

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Accumulate Readback Encoder"),
        });
    let mut color = rig.target.encode_readback(&gpu.device, &mut encoder);
    let mut aux = rig
        .target
        .encode_auxiliary_readback(&gpu.device, &mut encoder);
    gpu.queue.submit(Some(encoder.finish()));
    (drain(&mut color, &gpu.device), drain(&mut aux, &gpu.device))
}

fn luminance(px: &[f32]) -> f32 {
    0.2126 * px[0] + 0.7152 * px[1] + 0.0722 * px[2]
}

/// A broad panel above the sphere, bright enough to matter and soft enough that
/// the estimator is not measuring its own tolerance.
fn panel() -> LightDef {
    LightDef {
        area_extent: [5.0, 5.0],
        intensity: 4.0,
        ..base_light()
    }
}

/// A rect light with everything but its shape and brightness at rest.
fn base_light() -> LightDef {
    LightDef {
        kind: LightKind::RectArea,
        position: [0.0, 3.0, 0.0],
        direction: [0.0, -1.0, 0.0],
        color: [1.0, 1.0, 1.0],
        intensity: 1.0,
        range: 0.0,
        decay: 0.0,
        radius: 0.0,
        inner_cone: 0.0,
        outer_cone: std::f32::consts::FRAC_PI_4,
        area_extent: [2.0, 2.0],
        rotate: [0.0; 3],
        two_sided: false,
        ground_color: [0.0; 3],
        cast_shadow: false,
        shadow_map_size: 1024,
        shadow_bias: 0.0,
        visible: true,
        show_helper: false,
        helper_size: 1.0,
    }
}

#[test]
fn a_chunked_run_reaches_the_same_image_as_one_dispatch() {
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };
    let environment = EnvParams::constant([0.1, 0.12, 0.16], [0.03, 0.03, 0.04]);

    let mut one = rig(&gpu, Some(panel()));
    let (whole, whole_aux) = run(&gpu, &mut one, PathEstimator::Mis, &environment, 16, 0, 0.0);

    let mut many = rig(&gpu, Some(panel()));
    let (chunked, chunked_aux) = run(
        &gpu,
        &mut many,
        PathEstimator::Mis,
        &environment,
        16,
        4,
        0.0,
    );

    // Four dispatches against one, over the same sixteen samples. Reassociation
    // is the only difference there should be, so the tolerance is a hair rather
    // than a hedge: at a percent this test would pass with the chunks drawing
    // entirely different samples.
    let mut worst = 0.0f64;
    for (a, b) in whole.chunks_exact(4).zip(chunked.chunks_exact(4)) {
        for c in 0..3 {
            let (x, y) = (f64::from(a[c]), f64::from(b[c]));
            let denom = x.abs().max(y.abs()).max(1e-4);
            worst = worst.max((x - y).abs() / denom);
        }
    }
    assert!(
        worst < 1e-4,
        "a chunked run drifted from a one-shot run by {worst:.3e} relative; \
         the two are supposed to draw the same samples and differ only in the \
         order their sums are accumulated"
    );

    // The auxiliary channels take a different route -- each chunk reduces to its
    // own mean and the means are combined by chunk size -- so they get their own
    // tolerance rather than riding the colour's.
    let mut aux_worst = 0.0f64;
    for (a, b) in whole_aux.chunks_exact(4).zip(chunked_aux.chunks_exact(4)) {
        for c in 0..3 {
            let (x, y) = (f64::from(a[c]), f64::from(b[c]));
            aux_worst = aux_worst.max((x - y).abs());
        }
    }
    assert!(
        aux_worst < 2e-3,
        "the accumulated albedo drifted by {aux_worst:.3e} between a chunked \
         run and a one-shot one"
    );
}

#[test]
fn the_same_seed_renders_the_same_image_twice() {
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };
    let environment = EnvParams::constant([0.1, 0.12, 0.16], [0.03, 0.03, 0.04]);

    let mut first = rig(&gpu, Some(panel()));
    let (a, _) = run(
        &gpu,
        &mut first,
        PathEstimator::Mis,
        &environment,
        8,
        2,
        0.0,
    );
    let mut second = rig(&gpu, Some(panel()));
    let (b, _) = run(
        &gpu,
        &mut second,
        PathEstimator::Mis,
        &environment,
        8,
        2,
        0.0,
    );

    // Bit-identical, asserted as equality rather than a tolerance. A fixed seed
    // is the release's reproducibility claim, and a claim checked to within a
    // tolerance is a different, weaker claim.
    assert_eq!(
        a, b,
        "two runs of the same scene at the same seed produced different images"
    );
}

/// The clamp's own test, and it needs a scene that actually speckles.
///
/// Scattering only, with the light out of frame. Under that estimator a path
/// finds the light by chance rather than by aiming, which is precisely the
/// sampling failure a firefly is, and nothing the camera sees directly is an
/// emitter, so every bright value in the image arrived after a scatter and is
/// inside the clamp's remit. A small light rather than a panel, because a broad
/// source is easy to find and produces no fireflies at all.
#[test]
fn the_clamp_bounds_what_a_path_found_after_it_scattered() {
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };
    // Dim, so a pixel that sees no surface contributes almost nothing and the
    // bound below is about the clamp rather than about the sky.
    let environment = EnvParams::constant([0.01, 0.01, 0.01], [0.01, 0.01, 0.01]);
    let light = LightDef {
        position: [0.0, 2.0, 0.0],
        area_extent: [0.35, 0.35],
        intensity: 30000.0,
        ..base_light()
    };

    const CLAMP: f32 = 1.0;

    let mut loose = rig(&gpu, Some(light.clone()));
    let (unclamped, _) = run(
        &gpu,
        &mut loose,
        PathEstimator::Scatter,
        &environment,
        8,
        0,
        0.0,
    );
    let mut tight = rig(&gpu, Some(light.clone()));
    let (clamped, _) = run(
        &gpu,
        &mut tight,
        PathEstimator::Scatter,
        &environment,
        8,
        0,
        CLAMP,
    );

    let peak = |px: &[f32]| {
        px.chunks_exact(4)
            .map(luminance)
            .fold(0.0f32, |a, b| a.max(b))
    };
    let loose_peak = peak(&unclamped);
    let tight_peak = peak(&clamped);

    // Measured on the reference machine: 4018.7 unclamped against 0.134
    // clamped, at a clamp of one. Three orders of margin either side, so this
    // is testing the clamp rather than testing a threshold.
    assert!(
        loose_peak > CLAMP * 2.0,
        "the scene was supposed to speckle without a clamp and its brightest \
         pixel only reached {loose_peak}; the test is no longer measuring \
         anything"
    );
    // The mean of values each at or below the clamp cannot exceed it. The
    // margin is the environment a camera ray reaches without scattering, which
    // the clamp does not touch and which this scene keeps at a hundredth.
    assert!(
        tight_peak <= CLAMP + 0.05,
        "a clamp of {CLAMP} left a pixel at {tight_peak}"
    );
}

/// Half-float to single, for reading an `Rgba16Float` target back.
///
/// Written out rather than pulled in: one function against a dependency in a
/// public repository's supply chain is not a trade worth making.
fn f16_to_f32(bits: u16) -> f32 {
    let sign = u32::from(bits >> 15) << 31;
    let exp = u32::from((bits >> 10) & 0x1f);
    let frac = u32::from(bits & 0x3ff);
    let assembled = match exp {
        0 if frac == 0 => sign,
        // Subnormal: normalize by hand rather than pretending it is zero.
        0 => {
            let mut e = -1i32;
            let mut f = frac;
            while f & 0x400 == 0 {
                f <<= 1;
                e -= 1;
            }
            sign | (((127 - 15 + e) as u32) << 23) | ((f & 0x3ff) << 13)
        }
        0x1f => sign | 0x7f80_0000 | (frac << 13),
        _ => sign | ((exp + 127 - 15) << 23) | (frac << 13),
    };
    f32::from_bits(assembled)
}

/// The handoff itself: what the accumulator holds is what the post chain reads.
///
/// This is the whole architectural claim of the task stated as an assertion.
/// The composite pass already applies exposure, both grading slots, the tone map
/// and the grade to whatever sits in the shared high-dynamic-range target, so
/// the only thing that could go wrong on the traced side is the copy into it.
/// If this passes, a traced image inherits the entire look by construction and
/// there is no second look pipeline to keep in step.
#[test]
fn the_resolve_hands_the_accumulator_to_the_post_chain_unchanged() {
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };
    let environment = EnvParams::constant([0.1, 0.12, 0.16], [0.03, 0.03, 0.04]);
    let mut r = rig(&gpu, Some(panel()));
    let (accumulated, _) = run(&gpu, &mut r, PathEstimator::Mis, &environment, 4, 0, 0.0);

    let hdr = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Resolve Target"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // The same format the shared resolve target carries, which is what
        // makes the two interchangeable at the composite's input.
        format: Texture::HDR_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = hdr.create_view(&wgpu::TextureViewDescriptor::default());

    let resolve = TraceResolve::new(&gpu.device);
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Resolve Encoder"),
        });
    resolve.encode(&gpu.device, &mut encoder, r.target.color_view(), &view);
    let (buffer, padded) = solarxy_renderer::capture::encode_capture(
        &gpu.device,
        &mut encoder,
        &hdr,
        (0, 0, WIDTH, HEIGHT),
    );
    gpu.queue.submit(Some(encoder.finish()));

    let (tx, rx) = std::sync::mpsc::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |res| {
        let _ = tx.send(res);
    });
    loop {
        let _ = gpu.device.poll(wgpu::PollType::Poll);
        match rx.try_recv() {
            Ok(Ok(())) => break,
            Ok(Err(e)) => panic!("resolve readback failed: {e}"),
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(e) => panic!("resolve readback dropped: {e}"),
        }
    }
    let data = buffer.slice(..).get_mapped_range();

    let mut worst = 0.0f32;
    let mut seen_signal = false;
    for y in 0..HEIGHT {
        let row = (y * padded) as usize;
        for x in 0..WIDTH {
            let at = row + (x as usize) * 8;
            for c in 0..3 {
                let half = u16::from_le_bytes([data[at + c * 2], data[at + c * 2 + 1]]);
                let got = f16_to_f32(half);
                let want = accumulated[((y * WIDTH + x) * 4) as usize + c];
                if want > 0.05 {
                    seen_signal = true;
                }
                // Half float carries about three decimal digits, so the
                // tolerance is the format's and not the pass's.
                worst = worst.max((got - want).abs() / want.abs().max(1e-3));
            }
        }
    }
    drop(data);
    buffer.unmap();

    assert!(
        seen_signal,
        "the accumulator was black, so this compared nothing"
    );
    assert!(
        worst < 1e-2,
        "the resolve changed the image by {worst:.3e} relative; the composite \
         reads what this pass writes, so anything here is a look the raster \
         path does not get"
    );
}
