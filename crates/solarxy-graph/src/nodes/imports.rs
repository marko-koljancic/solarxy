//! The four import nodes: OBJ, glTF,
//! STL, PLY.
//!
//! Source nodes: no inputs, one Geometry output, `Mute` bypass. File
//! identity is the staged asset's content hash. On the web these cook
//! asynchronously (a `JobRequest` runs in the import worker under the
//! per-node generation guard); natively (and in tests) they parse inline
//! via the `solarxy-formats` byte loaders. Parse failures badge an ERROR
//! while keep-last-good keeps the previous geometry (a catalog delta from
//! Minimystix's silent empty-group behavior).

use solarxy_core::validation::{ValidationResult, validate_raw_model};
use solarxy_formats::AssetResolver;
use solarxy_kernel::GeometrySet;
use solarxy_kernel::transform::{RotateOrder, bake_transform, compose_trs};

use super::common::{geometry_output, migrate_strip_rendering_group, params_with};
use crate::assets::AssetTable;
use crate::cook::{CookCtx, CookError, CookOutcome, ImportOptions, Inputs, JobRequest, Outputs};
use crate::params::ParamValue;
use crate::registry::param_spec::{ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{
    BypassBehavior, Category, ContextSet, MigrateError, MigrateFn, NodeRole, NodeTypeDescriptor,
};

/// An [`AssetResolver`] over the in-memory [`AssetTable`], resolving a
/// sidecar's relative path by matching a staged asset's file name. The
/// self-contained-file case (STL, PLY, GLB, OBJ-without-MTL) never calls
/// it; OBJ+MTL and glTF+bin resolve their companions by name.
struct TableResolver<'a> {
    table: &'a AssetTable,
}

impl AssetResolver for TableResolver<'_> {
    fn read(&mut self, rel_path: &str) -> Option<Vec<u8>> {
        // Match by trailing file-name component against EVERY name the bytes
        // are staged under, not just the first-seen one: identical bytes staged
        // twice under different names are one content-addressed entry, and a
        // model referencing the second name must still resolve.
        self.table
            .find_by_name(rel_path)
            .map(|e| (*e.bytes).clone())
    }
}

/// Common import params: the file reference, a uniform import scale, and
/// recenter-to-origin. `accept` is the per-format extension filter.
fn common_params(accept: &[&str]) -> Vec<ParamSpec> {
    vec![
        ParamSpec::new(
            "file",
            "File",
            "object",
            ParamType::AssetRef {
                accept: accept.iter().map(ToString::to_string).collect(),
            },
            ParamValue::Asset(crate::params::AssetId(String::new())),
        )
        .doc(
            "The staged model file. Identity is the bytes' SHA-256, not the \
             path, so re-staging the same file costs nothing and a saved \
             `.slxy` carries a copy of the bytes -- the scene keeps loading \
             after the original moves or is deleted. The picker is \
             multi-select: stage companion files (an MTL, a `.bin`, textures) \
             in the same go and the parser resolves them by name. Left empty \
             the node cooks to nothing, without an error.",
        ),
        ParamSpec::new(
            "scale",
            "Scale",
            "object",
            ParamType::Float,
            ParamValue::Float(1.0),
        )
        .hard(0.0, 10000.0)
        .soft(0.001, 100.0)
        .doc(
            "A uniform multiplier baked into the points at import, about the \
             origin. Use it to reconcile units at the source -- a millimetre \
             CAD export needs 0.001 to land in metres. Unlike a downstream \
             `transform`, this is baked, so everything after it measures the \
             scaled model.",
        ),
        ParamSpec::new(
            "center_to_origin",
            "Center to Origin",
            "object",
            ParamType::Bool,
            ParamValue::Bool(false),
        )
        .doc(
            "Moves the model so its bounding-box centre sits at the origin. \
             Applied after Scale. Worth turning on for a file authored far \
             from the origin, which otherwise imports off-screen and orbits \
             around nothing.",
        ),
    ]
}

/// One import node's format specifics.
struct Format {
    type_id: &'static str,
    version: u32,
    display_name: &'static str,
    accept: &'static [&'static str],
    doc: &'static str,
    aliases: &'static [&'static str],
    extra: fn() -> Vec<ParamSpec>,
    migrate: MigrateFn,
}

/// `import_ply` migrations. v1 -> v2: the shared rendering-group strip
/// plus the historical `vertex_colors`, declared in v1 but never carried
/// into the parse path (dropping the stored value too: the v1 toggle did
/// nothing, so its setting carries no intent). v2 -> v3 restores
/// `vertex_colors` as a real end-to-end control; the registry default
/// fill supplies `true`, so no hook logic is needed for that step.
#[allow(clippy::unnecessary_wraps)] // signature matches MigrateFn
fn migrate_ply(
    from: u32,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), MigrateError> {
    migrate_strip_rendering_group(from, params)?;
    if from == 1 {
        params.remove("vertex_colors");
    }
    Ok(())
}

/// The restored `vertex_colors` toggle (v3): PLY colors now travel end to
/// end, so the param controls a real channel.
fn vertex_colors_extra() -> Vec<ParamSpec> {
    vec![
        ParamSpec::new(
            "vertex_colors",
            "Vertex Colors",
            "import",
            ParamType::Bool,
            ParamValue::Bool(true),
        )
        .doc(
            "Keep the file's per-vertex colours (red/green/blue, optional \
             alpha). On, colours import as the per-point colour attribute \
             and display in the viewport; off, they are dropped at import \
             for when a scan's colours are noise rather than signal.",
        ),
    ]
}

fn descriptor_for(f: &Format) -> NodeTypeDescriptor {
    let mut specific = common_params(f.accept);
    specific.extend((f.extra)());
    NodeTypeDescriptor {
        type_id: f.type_id,
        version: f.version,
        display_name: f.display_name,
        category: Category::Import,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![],
        outputs: vec![geometry_output()],
        params: params_with(f.display_name, specific),
        bypass: BypassBehavior::Mute,
        doc: f.doc,
        search_aliases: f.aliases,
        glyph: f.type_id,
        role: NodeRole::Standard,
        cook: cook_import,
        migrate: Some(f.migrate),
    }
}

const OBJ: Format = Format {
    type_id: "import_obj",
    // v3: gains `preserve_materials`; the strip migration's
    // v2 step is a no-op and the new param default-fills on load.
    version: 3,
    display_name: "Import OBJ",
    accept: &[".obj"],
    doc: "Loads a Wavefront OBJ, triangulating as it parses. Materials come \
          from the companion MTL and its textures, resolved by file name \
          against the staged assets rather than the file system.\n\n\
          This heads a chain: import, then `transform` to place the model, \
          `merge` to combine it with others, `bounds` to check where it \
          actually landed. It is also the validator's entry point -- an \
          import validates the raw file as it parses, the same check the \
          desktop viewer runs on load, so both products report the same \
          issues on the same file.\n\n\
          OBJ is a multi-file format and the MTL is not optional-by-accident: \
          stage the `.mtl` and its textures together with the `.obj` (the \
          picker is multi-select, and dropping the containing folder \
          traverses it) or the model arrives with geometry and no materials, \
          no error raised. On the web the parse runs in an import worker off \
          the main thread, so a heavy file does not freeze the canvas.",
    aliases: &["obj", "wavefront", "import"],
    extra: preserve_materials_extra,
    migrate: migrate_strip_rendering_group,
};
const GLTF: Format = Format {
    type_id: "import_gltf",
    version: 2,
    display_name: "Import glTF",
    accept: &[".gltf", ".glb"],
    doc: "Loads a glTF 2.0 model, either the self-contained binary `.glb` or \
          the `.gltf` JSON with its companion `.bin` and textures resolved by \
          file name against the staged assets. Materials come across natively \
          -- glTF is the format that survives the round trip best.\n\n\
          Reach for it when you have a choice of export from the DCC: it \
          heads the same chain as any import (`transform`, `merge`, `bounds` \
          downstream) and, like the others, validates the raw file as it \
          parses, so the Validation tab reports on it exactly as the desktop \
          viewer does.\n\n\
          Draco-compressed glTF is rejected outright, with a message asking \
          you to re-export without Draco -- there is no decoder in the app \
          yet, and the check runs in the import worker before the parse so \
          the previous geometry stays on screen. Prefer `.glb` when you can: \
          `.gltf` splits into files that all have to be staged together.",
    aliases: &["gltf", "glb", "import"],
    extra: preserve_materials_extra,
    migrate: migrate_strip_rendering_group,
};
const STL: Format = Format {
    type_id: "import_stl",
    version: 2,
    display_name: "Import STL",
    accept: &[".stl"],
    doc: "Loads an STL mesh, binary or ASCII (the loader sniffs which). One \
          file, no companions, no materials -- STL carries triangles and \
          nothing else.\n\n\
          This is the 3D-printing and CAD-handoff path. It pairs with the \
          validator more than most: STL is where degenerate triangles, \
          non-manifold edges, and flipped windings actually show up, and the \
          import validates the raw file as it parses, so the badge is \
          populated before you wire anything downstream.\n\n\
          STL stores a normal per facet rather than per vertex, and the \
          loader keeps none of them: a parsed STL arrives with positions and \
          triangles and no normals at all. That is why Recompute Normals \
          defaults to on -- turn it off and the mesh has no normals for \
          anything downstream to shade with.",
    aliases: &["stl", "import", "print"],
    extra: stl_extra,
    migrate: migrate_strip_rendering_group,
};
const PLY: Format = Format {
    type_id: "import_ply",
    version: 3,
    display_name: "Import PLY",
    accept: &[".ply"],
    doc: "Loads a PLY mesh or point cloud, binary or ASCII. Self-contained \
          like STL: one file, no companions, no materials.\n\n\
          PLY is the scanning and photogrammetry format, so this usually \
          heads a cleanup chain and pairs with the validator -- like every \
          import it validates the raw file as it parses, which on a \
          multi-million-point scan is where the issue counts actually matter. \
          On the web the parse runs in an import worker off the main thread.\n\n\
          A file with no face element loads as a true point cloud and draws \
          as camera-facing points. Vertex colours (red/green/blue, optional \
          alpha, uchar or float) survive the import as the per-point colour \
          attribute and display directly; the Vertex Colors toggle drops \
          them at the door when a scan's colours are noise rather than \
          signal. Points and point clouds are not click-selectable in the \
          viewport; select their node on the canvas instead.",
    aliases: &["ply", "import", "scan", "points", "cloud"],
    extra: vertex_colors_extra,
    migrate: migrate_ply,
};

/// The `preserve_materials` checkbox, shared by the two formats that carry
/// materials (OBJ via MTL, glTF natively). Off replaces every material
/// with the renderer's neutral default. STL and PLY have no materials and
/// no checkbox.
fn preserve_materials_extra() -> Vec<ParamSpec> {
    vec![
        ParamSpec::new(
            "preserve_materials",
            "Preserve Materials",
            "object",
            ParamType::Bool,
            ParamValue::Bool(true),
        )
        .doc(
            "On keeps the materials the file defines. Off drops every \
             material binding and the material table with them, so the whole \
             model draws in the renderer's neutral default -- the clay look \
             you want when judging form, or when a file's own materials are \
             fighting you.",
        ),
    ]
}
fn stl_extra() -> Vec<ParamSpec> {
    vec![
        ParamSpec::new(
            "recompute_normals",
            "Recompute Normals",
            "object",
            ParamType::Bool,
            ParamValue::Bool(true),
        )
        .doc(
            "Computes vertex normals from the triangles. The STL loader keeps \
             none of the file's facet normals, so off leaves the mesh with no \
             normals at all -- leave this on unless something downstream is \
             about to supply its own.",
        ),
    ]
}

#[must_use]
pub fn obj_descriptor() -> NodeTypeDescriptor {
    descriptor_for(&OBJ)
}
#[must_use]
pub fn gltf_descriptor() -> NodeTypeDescriptor {
    descriptor_for(&GLTF)
}
#[must_use]
pub fn stl_descriptor() -> NodeTypeDescriptor {
    descriptor_for(&STL)
}
#[must_use]
pub fn ply_descriptor() -> NodeTypeDescriptor {
    descriptor_for(&PLY)
}

/// The shared import cook. The format is the staged asset's file extension
/// (all four import nodes share this body); the resolved finishing options
/// travel with the async job so the worker returns finished geometry.
fn cook_import(
    p: &ResolvedParams,
    _in: &Inputs,
    cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    // No file staged yet: empty output, no error (the node simply has no
    // source; keep-last-good does not apply because there is no prior).
    let Some(asset) = p.asset("file") else {
        return Ok(CookOutcome::Done(Outputs::geometry(GeometrySet::empty())));
    };

    // The format is derivable from the staged asset's extension.
    let entry = cx.assets.get(asset).ok_or_else(|| CookError::Failed {
        message: "referenced asset is not staged".to_string(),
    })?;
    let format = entry
        .name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let options = import_options(p, &format);

    // On the web, hand heavy parsing (and its finishing) to the worker.
    if cx.async_jobs {
        return Ok(CookOutcome::Pending(JobRequest::ParseModel {
            asset: asset.clone(),
            format,
            options,
        }));
    }

    // Native / test path: parse, validate, and finish inline.
    let (set, validation) =
        parse_model_validated(&format, &entry.bytes, &entry.name, cx.assets, &options)
            .map_err(|message| CookError::Failed { message })?;
    cx.set_validation(validation);
    Ok(CookOutcome::Done(Outputs::geometry(set)))
}

/// Reads the resolved finishing options for an import. Format-specific
/// toggles are read only for the format that declares them (so this never
/// touches a param the node's descriptor lacks).
fn import_options(p: &ResolvedParams, format: &str) -> ImportOptions {
    ImportOptions {
        scale: p.f32("scale"),
        center_to_origin: p.bool("center_to_origin"),
        recompute_normals: (format == "stl").then(|| p.bool("recompute_normals")),
        preserve_materials: matches!(format, "gltf" | "glb" | "obj")
            .then(|| p.bool("preserve_materials")),
        vertex_colors: (format == "ply").then(|| p.bool("vertex_colors")),
    }
}

/// Parses staged bytes for a format via the `solarxy-formats` byte loaders.
/// Public so a native host or the import worker can fulfill a
/// [`JobRequest::ParseModel`] and feed the result back through
/// `Engine::submit_job_result`.
pub fn parse_bytes(
    format: &str,
    bytes: &[u8],
    name: &str,
    assets: &AssetTable,
) -> Result<solarxy_core::geometry::RawModelData, String> {
    let mut resolver = TableResolver { table: assets };
    match format {
        "obj" => solarxy_formats::obj::load_obj_bytes(bytes, &mut resolver),
        "gltf" | "glb" => solarxy_formats::gltf::load_gltf_bytes(bytes, &mut resolver),
        "stl" => solarxy_formats::stl::load_stl_bytes(bytes, name),
        "ply" => solarxy_formats::ply::load_ply_bytes(bytes, name),
        other => return Err(format!("unsupported import format '{other}'")),
    }
    .map_err(|e| e.to_string())
}

/// Parses staged bytes and finishes the result: the full import. Both the
/// native host ([`crate::engine::Engine::resolve_job`]) and the web import
/// worker call this, so the inline and off-thread paths produce identical
/// geometry.
pub fn parse_model(
    format: &str,
    bytes: &[u8],
    name: &str,
    assets: &AssetTable,
    options: &ImportOptions,
) -> Result<GeometrySet, String> {
    let raw = parse_bytes(format, bytes, name, assets)?;
    let mut set = GeometrySet::from_raw(raw);
    finish_import_set(&mut set, options);
    Ok(set)
}

/// [`parse_model`] plus the implicit load-time validation: the raw model is
/// validated as parsed (before finishing), exactly like the desktop
/// viewer's load validation (`validate_raw_model` with default config), so
/// the two products report identically on the same file. Both the inline
/// cook and the web import worker call this.
pub fn parse_model_validated(
    format: &str,
    bytes: &[u8],
    name: &str,
    assets: &AssetTable,
    options: &ImportOptions,
) -> Result<(GeometrySet, ValidationResult), String> {
    let raw = parse_bytes(format, bytes, name, assets)?;
    let validation = validate_raw_model(&raw, format);
    let mut set = GeometrySet::from_raw(raw);
    finish_import_set(&mut set, options);
    Ok((set, validation))
}

/// Applies the resolved finishing options to a freshly parsed set: the
/// format toggles first (recompute STL normals; drop glTF materials for a
/// neutral look), then the common uniform scale and recenter.
fn finish_import_set(set: &mut GeometrySet, o: &ImportOptions) {
    if o.recompute_normals == Some(true) {
        for m in &mut set.meshes {
            m.recompute_normals();
        }
    }
    if o.vertex_colors == Some(false) {
        // The loader lifted the file's colors into the reserved lane;
        // the toggle off drops them before anything downstream sees them.
        for m in &mut set.meshes {
            m.attributes.remove(solarxy_kernel::reserved::COLOR);
        }
    }
    if o.preserve_materials == Some(false) {
        // Neutralize: drop material bindings so the renderer's default
        // (clay) material is used, and release the material table.
        for m in &mut set.meshes {
            m.material_index = None;
        }
        set.materials.clear();
    }
    apply_scale_center(set, o.scale, o.center_to_origin);
}

/// Applies the common `scale` and `center_to_origin` options by baking a
/// transform (scale about the origin, then recenter). Failures (a zero
/// scale is excluded by the hard range) fall back to the unscaled set.
fn apply_scale_center(set: &mut GeometrySet, scale: f32, center: bool) {
    if (scale - 1.0).abs() < f32::EPSILON && !center {
        return;
    }
    let translate = if center {
        let c = set.bounds.center();
        [-c.x * scale, -c.y * scale, -c.z * scale]
    } else {
        [0.0; 3]
    };
    let matrix = compose_trs(
        translate,
        [0.0; 3],
        RotateOrder::Xyz,
        [scale, scale, scale],
        [0.0; 3],
    );
    if let Ok(baked) = bake_transform(set, &matrix) {
        *set = baked;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::coerce::Value;
    use std::collections::BTreeMap;

    // A tiny valid ASCII STL (one triangle).
    const TRI_STL: &str = "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid t\n";

    #[test]
    fn stl_import_cooks_inline_and_applies_scale() {
        let mut assets = AssetTable::new();
        let id = assets.stage("tri.stl", "model/stl", TRI_STL.as_bytes().to_vec());

        let mut stored = BTreeMap::new();
        stored.insert(
            "file".to_string(),
            crate::params::ParamSource::Literal(ParamValue::Asset(id)),
        );
        stored.insert(
            "scale".to_string(),
            crate::params::ParamSource::Literal(ParamValue::Float(2.0)),
        );
        let resolved =
            crate::registry::resolve::resolve_params(&stored, &stl_descriptor().params).unwrap();

        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(out) = cook_import(&resolved, &Inputs::default(), &mut cx).unwrap()
        else {
            panic!("inline import cooks synchronously");
        };
        let Some(Value::Geometry(set)) = out.get("geometry") else {
            panic!("outputs geometry");
        };
        assert_eq!(set.triangle_count(), 1);
        // Scaled by 2: the (1,0,0) vertex reaches x = 2.
        assert!((set.bounds.max.x - 2.0).abs() < 1e-5);
    }

    #[test]
    fn no_file_yields_empty_without_error() {
        let assets = AssetTable::new();
        let resolved =
            crate::registry::resolve::resolve_params(&BTreeMap::new(), &obj_descriptor().params)
                .unwrap();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(out) = cook_import(&resolved, &Inputs::default(), &mut cx).unwrap()
        else {
            panic!("cooks synchronously");
        };
        assert!(out.is_renderable_empty());
    }

    #[test]
    fn async_mode_returns_a_pending_job() {
        let mut assets = AssetTable::new();
        let id = assets.stage("tri.stl", "model/stl", TRI_STL.as_bytes().to_vec());
        let mut stored = BTreeMap::new();
        stored.insert(
            "file".to_string(),
            crate::params::ParamSource::Literal(ParamValue::Asset(id)),
        );
        let resolved =
            crate::registry::resolve::resolve_params(&stored, &stl_descriptor().params).unwrap();
        let mut cx = CookCtx::new(&assets, true); // async_jobs = true
        let outcome = cook_import(&resolved, &Inputs::default(), &mut cx).unwrap();
        assert!(matches!(
            outcome,
            CookOutcome::Pending(JobRequest::ParseModel { format, .. }) if format == "stl"
        ));
    }

    #[test]
    fn bad_bytes_are_a_cook_error() {
        let mut assets = AssetTable::new();
        let id = assets.stage(
            "bad.ply",
            "application/octet-stream",
            b"not a ply file".to_vec(),
        );
        let mut stored = BTreeMap::new();
        stored.insert(
            "file".to_string(),
            crate::params::ParamSource::Literal(ParamValue::Asset(id)),
        );
        let resolved =
            crate::registry::resolve::resolve_params(&stored, &ply_descriptor().params).unwrap();
        let mut cx = CookCtx::new(&assets, false);
        assert!(matches!(
            cook_import(&resolved, &Inputs::default(), &mut cx),
            Err(CookError::Failed { .. })
        ));
    }

    fn plain_options() -> ImportOptions {
        ImportOptions {
            scale: 1.0,
            center_to_origin: false,
            recompute_normals: None,
            preserve_materials: None,
            vertex_colors: None,
        }
    }

    fn bare_tri() -> solarxy_kernel::KernelMesh {
        solarxy_kernel::KernelMesh::new(
            "m",
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        )
    }

    #[test]
    fn recompute_normals_toggle_controls_stl_normals() {
        // Some(true) fills normals from topology.
        let mut set = GeometrySet::from_mesh(bare_tri());
        assert!(set.meshes[0].normals.is_none());
        finish_import_set(
            &mut set,
            &ImportOptions {
                recompute_normals: Some(true),
                ..plain_options()
            },
        );
        assert!(set.meshes[0].normals.is_some());

        // Some(false) leaves the mesh's (absent) normals untouched.
        let mut kept = GeometrySet::from_mesh(bare_tri());
        finish_import_set(
            &mut kept,
            &ImportOptions {
                recompute_normals: Some(false),
                ..plain_options()
            },
        );
        assert!(kept.meshes[0].normals.is_none());
    }

    #[test]
    fn preserve_materials_false_neutralizes_gltf_materials() {
        let material = std::sync::Arc::new(solarxy_core::RawMaterialData {
            name: "brass".to_string(),
            ..Default::default()
        });

        let mut mesh = bare_tri();
        mesh.material_index = Some(0);
        let mut set = GeometrySet::from_parts(vec![mesh], vec![material.clone()]);
        finish_import_set(
            &mut set,
            &ImportOptions {
                preserve_materials: Some(false),
                ..plain_options()
            },
        );
        assert!(set.materials.is_empty(), "materials dropped");
        assert_eq!(set.meshes[0].material_index, None, "binding cleared");

        // Some(true) keeps materials and bindings.
        let mut mesh2 = bare_tri();
        mesh2.material_index = Some(0);
        let mut kept = GeometrySet::from_parts(vec![mesh2], vec![material]);
        finish_import_set(
            &mut kept,
            &ImportOptions {
                preserve_materials: Some(true),
                ..plain_options()
            },
        );
        assert_eq!(kept.materials.len(), 1);
        assert_eq!(kept.meshes[0].material_index, Some(0));
    }

    /// `preserve_materials` is read exactly for the formats that declare
    /// it (OBJ and glTF/GLB); STL and PLY never see it, so `import_options`
    /// cannot touch a param their descriptors lack.
    #[test]
    fn import_options_reads_preserve_materials_per_format() {
        let resolved_defaults = |desc: fn() -> crate::registry::NodeTypeDescriptor| {
            crate::registry::resolve::resolve_params(
                &std::collections::BTreeMap::new(),
                &desc().params,
            )
            .unwrap()
        };

        let obj = import_options(&resolved_defaults(obj_descriptor), "obj");
        assert_eq!(obj.preserve_materials, Some(true), "obj declares it");

        let gltf = import_options(&resolved_defaults(gltf_descriptor), "gltf");
        assert_eq!(gltf.preserve_materials, Some(true), "gltf declares it");
        let glb = import_options(&resolved_defaults(gltf_descriptor), "glb");
        assert_eq!(glb.preserve_materials, Some(true), "glb declares it");

        let stl = import_options(&resolved_defaults(stl_descriptor), "stl");
        assert_eq!(stl.preserve_materials, None, "stl has no materials");
        let ply = import_options(&resolved_defaults(ply_descriptor), "ply");
        assert_eq!(ply.preserve_materials, None, "ply has no materials");

        // W3b: vertex_colors is PLY-only, default true.
        assert_eq!(ply.vertex_colors, Some(true), "ply declares it, on");
        assert_eq!(stl.vertex_colors, None);
        assert_eq!(obj.vertex_colors, None);
    }

    /// W3b: the toggle off strips the loader-lifted color lane before
    /// anything downstream sees it; on (the default) keeps it.
    #[test]
    fn vertex_colors_toggle_strips_the_color_lane() {
        use solarxy_kernel::{AttributeData, reserved};
        let colored_tri = || {
            let mut m = bare_tri();
            m.attributes.insert(
                reserved::COLOR.to_string(),
                AttributeData::Vec4(std::sync::Arc::new(vec![[1.0, 0.0, 0.0, 1.0]; 3])),
            );
            GeometrySet::from_mesh(m)
        };

        let mut kept = colored_tri();
        finish_import_set(&mut kept, &plain_options());
        assert!(kept.meshes[0].attributes.contains_key(reserved::COLOR));

        let mut stripped = colored_tri();
        finish_import_set(
            &mut stripped,
            &ImportOptions {
                vertex_colors: Some(false),
                ..plain_options()
            },
        );
        assert!(!stripped.meshes[0].attributes.contains_key(reserved::COLOR));
    }

    /// The v1 hook still strips the historical do-nothing param; the v3
    /// descriptor re-declares it for real (default true), and stepwise
    /// migration from v1 ends with the default-filled active toggle.
    #[test]
    fn ply_v3_migration_and_descriptor_shape() {
        let desc = ply_descriptor();
        assert_eq!(desc.version, 3);
        let vc = desc
            .params
            .iter()
            .find(|p| p.key == "vertex_colors")
            .expect("v3 re-declares vertex_colors");
        assert_eq!(vc.default, ParamValue::Bool(true));

        let mut raw = serde_json::Map::new();
        raw.insert("vertex_colors".to_string(), serde_json::json!(false));
        raw.insert("visible".to_string(), serde_json::json!(true));
        migrate_ply(1, &mut raw).unwrap();
        assert!(
            !raw.contains_key("vertex_colors"),
            "the historical inert value carries no intent and is dropped"
        );
        assert!(!raw.contains_key("visible"), "rendering group stripped");
        migrate_ply(2, &mut raw).unwrap();
        assert!(raw.is_empty(), "v2 to v3 is a pure default fill");
    }
}
