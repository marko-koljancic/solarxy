//! Direct lighting on a real device: does a light's density describe its
//! sampler, and do the two techniques add up to one image.
//!
//! Three questions, and none is answerable from Rust or from looking at a
//! picture.
//!
//! The first is whether a light's probability density describes the directions
//! its sampler actually produces. Get it wrong by a factor and the render is
//! still plausible: it converges, it has no artefacts, and every surface facing
//! that light is simply the wrong brightness. The instrument is the same one the
//! material response uses, a histogram against an independently written density,
//! and the independence is real rather than nominal: the sampler's density comes
//! from the point it chose on the rectangle, and the probe's other mode derives
//! one from where a ray *landed* on that rectangle.
//!
//! The second is whether that density agrees with geometry rather than merely
//! with itself. The mean of one over the density, over directions the sampler
//! drew, is the light's solid angle, and a rectangle's solid angle has a closed
//! form. That is what catches both halves being wrong by the same factor, which
//! a histogram cannot see.
//!
//! The third is the one this whole stage turns on. Multiple importance sampling
//! splits one integral between two techniques and weights each by how likely it
//! was to have produced the sample it produced. The weights are a partition of
//! unity, so **all three estimators have to converge to the same image**, and
//! only the variance may differ. The material's densities were settled a stage
//! ago as the one-sample mixture over every lobe that could have produced a
//! direction; this is where that convention meets a second density and either
//! holds or does not.

mod common;

use cgmath::SquareMatrix;
use solarxy_bvh::Bvh;
use solarxy_core::scene::{LightDef, LightKind};
use solarxy_renderer::pathtrace::arena::{ArenaMesh, ArenaPlacement, INSTANCE_VISIBLE, TraceArena};
use solarxy_renderer::pathtrace::light::{LIGHT_RECT, TracedLight};
use solarxy_renderer::pathtrace::material::TracedMaterial;
use solarxy_renderer::pathtrace::scene::MaterialTextures;
use solarxy_renderer::pathtrace::probe::{
    ColorPoll, LIGHT_RESULT_WIDTH, LightProbe, LightProbeMode, LightTap,
};
use solarxy_renderer::pathtrace::{TraceAtlas, TraceScene};

/// Samples per histogram or solid-angle estimate.
const SAMPLES: u32 = 8192;

/// A light that is nothing in particular, so each test says what it changed.
fn base_light(kind: LightKind) -> LightDef {
    LightDef {
        kind,
        position: [0.0, 4.0, 0.0],
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

/// The device, a one-triangle scene carrying a light pool, and the probe.
///
/// The triangle is not decoration: the light array rides the same buffer set the
/// geometry does, and reading it through the real scene group is what makes the
/// answer a property of the shipped bindings rather than of a layout a test
/// invented.
struct Harness {
    gpu: common::Gpu,
    scene: TraceScene,
    atlas: TraceAtlas,
    probe: LightProbe,
}

impl Harness {
    fn new(lights: Vec<TracedLight>) -> Option<Self> {
        let gpu = common::gpu_or_skip()?;

        let positions = [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let indices = [0u32, 1, 2];
        let bvh = Bvh::build_triangles(&positions, &indices);
        let placement = ArenaPlacement {
            mesh: 0,
            world: cgmath::Matrix4::from_scale(1.0).into(),
            inv_world: cgmath::Matrix4::from_scale(1.0).into(),
            material_base: 0,
            flags: INSTANCE_VISIBLE,
        };
        let boxes = [solarxy_core::aabb::AABB {
            min: cgmath::Point3::new(0.0, 0.0, 0.0),
            max: cgmath::Point3::new(1.0, 1.0, 0.0),
        }];
        let tlas = Bvh::build_tlas(&boxes);
        let mesh = ArenaMesh {
            bvh: &bvh,
            positions: &positions,
            indices: &indices,
            normals: None,
            uv0: None,
        };
        let arena = TraceArena::build(&tlas, &[mesh], &[placement])
            .with_materials(vec![TracedMaterial::fallback()])
            .with_lights(lights);
        let scene = TraceScene::upload(&gpu.device, &gpu.queue, &gpu.pathtrace, &arena);
        let atlas = TraceAtlas::new(&gpu.device, &gpu.queue, &gpu.pathtrace);
        let probe = LightProbe::new(&gpu.device, &gpu.pathtrace);

        Some(Self {
            gpu,
            scene,
            atlas,
            probe,
        })
    }

    fn run(&self, mode: LightProbeMode, taps: &[LightTap]) -> Vec<[f32; 4]> {
        let mut readback = self.probe.submit(
            &self.gpu.device,
            &self.gpu.queue,
            mode,
            &self.scene,
            &self.atlas,
            taps,
        );
        for _ in 0..2000 {
            match readback.poll(&self.gpu.device) {
                ColorPoll::Ready(values) => return values,
                ColorPoll::Pending => std::thread::sleep(std::time::Duration::from_millis(1)),
                ColorPoll::Failed => panic!("light readback failed"),
            }
        }
        panic!("light readback never resolved");
    }

    /// Draws `SAMPLES` connections to one light from one point.
    fn sample(&self, light: u32, origin: [f32; 3]) -> Vec<Drawn> {
        let taps: Vec<LightTap> = (0..SAMPLES)
            .map(|i| LightTap {
                origin: [origin[0], origin[1], origin[2], 0.0],
                direction: [0.0; 4],
                light,
                sample_index: i,
                strata: SAMPLES,
                seed: 0x9E37_79B9,
            })
            .collect();
        let raw = self.run(LightProbeMode::Sample, &taps);
        raw.chunks_exact(LIGHT_RESULT_WIDTH)
            .map(|c| Drawn {
                direction: [c[0][0], c[0][1], c[0][2]],
                pdf: c[0][3],
                radiance: [c[1][0], c[1][1], c[1][2]],
                distance: c[1][3],
            })
            .collect()
    }

    /// Asks what density the light-sampling technique gives each direction,
    /// derived from the intersection rather than from the sampler.
    fn intersect(&self, origin: [f32; 3], directions: &[[f32; 3]]) -> Vec<f32> {
        let taps: Vec<LightTap> = directions
            .iter()
            .map(|d| LightTap {
                origin: [origin[0], origin[1], origin[2], 0.0],
                direction: [d[0], d[1], d[2], 0.0],
                light: 0,
                sample_index: 0,
                strata: 0,
                seed: 0,
            })
            .collect();
        let raw = self.run(LightProbeMode::Intersect, &taps);
        raw.chunks_exact(LIGHT_RESULT_WIDTH)
            .map(|c| c[0][3])
            .collect()
    }
}

#[derive(Clone, Copy, Debug)]
struct Drawn {
    direction: [f32; 3],
    pdf: f32,
    radiance: [f32; 3],
    distance: f32,
}

/// The solid angle a rectangle subtends from a point, in closed form.
///
/// The spherical excess of the two triangles the rectangle splits into, summed
/// by van Oosterom and Strackee's formula, which is stable where the more
/// familiar sum-of-dihedral-angles form is not. This is the geometry the density
/// is claimed to describe, computed a completely different way, which is what
/// makes it worth having.
fn rectangle_solid_angle(origin: [f32; 3], centre: [f32; 3], u: [f32; 3], v: [f32; 3]) -> f64 {
    let corner = |su: f32, sv: f32| {
        [
            f64::from(centre[0] + u[0] * su + v[0] * sv - origin[0]),
            f64::from(centre[1] + u[1] * su + v[1] * sv - origin[1]),
            f64::from(centre[2] + u[2] * su + v[2] * sv - origin[2]),
        ]
    };
    let a = corner(-0.5, -0.5);
    let b = corner(0.5, -0.5);
    let c = corner(0.5, 0.5);
    let d = corner(-0.5, 0.5);
    triangle_solid_angle(a, b, c) + triangle_solid_angle(a, c, d)
}

fn triangle_solid_angle(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    let la = norm(a);
    let lb = norm(b);
    let lc = norm(c);
    let numerator = det(a, b, c).abs();
    let denominator = la * lb * lc + dot(a, b) * lc + dot(a, c) * lb + dot(b, c) * la;
    2.0 * numerator.atan2(denominator)
}

fn norm(v: [f64; 3]) -> f64 {
    dot(v, v).sqrt()
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn det(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
        + a[2] * (b[0] * c[1] - b[1] * c[0])
}

#[test]
fn a_rect_light_density_integrates_to_the_solid_angle_it_subtends() {
    // The sharpest single number available. If the sampler and its density
    // disagree by any factor, or if both are wrong by the same factor, the mean
    // of one over the density misses the closed form by exactly that factor.
    let def = base_light(LightKind::RectArea);
    let Some(harness) = Harness::new(TracedLight::pool(&[def.clone()])) else {
        return;
    };

    // Two shading points: one under the centre, and one well off to the side
    // where the rectangle is foreshortened and the cosine term earns its place.
    for origin in [[0.0f32, 0.0, 0.0], [2.5, 0.5, 1.5]] {
        let drawn = harness.sample(0, origin);
        let mut total = 0.0f64;
        let mut used = 0u32;
        for d in &drawn {
            assert!(d.pdf.is_finite(), "a density must be a number");
            if d.pdf > 0.0 {
                total += 1.0 / f64::from(d.pdf);
                used += 1;
            }
        }
        assert_eq!(
            used, SAMPLES,
            "every draw at {origin:?} should land on the rectangle"
        );
        let estimate = total / f64::from(SAMPLES);

        let light = TracedLight::from_def(&def).expect("rect");
        let expected = rectangle_solid_angle(origin, light.position, light.u, light.v);
        let error = (estimate - expected).abs() / expected;
        println!(
            "rect solid angle from {origin:?}: estimated {estimate:.6}, \
             closed form {expected:.6}, error {:.3}%",
            error * 100.0
        );
        // The estimator has no variance at all in exact arithmetic: it is the
        // mean of a quantity whose expectation is the integral of one over the
        // light, and every draw is a valid one. What is left is the sampler's
        // uniformity over area against the density's per-sample cosine, which is
        // a real spread, so this is a tolerance rather than an equality.
        assert!(
            error < 0.005,
            "the rect density does not describe the geometry it claims: \
             estimated {estimate:.6} against a closed form of {expected:.6}"
        );
    }
}

#[test]
fn the_rect_sampler_and_the_intersection_agree_on_the_density() {
    // Two independent routes to one number. The sampler computes its density
    // from the point it chose on the rectangle; the intersection computes one
    // from where a ray landed on it. A factor wrong in either shows up here, and
    // reporting the sampler's own density back would pass however wrong it was.
    let def = base_light(LightKind::RectArea);
    let Some(harness) = Harness::new(TracedLight::pool(&[def])) else {
        return;
    };

    let origin = [1.0f32, 0.0, -0.5];
    let drawn = harness.sample(0, origin);
    let directions: Vec<[f32; 3]> = drawn.iter().map(|d| d.direction).collect();
    let from_intersection = harness.intersect(origin, &directions);

    let mut worst = 0.0f32;
    for (d, other) in drawn.iter().zip(&from_intersection) {
        assert!(
            *other > 0.0,
            "a direction the sampler drew must hit the light it drew it from"
        );
        let error = (d.pdf - other).abs() / d.pdf;
        worst = worst.max(error);
    }
    println!("rect density, sampler against intersection: worst {worst:.6}");
    // Both compute the same expression from the same record, so the only
    // difference is where the point came from and float rounding along the way.
    assert!(
        worst < 1e-3,
        "the sampler's density and the intersection's disagree by {worst}"
    );
}

#[test]
fn a_one_sided_rect_lights_only_the_face_it_emits_from() {
    // The `axis` convention's one negation lives in the rect arm of the record
    // builder, and getting it backwards lights the wrong side of the panel while
    // looking like a rotation bug.
    let def = base_light(LightKind::RectArea);
    let Some(harness) = Harness::new(TracedLight::pool(&[def])) else {
        return;
    };

    // The panel sits at y = 4 emitting straight down, so a point below it is lit
    // and a point above it is not.
    let below = harness.sample(0, [0.0, 0.0, 0.0]);
    assert!(
        below.iter().all(|d| d.pdf > 0.0),
        "a point under the panel must see it"
    );
    let above = harness.sample(0, [0.0, 8.0, 0.0]);
    assert!(
        above.iter().all(|d| d.pdf == 0.0),
        "a point above a one-sided panel must not be lit by its back"
    );
}

#[test]
fn a_two_sided_rect_lights_both_faces() {
    let mut def = base_light(LightKind::RectArea);
    def.two_sided = true;
    let Some(harness) = Harness::new(TracedLight::pool(&[def])) else {
        return;
    };
    let above = harness.sample(0, [0.0, 8.0, 0.0]);
    assert!(
        above.iter().all(|d| d.pdf > 0.0),
        "a two-sided panel emits from its back as well"
    );
}

#[test]
fn a_point_light_with_a_radius_spreads_its_connections() {
    // Where a penumbra comes from: a point emitter sends every shadow ray to one
    // place, and one with a radius spreads them across a disc, so a receiver can
    // be partly occluded. Zero radius must stay exactly a point, because that is
    // every scene authored before the parameter existed.
    let mut hard = base_light(LightKind::Point);
    hard.position = [0.0, 4.0, 0.0];
    let Some(harness) = Harness::new(TracedLight::pool(&[hard.clone()])) else {
        return;
    };
    let drawn = harness.sample(0, [0.0, 0.0, 0.0]);
    let spread = direction_spread(&drawn);
    assert!(
        spread < 1e-6,
        "a zero radius must send every connection to the same place, got {spread}"
    );

    let mut soft = hard;
    soft.radius = 1.0;
    let Some(harness) = Harness::new(TracedLight::pool(&[soft])) else {
        return;
    };
    let drawn = harness.sample(0, [0.0, 0.0, 0.0]);
    let spread = direction_spread(&drawn);
    println!("point radius 1.0 at 4.0 away: angular spread {spread:.4} rad");
    // A disc of radius one at four units subtends about a quarter radian, so the
    // directions must span something of that order rather than merely differ.
    assert!(
        spread > 0.2,
        "a radius of one at four units should spread the connections, got {spread}"
    );
    assert!(
        drawn.iter().all(|d| (d.pdf - 1.0).abs() < 1e-6),
        "a point light stays a delta light whatever its radius: the extent buys \
         the penumbra and not a second sampling technique"
    );
}

/// The angle between the two most divergent directions drawn, in radians.
fn direction_spread(drawn: &[Drawn]) -> f32 {
    let mut mean = [0.0f32; 3];
    for d in drawn {
        for axis in 0..3 {
            mean[axis] += d.direction[axis];
        }
    }
    let len = (mean[0] * mean[0] + mean[1] * mean[1] + mean[2] * mean[2]).sqrt();
    if len <= 0.0 {
        return 0.0;
    }
    for m in &mut mean {
        *m /= len;
    }
    let mut worst = 0.0f32;
    for d in drawn {
        let c = (d.direction[0] * mean[0] + d.direction[1] * mean[1] + d.direction[2] * mean[2])
            .clamp(-1.0, 1.0);
        worst = worst.max(c.acos());
    }
    worst
}

#[test]
fn a_spot_light_falls_off_across_its_cone_and_stops_at_the_edge() {
    let mut def = base_light(LightKind::Spot);
    def.position = [0.0, 4.0, 0.0];
    def.outer_cone = 0.4;
    def.inner_cone = 0.2;
    let Some(harness) = Harness::new(TracedLight::pool(&[def])) else {
        return;
    };

    // Straight below the apex is inside the full-intensity core; four units out
    // at four units down is 45 degrees, well outside a 0.4 radian cone.
    let inside = harness.sample(0, [0.0, 0.0, 0.0]);
    assert!(
        inside[0].radiance[0] > 0.9,
        "the cone's core is at full intensity, got {}",
        inside[0].radiance[0]
    );
    let outside = harness.sample(0, [4.0, 0.0, 0.0]);
    assert!(
        outside[0].radiance[0] == 0.0,
        "nothing outside the outer cone receives any light, got {}",
        outside[0].radiance[0]
    );
    // And between them, something in between.
    let edge = harness.sample(0, [1.2, 0.0, 0.0]);
    assert!(
        edge[0].radiance[0] > 0.0 && edge[0].radiance[0] < 0.9,
        "the penumbra between the inner and outer cones is a falloff, got {}",
        edge[0].radiance[0]
    );
}

#[test]
fn a_directional_light_is_parallel_and_does_not_fall_off() {
    let mut def = base_light(LightKind::Directional);
    def.direction = [0.0, -1.0, 0.0];
    def.intensity = 3.0;
    let Some(harness) = Harness::new(TracedLight::pool(&[def])) else {
        return;
    };
    for origin in [[0.0f32, 0.0, 0.0], [50.0, -20.0, 30.0]] {
        let drawn = harness.sample(0, origin);
        assert_eq!(drawn[0].direction, [0.0, 1.0, 0.0]);
        assert!(
            (drawn[0].radiance[0] - 3.0).abs() < 1e-6,
            "a directional light has no distance to fall off over"
        );
        assert!(
            drawn[0].distance > 1e20,
            "its shadow ray has to reach past the scene"
        );
    }
}

#[test]
fn forty_lights_all_reach_the_kernel() {
    // The raster path binds eight because a uniform holds eight. Nothing here
    // may quietly acquire that ceiling, and the way to find out is to ask the
    // fortieth light for a connection rather than to trust a length.
    let defs: Vec<LightDef> = (0..40)
        .map(|i| {
            let mut d = base_light(LightKind::Point);
            #[allow(clippy::cast_precision_loss)]
            let intensity = i as f32 + 1.0;
            d.intensity = intensity;
            d
        })
        .collect();
    let pool = TracedLight::pool(&defs);
    assert_eq!(pool.len(), 40);
    let Some(harness) = Harness::new(pool) else {
        return;
    };
    for index in [0u32, 8, 39] {
        let drawn = harness.sample(index, [0.0, 0.0, 0.0]);
        #[allow(clippy::cast_precision_loss)]
        let expected = index as f32 + 1.0;
        assert!(
            (drawn[0].radiance[0] - expected).abs() < 1e-4,
            "light {index} of forty reports {} rather than {expected}",
            drawn[0].radiance[0]
        );
    }
}

#[test]
fn an_ambient_light_never_becomes_a_record_the_kernel_could_pick() {
    // Belt and braces against the pool: the kernel picks uniformly by index, so
    // an entry it cannot sample would take a share of the probability and return
    // nothing, leaving the whole scene dim by a ratio no one would connect to an
    // ambient light being present.
    let defs = vec![
        base_light(LightKind::Ambient),
        base_light(LightKind::RectArea),
    ];
    let pool = TracedLight::pool(&defs);
    assert_eq!(pool.len(), 1);
    assert_eq!(pool[0].kind, LIGHT_RECT);
}

/// The three estimators must converge to the same image.
///
/// This is the test the whole stage turns on. Multiple importance sampling
/// splits one integral between sampling the light and sampling the material,
/// and weights each sample by how likely each technique was to have produced
/// it. Those weights sum to one for any given direction, so the *expectation*
/// is identical however the work is divided, and only the variance moves.
///
/// So a disagreement here is not a tuning problem. It says one of the two
/// densities does not describe its own sampler, and the density most likely to
/// be at fault is the material's, because it is a mixture: `bsdf_result`
/// reports the sum over every lobe that could have produced a direction,
/// weighted by how likely that lobe was to be chosen, rather than the sampled
/// lobe's own density. Weighting a light's density against the wrong one of
/// those two shows up as a difference between these three numbers and as
/// nothing else at all.
///
/// The scene is built so all three modes can actually converge: a broad area
/// light close to a rough surface, which is the case scattering alone can find,
/// under a dim environment so the connections have a second thing to choose
/// between.
///
/// The tolerance is a twentieth of what the disagreement was when this test was
/// first written. It found a real one: connections were not occluded by area
/// lights, because a light is not geometry and the shadow walk only descended
/// the hierarchy, so every point that could see the environment *through* the
/// panel was lit by it. The scattering technique had always treated the panel as
/// opaque, which is why only a comparison between the two could see it. The three
/// now sit within two parts in a thousand of each other, and a tolerance loose
/// enough to have accepted the old two percent would be a tolerance that accepts
/// the next one too.
#[test]
fn the_three_estimators_agree_on_the_same_scene() {
    use solarxy_core::geometry::RawMaterialData;
    use solarxy_core::preferences::ProjectionMode;
    use solarxy_renderer::camera::{Camera, CameraUniform};
    use solarxy_renderer::pathtrace::{
        EnvParams, PathEstimator, PathKernel, PathUniforms, ReadbackPoll, TraceParams, TraceTarget,
    };

    const WIDTH: u32 = 96;
    const HEIGHT: u32 = 96;
    /// Enough that the scatter-only mode, which is the noisy one, settles to
    /// within the tolerance below on an image-wide mean.
    const SPP: u32 = 1024;

    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };

    // A rough dielectric sphere on nothing, under a broad panel.
    let (sphere_pos, sphere_idx) = solarxy_bvh::corpus::sphere(64, 32);
    let bvh = Bvh::build_triangles(&sphere_pos, &sphere_idx);
    let raw = RawMaterialData {
        base_color_factor: [0.8, 0.8, 0.8, 1.0],
        roughness_factor: 0.6,
        metallic_factor: 0.0,
        ..Default::default()
    };
    let material = TracedMaterial::from_raw(&raw, &MaterialTextures::default());
    let identity: [[f32; 4]; 4] = cgmath::Matrix4::from_scale(1.0).into();
    let placement = ArenaPlacement {
        mesh: 0,
        world: identity,
        inv_world: identity,
        material_base: 0,
        flags: INSTANCE_VISIBLE,
    };
    let boxes = [solarxy_core::aabb::AABB {
        min: cgmath::Point3::new(-1.0, -1.0, -1.0),
        max: cgmath::Point3::new(1.0, 1.0, 1.0),
    }];
    let tlas = Bvh::build_tlas(&boxes);
    let mesh = ArenaMesh {
        bvh: &bvh,
        positions: &sphere_pos,
        indices: &sphere_idx,
        normals: None,
        uv0: None,
    };

    // Broad and close: a small distant light would make the scatter-only mode
    // so noisy that the comparison would be measuring the tolerance rather than
    // the estimator.
    let mut panel = base_light(LightKind::RectArea);
    panel.position = [0.0, 3.0, 0.0];
    panel.area_extent = [6.0, 6.0];
    panel.intensity = 4.0;

    let arena = TraceArena::build(&tlas, &[mesh], &[placement])
        .with_materials(vec![material])
        .with_lights(TracedLight::pool(&[panel]));
    let scene = TraceScene::upload(&gpu.device, &gpu.queue, &gpu.pathtrace, &arena);
    let atlas = TraceAtlas::new(&gpu.device, &gpu.queue, &gpu.pathtrace);

    let camera = Camera {
        eye: cgmath::Point3::new(0.0, 0.5, 4.0),
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
        label: Some("Estimator Camera"),
        size: std::mem::size_of::<CameraUniform>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    gpu.queue
        .write_buffer(&camera_buffer, 0, bytemuck::bytes_of(&camera_uniform));

    let target = TraceTarget::new(&gpu.device, &gpu.pathtrace, WIDTH, HEIGHT);
    let uniforms = PathUniforms::new(&gpu.device, &camera_buffer);
    let kernel = PathKernel::new(&gpu.device, &gpu.pathtrace, &uniforms);

    // Dim, so the connections have a real choice to make between the panel and
    // the environment and the pick probability is genuinely exercised.
    let environment = EnvParams::constant([0.12, 0.14, 0.18], [0.04, 0.04, 0.05]);
    let params = TraceParams {
        tile_offset: [0, 0],
        tile_size: [WIDTH, HEIGHT],
        resolution: [WIDTH, HEIGHT],
        bounces: 6,
        transmissive_bounces: 0,
        samples: SPP,
        seed: 0x9E37_79B9,
        light_count: scene.light_count(),
        aperture_radius: 0.0,
        focus_distance: 0.0,
        aperture_blades: 0,
        ..TraceParams::default()
    };

    let mut means = Vec::new();
    for estimator in PathEstimator::ALL {
        uniforms.write(&gpu.queue, &params, &environment);
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Estimator Encoder"),
            });
        kernel.encode(
            &mut encoder,
            estimator,
            &scene,
            &atlas,
            &target,
            &uniforms,
            [WIDTH, HEIGHT],
        );
        let mut readback = target.encode_readback(&gpu.device, &mut encoder);
        gpu.queue.submit(Some(encoder.finish()));

        let pixels = loop {
            match readback.poll(&gpu.device) {
                ReadbackPoll::Ready(values) => break values,
                ReadbackPoll::Pending => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                ReadbackPoll::Failed => panic!("estimator readback failed"),
            }
        };
        let mut total = 0.0f64;
        for px in pixels.chunks_exact(4) {
            total += f64::from(px[0]) + f64::from(px[1]) + f64::from(px[2]);
            assert!(
                px[0].is_finite() && px[1].is_finite() && px[2].is_finite(),
                "{estimator:?} produced a pixel that is not a number"
            );
        }
        let mean = total / f64::from(WIDTH * HEIGHT * 3);
        println!("{estimator:?}: mean radiance {mean:.6}");
        means.push(mean);
    }

    let reference = means[0];
    for (estimator, mean) in PathEstimator::ALL.iter().zip(&means) {
        let error = (mean - reference).abs() / reference;
        assert!(
            error < 0.005,
            "{estimator:?} converged to {mean:.6} where the weighted estimator \
             gives {reference:.6}, a difference of {:.2}%. The weights are a \
             partition of unity, so these have one expectation; a gap means a \
             density does not describe its sampler.",
            error * 100.0
        );
    }
}

/// Tinted glass tints its shadow.
///
/// The criterion this stage owes, and the reason a shadow ray returns a colour
/// rather than a boolean. A transmissive surface between a light and a receiver
/// does not simply block: it passes a fraction of the light through and stains
/// it on the way. A renderer that answers occlusion with a yes or no can only
/// draw the shadow of a stained-glass window as a hole.
///
/// The scene is deliberately the smallest thing that shows it: a floor, a light
/// straight above, and a red transmissive sheet covering half of it. What is
/// compared is the shadowed half against the lit half, which controls for
/// everything about the material of the floor.
#[test]
fn a_transmissive_blocker_tints_the_shadow_it_casts() {
    use solarxy_core::geometry::RawMaterialData;
    use solarxy_core::preferences::ProjectionMode;
    use solarxy_renderer::camera::{Camera, CameraUniform};
    use solarxy_renderer::pathtrace::arena::INSTANCE_CAST_SHADOW;
    use solarxy_renderer::pathtrace::{
        EnvParams, PathEstimator, PathKernel, PathUniforms, ReadbackPoll, TraceParams, TraceTarget,
    };

    const WIDTH: u32 = 64;
    const HEIGHT: u32 = 64;
    const SPP: u32 = 256;

    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };

    // One plane mesh, placed twice: the floor, and a sheet above its left half.
    let (plane_pos, plane_idx) = solarxy_bvh::corpus::coplanar_grid(4, 1.0);
    let bvh = Bvh::build_triangles(&plane_pos, &plane_idx);

    let floor = TracedMaterial::from_raw(
        &RawMaterialData {
            base_color_factor: [0.8, 0.8, 0.8, 1.0],
            roughness_factor: 1.0,
            metallic_factor: 0.0,
            ..Default::default()
        },
        &MaterialTextures::default(),
    );
    // Red and almost entirely transmissive, so what reaches the floor beneath it
    // is both dimmed and stained.
    let glass = TracedMaterial::from_raw(
        &RawMaterialData {
            base_color_factor: [0.9, 0.1, 0.1, 1.0],
            roughness_factor: 0.05,
            metallic_factor: 0.0,
            transmission: 1.0,
            ..Default::default()
        },
        &MaterialTextures::default(),
    );

    // The grid is built in the XY plane, so both placements rotate it flat and
    // the sheet is lifted and shifted onto the left half.
    let flat = cgmath::Matrix4::from_angle_x(cgmath::Deg(-90.0));
    let floor_world = flat;
    let sheet_world = cgmath::Matrix4::from_translation(cgmath::Vector3::new(-1.0, 1.0, 0.0))
        * flat
        * cgmath::Matrix4::from_scale(0.5);
    let placements = [
        ArenaPlacement {
            mesh: 0,
            world: floor_world.into(),
            inv_world: floor_world
                .invert()
                .expect("the floor placement inverts")
                .into(),
            material_base: 0,
            flags: INSTANCE_VISIBLE | INSTANCE_CAST_SHADOW,
        },
        ArenaPlacement {
            mesh: 0,
            world: sheet_world.into(),
            inv_world: sheet_world
                .invert()
                .expect("the sheet placement inverts")
                .into(),
            material_base: 1,
            flags: INSTANCE_VISIBLE | INSTANCE_CAST_SHADOW,
        },
    ];
    let boxes = [
        solarxy_core::aabb::AABB {
            min: cgmath::Point3::new(-2.0, -0.01, -2.0),
            max: cgmath::Point3::new(2.0, 0.01, 2.0),
        },
        solarxy_core::aabb::AABB {
            min: cgmath::Point3::new(-2.0, 0.99, -1.0),
            max: cgmath::Point3::new(0.0, 1.01, 1.0),
        },
    ];
    let tlas = Bvh::build_tlas(&boxes);
    let mesh = ArenaMesh {
        bvh: &bvh,
        positions: &plane_pos,
        indices: &plane_idx,
        normals: None,
        uv0: None,
    };

    let mut sun = base_light(LightKind::Directional);
    sun.direction = [0.0, -1.0, 0.0];
    sun.intensity = 3.0;

    let arena = TraceArena::build(&tlas, &[mesh], &placements)
        .with_materials(vec![floor, glass])
        .with_lights(TracedLight::pool(&[sun]));
    let scene = TraceScene::upload(&gpu.device, &gpu.queue, &gpu.pathtrace, &arena);
    let atlas = TraceAtlas::new(&gpu.device, &gpu.queue, &gpu.pathtrace);

    // Straight down at the floor, so the left half of the image is under the
    // sheet and the right half is not.
    let camera = Camera {
        eye: cgmath::Point3::new(0.0, 6.0, 0.001),
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
        label: Some("Shadow Tint Camera"),
        size: std::mem::size_of::<CameraUniform>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    gpu.queue
        .write_buffer(&camera_buffer, 0, bytemuck::bytes_of(&camera_uniform));

    let target = TraceTarget::new(&gpu.device, &gpu.pathtrace, WIDTH, HEIGHT);
    let uniforms = PathUniforms::new(&gpu.device, &camera_buffer);
    let kernel = PathKernel::new(&gpu.device, &gpu.pathtrace, &uniforms);
    // Black, so every photon in the image came from the light through the sheet
    // and nothing is explained by ambient fill.
    let environment = EnvParams::constant([0.0; 3], [0.0; 3]);
    uniforms.write(
        &gpu.queue,
        &TraceParams {
            tile_offset: [0, 0],
            tile_size: [WIDTH, HEIGHT],
            resolution: [WIDTH, HEIGHT],
            bounces: 4,
            transmissive_bounces: 4,
            samples: SPP,
            seed: 0x9E37_79B9,
            light_count: scene.light_count(),
            aperture_radius: 0.0,
            focus_distance: 0.0,
            aperture_blades: 0,
            ..TraceParams::default()
        },
        &environment,
    );

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Shadow Tint Encoder"),
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
    let pixels = loop {
        match readback.poll(&gpu.device) {
            ReadbackPoll::Ready(values) => break values,
            ReadbackPoll::Pending => std::thread::sleep(std::time::Duration::from_millis(1)),
            ReadbackPoll::Failed => panic!("shadow tint readback failed"),
        }
    };

    // A column of the shadowed half and one of the lit half, well inside each so
    // no sample straddles the sheet's edge.
    let mean = |x0: u32, x1: u32| {
        let mut sum = [0.0f64; 3];
        let mut n = 0.0f64;
        for y in HEIGHT / 4..HEIGHT * 3 / 4 {
            for x in x0..x1 {
                let i = ((y * WIDTH + x) * 4) as usize;
                for c in 0..3 {
                    sum[c] += f64::from(pixels[i + c]);
                }
                n += 1.0;
            }
        }
        [sum[0] / n, sum[1] / n, sum[2] / n]
    };
    let shadowed = mean(4, WIDTH / 4);
    let lit = mean(WIDTH * 3 / 4, WIDTH - 4);
    println!("under the sheet {shadowed:.4?}, beside it {lit:.4?}");

    assert!(
        lit[0] > 0.05,
        "the lit half has to be lit for the comparison to mean anything"
    );
    assert!(
        shadowed[0] > 0.0,
        "a transmissive blocker must not cast a black shadow: it lets light \
         through, which is the difference between a shadow ray that returns a \
         colour and one that returns a boolean"
    );
    assert!(
        shadowed[0] < lit[0],
        "and it must still dim what passes through it"
    );
    // Red glass, so what gets through is red: the green channel loses far more
    // than the red one.
    let red_ratio = shadowed[0] / lit[0];
    let green_ratio = shadowed[1] / lit[1];
    println!("shadow keeps {red_ratio:.3} of red and {green_ratio:.3} of green");
    assert!(
        red_ratio > green_ratio * 2.0,
        "the shadow of red glass has to be red: kept {red_ratio:.3} of the red \
         and {green_ratio:.3} of the green"
    );
}
