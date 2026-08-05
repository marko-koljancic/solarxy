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
        struct_name: "CompositeParams",
        rust_size: std::mem::size_of::<solarxy_renderer::composite::CompositeParams>(),
        rust_type: "solarxy_renderer::composite::CompositeParams",
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
        let source = shader_source(case.shader);
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
