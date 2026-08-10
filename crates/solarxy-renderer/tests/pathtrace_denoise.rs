//! The denoiser on a real device: does it actually remove noise, does it keep
//! the edges its guides mark, and does it leave a converged image alone.
//!
//! All three matter and the middle one is the reason the filter has guides at
//! all. A blur removes noise; the question is what else it removes. So the
//! scene is two spheres of sharply different colour side by side, which puts a
//! material boundary through the middle of the frame where the albedo guide has
//! to hold it, and every assertion below is against a converged render of the
//! same scene rather than against an eye.

mod common;

use cgmath::SquareMatrix;
use solarxy_bvh::Bvh;
use solarxy_core::AABB;
use solarxy_core::geometry::RawMaterialData;
use solarxy_core::preferences::ProjectionMode;
use solarxy_renderer::camera::{Camera, CameraUniform};
use solarxy_renderer::pathtrace::arena::{ArenaMesh, ArenaPlacement, INSTANCE_VISIBLE, TraceArena};
use solarxy_renderer::pathtrace::denoise::Denoiser;
use solarxy_renderer::pathtrace::material::TracedMaterial;
use solarxy_renderer::pathtrace::scene::MaterialTextures;
use solarxy_renderer::pathtrace::{
    EnvParams, PathEstimator, PathKernel, PathUniforms, ReadbackPoll, TraceAtlas, TraceParams,
    TraceScene, TraceTarget,
};

const WIDTH: u32 = 128;
const HEIGHT: u32 = 96;
const SEED: u32 = 0x0DE1_5E00;

/// The two base colours either side of the boundary.
///
/// Far apart, so the step is unmistakable and an assertion about it is about
/// the filter rather than about the tolerance. Both rough and non-metallic, so
/// the auxiliary channels are written at the first hit rather than deferred
/// past a mirror.
const LEFT_COLOR: [f32; 4] = [0.85, 0.15, 0.10, 1.0];
const RIGHT_COLOR: [f32; 4] = [0.08, 0.20, 0.80, 1.0];

/// Bright above, dark below, so a rough scatter samples widely different
/// radiances and a low-sample frame is genuinely noisy. A uniform environment
/// would produce a clean image at one sample and nothing to denoise.
const SKY_UP: [f32; 3] = [1.4, 1.45, 1.5];
const SKY_DOWN: [f32; 3] = [0.02, 0.02, 0.03];

struct Rig {
    scene: TraceScene,
    atlas: TraceAtlas,
    kernel: PathKernel,
    uniforms: PathUniforms,
    target: TraceTarget,
}

/// Two spheres, one per material, side by side across the frame's middle.
fn rig(gpu: &common::Gpu) -> Rig {
    let (positions, indices) = solarxy_bvh::corpus::sphere(48, 24);
    let bvh = Bvh::build_triangles(&positions, &indices);
    let mesh = ArenaMesh {
        bvh: &bvh,
        positions: &positions,
        indices: &indices,
        normals: None,
        uv0: None,
    };

    let material = |color: [f32; 4]| {
        let raw = RawMaterialData {
            base_color_factor: color,
            roughness_factor: 0.7,
            metallic_factor: 0.0,
            ..RawMaterialData::default()
        };
        TracedMaterial::from_raw(&raw, &MaterialTextures::default())
    };

    let offset = 1.05_f32;
    let place = |x: f32, material_base: u32| {
        let world = cgmath::Matrix4::from_translation(cgmath::Vector3::new(x, 0.0, 0.0));
        ArenaPlacement {
            mesh: 0,
            world: world.into(),
            inv_world: world
                .invert()
                .unwrap_or_else(cgmath::Matrix4::identity)
                .into(),
            material_base,
            flags: INSTANCE_VISIBLE,
        }
    };
    let bounds = |x: f32| AABB {
        min: [x - 1.0, -1.0, -1.0].into(),
        max: [x + 1.0, 1.0, 1.0].into(),
    };
    let tlas = Bvh::build_tlas(&[bounds(-offset), bounds(offset)]);
    let arena = TraceArena::build(&tlas, &[mesh], &[place(-offset, 0), place(offset, 1)])
        .with_materials(vec![material(LEFT_COLOR), material(RIGHT_COLOR)]);

    let scene = TraceScene::upload(&gpu.device, &gpu.queue, &gpu.pathtrace, &arena);
    let atlas = TraceAtlas::new(&gpu.device, &gpu.queue, &gpu.pathtrace);

    let camera = Camera {
        eye: cgmath::Point3::new(0.0, 0.0, 5.0),
        target: cgmath::Point3::new(0.0, 0.0, 0.0),
        up: cgmath::Vector3::unit_y(),
        #[allow(clippy::cast_precision_loss)]
        aspect: WIDTH as f32 / HEIGHT as f32,
        fovy: 45.0,
        znear: 0.1,
        zfar: 100.0,
        projection: ProjectionMode::Perspective,
        ortho_scale: 1.0,
    };
    let mut camera_uniform = CameraUniform::new();
    camera_uniform.update_view_proj(&camera);
    let camera_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Denoise Camera"),
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

fn environment() -> EnvParams {
    EnvParams::constant(SKY_UP, SKY_DOWN)
}

/// Renders `samples` in one dispatch and reads the mean back.
fn render(gpu: &common::Gpu, rig: &Rig, samples: u32) -> Vec<f32> {
    rig.uniforms.write(
        &gpu.queue,
        &TraceParams {
            tile_offset: [0, 0],
            tile_size: [WIDTH, HEIGHT],
            resolution: [WIDTH, HEIGHT],
            bounces: 3,
            transmissive_bounces: 0,
            samples,
            seed: SEED,
            light_count: 0,
            aperture_radius: 0.0,
            focus_distance: 0.0,
            aperture_blades: 0,
            ..TraceParams::default()
        },
        &environment(),
    );
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Denoise Render Encoder"),
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
    let mut readback = rig.target.encode_readback(&gpu.device, &mut encoder);
    gpu.queue.submit(Some(encoder.finish()));
    drain(&mut readback, &gpu.device)
}

/// Renders, filters, and reads the filtered result back.
fn render_denoised(
    gpu: &common::Gpu,
    rig: &Rig,
    denoiser: &mut Denoiser,
    samples: u32,
) -> Vec<f32> {
    let raw = render(gpu, rig, samples);
    debug_assert!(!raw.is_empty());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Denoise Filter Encoder"),
        });
    denoiser.encode(&gpu.device, &gpu.queue, &mut encoder, &rig.target, samples);
    gpu.queue.submit(Some(encoder.finish()));

    let output = denoiser
        .output_texture()
        .expect("the filter allocated its scratch")
        .clone();
    let mut copy = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Denoise Readback Encoder"),
        });
    let (buffer, padded) = solarxy_renderer::capture::encode_capture(
        &gpu.device,
        &mut copy,
        &output,
        (0, 0, WIDTH, HEIGHT),
    );
    gpu.queue.submit(Some(copy.finish()));

    let (tx, rx) = std::sync::mpsc::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |res| {
        let _ = tx.send(res);
    });
    loop {
        let _ = gpu.device.poll(wgpu::PollType::Poll);
        match rx.try_recv() {
            Ok(Ok(())) => break,
            Ok(Err(e)) => panic!("denoise readback failed: {e}"),
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(e) => panic!("denoise readback dropped: {e}"),
        }
    }
    let data = buffer.slice(..).get_mapped_range();
    let mut out = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    for y in 0..HEIGHT {
        let row = (y * padded) as usize;
        out.extend_from_slice(bytemuck::cast_slice::<u8, f32>(
            &data[row..row + (WIDTH * 4 * 4) as usize],
        ));
    }
    drop(data);
    buffer.unmap();
    out
}

fn drain(
    readback: &mut solarxy_renderer::pathtrace::FloatReadback,
    device: &wgpu::Device,
) -> Vec<f32> {
    loop {
        match readback.poll(device) {
            ReadbackPoll::Ready(values) => return values,
            ReadbackPoll::Pending => std::thread::sleep(std::time::Duration::from_millis(1)),
            ReadbackPoll::Failed => panic!("denoise render readback failed"),
        }
    }
}

/// Mean absolute error against a reference, over the pixels that found a
/// surface.
///
/// Restricted to those on purpose. A camera ray that misses everything returns
/// the environment exactly, with no variance at all, so the sky is noise-free
/// at one sample and averaging it into the score dilutes the measurement with
/// pixels neither image could have got wrong. The reference's alpha lane counts
/// the samples that described a surface, which is exactly the mask wanted and
/// costs no second readback.
fn error_against(image: &[f32], reference: &[f32]) -> f64 {
    let mut total = 0.0f64;
    let mut count = 0u64;
    for (a, b) in image.chunks_exact(4).zip(reference.chunks_exact(4)) {
        if b[3] <= 0.0 {
            continue;
        }
        for c in 0..3 {
            total += f64::from(a[c] - b[c]).abs();
            count += 1;
        }
    }
    total / count.max(1) as f64
}

/// The mean colour of a column strip, which is how the material step is read.
fn strip_mean(image: &[f32], x0: u32, x1: u32) -> [f64; 3] {
    let mut sums = [0.0f64; 3];
    let mut count = 0u64;
    // The vertical middle only, where both spheres are at their widest and no
    // row straddles a silhouette top or bottom.
    for y in HEIGHT / 3..HEIGHT * 2 / 3 {
        for x in x0..x1 {
            let i = ((y * WIDTH + x) * 4) as usize;
            for c in 0..3 {
                sums[c] += f64::from(image[i + c]);
            }
            count += 1;
        }
    }
    [
        sums[0] / count as f64,
        sums[1] / count as f64,
        sums[2] / count as f64,
    ]
}

fn separation(image: &[f32]) -> f64 {
    // The two strips sit just inside each sphere, either side of the gap
    // between them, which is where the filter is most tempted to reach across.
    let left = strip_mean(image, WIDTH * 5 / 16, WIDTH * 7 / 16);
    let right = strip_mean(image, WIDTH * 9 / 16, WIDTH * 11 / 16);
    (0..3)
        .map(|c| (left[c] - right[c]).abs())
        .fold(0.0f64, f64::max)
}

#[test]
fn the_filter_removes_noise_without_moving_the_image() {
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };
    let rig = rig(&gpu);
    let mut denoiser = Denoiser::new(&gpu.device);

    // The answer, drawn heavily enough that it is the picture rather than an
    // estimate of it.
    let reference = render(&gpu, &rig, 512);
    let noisy = render(&gpu, &rig, 1);
    let filtered = render_denoised(&gpu, &rig, &mut denoiser, 1);

    let noisy_error = error_against(&noisy, &reference);
    let filtered_error = error_against(&filtered, &reference);

    eprintln!("one-sample error {noisy_error:.4}, filtered {filtered_error:.4}");
    assert!(
        noisy_error > 0.05,
        "the one-sample frame was already clean ({noisy_error:.4}); this scene \
         is no longer measuring a denoiser"
    );
    // Removing noise means getting closer to the answer, not merely getting
    // smoother. A blur that moved the image away from the reference would be
    // smoother and worse, and only a comparison against the reference can tell
    // those two apart.
    assert!(
        filtered_error < noisy_error * 0.45,
        "filtering a one-sample frame left an error of {filtered_error:.4} \
         against the unfiltered {noisy_error:.4}"
    );
}

#[test]
fn the_material_boundary_survives_the_filter() {
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };
    let rig = rig(&gpu);
    let mut denoiser = Denoiser::new(&gpu.device);

    let reference = render(&gpu, &rig, 512);
    let filtered = render_denoised(&gpu, &rig, &mut denoiser, 1);

    let wanted = separation(&reference);
    let kept = separation(&filtered);
    eprintln!("separation: reference {wanted:.4}, filtered {kept:.4}");

    assert!(
        wanted > 0.05,
        "the two materials are not far enough apart ({wanted:.4}) for this to \
         be a test of edge preservation"
    );
    // A filter that ignored its guides would average the two spheres toward
    // each other across a 33-pixel support, which at this framing is most of
    // the gap between them.
    assert!(
        kept > wanted * 0.8,
        "the material boundary collapsed from {wanted:.4} to {kept:.4}; the \
         albedo guide is not being honoured"
    );
}

#[test]
fn a_converged_image_is_left_almost_exactly_alone() {
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };
    let rig = rig(&gpu);
    let mut denoiser = Denoiser::new(&gpu.device);

    // The colour tolerance falls with the square root of the sample count, so
    // by the time an image is converged the filter has almost nothing it is
    // willing to average. That is the property that makes leaving it on for a
    // long render defensible rather than destructive.
    let converged = render(&gpu, &rig, 512);
    let filtered = render_denoised(&gpu, &rig, &mut denoiser, 512);
    let moved = error_against(&filtered, &converged);
    eprintln!("converged image moved by {moved:.5}");
    assert!(
        moved < 0.01,
        "filtering a converged image moved it by {moved:.5}"
    );
}

/// What the filter costs, and the before-and-after pictures.
///
/// A measurement rather than a regression gate, so it is `#[ignore]`d like the
/// throughput probes and run with `--release --ignored`.
///
/// The resolution is the one the interactive preview would use: half scale in
/// each axis of a 1920x1080 pane, which is a quarter of the pixels. The budget
/// it is measured against is the hundred milliseconds gate G6 set for one
/// sample plus a denoise, of which the trace itself measured 1.3 ms natively
/// and 3.3 ms in the browser.
#[test]
#[ignore = "measurement, not a regression gate; run with --release --ignored"]
fn the_filter_fits_the_interactive_budget() {
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };
    let rig = rig(&gpu);
    let mut denoiser = Denoiser::new(&gpu.device);
    // Half scale on a 1920x1080 pane.
    let preview = TraceTarget::new(&gpu.device, &gpu.pathtrace, 960, 540);

    // Warm: the first call allocates the scratch and compiles nothing, but the
    // driver has its own first-dispatch costs and they are not what is being
    // measured.
    for _ in 0..3 {
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        denoiser.encode(&gpu.device, &gpu.queue, &mut encoder, &preview, 1);
        gpu.queue.submit(Some(encoder.finish()));
    }
    let _ = gpu.device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });

    const RUNS: u32 = 20;
    let start = std::time::Instant::now();
    for _ in 0..RUNS {
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        denoiser.encode(&gpu.device, &gpu.queue, &mut encoder, &preview, 1);
        gpu.queue.submit(Some(encoder.finish()));
    }
    let _ = gpu.device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    let each = start.elapsed().as_secs_f64() * 1000.0 / f64::from(RUNS);
    eprintln!(
        "DENOISE 960x540, {} levels: {each:.2} ms per frame",
        solarxy_renderer::pathtrace::denoise::DENOISE_LEVELS
    );

    // The pictures, for the issue. Written beside the target directory rather
    // than into the repository.
    let out = std::env::var("SOLARXY_DENOISE_OUT").unwrap_or_else(|_| "/tmp".to_string());
    let noisy = render(&gpu, &rig, 1);
    let filtered = render_denoised(&gpu, &rig, &mut denoiser, 1);
    let converged = render(&gpu, &rig, 512);
    write_png(&noisy, &format!("{out}/denoise_1spp_raw.png"));
    write_png(&filtered, &format!("{out}/denoise_1spp_filtered.png"));
    write_png(&converged, &format!("{out}/denoise_512spp_reference.png"));
    eprintln!("DENOISE wrote three captures to {out}");
}

/// Float texels to an 8-bit PNG, for looking at.
///
/// No exposure and no tone map: this is a diagnostic of what the filter did,
/// not a picture of what the composite would make of it.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn write_png(floats: &[f32], path: &str) {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u8;
    let bytes: Vec<u8> = floats
        .chunks_exact(4)
        .flat_map(|p| [byte(p[0]), byte(p[1]), byte(p[2]), 255])
        .collect();
    image::RgbaImage::from_raw(WIDTH, HEIGHT, bytes)
        .expect("buffer matches the frame size")
        .save(path)
        .expect("write the denoise png");
}
