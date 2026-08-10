//! The material response on a real device: does the BSDF agree with itself, and
//! does it conserve energy.
//!
//! Two questions, and neither is answerable without a GPU or by looking at an
//! image.
//!
//! The first is whether the density the sampler reports describes the directions
//! it actually produces. Get that wrong by a factor and every render is still
//! plausible: it converges, it has no artefacts, and it is simply the wrong
//! picture. A missing reflection Jacobian is a factor of four, a missing
//! normalization a factor of pi, and both look like a material that is a bit too
//! bright. The instrument is a histogram: draw many directions, bin them, and ask
//! the *evaluate* entry point what density it claims at those bins. Comparing a
//! sampler to itself would pass however wrong both halves were, which is why the
//! probe has two modes and why they share one weight computation.
//!
//! The second is whether the surface can reflect more light than reaches it. The
//! integral of the response over the hemisphere is the directional albedo, and it
//! must not exceed one. It is allowed to fall short: a single-scattering
//! microfacet model loses the energy that would have bounced between microfacets,
//! and this release does not restore it, so the sweep below asserts the ceiling
//! and *reports* the deficit rather than asserting a floor. That is the white
//! furnace test in numbers; the picture version lives in the ignored render at the
//! end of this file.

mod common;

use solarxy_bvh::Bvh;
use solarxy_core::geometry::RawMaterialData;
use solarxy_renderer::pathtrace::arena::{ArenaMesh, ArenaPlacement, INSTANCE_VISIBLE, TraceArena};
use solarxy_renderer::pathtrace::material::TracedMaterial;
use solarxy_renderer::pathtrace::probe::{
    BSDF_RESULT_WIDTH, BsdfProbe, BsdfProbeMode, BsdfTap, ColorPoll,
};
use solarxy_renderer::pathtrace::scene::MaterialTextures;
use solarxy_renderer::pathtrace::{TraceAtlas, TraceScene};

/// Samples per histogram or albedo estimate.
///
/// Stratified, so the error falls faster than the square root of this. It is a
/// multiple of the probe's row width so the dispatch grid is exactly filled.
const SAMPLES: u32 = 8192;

/// Cosine bins over the full sphere. The upper half receives the reflected lobes
/// and the lower half the transmitted one, and both are covered because a
/// histogram that silently ignored half the sphere would pass while losing every
/// transmitted sample.
const BINS: usize = 16;

/// Azimuths per quadrature row, used to integrate the analytic density around a
/// ring of constant cosine.
///
/// This has to resolve the lobe in azimuth, not merely visit it. For an oblique
/// view the specular peak occupies roughly `alpha / sin theta` radians of azimuth,
/// which at a moderate roughness is narrower than a coarse ring's spacing: sixteen
/// azimuths are 22.5 degrees apart and a nine-degree lobe falls between them, which
/// reports the density as wrong by a factor of three when it is not.
const RING: usize = 64;

/// Cosine steps *within* each bin, and the reason they exist.
///
/// The density is not constant across a bin. Near a specular peak it rises by
/// orders of magnitude over one bin's width, so the value at the bin's centre is
/// not the bin's average and comparing against it reports a large error where there
/// is none. That is not a subtle effect: at roughness 0.4 the centre of the top bin
/// reads 45 percent low, and at a grazing clearcoat angle 90 percent. The bin's
/// average is what the empirical count estimates, so the analytic side has to
/// integrate across the bin as well as around it.
const SUB: usize = 16;

/// A bin needs this many samples before its density is compared. Below it the
/// Monte Carlo error swamps the comparison and the test would be asserting noise.
const MIN_BIN: u32 = 120;

/// How far an empirical density may sit from the analytic one, as a fraction.
///
/// Chosen against what it has to catch rather than against what the noise allows:
/// the mistakes this test exists for are factors of pi, two and four, and the
/// worst of those is 57 percent off. A bin holding `MIN_BIN` samples carries about
/// nine percent of its own noise, so a quarter leaves room for the noise and none
/// for a wrong constant.
const HISTOGRAM_TOLERANCE: f32 = 0.25;

/// How far the directional albedo may exceed one before the surface is creating
/// energy rather than losing it.
const ALBEDO_CEILING: f32 = 1.05;

/// The device, a one-triangle scene, and the probe, built once per test.
///
/// The triangle is not decoration: the material pool rides the same buffer set the
/// geometry does, and reading it through the real scene group is what makes the
/// answer a property of the shipped bindings.
struct Harness {
    gpu: common::Gpu,
    scene: TraceScene,
    atlas: TraceAtlas,
    probe: BsdfProbe,
}

impl Harness {
    fn new(materials: Vec<TracedMaterial>) -> Option<Self> {
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
        let arena = TraceArena::build(&tlas, &[mesh], &[placement]).with_materials(materials);
        let scene = TraceScene::upload(&gpu.device, &gpu.queue, &gpu.pathtrace, &arena);
        // No textures: every descriptor is unused, so each factor stands alone and
        // the answer is a property of the lobes rather than of the atlas.
        let atlas = TraceAtlas::new(&gpu.device, &gpu.queue, &gpu.pathtrace);
        let probe = BsdfProbe::new(&gpu.device, &gpu.pathtrace);

        Some(Self {
            gpu,
            scene,
            atlas,
            probe,
        })
    }

    fn run(&self, mode: BsdfProbeMode, taps: &[BsdfTap]) -> Vec<[f32; 4]> {
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
                ColorPoll::Failed => panic!("bsdf readback failed"),
            }
        }
        panic!("bsdf readback never resolved");
    }

    /// Draws `SAMPLES` directions for one outgoing direction and one material.
    fn sample(&self, material: u32, wo: [f32; 3], seed: u32) -> Vec<Sampled> {
        let taps: Vec<BsdfTap> = (0..SAMPLES)
            .map(|i| BsdfTap {
                wo: [wo[0], wo[1], wo[2], 0.0],
                wi: [0.0; 4],
                material,
                sample_index: i,
                strata: SAMPLES,
                seed,
            })
            .collect();
        let values = self.run(BsdfProbeMode::Sample, &taps);
        (0..SAMPLES as usize)
            .map(|i| {
                let d = values[i * BSDF_RESULT_WIDTH];
                let c = values[i * BSDF_RESULT_WIDTH + 1];
                Sampled {
                    wi: [d[0], d[1], d[2]],
                    pdf: d[3],
                    color: [c[0], c[1], c[2]],
                    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                    lobe: c[3] as u32,
                }
            })
            .collect()
    }

    /// Asks the evaluate entry point for the density at each of `directions`.
    fn evaluate(&self, material: u32, wo: [f32; 3], directions: &[[f32; 3]]) -> Vec<f32> {
        let taps: Vec<BsdfTap> = directions
            .iter()
            .map(|wi| BsdfTap {
                wo: [wo[0], wo[1], wo[2], 0.0],
                wi: [wi[0], wi[1], wi[2], 0.0],
                material,
                sample_index: 0,
                strata: 0,
                seed: 0,
            })
            .collect();
        let values = self.run(BsdfProbeMode::Evaluate, &taps);
        (0..directions.len())
            .map(|i| values[i * BSDF_RESULT_WIDTH][3])
            .collect()
    }
}

struct Sampled {
    wi: [f32; 3],
    pdf: f32,
    color: [f32; 3],
    lobe: u32,
}

fn traced(raw: &RawMaterialData) -> TracedMaterial {
    TracedMaterial::from_raw(raw, &MaterialTextures::default())
}

/// A direction at a given cosine and azimuth, in the probe's tangent space.
fn direction(cos_theta: f32, phi: f32) -> [f32; 3] {
    let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
    [sin_theta * phi.cos(), sin_theta * phi.sin(), cos_theta]
}

/// Compares the sampled directions against the density the evaluate mode claims.
///
/// The comparison is made on the marginal in cosine rather than over the full
/// sphere, and that is a deliberate reduction. A two-dimensional histogram over
/// the sphere needs orders of magnitude more samples per cell to say anything, and
/// the mistakes worth catching, a missing Jacobian or a missing normalization, are
/// constants: they move the marginal by exactly as much as they move the density.
/// What the reduction gives up is a rotational error in azimuth, which no term in
/// this BSDF can produce.
///
/// The callers deliberately use rough materials. The quadrature on the analytic
/// side has to resolve the lobe, and a near-mirror lobe is narrower than any grid
/// affordable here, so a smooth one is where the comparison can be trusted. Nothing
/// is given up by that: every mistake this test exists for is a constant factor and
/// is equally visible at any roughness. The roughness-dependent behaviour is covered
/// by the furnace sweep, which walks the whole range.
///
/// The analytic marginal is the density integrated over each bin, estimated by
/// asking the evaluate mode for `SUB` times `RING` directions spread across it in
/// cosine and around it in azimuth. That is still an independent answer: it comes
/// from the other entry point.
fn assert_histogram_matches(
    harness: &Harness,
    label: &str,
    material: u32,
    wo: [f32; 3],
    seed: u32,
) {
    let samples = harness.sample(material, wo, seed);

    let mut counts = [0u32; BINS];
    let mut drawn = 0u32;
    for s in &samples {
        // A non-positive density is a rejected sample. That is not a defect: a
        // rough microfacet can reflect below the horizon, where the response and
        // its density are both zero, and the path simply ends there carrying
        // nothing. What it means for this comparison is that the sampler's
        // distribution has an atom of mass on "rejected" and its continuous part
        // integrates to less than one.
        if s.pdf <= 0.0 {
            continue;
        }
        drawn += 1;
        let t = ((s.wi[2] + 1.0) * 0.5).clamp(0.0, 0.999_999);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let bin = (t * BINS as f32) as usize;
        counts[bin] += 1;
    }
    #[allow(clippy::cast_precision_loss)]
    let rejected = 1.0 - f64::from(drawn) / f64::from(SAMPLES);
    println!(
        "{label}: {:.1} percent of samples rejected",
        rejected * 100.0
    );
    assert!(
        drawn > SAMPLES / 4,
        "{label}: only {drawn} of {SAMPLES} samples carried a density; \
         the sampler is rejecting almost everything"
    );

    // A quadrature grid per bin: `SUB` cosines across it, `RING` azimuths around
    // each. Both are offset by a half step, so no sample lands exactly on a bin
    // edge or on the pole, where the ring collapses to a point.
    #[allow(clippy::cast_precision_loss)]
    let width = 2.0 / BINS as f32;
    let cells = SUB * RING;
    let mut ring_dirs = Vec::with_capacity(BINS * cells);
    for bin in 0..BINS {
        #[allow(clippy::cast_precision_loss)]
        let lo = -1.0 + width * bin as f32;
        for j in 0..SUB {
            #[allow(clippy::cast_precision_loss)]
            let cos_theta = lo + width * (j as f32 + 0.5) / SUB as f32;
            for k in 0..RING {
                #[allow(clippy::cast_precision_loss)]
                let phi = std::f32::consts::TAU * (k as f32 + 0.5) / RING as f32;
                ring_dirs.push(direction(cos_theta, phi));
            }
        }
    }
    let ring_pdfs = harness.evaluate(material, wo, &ring_dirs);

    let mut compared = 0usize;
    for bin in 0..BINS {
        if counts[bin] < MIN_BIN {
            continue;
        }
        // The empirical marginal density in cosine, over EVERY draw rather than
        // over the survivors. Renormalizing over the survivors would inflate every
        // bin by one over one minus the rejection rate, which at a grazing angle is
        // a factor of several and reads as the density being wrong everywhere.
        let empirical = f64::from(counts[bin]) / f64::from(SAMPLES) / f64::from(width);

        // The analytic one: the mean density over the whole bin, times the azimuth
        // measure. The Jacobian from solid angle to (cosine, azimuth) is one,
        // which is the whole reason this reduction is clean.
        let mean: f64 = ring_pdfs[bin * cells..(bin + 1) * cells]
            .iter()
            .map(|p| f64::from(*p))
            .sum::<f64>()
            / cells as f64;
        let analytic = mean * f64::from(std::f32::consts::TAU);

        assert!(
            analytic > 0.0,
            "{label}: bin {bin} holds {} samples but the evaluate mode claims a \
             density of zero there, so the two entry points disagree about the support",
            counts[bin]
        );
        let relative = ((empirical - analytic) / analytic).abs();
        assert!(
            relative <= f64::from(HISTOGRAM_TOLERANCE),
            "{label}: bin {bin} (cos theta {:.3}) empirical density {empirical:.4} against \
             analytic {analytic:.4}, {:.1} percent off over {} samples",
            -1.0 + width * (bin as f32 + 0.5),
            relative * 100.0,
            counts[bin]
        );
        compared += 1;
    }
    assert!(
        compared >= 3,
        "{label}: only {compared} bins held enough samples to compare, \
         so the histogram asserted almost nothing"
    );
}

/// The estimate of `integral of f times cosine` over the sphere, which is the
/// directional albedo for this outgoing direction.
fn directional_albedo(samples: &[Sampled]) -> [f32; 3] {
    let mut sum = [0.0f64; 3];
    for s in samples {
        // A terminated sample contributes nothing and still counts in the
        // denominator: it is a real outcome of the sampler, not a missing one.
        if s.pdf <= 0.0 {
            continue;
        }
        for (total, channel) in sum.iter_mut().zip(s.color.iter()) {
            *total += f64::from(*channel) / f64::from(s.pdf);
        }
    }
    #[allow(clippy::cast_possible_truncation)]
    std::array::from_fn(|c| (sum[c] / f64::from(SAMPLES)) as f32)
}

#[test]
fn the_specular_lobe_agrees_with_its_density() {
    // Fully metallic, so the selection distribution puts everything in the GGX
    // lobe and the histogram is testing one sampler rather than a mixture.
    let raw = RawMaterialData {
        metallic_factor: 1.0,
        roughness_factor: 0.8,
        ..Default::default()
    };
    let Some(harness) = Harness::new(vec![traced(&raw)]) else {
        return;
    };
    assert_histogram_matches(
        &harness,
        "specular, normal incidence",
        0,
        [0.0, 0.0, 1.0],
        0x51,
    );
    assert_histogram_matches(
        &harness,
        "specular, oblique",
        0,
        direction(0.6, 0.0),
        0x9E37_79B9,
    );
}

#[test]
fn the_diffuse_and_specular_mixture_agrees_with_its_density() {
    // A dielectric splits its samples between the diffuse and specular lobes, so
    // this is the mixture density under test rather than one lobe's.
    let raw = RawMaterialData {
        roughness_factor: 0.6,
        ..Default::default()
    };
    let Some(harness) = Harness::new(vec![traced(&raw)]) else {
        return;
    };

    let samples = harness.sample(0, [0.0, 0.0, 1.0], 0x1357_9BDF);
    let diffuse = samples.iter().filter(|s| s.lobe == 1).count();
    let specular = samples.iter().filter(|s| s.lobe == 2).count();
    assert!(
        diffuse > 100 && specular > 100,
        "the mixture did not exercise both lobes: {diffuse} diffuse, {specular} specular"
    );

    assert_histogram_matches(
        &harness,
        "dielectric mixture",
        0,
        [0.0, 0.0, 1.0],
        0x1357_9BDF,
    );
}

#[test]
fn the_clearcoat_lobe_agrees_with_its_density() {
    // The clearcoat's share is its Fresnel, which is four percent head-on and
    // rises towards grazing, so the oblique direction is the one that gives it
    // enough samples to say anything about.
    let raw = RawMaterialData {
        clearcoat: 1.0,
        clearcoat_roughness: 0.7,
        metallic_factor: 1.0,
        roughness_factor: 0.8,
        ..Default::default()
    };
    let Some(harness) = Harness::new(vec![traced(&raw)]) else {
        return;
    };

    let wo = direction(0.25, 0.0);
    let samples = harness.sample(0, wo, 0x0BAD_F00D);
    let clearcoat = samples.iter().filter(|s| s.lobe == 4).count();
    assert!(
        clearcoat > 100,
        "the clearcoat lobe was chosen only {clearcoat} times at a grazing angle, \
         so this test is not exercising it"
    );

    assert_histogram_matches(&harness, "clearcoat, grazing", 0, wo, 0x0BAD_F00D);
}

#[test]
fn the_white_furnace_never_creates_energy() {
    // A white surface across the roughness and metalness square, which is the
    // reference's furnace grid expressed as numbers. Every cell's directional
    // albedo must sit at or below one; where it sits below, the shortfall is the
    // multiple-scattering energy this release does not restore, and it is printed
    // rather than asserted so a later change can be read against it.
    const STEPS: usize = 6;

    let materials: Vec<TracedMaterial> = (0..STEPS)
        .flat_map(|m| {
            (0..STEPS).map(move |r| {
                #[allow(clippy::cast_precision_loss)]
                let denom = (STEPS - 1) as f32;
                #[allow(clippy::cast_precision_loss)]
                RawMaterialData {
                    base_color_factor: [1.0, 1.0, 1.0, 1.0],
                    metallic_factor: m as f32 / denom,
                    roughness_factor: r as f32 / denom,
                    ..Default::default()
                }
            })
        })
        .map(|raw| traced(&raw))
        .collect();

    let Some(harness) = Harness::new(materials) else {
        return;
    };

    // Two outgoing directions, because the deficit is strongly angle dependent:
    // a grazing view sees far more of the masking the single-scattering term
    // throws away.
    for (label, wo) in [
        ("normal incidence", [0.0, 0.0, 1.0]),
        ("oblique", direction(0.35, 0.0)),
    ] {
        println!("furnace, {label}: rows are metalness, columns roughness");
        for m in 0..STEPS {
            let mut row = String::new();
            for r in 0..STEPS {
                #[allow(clippy::cast_possible_truncation)]
                let material = (m * STEPS + r) as u32;
                let albedo = directional_albedo(&harness.sample(material, wo, 0x9E37_79B9));
                let worst = albedo.iter().fold(0.0f32, |a, b| a.max(*b));
                assert!(
                    worst <= ALBEDO_CEILING,
                    "furnace, {label}: metalness {m}/{}, roughness {r}/{} has a \
                     directional albedo of {worst:.4}, which creates energy",
                    STEPS - 1,
                    STEPS - 1
                );
                row.push_str(&format!(" {worst:.3}"));
            }
            println!("  m={m}:{row}");
        }
    }
}

#[test]
fn transmission_is_measured_rather_than_asserted() {
    // The transmission lobe's density does not describe its sampler: the source it
    // is ported from perturbs the macronormal by a uniform sphere offset and then
    // reports the reciprocal of one minus Fresnel, which is a weight above one
    // rather than a density. This test does not assert agreement, because there is
    // none to assert. It records the disagreement, so the number moves when the
    // lobe is replaced by the correct formulation and nobody has to rediscover
    // that it was wrong.
    let raw = RawMaterialData {
        transmission: 1.0,
        roughness_factor: 0.2,
        base_color_factor: [1.0, 1.0, 1.0, 1.0],
        ..Default::default()
    };
    let Some(harness) = Harness::new(vec![traced(&raw)]) else {
        return;
    };

    let wo = [0.0, 0.0, 1.0];
    let samples = harness.sample(0, wo, 0x9E37_79B9);
    let transmitted = samples.iter().filter(|s| s.lobe == 3).count();
    let below = samples
        .iter()
        .filter(|s| s.pdf > 0.0 && s.wi[2] < 0.0)
        .count();
    let albedo = directional_albedo(&samples);

    println!(
        "transmission: {transmitted} of {SAMPLES} samples took the lobe, \
         {below} crossed the surface, directional albedo {:.4} {:.4} {:.4}",
        albedo[0], albedo[1], albedo[2]
    );

    assert!(
        transmitted > 100,
        "the transmission lobe was chosen only {transmitted} times, \
         so this measurement describes nothing"
    );
    assert!(
        below > 0,
        "no sample crossed the surface, so the transmitted direction is not being \
         generated at all and this is a defect rather than the known approximation"
    );
    // The one thing that IS asserted: the approximation may be a poor density, and
    // it still must not manufacture light.
    let worst = albedo.iter().fold(0.0f32, |a, b| a.max(*b));
    assert!(
        worst <= ALBEDO_CEILING,
        "transmission has a directional albedo of {worst:.4}, which creates energy; \
         the approximate density is tolerated, energy creation is not"
    );
}

/// The white furnace as a picture, on the reference's framing.
///
/// `#[ignore]`d, because it is a thing to look at rather than a thing to assert.
/// The numeric sweep above is the gate; this is what shows *where* a deficit sits
/// and what a rough metal or a pane of glass actually looks like, which no scalar
/// does.
///
/// ```text
/// SOLARXY_PT_BSDF_PNG=/tmp/spheres.png cargo test --release -p solarxy-renderer \
///     --test pathtrace_bsdf -- --ignored --nocapture
/// ```
///
/// The framing is deliberately the one the reference's own furnace example uses, so
/// the output is comparable against a known-good picture rather than only against
/// itself: an eleven by eleven grid of white spheres of radius 0.4 at unit spacing,
/// roughness increasing left to right and metalness top to bottom, under a uniform
/// grey environment, through a forty-degree camera at `z = 18`.
///
/// Read it as follows. Under a *uniform* environment an energy-conserving surface
/// returns exactly what reached it and disappears into the background, so every
/// sphere that is visible at all is visible because of a deficit. The lower right
/// darkens: that is rough metal losing the light that would have scattered between
/// microfacets, which this release measures rather than restores. Set
/// `SOLARXY_PT_BSDF_GRADIENT=1` for a two-tone environment instead, which breaks the
/// furnace condition on purpose and is the legible version.
#[test]
#[ignore = "a picture to look at, not a gate; run with --release --ignored"]
fn the_sphere_grid_renders() {
    use solarxy_core::preferences::ProjectionMode;
    use solarxy_renderer::camera::{Camera, CameraUniform};
    use solarxy_renderer::pathtrace::{
        EnvParams, PathEstimator, PathKernel, ReadbackPoll, TraceParams, TraceTarget,
    };

    const GRID: usize = 11;
    const WIDTH: u32 = 900;
    const HEIGHT: u32 = 900;
    /// Samples per pixel. The tile loop below keeps one dispatch short enough that
    /// a driver watchdog does not see a long-running kernel.
    const SPP: u32 = 256;
    const TILE: u32 = 128;
    /// The reference's grey, 204 of 255, decoded out of sRGB because the tracer
    /// works in linear light.
    const GREY: f32 = 0.603_827_4;

    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };

    // One sphere mesh, one placement per cell. This is what the instance path is
    // for, and it also means the hierarchy is built once.
    let (sphere_pos, sphere_idx) = solarxy_bvh::corpus::sphere(64, 32);
    let bvh = Bvh::build_triangles(&sphere_pos, &sphere_idx);

    let mut materials = Vec::with_capacity(GRID * GRID);
    let mut placements = Vec::with_capacity(GRID * GRID);
    let mut boxes = Vec::with_capacity(GRID * GRID);
    #[allow(clippy::cast_precision_loss)]
    for row in 0..GRID {
        for col in 0..GRID {
            let raw = RawMaterialData {
                base_color_factor: [1.0, 1.0, 1.0, 1.0],
                roughness_factor: col as f32 / (GRID - 1) as f32,
                metallic_factor: row as f32 / (GRID - 1) as f32,
                ..Default::default()
            };
            materials.push(traced(&raw));

            let centre = cgmath::Vector3::new(
                col as f32 - (GRID as f32 - 1.0) * 0.5,
                (GRID as f32 - 1.0) * 0.5 - row as f32,
                0.0,
            );
            let world =
                cgmath::Matrix4::from_translation(centre) * cgmath::Matrix4::from_scale(0.4);
            let inv =
                cgmath::Matrix4::from_scale(1.0 / 0.4) * cgmath::Matrix4::from_translation(-centre);
            placements.push(ArenaPlacement {
                mesh: 0,
                world: world.into(),
                inv_world: inv.into(),
                #[allow(clippy::cast_possible_truncation)]
                material_base: (row * GRID + col) as u32,
                flags: INSTANCE_VISIBLE,
            });
            boxes.push(solarxy_core::aabb::AABB {
                min: cgmath::Point3::new(centre.x - 0.4, centre.y - 0.4, -0.4),
                max: cgmath::Point3::new(centre.x + 0.4, centre.y + 0.4, 0.4),
            });
        }
    }

    let tlas = Bvh::build_tlas(&boxes);
    let mesh = ArenaMesh {
        bvh: &bvh,
        positions: &sphere_pos,
        indices: &sphere_idx,
        normals: None,
        uv0: None,
    };
    let arena = TraceArena::build(&tlas, &[mesh], &placements).with_materials(materials);
    let scene = TraceScene::upload(&gpu.device, &gpu.queue, &gpu.pathtrace, &arena);
    let atlas = TraceAtlas::new(&gpu.device, &gpu.queue, &gpu.pathtrace);

    let camera = Camera {
        eye: cgmath::Point3::new(0.0, 0.0, 18.0),
        target: cgmath::Point3::new(0.0, 0.0, 0.0),
        up: cgmath::Vector3::unit_y(),
        #[allow(clippy::cast_precision_loss)]
        aspect: WIDTH as f32 / HEIGHT as f32,
        fovy: 40.0,
        znear: 1.0,
        zfar: 100.0,
        projection: ProjectionMode::Perspective,
        ortho_scale: 1.0,
    };
    let mut camera_uniform = CameraUniform::new();
    camera_uniform.update_view_proj(&camera);
    let camera_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Sphere Grid Camera"),
        size: std::mem::size_of::<CameraUniform>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    gpu.queue
        .write_buffer(&camera_buffer, 0, bytemuck::bytes_of(&camera_uniform));

    let target = TraceTarget::new(&gpu.device, &gpu.pathtrace, WIDTH, HEIGHT);
    let uniforms = solarxy_renderer::pathtrace::PathUniforms::new(&gpu.device, &camera_buffer);
    let kernel = PathKernel::new(&gpu.device, &gpu.pathtrace, &uniforms);

    let gradient = std::env::var("SOLARXY_PT_BSDF_GRADIENT").is_ok();
    let environment = if gradient {
        EnvParams::constant([0.35, 0.45, 0.70], [0.75, 0.72, 0.66])
    } else {
        EnvParams::constant([GREY, GREY, GREY], [GREY, GREY, GREY])
    };

    // Tiled, one dispatch per tile, which is the pacing the tile uniforms exist
    // for and the only reason a 256-sample kernel is safe to run at all.
    let started = std::time::Instant::now();
    for ty in (0..HEIGHT).step_by(TILE as usize) {
        for tx in (0..WIDTH).step_by(TILE as usize) {
            let w = TILE.min(WIDTH - tx);
            let h = TILE.min(HEIGHT - ty);
            uniforms.write(
                &gpu.queue,
                &TraceParams {
                    tile_offset: [tx, ty],
                    tile_size: [w, h],
                    resolution: [WIDTH, HEIGHT],
                    bounces: 12,
                    transmissive_bounces: 6,
                    samples: SPP,
                    seed: 0x9E37_79B9,
                    // The furnace has no lights by construction: the whole
                    // point is a surface lit only by a uniform environment, so
                    // whatever it returns came from the material.
                    light_count: 0,
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
                    label: Some("Sphere Grid Encoder"),
                });
            kernel.encode(
                &mut encoder,
                PathEstimator::Mis,
                &scene,
                &atlas,
                &target,
                &uniforms,
                [w, h],
            );
            gpu.queue.submit(Some(encoder.finish()));
            // Each tile is waited on before the next is written, because the tile
            // uniform is one buffer: queueing two dispatches against it would give
            // both the second tile's rect.
            let _ = gpu.device.poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            });
        }
    }
    println!(
        "sphere grid: {WIDTH}x{HEIGHT} at {SPP} spp in {:.1} s",
        started.elapsed().as_secs_f64()
    );

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Sphere Grid Readback"),
        });
    let mut readback = target.encode_readback(&gpu.device, &mut encoder);
    gpu.queue.submit(Some(encoder.finish()));
    let floats = loop {
        match readback.poll(&gpu.device) {
            ReadbackPoll::Ready(v) => break v,
            ReadbackPoll::Pending => std::thread::yield_now(),
            ReadbackPoll::Failed => panic!("sphere grid readback failed"),
        }
    };

    // The mean is the one number worth printing even without a file: under the
    // furnace configuration it should sit just below the environment, and how far
    // below is the deficit averaged over the whole grid.
    let mean: f64 = floats.chunks_exact(4).map(|p| f64::from(p[0])).sum::<f64>()
        / (f64::from(WIDTH) * f64::from(HEIGHT));
    println!(
        "mean red channel {mean:.4} against an environment of {:.4}",
        if gradient { f64::NAN } else { f64::from(GREY) }
    );

    if let Ok(path) = std::env::var("SOLARXY_PT_BSDF_PNG") {
        // sRGB encode, because this one is meant to be looked at rather than
        // measured. No exposure and no tone map: the composite chain is what does
        // that for a real render, and applying half of it here would misrepresent
        // both.
        let bytes: Vec<u8> = floats
            .chunks_exact(4)
            .flat_map(|p| {
                let encode = |v: f32| {
                    let c = v.clamp(0.0, 1.0);
                    let s = if c <= 0.003_130_8 {
                        c * 12.92
                    } else {
                        1.055 * c.powf(1.0 / 2.4) - 0.055
                    };
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let out = (s * 255.0 + 0.5) as u8;
                    out
                };
                [encode(p[0]), encode(p[1]), encode(p[2]), 255]
            })
            .collect();
        image::RgbaImage::from_raw(WIDTH, HEIGHT, bytes)
            .expect("buffer matches the target size")
            .save(&path)
            .expect("write the sphere grid png");
        println!("wrote {path}");
    }
}
