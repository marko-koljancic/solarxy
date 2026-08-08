//! The material record on a real device: what Rust wrote against what the
//! kernel read.
//!
//! The record's own tests assert its size and every field's offset, and
//! `uniform_layout.rs` asserts the WGSL declaration measures the same total.
//! Neither reaches the failure this file exists for. Nine sixteen-byte blocks
//! are nine chances to transpose two of them; a transposition changes no size,
//! satisfies both guards, and shades wrong in a way that reads as a lighting
//! bug. The only instrument that can settle it is a shader that echoes the
//! record back.
//!
//! So the probe writes every scalar in declaration order and this file compares
//! the echo against the struct it built. The second half checks the taps: a
//! material with textures, sampled through the real atlas, so the factor-times-
//! texture arithmetic and the descriptor's presence bit are exercised rather
//! than assumed.

mod common;

use solarxy_bvh::Bvh;
use solarxy_core::RawImageData;
use solarxy_core::geometry::{AlphaMode, RawMaterialData, ShadingModel};
use solarxy_renderer::pathtrace::arena::{ArenaMesh, ArenaPlacement, INSTANCE_VISIBLE, TraceArena};
use solarxy_renderer::pathtrace::atlas::{
    AtlasFilter, AtlasPlan, AtlasTexture, AtlasWrap, TEXTURE_UNUSED, TextureKey,
};
use solarxy_renderer::pathtrace::material::{
    FLAG_ALPHA_MODE_MASK, FLAG_SHADING_MODEL_MASK, FLAG_SHADING_MODEL_SHIFT, TracedMaterial,
};
use solarxy_renderer::pathtrace::probe::{ColorPoll, MATERIAL_RESULT_WIDTH, MaterialProbe, MaterialTap};
use solarxy_renderer::pathtrace::scene::{MaterialTextures, TextureSlot};
use solarxy_renderer::pathtrace::{TraceAtlas, TraceScene};

/// How far a sampled value may sit from its expected one. The atlas is
/// `rgba8unorm`, so a channel carries about 1/255; every assertion below
/// separates values by far more than that.
const EPS: f32 = 0.01;

/// Runs `taps` against `materials` and `textures`, returning the flat readout.
///
/// The arena carries one triangle because an empty one is not a scene: the
/// material pool rides the same buffer set the geometry does, and binding it
/// through the real scene group is the whole point.
fn run(
    materials: Vec<TracedMaterial>,
    textures: &[(RawImageData, AtlasWrap)],
    taps: &[MaterialTap],
) -> Option<Vec<[f32; 4]>> {
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

    let packed: Vec<AtlasTexture> = textures
        .iter()
        .map(|(image, wrap)| AtlasTexture {
            key: TextureKey {
                hash: image.hash,
                wrap_s: *wrap,
                wrap_t: *wrap,
            },
            image: std::sync::Arc::new(image.clone()),
        })
        .collect();
    let plan = AtlasPlan::pack_textures(&packed);
    let mut atlas = TraceAtlas::new(&gpu.device, &gpu.pathtrace);
    atlas.sync(&gpu.device, &gpu.queue, &gpu.pathtrace, &plan, &packed);

    let probe = MaterialProbe::new(&gpu.device, &gpu.pathtrace);
    let mut readback = probe.submit(&gpu.device, &gpu.queue, &scene, &atlas, taps);
    for _ in 0..1000 {
        match readback.poll(&gpu.device) {
            ColorPoll::Ready(values) => return Some(values),
            ColorPoll::Pending => std::thread::sleep(std::time::Duration::from_millis(1)),
            ColorPoll::Failed => panic!("material readback failed"),
        }
    }
    panic!("material readback never resolved");
}

fn tap(material: u32, uv: [f32; 2]) -> MaterialTap {
    MaterialTap {
        uv: [uv[0], uv[1], 0.0, 0.0],
        material,
        _pad: [0; 3],
    }
}

/// One tap's slice of the readout.
fn block(values: &[[f32; 4]], tap_index: usize, block_index: usize) -> [f32; 4] {
    values[tap_index * MATERIAL_RESULT_WIDTH + block_index]
}

fn close(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() <= EPS)
}

/// A material with every scalar set away from its default and away from every
/// other, so a transposed pair of blocks cannot read as correct by coincidence.
fn distinct() -> RawMaterialData {
    RawMaterialData {
        base_color_factor: [0.11, 0.12, 0.13, 0.14],
        emissive_factor: [0.21, 0.22, 0.23],
        emissive_strength: 1.0,
        attenuation_color: [0.31, 0.32, 0.33],
        attenuation_distance: 0.34,
        sheen_color: [0.41, 0.42, 0.43],
        sheen_roughness: 0.44,
        specular_color: [0.51, 0.52, 0.53],
        specular_intensity: 0.54,
        metallic_factor: 0.61,
        roughness_factor: 0.62,
        ior: 0.63,
        transmission: 0.64,
        thickness: 0.71,
        clearcoat: 0.72,
        clearcoat_roughness: 0.73,
        anisotropy: 0.74,
        anisotropy_rotation: 0.81,
        iridescence: 0.82,
        iridescence_ior: 0.83,
        iridescence_thickness_min: 0.84,
        iridescence_thickness_max: 0.91,
        occlusion_strength: 0.92,
        alpha_cutoff: 0.93,
        alpha_mode: AlphaMode::Blend,
        shading_model: ShadingModel::Toon,
        ..RawMaterialData::default()
    }
}

/// An image whose four channels are constant and distinguishable.
fn solid(rgba: [u8; 4]) -> RawImageData {
    let pixels = rgba.iter().copied().cycle().take(16 * 16 * 4).collect();
    RawImageData::new(pixels, 16, 16)
}

#[test]
fn the_kernel_reads_every_scalar_where_rust_wrote_it() {
    let mat = distinct();
    let record = TracedMaterial::from_raw(&mat, &MaterialTextures::default());
    let Some(values) = run(vec![record], &[], &[tap(0, [0.0, 0.0])]) else {
        return;
    };

    assert!(close(block(&values, 0, 0), mat.base_color_factor));
    assert!(close(
        block(&values, 0, 1),
        [
            mat.emissive_factor[0],
            mat.emissive_factor[1],
            mat.emissive_factor[2],
            mat.emissive_strength,
        ]
    ));
    assert!(close(
        block(&values, 0, 2),
        [
            mat.attenuation_color[0],
            mat.attenuation_color[1],
            mat.attenuation_color[2],
            mat.attenuation_distance,
        ]
    ));
    assert!(close(
        block(&values, 0, 3),
        [
            mat.sheen_color[0],
            mat.sheen_color[1],
            mat.sheen_color[2],
            mat.sheen_roughness,
        ]
    ));
    assert!(close(
        block(&values, 0, 4),
        [
            mat.specular_color[0],
            mat.specular_color[1],
            mat.specular_color[2],
            mat.specular_intensity,
        ]
    ));
    assert!(close(
        block(&values, 0, 5),
        [
            mat.metallic_factor,
            mat.roughness_factor,
            mat.ior,
            mat.transmission,
        ]
    ));
    assert!(close(
        block(&values, 0, 6),
        [
            mat.thickness,
            mat.clearcoat,
            mat.clearcoat_roughness,
            mat.anisotropy,
        ]
    ));
    assert!(close(
        block(&values, 0, 7),
        [
            mat.anisotropy_rotation,
            mat.iridescence,
            mat.iridescence_ior,
            mat.iridescence_thickness_min,
        ]
    ));

    // The eighth block's fourth lane is the flags word's bits, not a number.
    let tail = block(&values, 0, 8);
    assert!((tail[0] - mat.iridescence_thickness_max).abs() <= EPS);
    assert!((tail[1] - mat.occlusion_strength).abs() <= EPS);
    assert!((tail[2] - mat.alpha_cutoff).abs() <= EPS);
    let flags = tail[3].to_bits();
    assert_eq!(flags & FLAG_ALPHA_MODE_MASK, u32::from(AlphaMode::Blend));
    assert_eq!(
        (flags >> FLAG_SHADING_MODEL_SHIFT) & FLAG_SHADING_MODEL_MASK,
        u32::from(ShadingModel::Toon)
    );
}

#[test]
fn an_untextured_material_resolves_to_its_factors_alone() {
    let mut mat = distinct();
    mat.emissive_strength = 2.0;
    let record = TracedMaterial::from_raw(&mat, &MaterialTextures::default());
    let Some(values) = run(vec![record], &[], &[tap(0, [0.37, 0.61])]) else {
        return;
    };

    assert!(close(block(&values, 0, 9), mat.base_color_factor));
    let surface = block(&values, 0, 10);
    assert!((surface[0] - mat.metallic_factor).abs() <= EPS, "metallic");
    assert!(
        (surface[1] - mat.roughness_factor).abs() <= EPS,
        "roughness"
    );
    // No occlusion map, so occlusion is one whatever the strength says.
    assert!((surface[2] - 1.0).abs() <= EPS, "occlusion");
    assert_eq!(surface[3], 0.0, "no normal map");

    // Emissive is the factor times the strength, and the strength applies with
    // or without a map.
    let emissive = block(&values, 0, 11);
    for (i, (got, factor)) in emissive.iter().zip(mat.emissive_factor.iter()).enumerate() {
        assert!((got - factor * 2.0).abs() <= EPS, "emissive {i}");
    }
}

#[test]
fn a_textured_material_multiplies_its_factors_by_its_taps() {
    // Distinct constants per role, so a slot read through the wrong descriptor
    // shows up as the wrong number rather than as a plausible one. Base colour
    // is sRGB and the rest are data, which is the split the arranger makes.
    let base = solid([255, 128, 64, 255]);
    let mr = solid([0, 128, 64, 255]);
    let ao = solid([32, 0, 0, 255]);
    let images = [
        (base.clone(), AtlasWrap::Repeat),
        (mr.clone(), AtlasWrap::Repeat),
        (ao.clone(), AtlasWrap::Repeat),
    ];

    let packed: Vec<AtlasTexture> = images
        .iter()
        .map(|(image, wrap)| AtlasTexture {
            key: TextureKey {
                hash: image.hash,
                wrap_s: *wrap,
                wrap_t: *wrap,
            },
            image: std::sync::Arc::new(image.clone()),
        })
        .collect();
    let plan = AtlasPlan::pack_textures(&packed);

    let mut textures = MaterialTextures::default();
    for (slot_index, (image, wrap)) in [(0usize, &base), (2, &mr), (3, &ao)]
        .into_iter()
        .zip([AtlasWrap::Repeat; 3])
        .map(|((i, img), w)| (i, (img, w)))
    {
        let key = TextureKey {
            hash: image.hash,
            wrap_s: wrap,
            wrap_t: wrap,
        };
        let srgb = slot_index == 0;
        textures.slots[slot_index] = TextureSlot {
            desc: plan.descriptor(key, 0, AtlasFilter::Linear, srgb),
            rect: plan.rect(key).expect("packed"),
        };
    }

    let mut mat = RawMaterialData {
        base_color_factor: [1.0, 1.0, 1.0, 1.0],
        metallic_factor: 1.0,
        roughness_factor: 1.0,
        occlusion_strength: 1.0,
        ..RawMaterialData::default()
    };
    mat.emissive_factor = [0.0; 3];
    let record = TracedMaterial::from_raw(&mat, &textures);

    // A texel centre, so the bilinear tap is unambiguous.
    let Some(values) = run(vec![record], &images, &[tap(0, [0.5, 0.5])]) else {
        return;
    };

    // Base colour: the tap decoded from sRGB, times a unit factor.
    let expected_r = ((255.0f32 / 255.0 + 0.055) / 1.055).powf(2.4);
    let expected_g = ((128.0f32 / 255.0 + 0.055) / 1.055).powf(2.4);
    let resolved = block(&values, 0, 9);
    assert!((resolved[0] - expected_r).abs() <= EPS, "base colour red");
    assert!((resolved[1] - expected_g).abs() <= EPS, "base colour green");
    assert!((resolved[3] - 1.0).abs() <= EPS, "base colour alpha");

    // Metallic is the map's blue and roughness its green, glTF's assignment and
    // the raster path's. Data roles are not sRGB, so these are the raw values.
    let surface = block(&values, 0, 10);
    assert!(
        (surface[0] - 64.0 / 255.0).abs() <= EPS,
        "metallic from blue"
    );
    assert!(
        (surface[1] - 128.0 / 255.0).abs() <= EPS,
        "roughness from green"
    );
    // Occlusion is the map's red at full strength.
    assert!(
        (surface[2] - 32.0 / 255.0).abs() <= EPS,
        "occlusion from red"
    );
    assert_eq!(surface[3], 0.0, "still no normal map");
}

/// The fallback is what an uncovered slot and an empty scene both get, so the
/// kernel has to read it as a usable surface rather than as a hole.
#[test]
fn the_fallback_record_reads_back_as_a_grey_surface() {
    let Some(values) = run(vec![TracedMaterial::fallback()], &[], &[tap(0, [0.5, 0.5])]) else {
        return;
    };

    let resolved = block(&values, 0, 9);
    for i in 0..3 {
        assert!(
            (resolved[i] - resolved[0]).abs() <= EPS,
            "the fallback is grey"
        );
    }
    assert!(resolved[0] > 0.5 && resolved[0] < 0.7, "the raster clay");
    assert!((resolved[3] - 1.0).abs() <= EPS, "opaque");
    let surface = block(&values, 0, 10);
    assert!(surface[1] > 0.0, "not a mirror");
    assert_eq!(surface[3], 0.0, "no normal map");
}

/// A zero descriptor is a legal one naming layer zero, which is why an empty
/// slot is written explicitly. This asserts the shader agrees: the unused flag
/// is what suppresses a tap, not a zero.
#[test]
fn an_unused_descriptor_suppresses_its_tap() {
    let mut mat = RawMaterialData {
        base_color_factor: [0.25, 0.5, 0.75, 1.0],
        ..RawMaterialData::default()
    };
    mat.emissive_factor = [0.0; 3];
    let mut record = TracedMaterial::from_raw(&mat, &MaterialTextures::default());
    // A rectangle that would sample somewhere real if the descriptor were read.
    record.tex_rect[0] = [1.0, 1.0, 0.0, 0.0];
    assert_eq!(record.tex_desc[0], TEXTURE_UNUSED);

    let Some(values) = run(
        vec![record],
        &[(solid([0, 0, 0, 0]), AtlasWrap::Repeat)],
        &[tap(0, [0.5, 0.5])],
    ) else {
        return;
    };

    // Had the tap run, the base colour would be multiplied by transparent black.
    assert!(close(block(&values, 0, 9), mat.base_color_factor));
}

/// Several materials in one pool, read by index, which is what
/// `Instance::material_base` will do.
#[test]
fn each_tap_reads_the_material_its_index_names() {
    let colours = [
        [1.0f32, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 1.0],
        [0.0, 0.0, 1.0, 1.0],
    ];
    let pool: Vec<TracedMaterial> = colours
        .iter()
        .map(|c| {
            let mut mat = RawMaterialData {
                base_color_factor: *c,
                ..RawMaterialData::default()
            };
            mat.emissive_factor = [0.0; 3];
            TracedMaterial::from_raw(&mat, &MaterialTextures::default())
        })
        .collect();
    let taps: Vec<MaterialTap> = (0..3).map(|i| tap(i, [0.0, 0.0])).collect();

    let Some(values) = run(pool, &[], &taps) else {
        return;
    };

    for (i, colour) in colours.iter().enumerate() {
        assert!(close(block(&values, i, 0), *colour), "material {i}");
    }
}
