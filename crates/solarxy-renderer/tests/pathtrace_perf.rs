//! Traversal throughput, and a picture of what was traversed.
//!
//! A measurement rather than a regression gate, so it is `#[ignore]`d and run
//! deliberately:
//!
//! ```text
//! cargo test --release -p solarxy-renderer --test pathtrace_perf -- --ignored --nocapture
//! SOLARXY_PT_DEBUG_PNG=/tmp/pt.png cargo test --release -p solarxy-renderer \
//!     --test pathtrace_perf -- --ignored --nocapture
//! ```
//!
//! Two reasons it is not in CI. Runner GPUs are software rasterizers, so the
//! number would describe the runner. And the number that matters is the one
//! from the reference machine, recorded beside the others in the milestone's
//! amendments, where a change in it can be read against a change in the code.
//!
//! The picture is the other half. The parity corpus is the rigorous check, but
//! it drives a few thousand rays at one mesh; a traversal that is wrong in a
//! way that still runs fast produces a visibly wrong image long before it
//! produces a suspicious timing, and looking at one costs nothing.

mod common;

use std::time::Instant;

use cgmath::{Matrix4, SquareMatrix};
use solarxy_bvh::{Bvh, corpus};
use solarxy_core::aabb::AABB;
use solarxy_renderer::camera::{CameraUniform, camera_from_bounds};
use solarxy_renderer::pathtrace::arena::{ArenaMesh, ArenaPlacement, INSTANCE_VISIBLE, TraceArena};
use solarxy_renderer::pathtrace::{
    DebugChannel, PathTracer, ReadbackPoll, TraceAtlas, TraceParams, TraceScene, TraceTarget,
    TraceUniforms,
};

/// The framing the milestone's other figures use, so the numbers compare.
const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
/// One untimed run first: shader specialization and first-touch buffer
/// residency land on whichever run goes first, and would otherwise be reported
/// as thermal drift with the sign inverted.
const RUNS: usize = 3;

fn transformed(positions: &[[f32; 3]], world: &[[f32; 4]; 4]) -> Vec<[f32; 3]> {
    positions
        .iter()
        .map(|p| {
            [
                world[0][0] * p[0] + world[1][0] * p[1] + world[2][0] * p[2] + world[3][0],
                world[0][1] * p[0] + world[1][1] * p[1] + world[2][1] * p[2] + world[3][1],
                world[0][2] * p[0] + world[1][2] * p[1] + world[2][2] * p[2] + world[3][2],
            ]
        })
        .collect()
}

fn bounds_of(positions: &[[f32; 3]]) -> AABB {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in positions {
        for axis in 0..3 {
            min[axis] = min[axis].min(p[axis]);
            max[axis] = max[axis].max(p[axis]);
        }
    }
    AABB {
        min: cgmath::Point3::new(min[0], min[1], min[2]),
        max: cgmath::Point3::new(max[0], max[1], max[2]),
    }
}

#[test]
#[ignore = "measurement, not a regression gate; run with --release --ignored"]
fn primary_ray_throughput() {
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };

    // A sphere on a ground plane. The plane is load-bearing even for primary
    // rays: without it most of the frame is sky, and the figure describes how
    // fast the hierarchy can be missed rather than how fast it can be walked.
    let generate = Instant::now();
    let (sphere_pos, sphere_idx) = corpus::sphere(1000, 500);
    let (plane_pos, plane_idx) = corpus::coplanar_grid(8, 2.0);
    let tri_count = (sphere_idx.len() + plane_idx.len()) / 3;
    println!(
        "scene: {tri_count} triangles, generated in {:.1} ms",
        generate.elapsed().as_secs_f64() * 1000.0
    );

    let build = Instant::now();
    let sphere_bvh = Bvh::build_triangles(&sphere_pos, &sphere_idx);
    let plane_bvh = Bvh::build_triangles(&plane_pos, &plane_idx);
    println!(
        "build: {:.1} ms, sphere depth {}, {} nodes",
        build.elapsed().as_secs_f64() * 1000.0,
        sphere_bvh.stats().max_depth,
        sphere_bvh.stats().node_count
    );

    // The plane lies flat under the sphere, rotated onto XZ and dropped.
    let plane_world: [[f32; 4]; 4] =
        (Matrix4::from_translation(cgmath::Vector3::new(0.0, -1.0, 0.0))
            * Matrix4::from_angle_x(cgmath::Deg(-90.0)))
        .into();
    let plane_inv: [[f32; 4]; 4] = Matrix4::from(plane_world)
        .invert()
        .expect("the plane placement is invertible")
        .into();
    let identity: [[f32; 4]; 4] = Matrix4::identity().into();

    let meshes = [
        ArenaMesh {
            bvh: &sphere_bvh,
            positions: &sphere_pos,
            indices: &sphere_idx,
            normals: None,
            uv0: None,
        },
        ArenaMesh {
            bvh: &plane_bvh,
            positions: &plane_pos,
            indices: &plane_idx,
            normals: None,
            uv0: None,
        },
    ];
    let placements = [
        ArenaPlacement {
            mesh: 0,
            world: identity,
            inv_world: identity,
            material_base: 0,
            flags: INSTANCE_VISIBLE,
        },
        ArenaPlacement {
            mesh: 1,
            world: plane_world,
            inv_world: plane_inv,
            material_base: 0,
            flags: INSTANCE_VISIBLE,
        },
    ];
    // The camera frames the sphere alone while the floor extends well past it,
    // so most of the frame is geometry. Coverage is reported below for the same
    // reason: a figure taken over a frame that is mostly sky describes how fast
    // the hierarchy can be missed.
    let subject_bounds = bounds_of(&sphere_pos);
    let plane_bounds = bounds_of(&transformed(&plane_pos, &plane_world));
    let tlas = Bvh::build_tlas(&[subject_bounds, plane_bounds]);
    let arena = TraceArena::build(&tlas, &meshes, &placements);
    let scene = TraceScene::upload(&gpu.device, &gpu.queue, &gpu.pathtrace, &arena);

    let camera = camera_from_bounds(&subject_bounds, WIDTH as f32 / HEIGHT as f32);
    let mut camera_uniform = CameraUniform::new();
    camera_uniform.update_view_proj(&camera);
    let camera_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Pathtrace Perf Camera"),
        size: std::mem::size_of::<CameraUniform>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    gpu.queue
        .write_buffer(&camera_buffer, 0, bytemuck::bytes_of(&camera_uniform));

    let tracer = PathTracer::new(&gpu.device, &gpu.pathtrace);
    // No textures in a throughput scene; the null atlas satisfies the sampled
    // group, which a pipeline layout requires whether the kernel samples or not.
    let atlas = TraceAtlas::new(&gpu.device, &gpu.pathtrace);
    let target = TraceTarget::new(&gpu.device, &gpu.pathtrace, WIDTH, HEIGHT);
    let uniforms = TraceUniforms::new(&gpu.device, &gpu.pathtrace, &camera_buffer);
    uniforms.write(
        &gpu.queue,
        &TraceParams {
            tile_offset: [0, 0],
            tile_size: [WIDTH, HEIGHT],
            resolution: [WIDTH, HEIGHT],
        },
    );

    let dispatch = |channel| {
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Pathtrace Perf Encoder"),
            });
        tracer.encode(
            &mut encoder,
            channel,
            &scene,
            &atlas,
            &target,
            &uniforms,
            [WIDTH, HEIGHT],
        );
        gpu.queue.submit(Some(encoder.finish()));
        // A test may block where the renderer may not. The shipped path polls
        // once a frame and gets on with something else, because WebGPU has no
        // blocking wait; here the wall clock around a submit is the measurement.
        let _ = gpu.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });
    };

    dispatch(DebugChannel::Normal);

    // Coverage before throughput, so the throughput figure can be read. Alpha
    // carries whether the ray hit anything.
    let covered = read_target(&gpu, &target)
        .chunks_exact(4)
        .filter(|p| p[3] > 0.0)
        .count();
    let pixels = f64::from(WIDTH) * f64::from(HEIGHT);
    println!(
        "coverage: {:.1}% of pixels hit geometry",
        covered as f64 / pixels * 100.0
    );

    for run in 1..=RUNS {
        let started = Instant::now();
        dispatch(DebugChannel::Normal);
        let secs = started.elapsed().as_secs_f64();
        println!(
            "run {run}: {:.1} ms for {WIDTH}x{HEIGHT}, {:.1} Mrays/s primary",
            secs * 1000.0,
            pixels / secs / 1.0e6
        );
    }

    if let Ok(path) = std::env::var("SOLARXY_PT_DEBUG_PNG") {
        dispatch(DebugChannel::Normal);
        write_png(&gpu, &target, &path);
        println!("wrote {path}");
    }
}

/// Pulls the float target back to the CPU.
fn read_target(gpu: &common::Gpu, target: &TraceTarget) -> Vec<f32> {
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Pathtrace Perf Readback"),
        });
    let mut readback = target.encode_readback(&gpu.device, &mut encoder);
    gpu.queue.submit(Some(encoder.finish()));

    loop {
        match readback.poll(&gpu.device) {
            ReadbackPoll::Ready(v) => return v,
            ReadbackPoll::Pending => std::thread::yield_now(),
            ReadbackPoll::Failed => panic!("debug readback failed"),
        }
    }
}

/// Resolves the float target to an 8-bit PNG, for looking at.
fn write_png(gpu: &common::Gpu, target: &TraceTarget, path: &str) {
    let floats = read_target(gpu, target);

    // The channel is already in `0..1` and this is a diagnostic, not the
    // composite chain: no exposure, no tone map, no transfer function.
    let bytes: Vec<u8> = floats
        .chunks_exact(4)
        .flat_map(|p| {
            [
                (p[0].clamp(0.0, 1.0) * 255.0) as u8,
                (p[1].clamp(0.0, 1.0) * 255.0) as u8,
                (p[2].clamp(0.0, 1.0) * 255.0) as u8,
                255,
            ]
        })
        .collect();
    image::RgbaImage::from_raw(target.width(), target.height(), bytes)
        .expect("buffer matches the target size")
        .save(path)
        .expect("write the debug png");
}
