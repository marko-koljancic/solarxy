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
        raw.as_chunks::<LIGHT_RESULT_WIDTH>()
            .0
            .iter()
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
        raw.as_chunks::<LIGHT_RESULT_WIDTH>()
            .0
            .iter()
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
    let Some(harness) = Harness::new(TracedLight::pool(std::slice::from_ref(&def))) else {
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
        for (m, v) in mean.iter_mut().zip(d.direction) {
            *m += v;
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
        for px in pixels.as_chunks::<4>().0 {
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

/// The three estimators agree through a pane of blended alpha.
///
/// The pane between the light and the sphere is the surface the two techniques
/// used to disagree about: the shadow walk attenuated connections through it by
/// its authored opacity while the scatter walk shaded it opaque, so next-event
/// estimation lit the sphere through a pane that scattering said was a
/// ceiling. The integrator now resolves blended coverage stochastically from
/// the alpha-test dimension, with the same `1 - alpha` expectation the shadow
/// walk charges, and the three estimators must converge on one image again.
/// Alpha sits mid-range so neither limit -- always there, never there -- could
/// pass by accident.
#[test]
fn the_estimators_agree_through_a_blended_pane() {
    use solarxy_core::geometry::{AlphaMode, RawMaterialData};
    use solarxy_core::preferences::ProjectionMode;
    use solarxy_renderer::camera::{Camera, CameraUniform};
    use solarxy_renderer::pathtrace::arena::INSTANCE_CAST_SHADOW;
    use solarxy_renderer::pathtrace::{
        EnvParams, PathEstimator, PathKernel, PathUniforms, ReadbackPoll, TraceParams, TraceTarget,
    };

    const WIDTH: u32 = 96;
    const HEIGHT: u32 = 96;
    /// More than the unobstructed estimator comparison uses, because the pane
    /// adds a coin flip to every scattered path that crosses it and the
    /// scatter-only mode pays for that in variance.
    const SPP: u32 = 2048;

    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };

    // The same rough sphere under the same broad panel as the unobstructed
    // comparison, with a wide blended pane lying between them so every
    // connection from the sphere to the light crosses it.
    let (sphere_pos, sphere_idx) = solarxy_bvh::corpus::sphere(64, 32);
    let sphere_bvh = Bvh::build_triangles(&sphere_pos, &sphere_idx);
    let (pane_pos, pane_idx) = solarxy_bvh::corpus::coplanar_grid(4, 1.0);
    let pane_bvh = Bvh::build_triangles(&pane_pos, &pane_idx);

    let surface = TracedMaterial::from_raw(
        &RawMaterialData {
            base_color_factor: [0.8, 0.8, 0.8, 1.0],
            roughness_factor: 0.6,
            metallic_factor: 0.0,
            ..Default::default()
        },
        &MaterialTextures::default(),
    );
    // Mid-range coverage, no transmission: everything that reaches the sphere
    // through this pane does so because blended alpha let it through.
    let pane = TracedMaterial::from_raw(
        &RawMaterialData {
            base_color_factor: [0.9, 0.9, 0.9, 0.45],
            roughness_factor: 0.6,
            metallic_factor: 0.0,
            alpha_mode: AlphaMode::Blend,
            ..Default::default()
        },
        &MaterialTextures::default(),
    );

    let identity: [[f32; 4]; 4] = cgmath::Matrix4::from_scale(1.0).into();
    // The grid is built in the XY plane; lie it flat, widen it, and lift it
    // between the sphere and the panel.
    let pane_world = cgmath::Matrix4::from_translation(cgmath::Vector3::new(0.0, 1.8, 0.0))
        * cgmath::Matrix4::from_angle_x(cgmath::Deg(-90.0))
        * cgmath::Matrix4::from_scale(2.5);
    let placements = [
        ArenaPlacement {
            mesh: 0,
            world: identity,
            inv_world: identity,
            material_base: 0,
            flags: INSTANCE_VISIBLE | INSTANCE_CAST_SHADOW,
        },
        ArenaPlacement {
            mesh: 1,
            world: pane_world.into(),
            inv_world: pane_world
                .invert()
                .expect("the pane placement inverts")
                .into(),
            material_base: 1,
            flags: INSTANCE_VISIBLE | INSTANCE_CAST_SHADOW,
        },
    ];
    let boxes = [
        solarxy_core::aabb::AABB {
            min: cgmath::Point3::new(-1.0, -1.0, -1.0),
            max: cgmath::Point3::new(1.0, 1.0, 1.0),
        },
        solarxy_core::aabb::AABB {
            min: cgmath::Point3::new(-5.0, 1.79, -5.0),
            max: cgmath::Point3::new(5.0, 1.81, 5.0),
        },
    ];
    let tlas = Bvh::build_tlas(&boxes);
    let meshes = [
        ArenaMesh {
            bvh: &sphere_bvh,
            positions: &sphere_pos,
            indices: &sphere_idx,
            normals: None,
            uv0: None,
        },
        ArenaMesh {
            bvh: &pane_bvh,
            positions: &pane_pos,
            indices: &pane_idx,
            normals: None,
            uv0: None,
        },
    ];

    let mut panel = base_light(LightKind::RectArea);
    panel.position = [0.0, 3.0, 0.0];
    panel.area_extent = [6.0, 6.0];
    panel.intensity = 4.0;

    let arena = TraceArena::build(&tlas, &meshes, &placements)
        .with_materials(vec![surface, pane])
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
        label: Some("Blend Estimator Camera"),
        size: std::mem::size_of::<CameraUniform>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    gpu.queue
        .write_buffer(&camera_buffer, 0, bytemuck::bytes_of(&camera_uniform));

    let target = TraceTarget::new(&gpu.device, &gpu.pathtrace, WIDTH, HEIGHT);
    let uniforms = PathUniforms::new(&gpu.device, &camera_buffer);
    let kernel = PathKernel::new(&gpu.device, &gpu.pathtrace, &uniforms);

    let environment = EnvParams::constant([0.12, 0.14, 0.18], [0.04, 0.04, 0.05]);
    let params = TraceParams {
        tile_offset: [0, 0],
        tile_size: [WIDTH, HEIGHT],
        resolution: [WIDTH, HEIGHT],
        bounces: 6,
        // Plenty: each blended crossing charges this budget in both walks, so
        // a starved budget would measure the clamp rather than the estimator.
        transmissive_bounces: 6,
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
                label: Some("Blend Estimator Encoder"),
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
                ReadbackPoll::Failed => panic!("blend estimator readback failed"),
            }
        };
        let mut total = 0.0f64;
        for px in pixels.as_chunks::<4>().0 {
            total += f64::from(px[0]) + f64::from(px[1]) + f64::from(px[2]);
            assert!(
                px[0].is_finite() && px[1].is_finite() && px[2].is_finite(),
                "{estimator:?} produced a pixel that is not a number"
            );
        }
        let mean = total / f64::from(WIDTH * HEIGHT * 3);
        println!("{estimator:?}: mean radiance through the pane {mean:.6}");
        means.push(mean);
    }

    let reference = means[0];
    for (estimator, mean) in PathEstimator::ALL.iter().zip(&means) {
        let error = (mean - reference).abs() / reference;
        assert!(
            error < 0.005,
            "{estimator:?} converged to {mean:.6} where the weighted estimator \
             gives {reference:.6}, a difference of {:.2}%. The two techniques \
             must tell one story about whether a blended pane blocks light: \
             the shadow walk charges its authored opacity, and the scatter \
             walk has to pass through with the same expectation.",
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
    // is both dimmed and stained. Thin, stated rather than inherited from the
    // default: it is the field that decides whether the shadow walk will admit
    // a straight connection at all, and this test is the thin case, the one
    // that connection is correct about.
    let glass = TracedMaterial::from_raw(
        &RawMaterialData {
            base_color_factor: [0.9, 0.1, 0.1, 1.0],
            roughness_factor: 0.05,
            metallic_factor: 0.0,
            transmission: 1.0,
            thickness: 0.0,
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

/// Renders the tinted-blocker scene with the sheet's `thickness` set as given,
/// and returns the mean colour under the sheet and beside it.
///
/// Everything except that one field is held fixed, which is the whole point:
/// `thickness` is the field the material response branches on to tell a thin
/// pane from a refractive solid, and the rule under test is that the shadow
/// walk reads the same field the same way.
fn sheet_shadow_means(gpu: &common::Gpu, thickness: f32) -> ([f64; 3], [f64; 3]) {
    sheet_shadow_means_biased(gpu, thickness, 0.0)
}

/// The same, with the lobe-weight split deliberately skewed.
///
/// The weights are a sampling density: they choose the lobe and they set the
/// mixture density the sample is charged, and nothing else reads them. A scene
/// rendered at two different splits therefore has to converge to the same
/// image, and that is the acceptance criterion this release owes for the
/// rewritten transmission lobe, read literally rather than at one shading point.
fn sheet_shadow_means_biased(
    gpu: &common::Gpu,
    thickness: f32,
    lobe_bias: f32,
) -> ([f64; 3], [f64; 3]) {
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
    // White rather than red here: the question is how much light arrives, not
    // what colour it is, and a tint would only make the comparison harder to
    // read. `thickness` is the variable.
    let glass = TracedMaterial::from_raw(
        &RawMaterialData {
            base_color_factor: [1.0, 1.0, 1.0, 1.0],
            roughness_factor: 0.05,
            metallic_factor: 0.0,
            transmission: 1.0,
            ior: 1.5,
            thickness,
            ..Default::default()
        },
        &MaterialTextures::default(),
    );

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
        label: Some("Refractive Solid Camera"),
        size: std::mem::size_of::<CameraUniform>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    gpu.queue
        .write_buffer(&camera_buffer, 0, bytemuck::bytes_of(&camera_uniform));

    let target = TraceTarget::new(&gpu.device, &gpu.pathtrace, WIDTH, HEIGHT);
    let uniforms = PathUniforms::new(&gpu.device, &camera_buffer);
    let kernel = PathKernel::with_lobe_bias(&gpu.device, &gpu.pathtrace, &uniforms, lobe_bias);
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
            label: Some("Refractive Solid Encoder"),
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
            ReadbackPoll::Failed => panic!("refractive solid readback failed"),
        }
    };

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
    (mean(4, WIDTH / 4), mean(WIDTH * 3 / 4, WIDTH - 4))
}

/// A refractive solid casts a shadow; a thin pane does not.
///
/// Next-event estimation connects a shading point to a light along a straight
/// line, and treats a transmissive surface in the way as a tinted filter. That
/// is right for a thin pane, whose parallel faces preserve the ray's direction
/// so the straight segment is the real path. It is wrong for a refractive
/// solid, where light reaching the far side arrives along a bent path and there
/// is generally no straight path at all: the connection is not approximate, it
/// describes a different phenomenon.
///
/// The two cases are separated by `thickness`, the same field the material
/// response branches on, so one rule decides both walks and they cannot
/// disagree about what a surface is. This renders one scene twice, changing
/// only that field, which is the sharpest available form of that claim.
#[test]
fn a_refractive_solid_casts_a_shadow_where_a_thin_pane_does_not() {
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };

    let (thin_shadowed, thin_lit) = sheet_shadow_means(&gpu, 0.0);
    let (solid_shadowed, solid_lit) = sheet_shadow_means(&gpu, 0.5);

    println!("thin  pane: under {thin_shadowed:.4?}, beside {thin_lit:.4?}");
    println!("solid ball: under {solid_shadowed:.4?}, beside {solid_lit:.4?}");

    assert!(
        thin_lit[0] > 0.05 && solid_lit[0] > 0.05,
        "the lit half has to be lit in both renders for the comparison to mean \
         anything: thin {:.4}, solid {:.4}",
        thin_lit[0],
        solid_lit[0]
    );
    // The floor beside the sheet is not under it, so nothing about this rule
    // should touch it. If it moved, the change reached further than the shadow
    // walk and that is a finding rather than a pass.
    let lit_drift = (solid_lit[0] - thin_lit[0]).abs() / thin_lit[0];
    assert!(
        lit_drift < 0.02,
        "the unobstructed half must not move when only the blocker's thickness \
         changes: drifted {:.1} percent",
        lit_drift * 100.0
    );
    assert!(
        thin_shadowed[0] > 0.0,
        "a thin transmissive pane must still let light through: it is the case \
         the straight-line connection is correct about"
    );
    let kept = solid_shadowed[0] / thin_shadowed[0];
    println!("the solid keeps {kept:.4} of the light the thin pane let through");
    assert!(
        solid_shadowed[0] < thin_shadowed[0] * 0.25,
        "a refractive solid must not admit the straight connection a thin pane \
         admits: it let through {kept:.4} of what the pane did, where the rule \
         is that it should admit almost none of it"
    );
}

/// The three estimators agree about a floor lit through a refractive solid.
///
/// This is the criterion the refusal rule owes, and it is the honest form of
/// "the connection stopped lying": it is not asserted from a picture. A
/// connection-only estimator and a scattering-only estimator are two techniques
/// for the same integral, so they must converge to the same image. While the
/// shadow walk admitted a straight line through a refractive solid, they could
/// not: the connection technique reported a floor lit as though the ball were
/// absent, and the scattering technique reported the bent transport that is
/// actually there.
///
/// The tolerance here is looser than the unobstructed and blended comparisons,
/// and the reason is stated rather than hidden. The floor beneath the ball is
/// reached only along refracted paths, so the scatter-only estimator carries
/// the whole variance of that transport for the same sample budget, which is
/// the very thing this release documents as converging slowly.
#[test]
fn the_estimators_agree_under_a_refractive_solid() {
    use solarxy_core::geometry::RawMaterialData;
    use solarxy_core::preferences::ProjectionMode;
    use solarxy_renderer::camera::{Camera, CameraUniform};
    use solarxy_renderer::pathtrace::arena::INSTANCE_CAST_SHADOW;
    use solarxy_renderer::pathtrace::{
        EnvParams, PathEstimator, PathKernel, PathUniforms, ReadbackPoll, TraceParams, TraceTarget,
    };

    const WIDTH: u32 = 96;
    const HEIGHT: u32 = 96;
    const SPP: u32 = 2048;

    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };

    // A glass ball resting above a floor, with a broad panel overhead. The
    // subject is the floor beneath the ball, which is the only place the two
    // techniques ever disagreed.
    let (sphere_pos, sphere_idx) = solarxy_bvh::corpus::sphere(64, 32);
    let sphere_bvh = Bvh::build_triangles(&sphere_pos, &sphere_idx);
    let (floor_pos, floor_idx) = solarxy_bvh::corpus::coplanar_grid(4, 1.0);
    let floor_bvh = Bvh::build_triangles(&floor_pos, &floor_idx);

    let floor_mat = TracedMaterial::from_raw(
        &RawMaterialData {
            base_color_factor: [0.8, 0.8, 0.8, 1.0],
            roughness_factor: 1.0,
            metallic_factor: 0.0,
            ..Default::default()
        },
        &MaterialTextures::default(),
    );
    // A solid: `thickness` is what makes it one, and it is the field both walks
    // read to decide whether a straight connection through it exists.
    let glass = TracedMaterial::from_raw(
        &RawMaterialData {
            base_color_factor: [1.0, 1.0, 1.0, 1.0],
            roughness_factor: 0.05,
            metallic_factor: 0.0,
            transmission: 1.0,
            ior: 1.5,
            thickness: 0.8,
            ..Default::default()
        },
        &MaterialTextures::default(),
    );

    let ball_world = cgmath::Matrix4::from_translation(cgmath::Vector3::new(0.0, 0.9, 0.0))
        * cgmath::Matrix4::from_scale(0.7);
    let floor_world =
        cgmath::Matrix4::from_angle_x(cgmath::Deg(-90.0)) * cgmath::Matrix4::from_scale(3.0);
    let placements = [
        ArenaPlacement {
            mesh: 0,
            world: ball_world.into(),
            inv_world: ball_world.invert().expect("the ball inverts").into(),
            material_base: 1,
            flags: INSTANCE_VISIBLE | INSTANCE_CAST_SHADOW,
        },
        ArenaPlacement {
            mesh: 1,
            world: floor_world.into(),
            inv_world: floor_world.invert().expect("the floor inverts").into(),
            material_base: 0,
            flags: INSTANCE_VISIBLE | INSTANCE_CAST_SHADOW,
        },
    ];
    let boxes = [
        solarxy_core::aabb::AABB {
            min: cgmath::Point3::new(-0.8, 0.1, -0.8),
            max: cgmath::Point3::new(0.8, 1.7, 0.8),
        },
        solarxy_core::aabb::AABB {
            min: cgmath::Point3::new(-3.5, -0.01, -3.5),
            max: cgmath::Point3::new(3.5, 0.01, 3.5),
        },
    ];
    let tlas = Bvh::build_tlas(&boxes);
    let meshes = [
        ArenaMesh {
            bvh: &sphere_bvh,
            positions: &sphere_pos,
            indices: &sphere_idx,
            normals: None,
            uv0: None,
        },
        ArenaMesh {
            bvh: &floor_bvh,
            positions: &floor_pos,
            indices: &floor_idx,
            normals: None,
            uv0: None,
        },
    ];

    let mut panel = base_light(LightKind::RectArea);
    panel.position = [0.0, 3.5, 0.0];
    panel.area_extent = [4.0, 4.0];
    panel.intensity = 6.0;

    let arena = TraceArena::build(&tlas, &meshes, &placements)
        .with_materials(vec![floor_mat, glass])
        .with_lights(TracedLight::pool(&[panel]));
    let scene = TraceScene::upload(&gpu.device, &gpu.queue, &gpu.pathtrace, &arena);
    let atlas = TraceAtlas::new(&gpu.device, &gpu.queue, &gpu.pathtrace);

    // Looking down at the floor past the ball, so the region the two techniques
    // disagreed about fills the frame.
    let camera = Camera {
        eye: cgmath::Point3::new(0.0, 2.6, 2.6),
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
        label: Some("Refractive Estimator Camera"),
        size: std::mem::size_of::<CameraUniform>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    gpu.queue
        .write_buffer(&camera_buffer, 0, bytemuck::bytes_of(&camera_uniform));

    let target = TraceTarget::new(&gpu.device, &gpu.pathtrace, WIDTH, HEIGHT);
    let uniforms = PathUniforms::new(&gpu.device, &camera_buffer);
    let kernel = PathKernel::new(&gpu.device, &gpu.pathtrace, &uniforms);

    let environment = EnvParams::constant([0.12, 0.14, 0.18], [0.04, 0.04, 0.05]);
    let params = TraceParams {
        tile_offset: [0, 0],
        tile_size: [WIDTH, HEIGHT],
        resolution: [WIDTH, HEIGHT],
        bounces: 6,
        // Two crossings per traversal of the ball, so a starved budget would
        // measure the budget rather than the estimator.
        transmissive_bounces: 6,
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
                label: Some("Refractive Estimator Encoder"),
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
                ReadbackPoll::Failed => panic!("refractive estimator readback failed"),
            }
        };
        let mut total = 0.0f64;
        for px in pixels.as_chunks::<4>().0 {
            total += f64::from(px[0]) + f64::from(px[1]) + f64::from(px[2]);
            assert!(
                px[0].is_finite() && px[1].is_finite() && px[2].is_finite(),
                "{estimator:?} produced a pixel that is not a number"
            );
        }
        let mean = total / f64::from(WIDTH * HEIGHT * 3);
        println!("{estimator:?}: mean radiance under the solid {mean:.6}");
        means.push(mean);
    }

    // The scattering technique never consulted the shadow walk, so its number
    // is the one thing here the rule cannot have moved. It is the reference.
    let mis = means[0];
    let scatter = means[1];
    let next_event = means[2];

    let weighted_error = (mis - scatter).abs() / scatter;
    println!(
        "weighted against scattering: {:.3}%",
        weighted_error * 100.0
    );
    assert!(
        weighted_error < 0.01,
        "the weighted estimator converged to {mis:.6} where the scattering \
         estimator, which never consulted the shadow walk and is therefore \
         unmoved by this rule, gives {scatter:.6}: a difference of {:.2}%. \
         These are the two techniques that can find bent transport at all, and \
         they have to tell one story about a floor lit through a solid.",
        weighted_error * 100.0
    );

    // Next-event estimation alone cannot find this transport, and after the
    // rule it correctly stops pretending to. It may fall short of the truth;
    // what it must never do is exceed it, because that is the shape of the
    // defect being fixed: a straight connection through a solid invents light
    // that no path delivers.
    println!(
        "connection-only against scattering: {:.3}%",
        (next_event - scatter) / scatter * 100.0
    );
    assert!(
        next_event <= scatter * 1.01,
        "the connection-only estimator gives {next_event:.6} against the \
         scattering estimator's {scatter:.6}. Exceeding it means a straight \
         line through a refractive solid is still delivering light that no \
         actual path delivers, which is the defect this rule exists to remove."
    );
}

/// A rendered scene converges to the same image whichever way the lobe split is
/// set.
///
/// The scene-level reading of the invariance the probe suite asserts at one
/// shading point, and the literal form of the criterion the transmission rewrite
/// owes. The lobe weights decide which lobe a sample is drawn from and they set
/// the mixture density that sample is charged against; those are the only two
/// places they are read, so the two effects have to cancel and the picture must
/// not move.
///
/// A whole render carries variance the probe does not, so the tolerance is
/// looser than the probe's two percent and looser than the estimator
/// comparisons' half a percent: skewing the split deliberately starves one lobe,
/// which costs samples where they are most needed.
#[test]
fn a_rendered_scene_does_not_move_when_the_lobe_split_does() {
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };

    // The solid, because it is the case the rewritten lobe is about and the one
    // whose transport runs entirely through the transmission branch.
    let (shipped, _) = sheet_shadow_means_biased(&gpu, 0.5, 0.0);
    // Reflection share from about four percent to about sixty-four, which is
    // more than an order of magnitude off the sampling optimum.
    let (skewed, _) = sheet_shadow_means_biased(&gpu, 0.5, 0.6);

    println!("under the solid: shipped split {shipped:.5?}, skewed split {skewed:.5?}");
    let error = (shipped[0] - skewed[0]).abs() / shipped[0].max(1e-6);
    println!("{:.2}% apart", error * 100.0);

    assert!(
        error < 0.05,
        "the same scene rendered to {:.5} at the shipped lobe split and {:.5} \
         with the split skewed, {:.2}% apart. The weights are a sampling \
         density and are read in exactly two places, the selection and the \
         mixture density, so those two have to cancel and the image must not \
         move. A difference here means they are acting as a response instead, \
         and every render carries a factor no picture would reveal.",
        shipped[0],
        skewed[0],
        error * 100.0
    );
}
