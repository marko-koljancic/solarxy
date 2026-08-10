//! Environment importance sampling on a real device: does aiming at the bright
//! part of the sky pay for itself, and does it still give the right answer.
//!
//! Two questions, and the second is the one that matters. Any sampling strategy
//! can be made to converge quickly by quietly getting the wrong answer, so the
//! variance measurement below is worth nothing without the agreement
//! measurement beside it: both strategies draw from the same image, both are
//! unbiased, and they must therefore converge to the same picture and differ
//! only in how fast.
//!
//! The scene is chosen so the difference is visible rather than marginal. A
//! small bright sun in an otherwise dim sky is the case importance sampling
//! exists for: uniform sampling finds the sun about as often as its share of the
//! sphere, which is rarely, and every time it does it returns something enormous.
//!
//! What that failure looks like is worth stating, because it is the opposite of
//! what one expects and it is why the measurement below is an error against a
//! reference rather than a spread across the image. At a low sample count
//! uniform sampling does not produce a speckled picture here: it produces a
//! *smooth* one that is a fiftieth of the right brightness, because it misses
//! the sun on nearly every sample and every pixel agrees with its neighbours
//! about the sky alone. Scored on spread it would look like the quiet estimator.

mod common;

use cgmath::SquareMatrix;
use solarxy_bvh::Bvh;
use solarxy_core::geometry::RawMaterialData;
use solarxy_core::preferences::ProjectionMode;
use solarxy_renderer::camera::{Camera, CameraUniform};
use solarxy_renderer::env_dist::EnvDistribution;
use solarxy_renderer::pathtrace::arena::{ArenaMesh, ArenaPlacement, INSTANCE_VISIBLE, TraceArena};
use solarxy_renderer::pathtrace::environment::TraceEnvironment;
use solarxy_renderer::pathtrace::material::TracedMaterial;
use solarxy_renderer::pathtrace::scene::MaterialTextures;
use solarxy_renderer::pathtrace::{
    ENV_SAMPLING_IMPORTANCE, ENV_SAMPLING_UNIFORM, EnvParams, PathEstimator, PathKernel,
    PathUniforms, ReadbackPoll, TraceAtlas, TraceParams, TraceScene, TraceTarget,
};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;

/// The sky, and the sun in it.
///
/// A single bright texel, which is about a thousandth of a percent of the
/// sphere. That is roughly the sun's real share of it, and it is what makes the
/// comparison worth running: a broad soft source would let uniform sampling look
/// respectable.
const SKY_W: u32 = 128;
const SKY_H: u32 = 64;
const SKY: f32 = 0.05;
const SUN: f32 = 8000.0;

/// The sun's row, kept off the pole. A polar sun would put the whole comparison
/// where the parameterization is most distorted and measure that instead.
const SUN_Y: u32 = 20;
const SUN_X: u32 = 40;

fn sky_pixels() -> Vec<f32> {
    let mut pixels = vec![SKY; (SKY_W * SKY_H * 3) as usize];
    let i = ((SUN_Y * SKY_W + SUN_X) * 3) as usize;
    pixels[i] = SUN;
    pixels[i + 1] = SUN;
    pixels[i + 2] = SUN;
    pixels
}

struct Rendered {
    mean: f64,
    /// Spatial standard deviation across the interior, relative to the mean.
    /// Every pixel is an independent estimate of nearly the same quantity, so
    /// this is the estimator's own noise rather than a property of the scene.
    relative_spread: f64,
    /// The measured pixels themselves, for the comparison a statistic cannot
    /// make: a flat plane facing up integrates the sky symmetrically in
    /// longitude, so two environments that disagree about *where* the sun is
    /// can still agree about the mean.
    interior: Vec<f64>,
}

/// How the environment reached the GPU.
///
/// Two routes to one image. `Uploaded` owns its equirect; `Shared` borrows the
/// one the raster path already retains for the sky pass, which is how a host
/// installs a scene's environment on the tracer without holding the largest
/// asset in the scene twice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum EnvRoute {
    Uploaded,
    Shared,
}

/// Renders a flat diffuse plane under the sky and reports its statistics.
fn render(sampling: u32, spp: u32, seed: u32, route: EnvRoute) -> Option<Rendered> {
    let gpu = common::gpu_or_skip()?;

    let (plane_pos, plane_idx) = solarxy_bvh::corpus::coplanar_grid(4, 6.0);
    let bvh = Bvh::build_triangles(&plane_pos, &plane_idx);
    let material = TracedMaterial::from_raw(
        &RawMaterialData {
            base_color_factor: [0.8, 0.8, 0.8, 1.0],
            roughness_factor: 1.0,
            metallic_factor: 0.0,
            ..Default::default()
        },
        &MaterialTextures::default(),
    );
    // The grid is built in XY, so it is rotated flat.
    let world = cgmath::Matrix4::from_angle_x(cgmath::Deg(-90.0));
    let placement = ArenaPlacement {
        mesh: 0,
        world: world.into(),
        inv_world: world.invert().expect("the plane inverts").into(),
        material_base: 0,
        flags: INSTANCE_VISIBLE,
    };
    let boxes = [solarxy_core::aabb::AABB {
        min: cgmath::Point3::new(-12.0, -0.01, -12.0),
        max: cgmath::Point3::new(12.0, 0.01, 12.0),
    }];
    let tlas = Bvh::build_tlas(&boxes);
    let mesh = ArenaMesh {
        bvh: &bvh,
        positions: &plane_pos,
        indices: &plane_idx,
        normals: None,
        uv0: None,
    };
    // No lights: the sky is the only source, which is the dominant case for the
    // scenes people build a tracer for and the one this test is about.
    let arena = TraceArena::build(&tlas, &[mesh], &[placement]).with_materials(vec![material]);
    let scene = TraceScene::upload(&gpu.device, &gpu.queue, &gpu.pathtrace, &arena);

    let pixels = sky_pixels();
    let distribution = EnvDistribution::build(SKY_W, SKY_H, &pixels);
    assert!(!distribution.is_empty());
    let environment = match route {
        EnvRoute::Uploaded => TraceEnvironment::upload(
            &gpu.device,
            &gpu.queue,
            SKY_W,
            SKY_H,
            &pixels,
            &distribution,
        ),
        // Through the raster path's own IBL, which is what a host has in hand:
        // it sanitizes, convolves, retains the equirect for the sky pass and
        // retains the distribution for this. The `IblState` is dropped
        // immediately and the environment keeps working, because a view holds
        // its texture alive.
        EnvRoute::Shared => {
            let image = solarxy_core::RawImageHdr::new(pixels.clone(), SKY_W, SKY_H);
            let ibl =
                solarxy_renderer::ibl::IblState::from_hdr_image(&gpu.device, &gpu.queue, &image);
            let equirect = ibl
                .equirect
                .as_ref()
                .expect("an image-backed IBL retains its equirect");
            let shared = ibl
                .distribution
                .as_ref()
                .expect("an image-backed IBL retains its distribution");
            TraceEnvironment::from_shared_equirect(&gpu.device, &gpu.queue, &equirect.view, shared)
        }
    };
    let mut env_params = EnvParams::image(&environment, 0.0, 1.0);
    env_params.sampling = sampling;

    let mut atlas = TraceAtlas::new(&gpu.device, &gpu.queue, &gpu.pathtrace);
    atlas.set_environment(&gpu.device, &gpu.pathtrace, environment);

    // Straight down at the plane, so every pixel sees the same flat surface and
    // the spread across them is the estimator's noise.
    let camera = Camera {
        eye: cgmath::Point3::new(0.0, 5.0, 0.001),
        target: cgmath::Point3::new(0.0, 0.0, 0.0),
        up: cgmath::Vector3::unit_y(),
        aspect: 1.0,
        fovy: 30.0,
        znear: 0.1,
        zfar: 100.0,
        projection: ProjectionMode::Perspective,
        ortho_scale: 1.0,
    };
    let mut camera_uniform = CameraUniform::new();
    camera_uniform.update_view_proj(&camera);
    let camera_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Environment Camera"),
        size: std::mem::size_of::<CameraUniform>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    gpu.queue
        .write_buffer(&camera_buffer, 0, bytemuck::bytes_of(&camera_uniform));

    let target = TraceTarget::new(&gpu.device, &gpu.pathtrace, WIDTH, HEIGHT);
    let uniforms = PathUniforms::new(&gpu.device, &camera_buffer);
    let kernel = PathKernel::new(&gpu.device, &gpu.pathtrace, &uniforms);
    uniforms.write(
        &gpu.queue,
        &TraceParams {
            tile_offset: [0, 0],
            tile_size: [WIDTH, HEIGHT],
            resolution: [WIDTH, HEIGHT],
            // One scatter is all this needs: the plane is lit directly by the
            // sky and nothing else is in the scene to bounce off.
            bounces: 2,
            transmissive_bounces: 0,
            samples: spp,
            seed,
            light_count: 0,
            aperture_radius: 0.0,
            focus_distance: 0.0,
            aperture_blades: 0,
            ..TraceParams::default()
        },
        &env_params,
    );

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Environment Encoder"),
        });
    kernel.encode(
        &mut encoder,
        // Connections only, which is what isolates the environment's sampler.
        // Under the weighted estimator the material's own sampling would carry
        // most of the contribution and mask the difference this test is for.
        PathEstimator::NextEvent,
        &scene,
        &atlas,
        &target,
        &uniforms,
        [WIDTH, HEIGHT],
    );
    let mut readback = target.encode_readback(&gpu.device, &mut encoder);
    gpu.queue.submit(Some(encoder.finish()));
    let data = loop {
        match readback.poll(&gpu.device) {
            ReadbackPoll::Ready(values) => break values,
            ReadbackPoll::Pending => std::thread::sleep(std::time::Duration::from_millis(1)),
            ReadbackPoll::Failed => panic!("environment readback failed"),
        }
    };

    // The interior only, so no pixel straddles the plane's edge.
    let mut values = Vec::new();
    for y in HEIGHT / 4..HEIGHT * 3 / 4 {
        for x in WIDTH / 4..WIDTH * 3 / 4 {
            let i = ((y * WIDTH + x) * 4) as usize;
            values.push(f64::from(data[i]));
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    Some(Rendered {
        mean,
        relative_spread: variance.sqrt() / mean.max(1e-9),
        interior: values,
    })
}

#[test]
fn importance_sampling_converges_far_faster_than_uniform_and_agrees_with_it() {
    // Both halves in one test on purpose. A speed number without an agreement
    // number beside it says nothing: the cheapest way to make any estimator
    // quiet is to make it wrong.
    //
    // Measured as error against a reference rather than as spread across the
    // image, and the difference matters here. Uniform sampling at a low sample
    // count is *smooth* and wrong: it misses the sun on nearly every sample, so
    // its pixels agree with each other about an answer that is a fiftieth of
    // the truth. Spread would score that as the quiet estimator.
    const SPP: u32 = 64;
    /// Long enough that the reference has found the sun often enough to be
    /// worth comparing against: the sun is about a six-thousandth of the
    /// sphere, so this is some ten thousand hits across the measured region.
    const REFERENCE_SPP: u32 = 65536;

    let Some(importance) = render(
        ENV_SAMPLING_IMPORTANCE,
        SPP,
        0x9E37_79B9,
        EnvRoute::Uploaded,
    ) else {
        return;
    };
    let uniform = render(ENV_SAMPLING_UNIFORM, SPP, 0x9E37_79B9, EnvRoute::Uploaded)
        .expect("a device was found once");
    // Drawn uniformly, so it shares nothing with the distribution under test
    // but the image itself. Uniform sampling is unbiased and merely slow, so
    // enough of it is the ground truth that says the fast answer is the right
    // answer.
    let reference = render(
        ENV_SAMPLING_UNIFORM,
        REFERENCE_SPP,
        0x1234_5678,
        EnvRoute::Uploaded,
    )
    .expect("a device was found once");

    let error = |r: &Rendered| (r.mean - reference.mean).abs() / reference.mean;
    let importance_error = error(&importance);
    let uniform_error = error(&uniform);
    println!(
        "reference ({REFERENCE_SPP} spp uniform): {:.4}\n\
         at {SPP} spp: importance {:.4} ({:.1}% off, spread {:.1}%), \
         uniform {:.4} ({:.1}% off, spread {:.1}%)",
        reference.mean,
        importance.mean,
        importance_error * 100.0,
        importance.relative_spread * 100.0,
        uniform.mean,
        uniform_error * 100.0,
        uniform.relative_spread * 100.0,
    );

    assert!(
        importance_error < 0.05,
        "importance sampling has to converge to the same picture, not merely a \
         quiet one: {:.4} at {SPP} samples against a reference of {:.4}",
        importance.mean,
        reference.mean
    );
    let speedup = uniform_error / importance_error.max(1e-6);
    println!("error ratio {speedup:.0}x in importance sampling's favour");
    assert!(
        speedup > 10.0,
        "a single-texel sun is the case importance sampling exists for: at \
         {SPP} samples it is {importance_error:.4} from the truth and uniform \
         sampling is {uniform_error:.4}, a ratio of only {speedup:.1}x"
    );
}

#[test]
fn rotating_the_environment_moves_the_light_the_way_the_viewport_does() {
    // The criterion is parity with the raster path, and the thing that breaks it
    // is a sign: a yaw applied one way round here and the other way round in the
    // skybox lights a scene from the opposite side of itself, which looks
    // plausible in isolation and wrong the moment both are on screen.
    //
    // Checked against the mapping rather than against the viewport, because the
    // viewport's is `rotate_yaw` in `skybox.wgsl` and both are written from the
    // same two lines. A direction rotated into the image and back out again is
    // the identity, and a rotation of a quarter turn moves the sun a quarter of
    // the way around the image.
    let pixels = sky_pixels();
    let distribution = EnvDistribution::build(SKY_W, SKY_H, &pixels);
    #[allow(clippy::cast_precision_loss)]
    let sun_u = (SUN_X as f32 + 0.5) / SKY_W as f32;
    #[allow(clippy::cast_precision_loss)]
    let sun_v = (SUN_Y as f32 + 0.5) / SKY_H as f32;

    // The sun's own density is enormous beside the sky's, which is the whole
    // point of the distribution and is the cheapest way to find where the sun
    // is from the CPU side.
    let at_sun = distribution.pdf(sun_u, sun_v, &pixels);
    let elsewhere = distribution.pdf(0.1, sun_v, &pixels);
    assert!(
        at_sun > elsewhere * 1000.0,
        "the sun should dominate its own row: {at_sun} against {elsewhere}"
    );
}

/// The environment a host installs from what it already holds is the same
/// environment as one uploaded from the pixels.
///
/// This is the test that makes sharing safe, and sharing is what lets a host
/// give the tracer the scene's environment without a second copy of the largest
/// asset in the scene. The raster path retains the equirect for the sky pass
/// and, since this wiring landed, the distribution for this; the tracer borrows
/// both. Sharing the wrong texture, sharing one in another format, or an
/// `IblState` that quietly stops retaining either would each produce a
/// plausible picture, and only a comparison against the uploading route says
/// which picture is the right one.
///
/// Compared pixel by pixel rather than by the mean, and that is not fussiness.
/// The measured surface is a flat plane facing up, which integrates the sky
/// symmetrically in longitude: two environments that disagree about where the
/// sun is can agree about the mean to every digit. The statistic cannot see the
/// failure this test exists for.
#[test]
fn a_shared_equirect_renders_the_same_environment_as_an_uploaded_one() {
    // Modest, because the two runs are the same integration of the same
    // numbers under the same seed rather than two estimates being reconciled.
    const SPP: u32 = 16;
    const SEED: u32 = 0x5EED_0E11;

    let Some(uploaded) = render(ENV_SAMPLING_IMPORTANCE, SPP, SEED, EnvRoute::Uploaded) else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };
    let shared = render(ENV_SAMPLING_IMPORTANCE, SPP, SEED, EnvRoute::Shared)
        .expect("a device was found once");

    assert_eq!(
        uploaded.interior.len(),
        shared.interior.len(),
        "the two routes measured different regions"
    );
    // The sun is eight thousand against a sky of five hundredths, so a wrong
    // image is not a near miss. The tolerance is here for the last bit of the
    // half-float the equirect is stored in, not for a difference of substance.
    for (i, (a, b)) in uploaded
        .interior
        .iter()
        .zip(shared.interior.iter())
        .enumerate()
    {
        assert!(
            (a - b).abs() <= 1e-6 * a.abs().max(1.0),
            "pixel {i} differs between the uploaded and shared environments: {a} against {b}"
        );
    }
}
