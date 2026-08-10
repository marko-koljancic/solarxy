//! Asserts every hand-mirrored uniform struct has the SAME SIZE in WGSL as
//! it does in Rust.
//!
//! This exists because of a real bug that shipped. `LabelParams` gained a
//! `chip_on: u32` and a trailing 3-word pad; the pad was written
//! `_pad: vec3<u32>`, which has 16-byte ALIGNMENT in WGSL and so pushed the
//! struct to 112 bytes against the Rust struct's 96. Every symptom of that
//! was invisible to the tests that existed:
//!
//! - The Rust-side `const _: () = assert!(size_of == 96)` passed. It cannot
//!   see the shader.
//! - The bind-group layouts use `min_binding_size: None`, so nothing failed
//!   when the bind group was built.
//! - The failure landed at DRAW time, invalidating the render-pass encoder.
//!   Because a pane's encoder also carries the composite pass, the whole
//!   frame was discarded and the viewport went black -- with no Rust error,
//!   no panic, and no failing test.
//!
//! Naga is the same layouter wgpu itself uses to validate the binding, so
//! this test computes the exact number the GPU would have compared against.
//! It needs no adapter and runs on every `cargo test`.
//!
//! Adding a uniform: put it in `CASES` below. The cost is one line and it
//! buys you the whole class of "the shader and the struct disagree".
//!
//! What it does NOT buy: field order. A same-size transposition passes both
//! this guard and the Rust assert; that class is pinned by the golden
//! captures instead, which is one of the reasons the principled scene is in
//! the capture set.

use wgpu::naga;

/// One WGSL struct to check, and the Rust size it must match.
struct Case {
    /// Shader source, relative to `src/shaders/`.
    shader: &'static str,
    /// Sources prepended before `shader` so it parses.
    ///
    /// Empty for every pass that owns a whole shader. The path tracer's kernels
    /// are fragments composed by `concat!`, because WGSL has no include
    /// mechanism and its traversal has to be one text shared by several
    /// kernels and by the test that pins it to a CPU twin. A fragment that
    /// names the traversal's types does not parse alone, so the case has to
    /// name what the host composes it over.
    prelude: &'static [&'static str],
    /// The `struct` name inside that shader.
    struct_name: &'static str,
    /// `std::mem::size_of` of the Rust type it mirrors.
    rust_size: usize,
    /// The Rust type's path, for the failure message.
    rust_type: &'static str,
}

/// The uniforms whose Rust and WGSL definitions are maintained by hand.
///
/// Note the deliberate asymmetry with the repo's stated convention: a WGSL
/// struct may declare a PREFIX of the CPU struct (wgpu enforces size at the
/// binding, not shape), and several shaders rely on that. Those are not
/// listed here, because a prefix is legitimately smaller. Only structs that
/// declare the WHOLE CPU struct belong in this table.
const CASES: &[Case] = &[
    Case {
        shader: "label.wgsl",
        prelude: &[],
        struct_name: "LabelParams",
        rust_size: solarxy_renderer::labels::LABEL_PARAMS_SIZE,
        rust_type: "solarxy_renderer::labels::LabelParams",
    },
    // `shader.wgsl` declares this one whole, including its trailing
    // scalar, so it belongs here. It earned its place when that trailing
    // slot stopped being padding and became the environment's IBL
    // intensity: a rename on one side and not the other would put a real
    // value where the shader expected nothing, and the Rust-side size
    // assert cannot see the shader.
    Case {
        shader: "shader.wgsl",
        prelude: &[],
        struct_name: "LightsUniform",
        rust_size: std::mem::size_of::<solarxy_renderer::light::LightsUniform>(),
        rust_type: "solarxy_renderer::light::LightsUniform",
    },
    // `shader.wgsl` declares this one whole too. It earned its place when
    // the principled surface properties appended six vec4-shaped blocks:
    // three of them carry a `vec3`, which WGSL aligns to 16 bytes in the
    // uniform address space while Rust aligns `[f32; 3]` to 4. Get the
    // pairing wrong and the Rust-side size assert still passes, the shader
    // still compiles, and the viewport goes black at draw time. Nothing
    // else in the build compares the two sides.
    Case {
        shader: "shader.wgsl",
        prelude: &[],
        struct_name: "MaterialUniform",
        rust_size: std::mem::size_of::<solarxy_renderer::material::MaterialUniform>(),
        rust_type: "solarxy_renderer::material::MaterialUniform",
    },
    // `composite.wgsl` declares this one whole. It was absent while the
    // struct was eight scalars and nothing could misalign; the colour
    // grade appended seven `vec3`-carrying blocks and made it exactly the
    // shape the two entries above are here for. The composite pass is also
    // the worst place to get this wrong: it is the last pass in every
    // pane's encoder, so a rejected binding discards the whole frame.
    Case {
        shader: "composite.wgsl",
        prelude: &[],
        struct_name: "CompositeParams",
        rust_size: std::mem::size_of::<solarxy_renderer::composite::CompositeParams>(),
        rust_type: "solarxy_renderer::composite::CompositeParams",
    },
    // The path tracer's records. These are storage-buffer structs rather than
    // uniforms, and they belong here for the same reason: nothing else in the
    // build compares the two sides, and the arithmetic that reads them is
    // index arithmetic, so a stride that disagrees does not fail, it reads the
    // wrong bytes and traces a plausible image of nothing.
    //
    // `BvhNode` is the one that most needs watching. Its fourth field is named
    // `meta` in Rust and `packed` in WGSL, because `meta` is a WGSL reserved
    // keyword; two names for the same four bytes, held together by nothing
    // except this row.
    Case {
        shader: "pathtrace/traverse.wgsl",
        prelude: &[],
        struct_name: "BvhNode",
        rust_size: std::mem::size_of::<solarxy_bvh::BvhNode>(),
        rust_type: "solarxy_bvh::BvhNode",
    },
    Case {
        shader: "pathtrace/traverse.wgsl",
        prelude: &[],
        struct_name: "Instance",
        rust_size: std::mem::size_of::<solarxy_renderer::pathtrace::arena::Instance>(),
        rust_type: "solarxy_renderer::pathtrace::arena::Instance",
    },
    Case {
        shader: "pathtrace/traverse.wgsl",
        prelude: &[],
        struct_name: "VertexAttr",
        rust_size: std::mem::size_of::<solarxy_renderer::pathtrace::arena::VertexAttr>(),
        rust_type: "solarxy_renderer::pathtrace::arena::VertexAttr",
    },
    // `TracedMaterial` is the widest of them and the one where a same-size
    // transposition is most plausible: nine sixteen-byte blocks, five of which
    // are a colour and a scalar. This row buys the total only. Field order is
    // pinned on the Rust side by `record_offsets_are_the_documented_ones` and
    // across the boundary by `tests/pathtrace_material.rs`, which reads a record
    // back through the real binding.
    Case {
        shader: "pathtrace/traverse.wgsl",
        prelude: &[],
        struct_name: "TracedMaterial",
        rust_size: std::mem::size_of::<solarxy_renderer::pathtrace::material::TracedMaterial>(),
        rust_type: "solarxy_renderer::pathtrace::material::TracedMaterial",
    },
    // `camera.wgsl` is a fragment: it returns the traversal's `Ray`, so it only
    // parses composed over it, the same way the host builds it. The struct lived
    // in `trace.wgsl` until a second kernel needed a camera ray.
    Case {
        shader: "pathtrace/camera.wgsl",
        prelude: &["pathtrace/traverse.wgsl"],
        struct_name: "TraceParams",
        rust_size: std::mem::size_of::<solarxy_renderer::pathtrace::TraceParams>(),
        rust_type: "solarxy_renderer::pathtrace::TraceParams",
    },
    // The light record. Six sixteen-byte blocks, each a vec3f plus a scalar, so
    // it needs no pad on either side -- which is exactly the shape that stops
    // being true the moment someone promotes one of those scalars to a vector
    // or inserts a scalar between two vectors.
    Case {
        shader: "pathtrace/traverse.wgsl",
        prelude: &[],
        struct_name: "TracedLight",
        rust_size: std::mem::size_of::<solarxy_renderer::pathtrace::light::TracedLight>(),
        rust_type: "solarxy_renderer::pathtrace::light::TracedLight",
    },
    Case {
        shader: "pathtrace/parity.wgsl",
        prelude: &["pathtrace/traverse.wgsl"],
        struct_name: "CorpusRay",
        rust_size: std::mem::size_of::<solarxy_renderer::pathtrace::probe::CorpusRay>(),
        rust_type: "solarxy_renderer::pathtrace::probe::CorpusRay",
    },
    Case {
        shader: "pathtrace/parity.wgsl",
        prelude: &["pathtrace/traverse.wgsl"],
        struct_name: "CorpusHit",
        rust_size: std::mem::size_of::<solarxy_renderer::pathtrace::probe::CorpusHit>(),
        rust_type: "solarxy_renderer::pathtrace::probe::CorpusHit",
    },
    // A probe's request struct crosses the boundary like anything else, and this
    // row is here because the omission bit immediately: the material probe's tap
    // was written with a `vec3u` tail, which WGSL aligns to 16, so it measured 48
    // against the Rust twin's 32 and every dispatch failed validation. That was a
    // loud failure. The same mistake inside a record the kernel indexes is a
    // silent one.
    Case {
        shader: "pathtrace/material_probe.wgsl",
        prelude: &[
            "pathtrace/traverse.wgsl",
            "pathtrace/atlas.wgsl",
            "pathtrace/material.wgsl",
        ],
        struct_name: "MaterialTap",
        rust_size: std::mem::size_of::<solarxy_renderer::pathtrace::probe::MaterialTap>(),
        rust_type: "solarxy_renderer::pathtrace::probe::MaterialTap",
    },
    // The BSDF probe's tap, for the same reason. Its scalar tail packs into the
    // third sixteen-byte block and needs no pad, which is a shape that only stays
    // true while all four fields are scalars: promoting any of them to a vector
    // would move the struct to 64 on the WGSL side and leave Rust at 48.
    Case {
        shader: "pathtrace/bsdf_probe.wgsl",
        prelude: &[
            "pathtrace/traverse.wgsl",
            "pathtrace/atlas.wgsl",
            "pathtrace/material.wgsl",
            "pathtrace/rand.wgsl",
            "pathtrace/bsdf.wgsl",
        ],
        struct_name: "BsdfTap",
        rust_size: std::mem::size_of::<solarxy_renderer::pathtrace::probe::BsdfTap>(),
        rust_type: "solarxy_renderer::pathtrace::probe::BsdfTap",
    },
    // The light probe's tap, on the same reasoning as the BSDF probe's: its
    // scalar tail packs into the third sixteen-byte block and needs no pad, and
    // that stays true only while all four of those fields are scalars.
    Case {
        shader: "pathtrace/light_probe.wgsl",
        prelude: &[
            "pathtrace/traverse.wgsl",
            "pathtrace/atlas.wgsl",
            "pathtrace/material.wgsl",
            "pathtrace/rand.wgsl",
            "pathtrace/bsdf.wgsl",
            "pathtrace/environment.wgsl",
            "pathtrace/light.wgsl",
        ],
        struct_name: "LightTap",
        rust_size: std::mem::size_of::<solarxy_renderer::pathtrace::probe::LightTap>(),
        rust_type: "solarxy_renderer::pathtrace::probe::LightTap",
    },
    // The environment uniform, declared whole on both sides. It is two `vec4f`
    // today and could as easily have been two `vec3f` with the spare lane used for
    // something, which is the shape that would measure 32 in Rust and 32 in WGSL
    // only by luck; a row here means the next person to reach for that lane finds
    // out from a test rather than from a black image. The importance-sampled
    // environment replaces these fields, and this row is what will catch its
    // replacement being a different size on one side.
    Case {
        shader: "pathtrace/environment.wgsl",
        // The whole stack beneath it: the sampler for the stratified draws, and
        // the lobes for `luminance`, which the density reads a texel through.
        prelude: &[
            "pathtrace/traverse.wgsl",
            "pathtrace/atlas.wgsl",
            "pathtrace/material.wgsl",
            "pathtrace/rand.wgsl",
            "pathtrace/bsdf.wgsl",
        ],
        struct_name: "EnvParams",
        rust_size: std::mem::size_of::<solarxy_renderer::pathtrace::EnvParams>(),
        rust_type: "solarxy_renderer::pathtrace::EnvParams",
    },
];

fn shader_source(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/shaders")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The byte span naga computes for a named struct, which is what wgpu
/// compares a bound buffer's size against.
fn wgsl_struct_size(source: &str, struct_name: &str) -> u32 {
    let module = naga::front::wgsl::parse_str(source)
        .unwrap_or_else(|e| panic!("parse {struct_name}'s shader: {e:?}"));
    let mut layouter = naga::proc::Layouter::default();
    layouter
        .update(module.to_ctx())
        .unwrap_or_else(|e| panic!("layout {struct_name}: {e:?}"));

    for (handle, ty) in module.types.iter() {
        if ty.name.as_deref() == Some(struct_name) {
            return layouter[handle].size;
        }
    }
    panic!("no struct named `{struct_name}` in the shader");
}

#[test]
fn wgsl_uniforms_match_their_rust_structs() {
    for case in CASES {
        let mut source = String::new();
        for fragment in case.prelude {
            source.push_str(&shader_source(fragment));
        }
        source.push_str(&shader_source(case.shader));
        let wgsl = wgsl_struct_size(&source, case.struct_name) as usize;
        assert_eq!(
            wgsl, case.rust_size,
            "\n`{}` in {} spans {wgsl} bytes; `{}` is {} bytes.\n\
             \n\
             wgpu compares these at DRAW time (the bind-group layouts use \
             `min_binding_size: None`), and a mismatch invalidates the \
             encoder rather than raising a Rust error -- which, because a \
             pane's encoder carries its composite pass, renders as a black \
             viewport with nothing in the log.\n\
             \n\
             The usual cause is a vector-typed padding field: `vec3<u32>` \
             aligns to 16 bytes in WGSL, `[u32; 3]` to 4 in Rust. Declare \
             padding as scalars.\n",
            case.struct_name, case.shader, case.rust_type, case.rust_size,
        );
    }
}

/// The guard is only worth having if it fails on the shape that broke.
///
/// Rather than trusting the reasoning above, this reproduces the exact
/// declaration that shipped and asserts naga reports the bad number.
#[test]
fn a_vector_typed_pad_is_detected() {
    let bad = r"
        struct LabelParams {
            text_color: vec4<f32>,
            chip_color: vec4<f32>,
            dot_color: vec4<f32>,
            text_px: f32,
            advance_px: f32,
            dot_px: f32,
            text_gap_px: f32,
            chip_pad_x: f32,
            chip_pad_y: f32,
            chip_radius: f32,
            label_count: u32,
            chip_on: u32,
            _pad: vec3<u32>,
        }
        @group(0) @binding(0) var<uniform> lp: LabelParams;
        @vertex fn vs() -> @builtin(position) vec4<f32> {
            return vec4(f32(lp.chip_on), 0.0, 0.0, 1.0);
        }
    ";
    assert_eq!(
        wgsl_struct_size(bad, "LabelParams"),
        112,
        "the vec3-padded form must still measure 112; if this changes, the \
         bug this test guards has changed shape too"
    );

    // And the scalar form, which is what the real shader now uses.
    let good = bad.replace("_pad: vec3<u32>,", "_pad0: u32, _pad1: u32, _pad2: u32,");
    assert_eq!(wgsl_struct_size(&good, "LabelParams"), 96);
}
