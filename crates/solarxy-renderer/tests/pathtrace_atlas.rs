//! The atlas on a real device: what the packer arranged against what the
//! kernel reads back.
//!
//! The packer's own tests cover the arithmetic and never touch a GPU. What they
//! cannot cover is the reason the guard ring exists at all: a bilinear tap at
//! the extreme edge of a sub-rectangle reaches half a texel past it, and
//! whether that lands on the texture's own border or on whatever was packed
//! beside it is decided by hardware interpolation. So the check here is a
//! deliberately hostile arrangement, two solid-colour textures packed adjacent,
//! sampled at every edge of one of them, asserting the other's colour never
//! appears.
//!
//! The rest is the same shape: the shader's wrap arithmetic against the CPU's,
//! and its sRGB decode against the transfer function the raster path gets from
//! the texture format.

mod common;

use solarxy_core::RawImageData;
use solarxy_renderer::pathtrace::TraceAtlas;
use solarxy_renderer::pathtrace::atlas::{AtlasFilter, AtlasPlan, AtlasTexture, AtlasWrap, TextureKey};
use solarxy_renderer::pathtrace::probe::{AtlasProbe, AtlasTap, ColorPoll};

/// How far a sample may sit from its expected value.
///
/// Generous against the value and tight against the mistake: every assertion
/// below separates colours that differ by a whole channel, so a tolerance this
/// wide still fails the moment a neighbour bleeds in.
const EPS: f32 = 0.02;

fn solid(width: u32, height: u32, rgba: [u8; 4]) -> RawImageData {
    let pixels = rgba
        .iter()
        .copied()
        .cycle()
        .take((width * height * 4) as usize)
        .collect();
    RawImageData::new(pixels, width, height)
}

/// An image whose red channel ramps left to right and green top to bottom, so
/// a sample names the position it came from.
fn ramp(width: u32, height: u32) -> RawImageData {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let r = (x * 255 / (width - 1).max(1)) as u8;
            let g = (y * 255 / (height - 1).max(1)) as u8;
            pixels.extend_from_slice(&[r, g, 0, 255]);
        }
    }
    RawImageData::new(pixels, width, height)
}

fn key(image: &RawImageData, wrap: AtlasWrap) -> TextureKey {
    TextureKey {
        hash: image.hash,
        wrap_s: wrap,
        wrap_t: wrap,
    }
}

/// Uploads `images` and samples `taps`, returning one linear RGBA per tap.
fn run(
    images: &[(RawImageData, AtlasWrap)],
    taps_for: impl Fn(&AtlasPlan) -> Vec<AtlasTap>,
) -> Option<Vec<[f32; 4]>> {
    let gpu = common::gpu_or_skip()?;
    let textures: Vec<AtlasTexture> = images
        .iter()
        .map(|(image, wrap)| AtlasTexture {
            key: key(image, *wrap),
            image: std::sync::Arc::new(image.clone()),
        })
        .collect();
    let plan = AtlasPlan::pack_textures(&textures);

    let mut atlas = TraceAtlas::new(&gpu.device, &gpu.queue, &gpu.pathtrace);
    atlas.sync(&gpu.device, &gpu.queue, &gpu.pathtrace, &plan, &textures);

    let taps = taps_for(&plan);
    let probe = AtlasProbe::new(&gpu.device, &gpu.pathtrace);
    let mut readback = probe.submit(&gpu.device, &gpu.queue, &atlas, &taps);
    for _ in 0..1000 {
        match readback.poll(&gpu.device) {
            ColorPoll::Ready(colors) => return Some(colors),
            ColorPoll::Pending => std::thread::sleep(std::time::Duration::from_millis(1)),
            ColorPoll::Failed => panic!("atlas readback failed"),
        }
    }
    panic!("atlas readback never resolved");
}

fn close(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() <= EPS)
}

#[test]
fn a_guard_border_keeps_a_neighbour_out_of_an_edge_sample() {
    // Four 64-square solids of four distinct colours. In the 256 page the
    // packer picks, three sit in a row and the fourth wraps beneath the first,
    // so every texture has a neighbour on at least one axis and the first has
    // one on both.
    //
    // Clamped rather than tiled, deliberately: under Repeat, `fract(1.0)` is
    // zero, so a tap at the far edge silently becomes a tap at the near one and
    // the interesting boundary is never reached. Clamp maps zero and one onto
    // the rectangle's two extremes, which is where a bilinear tap reaches half
    // a texel past the rectangle and finds either its own border or the
    // neighbour.
    let colours: [[u8; 4]; 4] = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 0, 255],
    ];
    let images: Vec<(RawImageData, AtlasWrap)> = colours
        .iter()
        .map(|c| (solid(64, 64, *c), AtlasWrap::Clamp))
        .collect();
    let keys: Vec<TextureKey> = images.iter().map(|(i, w)| key(i, *w)).collect();

    // Every edge and corner of every texture, in texture order.
    let edges = [
        [0.0f32, 0.5],
        [1.0, 0.5],
        [0.5, 0.0],
        [0.5, 1.0],
        [0.0, 0.0],
        [1.0, 0.0],
        [0.0, 1.0],
        [1.0, 1.0],
    ];
    let sampled = keys.clone();
    let Some(colors) = run(&images, move |plan| {
        assert_eq!(plan.page(), 256, "the arrangement this test relies on");
        assert_eq!(plan.layers(), 1);
        sampled
            .iter()
            .flat_map(|k| {
                let rect = plan.rect(*k).expect("packed");
                let desc = plan.descriptor(*k, 0, AtlasFilter::Linear, false);
                edges.into_iter().map(move |uv| AtlasTap {
                    rect,
                    uv,
                    desc,
                    _pad: 0,
                })
            })
            .collect()
    }) else {
        return;
    };

    for (t, colour) in colours.iter().enumerate() {
        let expected = colour.map(|c| f32::from(c) / 255.0);
        for (e, c) in colors[t * edges.len()..(t + 1) * edges.len()]
            .iter()
            .enumerate()
        {
            assert!(
                close(*c, expected),
                "texture {t} edge {e} sampled {c:?}, expected {expected:?}: \
                 a neighbour bled through the guard ring"
            );
        }
    }
}

#[test]
fn the_shader_wraps_a_coordinate_the_way_the_packer_assumed() {
    // A ramp sampled past both ends of the unit square. Under Repeat the two
    // must agree with their in-range equivalents, which is the property that
    // makes a tiled texture tile at all.
    let image = ramp(64, 64);
    let tiled = key(&image, AtlasWrap::Repeat);
    let Some(colors) = run(&[(image, AtlasWrap::Repeat)], |plan| {
        let rect = plan.rect(tiled).expect("packed");
        let desc = plan.descriptor(tiled, 0, AtlasFilter::Nearest, false);
        [[0.25, 0.25], [1.25, 0.25], [-0.75, 0.25], [2.25, 0.25]]
            .into_iter()
            .map(|uv| AtlasTap {
                rect,
                uv,
                desc,
                _pad: 0,
            })
            .collect()
    }) else {
        return;
    };

    for (i, c) in colors.iter().enumerate().skip(1) {
        assert!(
            close(*c, colors[0]),
            "tap {i} sampled {c:?}, not the wrapped equivalent {:?}",
            colors[0]
        );
    }
    // And the sample is actually the quarter-way texel, not a constant.
    assert!((colors[0][0] - 0.25).abs() < 0.05, "{:?}", colors[0]);
}

#[test]
fn a_clamped_coordinate_stops_at_the_edge_instead_of_tiling() {
    let image = ramp(64, 64);
    let clamped = key(&image, AtlasWrap::Clamp);
    let Some(colors) = run(&[(image, AtlasWrap::Clamp)], |plan| {
        let rect = plan.rect(clamped).expect("packed");
        let desc = plan.descriptor(clamped, 0, AtlasFilter::Nearest, false);
        [[-0.5, 0.5], [1.5, 0.5]]
            .into_iter()
            .map(|uv| AtlasTap {
                rect,
                uv,
                desc,
                _pad: 0,
            })
            .collect()
    }) else {
        return;
    };
    assert!(colors[0][0] < 0.05, "left clamp sampled {:?}", colors[0]);
    assert!(colors[1][0] > 0.95, "right clamp sampled {:?}", colors[1]);
}

#[test]
fn the_srgb_flag_decodes_and_its_absence_does_not() {
    // 188 is 0.7373 stored, which is 0.4969 linear. Far enough from the stored
    // value that a missing decode cannot pass as rounding.
    let image = solid(16, 16, [188, 188, 188, 255]);
    let k = key(&image, AtlasWrap::Repeat);
    let Some(colors) = run(&[(image, AtlasWrap::Repeat)], |plan| {
        let rect = plan.rect(k).expect("packed");
        vec![
            AtlasTap {
                rect,
                uv: [0.5, 0.5],
                desc: plan.descriptor(k, 0, AtlasFilter::Nearest, true),
                _pad: 0,
            },
            AtlasTap {
                rect,
                uv: [0.5, 0.5],
                desc: plan.descriptor(k, 0, AtlasFilter::Nearest, false),
                _pad: 0,
            },
        ]
    }) else {
        return;
    };
    assert!(
        (colors[0][0] - 0.4969).abs() < EPS,
        "decoded {:?}",
        colors[0]
    );
    assert!((colors[1][0] - 0.7373).abs() < EPS, "raw {:?}", colors[1]);
    // Alpha is not a colour channel and must survive the decode untouched.
    assert!((colors[0][3] - 1.0).abs() < EPS);
}

#[test]
fn one_image_serves_a_colour_slot_and_a_data_slot_from_one_packing() {
    // The dedupe the raster path's cache cannot make, because it keys on the
    // colour space and this does not: sRGB is a flag the kernel applies, so the
    // texels are the same texels.
    let image = solid(32, 32, [188, 0, 0, 255]);
    let k = key(&image, AtlasWrap::Repeat);
    let textures = [AtlasTexture {
        key: k,
        image: std::sync::Arc::new(image),
    }];
    let plan = AtlasPlan::pack_textures(&textures);
    assert_eq!(plan.entries().len(), 1);

    let colour = plan.descriptor(k, 0, AtlasFilter::Linear, true);
    let data = plan.descriptor(k, 0, AtlasFilter::Linear, false);
    assert_ne!(colour, data);
    // Same layer and same rectangle: only the transfer function differs.
    assert_eq!(colour & 0xFF, data & 0xFF);
}

#[test]
fn an_untextured_scene_binds_a_null_atlas_and_samples_nothing() {
    // The empty case is reached in the ordinary course of editing, so it has to
    // be a real texture: a pipeline layout is satisfied by a bind group or by
    // nothing at all.
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };
    let atlas = TraceAtlas::new(&gpu.device, &gpu.queue, &gpu.pathtrace);
    assert_eq!(atlas.page(), 1);
    assert_eq!(atlas.layers(), 1);

    let plan = AtlasPlan::pack_textures(&[]);
    let probe = AtlasProbe::new(&gpu.device, &gpu.pathtrace);
    let taps = vec![AtlasTap {
        rect: [1.0, 1.0, 0.0, 0.0],
        uv: [0.5, 0.5],
        desc: plan.descriptor(
            TextureKey {
                hash: 1,
                wrap_s: AtlasWrap::Repeat,
                wrap_t: AtlasWrap::Repeat,
            },
            0,
            AtlasFilter::Linear,
            true,
        ),
        _pad: 0,
    }];
    let mut readback = probe.submit(&gpu.device, &gpu.queue, &atlas, &taps);
    for _ in 0..1000 {
        match readback.poll(&gpu.device) {
            ColorPoll::Ready(colors) => {
                assert_eq!(colors[0], [0.0, 0.0, 0.0, 0.0]);
                return;
            }
            ColorPoll::Pending => std::thread::sleep(std::time::Duration::from_millis(1)),
            ColorPoll::Failed => panic!("atlas readback failed"),
        }
    }
    panic!("atlas readback never resolved");
}
