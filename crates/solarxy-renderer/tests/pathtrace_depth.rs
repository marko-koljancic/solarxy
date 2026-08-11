//! The depth pass against the intersector the picking path has always used.
//!
//! The same joint the traversal is pinned at, asked a different question. The
//! traversal parity test compares hit records; this compares the number the
//! pass reports, which depends on three things beyond the traversal being
//! right: the ray has to be built through the middle of the pixel, the
//! distance has to be projected onto the camera's axis rather than left along
//! the ray, and a miss has to be finite.
//!
//! The projection is the part worth a test rather than a comment. Reporting the
//! ray's own length looks correct in the middle of frame and is wrong
//! everywhere else, by the cosine between the ray and the axis, so a defocus
//! applied downstream would curve the focal surface into a sphere about the
//! eye. A test that only looked at the centre pixel would pass.

mod common;

use cgmath::{InnerSpace, Point3, Vector3};
use solarxy_bvh::Bvh;
use solarxy_core::aabb::AABB;
use solarxy_core::preferences::ProjectionMode;
use solarxy_core::raycast::{Ray, intersect_triangle, screen_to_world_ray};
use solarxy_renderer::camera::Camera;
use solarxy_renderer::camera_state::CameraState;
use solarxy_renderer::capture::{CaptureFloatPoll, PendingCapture};
use solarxy_renderer::pathtrace::arena::{ArenaMesh, ArenaPlacement, INSTANCE_VISIBLE, TraceArena};
use solarxy_renderer::pathtrace::depth::{DEPTH_MISS, DepthPass, DepthTarget};
use solarxy_renderer::pathtrace::{TraceScene, TraceUniforms};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 48;

/// Two quads square to the camera at two distances, and one tilted.
///
/// Square-on quads make the answer checkable by hand: every pixel that sees one
/// reports the same axis distance wherever it sits in frame, which is the whole
/// claim about the projection. The tilted one makes sure the test is not
/// passing on a constant.
fn geometry() -> (Vec<[f32; 3]>, Vec<u32>) {
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    let mut quad = |corners: [[f32; 3]; 4]| {
        let base = u32::try_from(positions.len()).expect("small mesh");
        positions.extend_from_slice(&corners);
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    };
    // Wide enough to run off every edge of frame at this field of view, so a
    // pixel near a corner is on the plane rather than past it. That corner is
    // where the cosine between the ray and the axis is furthest from one, which
    // is the whole thing under test.
    const FAR_EDGE: f32 = 40.0;
    // Square on at z = 0, filling the left of frame.
    quad([
        [-FAR_EDGE, -FAR_EDGE, 0.0],
        [-0.2, -FAR_EDGE, 0.0],
        [-0.2, FAR_EDGE, 0.0],
        [-FAR_EDGE, FAR_EDGE, 0.0],
    ]);
    // Square on at z = -2, filling the right.
    quad([
        [0.2, -FAR_EDGE, -2.0],
        [FAR_EDGE, -FAR_EDGE, -2.0],
        [FAR_EDGE, FAR_EDGE, -2.0],
        [0.2, FAR_EDGE, -2.0],
    ]);
    // And a tilted strip down the middle, so a distance that varies is in the
    // picture too and the comparison is not passing on two constants.
    quad([
        [-0.15, -FAR_EDGE, 1.0],
        [0.15, -FAR_EDGE, 1.0],
        [0.15, FAR_EDGE, -3.0],
        [-0.15, FAR_EDGE, -3.0],
    ]);
    (positions, indices)
}

fn camera(projection: ProjectionMode) -> Camera {
    Camera {
        eye: Point3::new(0.0, 0.0, 8.0),
        target: Point3::new(0.0, 0.0, 0.0),
        up: Vector3::new(0.0, 1.0, 0.0),
        #[allow(clippy::cast_precision_loss)]
        aspect: WIDTH as f32 / HEIGHT as f32,
        // Wide, so the corners of frame sit well off the axis and the cosine
        // this test is about is far from one.
        fovy: 70.0,
        znear: 0.01,
        zfar: 100.0,
        projection,
        ortho_scale: 4.0,
    }
}

/// What the pass should have written at every pixel, computed on the processor.
fn expected(cam: &Camera, positions: &[[f32; 3]], indices: &[u32]) -> Vec<f32> {
    // The same matrix the uniform carries, clip convention included. The raw
    // cgmath projection is the OpenGL one, whose near plane is at a normalized
    // depth of minus one rather than nought, so unprojecting nought against it
    // starts the ray somewhere in the middle of the frustum. That is a
    // hundredth of a unit here and it is exactly the kind of quiet offset a
    // depth pass exists to not have.
    let view_proj = cam.build_view_projection_matrix();
    let forward = (cam.target - cam.eye).normalize();
    let point = |i: u32| {
        let p = positions[i as usize];
        Point3::new(p[0], p[1], p[2])
    };
    let mut out = vec![DEPTH_MISS; (WIDTH * HEIGHT) as usize];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            #[allow(clippy::cast_precision_loss)]
            let ray: Ray = screen_to_world_ray(
                (x as f32 + 0.5, y as f32 + 0.5),
                (WIDTH as f32, HEIGHT as f32),
                view_proj,
            );
            let mut nearest = f32::INFINITY;
            for tri in indices.chunks_exact(3) {
                if let Some((t, _)) =
                    intersect_triangle(&ray, point(tri[0]), point(tri[1]), point(tri[2]))
                    && t < nearest
                {
                    nearest = t;
                }
            }
            if nearest.is_finite() {
                // The axial component of the vector from the camera to the
                // surface, which is what the kernel writes and what a
                // compositor reads. Measured from the eye rather than from the
                // ray's origin, because the ray starts on the near plane.
                let surface = ray.origin + ray.direction * nearest;
                out[(y * WIDTH + x) as usize] = (surface - cam.eye).dot(forward);
            }
        }
    }
    out
}

/// Runs the pass and reads it back.
fn measured(gpu: &common::Gpu, cam: Camera, positions: &[[f32; 3]], indices: &[u32]) -> Vec<f32> {
    let blas = Bvh::build_triangles(positions, indices);
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in positions {
        for axis in 0..3 {
            min[axis] = min[axis].min(p[axis]);
            max[axis] = max[axis].max(p[axis]);
        }
    }
    let bounds = AABB {
        min: Point3::new(min[0], min[1], min[2]),
        max: Point3::new(max[0], max[1], max[2]),
    };
    let tlas = Bvh::build_tlas(&[bounds]);
    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let arena = TraceArena::build(
        &tlas,
        &[ArenaMesh {
            bvh: &blas,
            positions,
            indices,
            normals: None,
            uv0: None,
        }],
        &[ArenaPlacement {
            mesh: 0,
            world: identity,
            inv_world: identity,
            material_base: 0,
            flags: INSTANCE_VISIBLE,
        }],
    );
    let scene = TraceScene::upload(&gpu.device, &gpu.queue, &gpu.pathtrace, &arena);

    let camera_state = CameraState::from_camera(&gpu.device, &gpu.layouts.camera, cam);
    let pass = DepthPass::new(&gpu.device, &gpu.pathtrace);
    let target = DepthTarget::new(&gpu.device, &gpu.pathtrace, WIDTH, HEIGHT);
    let uniforms = TraceUniforms::new(&gpu.device, &gpu.pathtrace, &camera_state.buffer);
    uniforms.write(
        &gpu.queue,
        &solarxy_renderer::pathtrace::TraceParams {
            tile_offset: [0, 0],
            tile_size: [WIDTH, HEIGHT],
            resolution: [WIDTH, HEIGHT],
            aperture_radius: 0.0,
            ..solarxy_renderer::pathtrace::TraceParams::default()
        },
    );

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Depth Test"),
        });
    pass.encode(&mut encoder, &scene, &target, &uniforms, [WIDTH, HEIGHT]);
    let (buffer, padded) = solarxy_renderer::capture::encode_capture(
        &gpu.device,
        &mut encoder,
        target.texture(),
        (0, 0, WIDTH, HEIGHT),
    );
    gpu.queue.submit(std::iter::once(encoder.finish()));

    // Read through the shared float readback rather than by hand, because
    // that is the path a still render takes and a single-channel source is
    // the one width it had never been asked for.
    let pending = PendingCapture::arm(buffer, padded, WIDTH, HEIGHT);
    let started = std::time::Instant::now();
    loop {
        assert!(
            started.elapsed() < std::time::Duration::from_secs(30),
            "the depth readback never landed"
        );
        match pending.poll_floats(&gpu.device, wgpu::TextureFormat::R32Float) {
            CaptureFloatPoll::Ready(floats) => {
                assert_eq!(
                    floats.len(),
                    (WIDTH * HEIGHT) as usize,
                    "a single-channel readback came back at the wrong width"
                );
                return floats;
            }
            CaptureFloatPoll::Failed => panic!("depth readback failed"),
            CaptureFloatPoll::Pending => std::thread::yield_now(),
        }
    }
}

fn compare(projection: ProjectionMode) {
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };
    let (positions, indices) = geometry();
    let cam = camera(projection);
    let want = expected(&cam, &positions, &indices);
    let have = measured(&gpu, cam, &positions, &indices);
    assert_eq!(have.len(), want.len());

    let mut hits = 0;
    let mut worst = 0.0f32;
    for (i, (w, h)) in want.iter().zip(&have).enumerate() {
        let (x, y) = (i as u32 % WIDTH, i as u32 / WIDTH);
        assert_eq!(
            *w >= DEPTH_MISS,
            *h >= DEPTH_MISS,
            "pixel ({x}, {y}) disagrees about whether it found anything: \
             processor {w}, kernel {h}"
        );
        if *w >= DEPTH_MISS {
            continue;
        }
        hits += 1;
        // Relative, because the two build their rays through different
        // matrices: one inverts the whole view-projection, the other unprojects
        // and rotates. They agree to float noise, not to bits.
        let error = (w - h).abs() / w.abs().max(1e-6);
        worst = worst.max(error);
        assert!(
            error < 1e-3,
            "pixel ({x}, {y}): processor says {w}, the kernel says {h}"
        );
    }
    assert!(
        hits > (WIDTH * HEIGHT / 4) as usize,
        "only {hits} pixels found geometry, so this compared mostly misses"
    );
    eprintln!("{projection:?}: {hits} hits, worst relative error {worst:.2e}");
}

/// A perspective camera, where the axis projection is the whole question.
#[test]
fn the_depth_pass_agrees_with_the_engines_own_intersector() {
    compare(ProjectionMode::Perspective);
}

/// And an orthographic one, where every ray is parallel to the axis and the
/// cosine is one. The kernel asks no question about which projection is in use;
/// this is what says it does not have to.
#[test]
fn an_orthographic_camera_needs_no_special_case() {
    compare(ProjectionMode::Orthographic);
}

/// The two square-on quads, read where the answer is known without a second
/// implementation to compare against.
///
/// A distance along the ray would report a larger number towards the edges of
/// frame, by one over the cosine, which at this field of view is a fifth at the
/// corners. A distance along the axis is flat, and that is the property a
/// compositor's defocus depends on.
#[test]
fn a_plane_square_to_the_camera_reads_one_distance_across_the_whole_frame() {
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };
    let (positions, indices) = geometry();
    let cam = camera(ProjectionMode::Perspective);
    let have = measured(&gpu, cam, &positions, &indices);

    // The near quad spans the left of frame; sample its column of pixels down
    // the whole height, away from the tilted strip in the middle.
    let column = 4;
    let mut readings: Vec<f32> = Vec::new();
    for y in 0..HEIGHT {
        let d = have[(y * WIDTH + column) as usize];
        assert!(d < DEPTH_MISS, "the near quad should cover column {column}");
        readings.push(d);
    }
    let first = readings[0];
    for (y, d) in readings.iter().enumerate() {
        assert!(
            (d - first).abs() < 1e-3,
            "row {y} of a plane square to the camera reads {d} against {first} \
             at the top, so the distance is being reported along the ray"
        );
    }
    // And it is the distance from the camera to that plane: the eye is at
    // z = 8 and the quad is at z = 0. A pass that measured from where its rays
    // start would report a near plane less, which is small and wrong.
    assert!(
        (first - 8.0).abs() < 1e-3,
        "the near plane reads {first}, and the camera is eight units from it"
    );
}
