//! Ray generation on a real device: where a pixel points, what an aperture does
//! to it, and what the auxiliary channels say about the surface it found.
//!
//! Four things live here, and each is invisible from Rust.
//!
//! **The orthographic ray.** Parallel rays have their origin varying per pixel
//! and their direction fixed, which is the opposite of a perspective camera and
//! the reason the kernel unprojects two points rather than aiming from the eye.
//! Anchoring at the eye collapses every orthographic pixel onto the view axis,
//! which is the bug that froze gizmo drags in axis views before 0.8.0. The
//! picking path was fixed then; this is the check that the tracer did not
//! quietly re-introduce it.
//!
//! **Pinhole equivalence.** An aperture of zero has to reproduce the image a
//! camera without one produced. It is the state every scene authored before
//! there was a lens is in, so a lens that changes the picture at f-stop zero
//! would change every existing render.
//!
//! **Bokeh.** An out-of-focus highlight takes the shape of the opening, and the
//! opening is a polygon with as many sides as the iris has blades. Counting the
//! corners of a blurred disc is the one property of depth of field that a
//! statistic can state and an eye can confirm.
//!
//! **The auxiliary channels.** Albedo and a world normal, written at the first
//! surface rough enough to look like itself, packed into one texture because
//! the storage budget is four and the accumulator needs the other two.

mod common;

use cgmath::SquareMatrix;
use solarxy_bvh::Bvh;
use solarxy_core::geometry::RawMaterialData;
use solarxy_core::preferences::ProjectionMode;
use solarxy_renderer::camera::{Camera, CameraUniform};
use solarxy_renderer::pathtrace::arena::{ArenaMesh, ArenaPlacement, INSTANCE_VISIBLE, TraceArena};
use solarxy_renderer::pathtrace::material::TracedMaterial;
use solarxy_renderer::pathtrace::scene::MaterialTextures;
use solarxy_renderer::pathtrace::{
    DebugChannel, EnvParams, PathEstimator, PathKernel, PathTracer, PathUniforms, ReadbackPoll,
    TraceAtlas, TraceParams, TraceScene, TraceTarget, TraceUniforms, unpack_aov_normal,
};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;

/// A camera looking down the negative z axis at the origin.
fn camera(projection: ProjectionMode) -> Camera {
    Camera {
        eye: cgmath::Point3::new(0.0, 0.0, 6.0),
        target: cgmath::Point3::new(0.0, 0.0, 0.0),
        up: cgmath::Vector3::unit_y(),
        aspect: 1.0,
        fovy: 45.0,
        znear: 0.1,
        zfar: 100.0,
        projection,
        ortho_scale: 2.0,
    }
}

fn camera_buffer(gpu: &common::Gpu, camera: &Camera) -> wgpu::Buffer {
    let mut uniform = CameraUniform::new();
    uniform.update_view_proj(camera);
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Camera Test Camera"),
        size: std::mem::size_of::<CameraUniform>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    gpu.queue
        .write_buffer(&buffer, 0, bytemuck::bytes_of(&uniform));
    buffer
}

/// A scene of one axis-aligned plane a long way behind the origin, big enough
/// that every ray in frame lands on it.
fn wall_scene(gpu: &common::Gpu, roughness: f32, colour: [f32; 3]) -> (TraceScene, TraceAtlas) {
    let (plane_pos, plane_idx) = solarxy_bvh::corpus::coplanar_grid(2, 40.0);
    let bvh = Bvh::build_triangles(&plane_pos, &plane_idx);
    let material = TracedMaterial::from_raw(
        &RawMaterialData {
            base_color_factor: [colour[0], colour[1], colour[2], 1.0],
            roughness_factor: roughness,
            metallic_factor: 0.0,
            ..Default::default()
        },
        &MaterialTextures::default(),
    );
    // The grid is built in XY facing +Z, which is straight at the camera.
    let world = cgmath::Matrix4::from_translation(cgmath::Vector3::new(0.0, 0.0, -8.0));
    let placement = ArenaPlacement {
        mesh: 0,
        world: world.into(),
        inv_world: world.invert().expect("the wall inverts").into(),
        material_base: 0,
        flags: INSTANCE_VISIBLE,
    };
    let boxes = [solarxy_core::aabb::AABB {
        min: cgmath::Point3::new(-40.0, -40.0, -8.01),
        max: cgmath::Point3::new(40.0, 40.0, -7.99),
    }];
    let tlas = Bvh::build_tlas(&boxes);
    let mesh = ArenaMesh {
        bvh: &bvh,
        positions: &plane_pos,
        indices: &plane_idx,
        normals: None,
        uv0: None,
    };
    let arena = TraceArena::build(&tlas, &[mesh], &[placement]).with_materials(vec![material]);
    (
        TraceScene::upload(&gpu.device, &gpu.queue, &gpu.pathtrace, &arena),
        TraceAtlas::new(&gpu.device, &gpu.queue, &gpu.pathtrace),
    )
}

fn spin(gpu: &common::Gpu, readback: &mut solarxy_renderer::pathtrace::FloatReadback) -> Vec<f32> {
    for _ in 0..2000 {
        match readback.poll(&gpu.device) {
            ReadbackPoll::Ready(values) => return values,
            ReadbackPoll::Pending => std::thread::sleep(std::time::Duration::from_millis(1)),
            ReadbackPoll::Failed => panic!("readback failed"),
        }
    }
    panic!("readback never resolved");
}

/// The debug kernel's depth channel over one scene: how far each pixel's ray
/// travelled, and whether it hit anything at all.
///
/// Distance along the ray from the near plane, which is what the two-point
/// reconstruction produces and what `screen_to_world_ray` documents on the
/// picking side.
fn depth_map(gpu: &common::Gpu, camera: &Camera) -> Vec<(f32, bool)> {
    let (scene, atlas) = wall_scene(gpu, 1.0, [0.8, 0.8, 0.8]);
    let buffer = camera_buffer(gpu, camera);
    let tracer = PathTracer::new(&gpu.device, &gpu.pathtrace);
    let target = TraceTarget::new(&gpu.device, &gpu.pathtrace, WIDTH, HEIGHT);
    let uniforms = TraceUniforms::new(&gpu.device, &gpu.pathtrace, &buffer);
    uniforms.write(&gpu.queue, &params(1));

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Camera Test Encoder"),
        });
    tracer.encode(
        &mut encoder,
        DebugChannel::Depth,
        &scene,
        &atlas,
        &target,
        &uniforms,
        [WIDTH, HEIGHT],
    );
    let mut readback = target.encode_readback(&gpu.device, &mut encoder);
    gpu.queue.submit(Some(encoder.finish()));
    // Alpha carries whether anything was hit, which is what this channel puts
    // there so a reader can tell a miss from a black surface.
    spin(gpu, &mut readback)
        .chunks_exact(4)
        .map(|px| (px[0], px[3] > 0.5))
        .collect()
}

/// Whether a pixel's ray found the wall.
fn hit_mask(gpu: &common::Gpu, camera: &Camera) -> Vec<bool> {
    depth_map(gpu, camera).into_iter().map(|(_, h)| h).collect()
}

fn params(samples: u32) -> TraceParams {
    TraceParams {
        tile_offset: [0, 0],
        tile_size: [WIDTH, HEIGHT],
        resolution: [WIDTH, HEIGHT],
        bounces: 2,
        transmissive_bounces: 0,
        samples,
        seed: 0x9E37_79B9,
        light_count: 0,
        aperture_radius: 0.0,
        focus_distance: 0.0,
        aperture_blades: 0,
    }
}

#[test]
fn an_orthographic_camera_traces_parallel_rays_rather_than_a_fan() {
    // The whole point of unprojecting two planes. An orthographic camera's rays
    // are parallel with the origin varying per pixel; aiming them all from the
    // eye collapses every pixel onto the view axis, so the image becomes one
    // ray repeated and the frame is uniformly whatever sits dead centre.
    //
    // Measured against a wall two units narrower than the ortho frame, so a
    // correct parallel camera sees the wall in the middle of frame and misses
    // past its edges, while a collapsed one either hits everywhere or misses
    // everywhere.
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };

    let mut ortho = camera(ProjectionMode::Orthographic);
    // Half-height 60 against a wall that reaches 40, so the frame is larger
    // than the wall and its edges are genuinely off it.
    ortho.ortho_scale = 60.0;
    let mask = hit_mask(&gpu, &ortho);

    let at = |x: u32, y: u32| mask[(y * WIDTH + x) as usize];
    assert!(
        at(WIDTH / 2, HEIGHT / 2),
        "the centre of frame is on the wall"
    );
    assert!(
        !at(0, 0) && !at(WIDTH - 1, HEIGHT - 1),
        "the corners of an orthographic frame wider than the wall are off it. \
         Hitting everywhere means every ray started at the eye and pointed the \
         same way, which is the pick-ray bug 0.8.0 fixed"
    );

    // And the converse, so the assertion above cannot be satisfied by a camera
    // that simply misses: shrink the frame inside the wall and everything hits.
    ortho.ortho_scale = 4.0;
    let inside = hit_mask(&gpu, &ortho);
    assert!(
        inside.iter().all(|hit| *hit),
        "an orthographic frame entirely inside the wall hits at every pixel"
    );
}

#[test]
fn the_two_projections_differ_in_exactly_the_way_they_should() {
    // The other side of the orthographic test, and the one that actually
    // discriminates. "Every pixel hit the wall" does not: a camera that
    // collapsed every ray onto the view axis would hit at every pixel too, and
    // more reliably. What separates them is the *shape* of the depth across
    // the frame.
    //
    // Against a flat wall square to the view, a perspective camera's corner ray
    // travels further than its centre ray by exactly one over the cosine of the
    // angle between them, because the ray is longer than the axis it leans away
    // from. A camera with parallel rays travels the same distance everywhere.
    // So one profile is domed and the other is flat, and neither can be
    // mistaken for the other.
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };

    let profile = |camera: &Camera| {
        let map = depth_map(&gpu, camera);
        let at = |x: u32, y: u32| {
            let (depth, hit) = map[(y * WIDTH + x) as usize];
            assert!(hit, "the sample at {x},{y} has to be on the wall");
            depth
        };
        (at(WIDTH / 2, HEIGHT / 2), at(2, 2))
    };

    let (centre, corner) = profile(&camera(ProjectionMode::Perspective));
    let ratio = corner / centre;
    // Half the diagonal of a 45-degree square frame is about 31.4 degrees off
    // axis, and one over its cosine is about 1.17.
    println!("perspective: centre {centre:.3}, corner {corner:.3}, ratio {ratio:.3}");
    assert!(
        ratio > 1.10,
        "a perspective corner ray leans away from the axis and so travels          further: ratio {ratio:.3}. A ratio of one means every ray took the          same direction, which is the collapse the two-point reconstruction          exists to prevent"
    );

    let mut ortho = camera(ProjectionMode::Orthographic);
    ortho.ortho_scale = 4.0;
    let (centre, corner) = profile(&ortho);
    println!("orthographic: centre {centre:.3}, corner {corner:.3}");
    assert!(
        (corner - centre).abs() < 1e-3,
        "parallel rays reach a square wall at the same distance everywhere:          {centre} at the centre against {corner} at the corner"
    );
}

/// Renders the wall through the path kernel and returns the colour buffer.
fn render(gpu: &common::Gpu, camera: &Camera, lens: (f32, f32, u32), samples: u32) -> Vec<f32> {
    let (scene, atlas) = wall_scene(gpu, 1.0, [0.8, 0.8, 0.8]);
    let buffer = camera_buffer(gpu, camera);
    let target = TraceTarget::new(&gpu.device, &gpu.pathtrace, WIDTH, HEIGHT);
    let uniforms = PathUniforms::new(&gpu.device, &buffer);
    let kernel = PathKernel::new(&gpu.device, &gpu.pathtrace, &uniforms);
    let mut p = params(samples);
    p.aperture_radius = lens.0;
    p.focus_distance = lens.1;
    p.aperture_blades = lens.2;
    uniforms.write(
        &gpu.queue,
        &p,
        &EnvParams::constant([0.6, 0.6, 0.6], [0.2, 0.2, 0.2]),
    );

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Camera Render Encoder"),
        });
    kernel.encode(
        &mut encoder,
        PathEstimator::Mis,
        &scene,
        &atlas,
        &target,
        &uniforms,
        [WIDTH, HEIGHT],
    );
    let mut readback = target.encode_readback(&gpu.device, &mut encoder);
    gpu.queue.submit(Some(encoder.finish()));
    spin(gpu, &mut readback)
}

#[test]
fn a_zero_aperture_reproduces_the_pinhole_image_exactly() {
    // Every camera authored before there was a lens has an f-stop of zero, so
    // this is the compatibility statement: a lens that changed the picture at
    // zero would change every render that already exists.
    //
    // Exact rather than approximate, and it can be: the aperture is branched
    // away at zero rather than multiplied by zero, so the two runs execute the
    // same arithmetic on the same random numbers and must agree bit for bit.
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };
    let cam = camera(ProjectionMode::Perspective);
    let pinhole = render(&gpu, &cam, (0.0, 0.0, 0), 4);
    // A focus distance and a blade count with no aperture to open: neither may
    // do anything on its own.
    let with_focus = render(&gpu, &cam, (0.0, 5.0, 6), 4);
    assert_eq!(
        pinhole, with_focus,
        "focus distance and blade count must do nothing while the aperture is \
         shut, or a stored value would silently change an existing render"
    );
}

#[test]
fn an_aperture_blurs_what_is_not_at_the_focus_distance() {
    // The wall is at 8 units and the camera focuses at 2, so it is thoroughly
    // out of focus. What that does to a flat wall is not visible in the middle
    // of it, which is why this measures the *edge*: a blurred image of a hard
    // edge is a soft one, and softness is a gradient where a pinhole has a step.
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };
    let mut cam = camera(ProjectionMode::Perspective);
    // Frame the wall's edge: it reaches x = 40 at z = -8, and a 45-degree
    // camera at z = 6 sees about 11.6 units across at that depth, so moving the
    // eye out puts the edge in frame.
    cam.eye = cgmath::Point3::new(38.0, 0.0, 6.0);
    cam.target = cgmath::Point3::new(38.0, 0.0, -8.0);

    let sharp = render(&gpu, &cam, (0.0, 0.0, 0), 64);
    let blurred = render(&gpu, &cam, (0.6, 2.0, 0), 64);

    // The horizontal gradient across the middle row: a step is concentrated in
    // one pixel and a soft edge is spread over many, so the largest single
    // difference between neighbours is the sharpest thing in the image.
    let sharpest = |image: &[f32]| {
        let y = HEIGHT / 2;
        let mut worst = 0.0f32;
        for x in 1..WIDTH {
            let a = image[((y * WIDTH + x - 1) * 4) as usize];
            let b = image[((y * WIDTH + x) * 4) as usize];
            worst = worst.max((b - a).abs());
        }
        worst
    };
    let sharp_step = sharpest(&sharp);
    let blurred_step = sharpest(&blurred);
    println!("sharpest neighbour step: pinhole {sharp_step:.4}, aperture {blurred_step:.4}");
    assert!(
        sharp_step > 0.02,
        "the pinhole image needs a real edge in it for the comparison to mean \
         anything, got {sharp_step}"
    );
    assert!(
        blurred_step < sharp_step * 0.6,
        "an open aperture on a wall six units from the focus plane has to \
         soften its edge: {blurred_step:.4} against a pinhole's {sharp_step:.4}"
    );
}

#[test]
fn the_tent_filter_is_centred_and_spans_a_whole_pixel() {
    // The filter itself, without a scene: a tent maps a uniform pair onto an
    // offset that averages to the pixel centre and reaches a full pixel either
    // side of it. A box filter would average to the same place and never leave
    // the pixel, so the span is what distinguishes them, and the mean is what
    // says the tent is not lopsided -- a lopsided reconstruction filter shifts
    // the whole image by a fraction of a pixel, which looks like a camera that
    // is slightly mis-aimed.
    //
    // The mapping is `sqrt(2u) - 1` below the midpoint and `1 - sqrt(2 - 2u)`
    // above it, plus a half so the offset is a position within the pixel; this
    // is the CPU twin, and it is here rather than in the shader's own tests
    // because what it asserts is arithmetic rather than anything a device does.
    let tent = |u: f32| {
        let t = u * 2.0;
        let v = if t < 1.0 {
            t.sqrt() - 1.0
        } else {
            1.0 - (2.0 - t).sqrt()
        };
        v + 0.5
    };

    const N: u32 = 100_000;
    let mut sum = 0.0f64;
    let mut lowest = f32::INFINITY;
    let mut highest = f32::NEG_INFINITY;
    for i in 0..N {
        #[allow(clippy::cast_precision_loss)]
        let u = (i as f32 + 0.5) / N as f32;
        let v = tent(u);
        sum += f64::from(v);
        lowest = lowest.min(v);
        highest = highest.max(v);
    }
    let mean = sum / f64::from(N);
    println!("tent: mean {mean:.5}, span {lowest:.4} to {highest:.4}");
    assert!(
        (mean - 0.5).abs() < 1e-3,
        "a lopsided filter shifts the whole image; mean is {mean}"
    );
    assert!(
        lowest < -0.45 && highest > 1.45,
        "the tent has to reach a pixel either side so neighbours overlap and \
         an edge is reconstructed rather than stepped: {lowest} to {highest}"
    );
}

#[test]
fn a_bladed_aperture_is_a_polygon_and_a_bladeless_one_is_a_disc() {
    // The shape of the opening is the shape of an out-of-focus highlight, and
    // it is measurable without rendering one: sample the aperture many times
    // and ask how far the samples reach in each direction. A disc reaches the
    // same distance everywhere; a hexagon reaches its corners and falls short
    // between them by the cosine of half its wedge, which for six sides is
    // about 13 percent.
    //
    // A CPU twin of `aperture_offset`, for the reason the tent's is: what it
    // asserts is the geometry of the sampling rather than anything a device
    // contributes.
    let offset = |u: f32, v: f32, blades: u32| -> (f32, f32) {
        if blades < 3 {
            let r = u.sqrt();
            let theta = v * 2.0 * std::f32::consts::PI;
            return (r * theta.cos(), r * theta.sin());
        }
        #[allow(clippy::cast_precision_loss)]
        let count = blades as f32;
        let wedge = (u * count).floor();
        let a = (u * count).fract();
        let (mut p, mut q) = (a, v);
        if p + q > 1.0 {
            p = 1.0 - p;
            q = 1.0 - q;
        }
        let step = 2.0 * std::f32::consts::PI / count;
        let first = ((wedge * step).cos(), (wedge * step).sin());
        let second = (((wedge + 1.0) * step).cos(), ((wedge + 1.0) * step).sin());
        (first.0 * p + second.0 * q, first.1 * p + second.1 * q)
    };

    // The furthest sample in each of many angular bins, which traces the
    // opening's outline.
    const BINS: usize = 360;
    let outline = |blades: u32| {
        let mut reach = vec![0.0f32; BINS];
        let mut state = 0x9E37_79B9u32;
        let mut next = || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            f32::from(u16::try_from(state >> 16).unwrap_or(0)) / 65536.0
        };
        for _ in 0..400_000 {
            let (x, y) = offset(next(), next(), blades);
            let angle = y.atan2(x);
            let normalized = (angle / (2.0 * std::f32::consts::PI)) + 0.5;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let bin = ((normalized * BINS as f32) as usize).min(BINS - 1);
            reach[bin] = reach[bin].max((x * x + y * y).sqrt());
        }
        reach
    };

    let disc = outline(0);
    let disc_min = disc.iter().copied().fold(f32::INFINITY, f32::min);
    let disc_max = disc.iter().copied().fold(0.0f32, f32::max);
    println!("circular opening reaches {disc_min:.3} to {disc_max:.3}");
    assert!(
        disc_min > disc_max * 0.97,
        "a bladeless aperture is round: it reached {disc_min} in one direction \
         and {disc_max} in another"
    );

    for blades in [6u32, 8] {
        let poly = outline(blades);
        let poly_min = poly.iter().copied().fold(f32::INFINITY, f32::min);
        let poly_max = poly.iter().copied().fold(0.0f32, f32::max);
        // A regular polygon's inradius over its circumradius.
        #[allow(clippy::cast_precision_loss)]
        let expected = (std::f32::consts::PI / blades as f32).cos();
        let ratio = poly_min / poly_max;
        println!("{blades} blades: reach ratio {ratio:.3}, a polygon's is {expected:.3}");
        assert!(
            (ratio - expected).abs() < 0.03,
            "{blades} blades should trace a {blades}-sided polygon, whose \
             narrowest reach is {expected:.3} of its widest; got {ratio:.3}"
        );
    }
}

#[test]
fn the_auxiliary_channels_describe_the_surface_the_pixel_found() {
    // Albedo and a world normal, which is what a denoiser steers by. They have
    // to be the surface rather than the light on it: an edge in them is an edge
    // worth preserving, and if they carried the shading instead they would
    // carry its noise too and the filter would preserve that.
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };
    let colour = [0.2f32, 0.7, 0.35];
    let (scene, atlas) = wall_scene(&gpu, 1.0, colour);
    let cam = camera(ProjectionMode::Perspective);
    let buffer = camera_buffer(&gpu, &cam);
    let target = TraceTarget::new(&gpu.device, &gpu.pathtrace, WIDTH, HEIGHT);
    let uniforms = PathUniforms::new(&gpu.device, &buffer);
    let kernel = PathKernel::new(&gpu.device, &gpu.pathtrace, &uniforms);
    uniforms.write(
        &gpu.queue,
        &params(4),
        &EnvParams::constant([0.6, 0.6, 0.6], [0.2, 0.2, 0.2]),
    );

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Camera AOV Encoder"),
        });
    kernel.encode(
        &mut encoder,
        PathEstimator::Mis,
        &scene,
        &atlas,
        &target,
        &uniforms,
        [WIDTH, HEIGHT],
    );
    let mut readback = target.encode_auxiliary_readback(&gpu.device, &mut encoder);
    gpu.queue.submit(Some(encoder.finish()));
    let aux = spin(&gpu, &mut readback);

    // The wall faces +Z, straight at the camera, and is a flat colour.
    let centre = ((HEIGHT / 2 * WIDTH + WIDTH / 2) * 4) as usize;
    let albedo = [aux[centre], aux[centre + 1], aux[centre + 2]];
    for (got, want) in albedo.iter().zip(colour.iter()) {
        assert!(
            (got - want).abs() < 1e-3,
            "the albedo channel has to be the base colour, unlit and unshaded: \
             got {albedo:?} against {colour:?}"
        );
    }

    let normal = unpack_aov_normal(aux[centre + 3]);
    println!("centre albedo {albedo:?}, normal {normal:?}");
    assert!(
        (normal[2] - 1.0).abs() < 0.01,
        "a wall facing the camera has a world normal of +Z: got {normal:?}"
    );
}

#[test]
fn a_packed_normal_survives_the_round_trip_from_every_direction() {
    // The decoder alone, against the encoding the kernel writes. It is packed
    // arithmetically rather than by reinterpreting bits, because an arbitrary
    // bit pattern read as a float can be a denormal that a platform is free to
    // flush; this is what says the arithmetic inverts.
    let pack = |n: [f64; 3]| -> f32 {
        let l1 = n[0].abs() + n[1].abs() + n[2].abs();
        let s = [n[0] / l1, n[1] / l1, n[2] / l1];
        let (mut x, mut y) = (s[0], s[1]);
        if s[2] < 0.0 {
            let sx = if s[0] >= 0.0 { 1.0 } else { -1.0 };
            let sy = if s[1] >= 0.0 { 1.0 } else { -1.0 };
            x = (1.0 - s[1].abs()) * sx;
            y = (1.0 - s[0].abs()) * sy;
        }
        let steps = 4096.0f64;
        let q = |v: f64| {
            ((v * 0.5 + 0.5) * (steps - 1.0))
                .clamp(0.0, steps - 1.0)
                .round()
        };
        #[allow(clippy::cast_possible_truncation)]
        {
            (q(x) * steps + q(y)) as f32
        }
    };

    // A spiral over the whole sphere, so both hemispheres and the fold between
    // them are covered.
    let mut worst = 0.0f64;
    for i in 0..2000 {
        let t = f64::from(i) / 2000.0;
        let z = 1.0 - 2.0 * t;
        let r = (1.0 - z * z).max(0.0).sqrt();
        let phi = f64::from(i) * 2.399_963_2;
        let n = [r * phi.cos(), r * phi.sin(), z];
        let back = unpack_aov_normal(pack(n));
        let dot = n[0] * f64::from(back[0]) + n[1] * f64::from(back[1]) + n[2] * f64::from(back[2]);
        worst = worst.max(dot.clamp(-1.0, 1.0).acos().to_degrees());
    }
    println!("worst octahedral round-trip error: {worst:.4} degrees");
    assert!(
        worst < 0.5,
        "twelve bits per component should hold a normal to a fraction of a \
         degree; worst was {worst} degrees"
    );
}
