//! Source-level rules for the path tracer's shaders, checked without a GPU.
//!
//! Three things live here that nothing else catches.
//!
//! The first is the uniformity discipline. The browser's WGSL analysis rejects
//! derivative-dependent work under non-uniform control flow at pipeline
//! creation, and a traversal kernel is exactly the branchy shape that trips it.
//! The failure arrives late, in one browser, with a message that reads like a
//! type error. Stating the rule in a comment lasts until the first person who
//! has not read the comment; grepping for it lasts.
//!
//! The second is that every fragment still parses on its own and composed. WGSL
//! has no include mechanism, so the kernels are assembled from fragments by
//! `concat!`, and a fragment that parses alone can still collide with another
//! over a name. Parsing the real compositions here means a syntax error is a
//! fast test failure rather than a pipeline-creation failure behind a GPU.
//!
//! The third is the binding budget and the descriptor layout. Numbers reserved
//! for a stage that has not arrived are invisible to the compiler, and a
//! descriptor is one word with no struct anywhere for the uniform-layout table
//! to measure, so both are held by grep or by nothing.

use std::path::{Path, PathBuf};

use wgpu::naga;

/// Constructs that make a shader's result depend on a derivative, which the
/// uniformity analysis then has to prove is uniform. The kernel samples with an
/// explicit level everywhere instead, so none of these has a legitimate use in
/// this directory.
const FORBIDDEN: &[(&str, &str)] = &[
    (
        "textureSample(",
        "implicit derivatives; use textureSampleLevel",
    ),
    (
        "textureSampleBias(",
        "implicit derivatives; use textureSampleLevel",
    ),
    (
        "textureSampleCompare(",
        "implicit derivatives; use textureSampleCompareLevel",
    ),
    ("dpdx", "derivative; not available in compute"),
    ("dpdy", "derivative; not available in compute"),
    ("fwidth", "derivative; not available in compute"),
    (
        "workgroupBarrier",
        "a barrier under non-uniform control flow is the failure this rule exists for",
    ),
    (
        "storageBarrier",
        "a barrier under non-uniform control flow is the failure this rule exists for",
    ),
];

fn shader_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/shaders/pathtrace")
}

fn read(name: &str) -> String {
    let path = shader_dir().join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Every `.wgsl` under `src/shaders/pathtrace`, so a fragment added later is
/// covered without anyone remembering to list it.
fn every_fragment() -> Vec<(String, String)> {
    let dir = shader_dir();
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display())) {
        let path = entry.expect("directory entry").path();
        if path.extension().is_some_and(|e| e == "wgsl") {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .expect("utf-8 filename")
                .to_string();
            out.push((name.clone(), read(&name)));
        }
    }
    assert!(!out.is_empty(), "no shader fragments found in {dir:?}");
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn no_pathtrace_shader_depends_on_a_derivative_or_a_barrier() {
    for (name, source) in every_fragment() {
        // Comments legitimately name these constructs to say why they are
        // banned, so judge code rather than prose.
        let code: String = source
            .lines()
            .map(|line| line.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        for (needle, why) in FORBIDDEN {
            assert!(
                !code.contains(needle),
                "{name} uses `{needle}`: {why}.\n\
                 The browser rejects this at pipeline creation, in one browser, \
                 with a message that reads like a type error."
            );
        }
    }
}

/// The fragments that declare no entry point and are composed under the ones
/// that do. Each has to parse alone.
const BASES: &[&str] = &["traverse.wgsl", "atlas.wgsl"];

#[test]
fn every_base_fragment_parses_on_its_own() {
    // A base declares no entry point and depends on nothing above it, so it is
    // the one composition-independent thing in the directory.
    for base in BASES {
        let source = read(base);
        if let Err(e) = naga::front::wgsl::parse_str(&source) {
            panic!("{base} does not parse: {}", e.emit_to_string(&source));
        }
    }
}

/// Every composition the host builds, in the order it concatenates them.
///
/// A table rather than "base plus each fragment", because that stopped being
/// true the moment a kernel needed two fragments under it. This mirrors the
/// `concat!` calls in `pathtrace/mod.rs` and `pathtrace/probe.rs`, so a kernel
/// added there without a row here is a kernel nothing parses; a row here that
/// names a file that is gone fails on the read.
const RECIPES: &[(&str, &[&str])] = &[
    (
        "the debug kernel",
        &["traverse.wgsl", "atlas.wgsl", "trace.wgsl"],
    ),
    ("the traversal probe", &["traverse.wgsl", "parity.wgsl"]),
    ("the atlas probe", &["atlas.wgsl", "atlas_probe.wgsl"]),
];

#[test]
fn every_fragment_the_host_composes_appears_in_a_recipe() {
    // A fragment on disk that nothing composes is either dead or a kernel
    // nobody is parsing, and the second is the failure this catches.
    for (name, _) in every_fragment() {
        assert!(
            RECIPES
                .iter()
                .any(|(_, parts)| parts.contains(&name.as_str())),
            "{name} is in the shader directory but in no composition; \
             add it to a recipe or delete it"
        );
    }
}

#[test]
fn every_composition_the_host_builds_parses() {
    // A fragment that parses alone can still collide with another over a name,
    // and a kernel fragment is not standalone WGSL by design: it names the
    // types and bindings the fragments beneath it declare. What has to hold is
    // that what the host actually concatenates parses, which is a fast failure
    // here instead of a pipeline-creation failure behind a GPU.
    for (label, parts) in RECIPES {
        let composed: String = parts.iter().map(|p| read(p)).collect();
        if let Err(e) = naga::front::wgsl::parse_str(&composed) {
            panic!(
                "{label} ({}) does not parse: {}",
                parts.join(" + "),
                e.emit_to_string(&composed)
            );
        }
    }
}

#[test]
fn the_workgroup_size_matches_the_constant_the_host_dispatches_against() {
    // The dispatch is sized by dividing the tile by this number. If the two
    // drift, the kernel quietly stops covering the tile's right and bottom
    // edges, or runs invocations that bounds-check themselves away.
    let declared = format!(
        "@compute @workgroup_size({size}, {size}, 1)",
        size = solarxy_renderer::pathtrace::WORKGROUP_SIZE
    );
    let mut entry_points = 0;
    for (name, source) in every_fragment() {
        for line in source.lines() {
            if line.trim_start().starts_with("@compute") {
                entry_points += 1;
                assert_eq!(
                    line.trim(),
                    declared,
                    "{name} declares a workgroup size the host does not dispatch against"
                );
            }
        }
    }
    assert!(entry_points > 0, "no compute entry point found");
}

#[test]
fn the_traversal_declares_the_binding_numbers_the_budget_reserved() {
    // Core WebGPU grants eight storage buffers per stage and the design spends
    // seven, leaving one as the escape hatch a ninth logical array would
    // otherwise force. Materials and lights are 4 and 5; nothing may quietly
    // renumber into them or into 7.
    let source = read("traverse.wgsl");
    for (binding, name) in [
        (0u32, "bvh_nodes"),
        (1, "prim_indices"),
        (2, "vertex_pos"),
        (3, "vertex_attr"),
        (6, "instances"),
    ] {
        let decl = format!("@group(0) @binding({binding}) var<storage, read> {name}");
        assert!(
            source.contains(&decl),
            "traverse.wgsl no longer declares `{name}` at binding {binding}"
        );
    }
    for reserved in [4u32, 5, 7] {
        assert!(
            !source.contains(&format!("@group(0) @binding({reserved})")),
            "binding {reserved} of the scene group is reserved; \
             4 and 5 are materials and lights, 7 is the escape hatch"
        );
    }
}

#[test]
fn the_atlas_declares_the_sampled_group_the_budget_reserved() {
    // The sampled group is the atlas and its two samplers; 3 to 6 belong to the
    // environment, which arrives with its own consumer. Group 2 was declared
    // empty rather than skipped for exactly this: nothing renumbers.
    let source = read("atlas.wgsl");
    for (binding, decl) in [
        (0u32, "var atlas: texture_2d_array<f32>"),
        (1, "var atlas_nearest: sampler"),
        (2, "var atlas_linear: sampler"),
    ] {
        let expected = format!("@group(2) @binding({binding}) {decl}");
        assert!(
            source.contains(&expected),
            "atlas.wgsl no longer declares `{decl}` at binding {binding}"
        );
    }
    for (name, source) in every_fragment() {
        for reserved in [3u32, 4, 5, 6] {
            assert!(
                !source.contains(&format!("@group(2) @binding({reserved})")),
                "{name} takes sampled binding {reserved}; 3 to 6 are reserved \
                 for the environment equirect, its sampler, and the two \
                 sampling-distribution textures"
            );
        }
    }
}

#[test]
fn the_descriptor_bit_layout_is_the_same_on_both_sides() {
    // The packer writes these bits and the shader reads them, and nothing else
    // holds the two together: there is no struct here for the uniform-layout
    // table to measure, because a descriptor is one word inside a material
    // record that does not exist yet.
    let source = read("atlas.wgsl");
    for constant in [
        "const TEX_LAYER_MASK: u32 = 0xFFu;",
        "const TEX_UV_SHIFT: u32 = 8u;",
        "const TEX_WRAP_S_SHIFT: u32 = 11u;",
        "const TEX_WRAP_T_SHIFT: u32 = 13u;",
        "const TEX_FILTER_BIT: u32 = 1u << 15u;",
        "const TEX_SRGB_BIT: u32 = 1u << 16u;",
        "const TEX_UNUSED_BIT: u32 = 1u << 31u;",
    ] {
        assert!(
            source.contains(constant),
            "the descriptor layout moved: `{constant}` is gone from atlas.wgsl. \
             `TextureDescriptor::pack` has to move with it."
        );
    }
}

#[test]
fn the_traversal_stack_is_one_constant_rather_than_a_literal() {
    // The spike declared `array<u32, 64>` beside a `STACK_SIZE` constant used
    // only in the bounds guard. Two numbers that must move together, and one of
    // them invisible to the guard, is how a traversal starts overflowing
    // silently on deep geometry.
    let source = read("traverse.wgsl");
    assert!(
        source.contains("const STACK_SIZE: u32 = 64u;"),
        "the stack bound moved or changed shape"
    );
    assert!(
        !source.contains("array<u32, 64>"),
        "a stack is declared with a literal size instead of STACK_SIZE"
    );
}
