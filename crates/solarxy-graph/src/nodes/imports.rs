//! The four import nodes (node catalog part II, section 13): OBJ, glTF,
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
    BypassBehavior, Category, ContextMask, MigrateError, MigrateFn, NodeTypeDescriptor,
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
        // Match by the trailing file-name component (staged assets carry
        // their original name).
        let wanted = rel_path.rsplit(['/', '\\']).next().unwrap_or(rel_path);
        self.table
            .entries()
            .find(|(_, e)| e.name.rsplit(['/', '\\']).next().unwrap_or(&e.name) == wanted)
            .map(|(_, e)| (*e.bytes).clone())
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
        ),
        ParamSpec::new(
            "scale",
            "Scale",
            "object",
            ParamType::Float,
            ParamValue::Float(1.0),
        )
        .hard(0.0, 10000.0)
        .soft(0.001, 100.0),
        ParamSpec::new(
            "center_to_origin",
            "Center to Origin",
            "object",
            ParamType::Bool,
            ParamValue::Bool(false),
        ),
    ]
}

/// One import node's format specifics.
struct Format {
    type_id: &'static str,
    display_name: &'static str,
    accept: &'static [&'static str],
    doc: &'static str,
    aliases: &'static [&'static str],
    extra: fn() -> Vec<ParamSpec>,
    migrate: MigrateFn,
}

/// v1 -> v2 for `import_ply`: the shared rendering-group strip plus
/// `vertex_colors`, declared in v1 but never carried into the parse path
/// (no end-to-end vertex-color channel exists; it returns with the
/// renderer attribute, backlog note). Silently stripped.
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

fn descriptor_for(f: &Format) -> NodeTypeDescriptor {
    let mut specific = common_params(f.accept);
    specific.extend((f.extra)());
    NodeTypeDescriptor {
        type_id: f.type_id,
        version: 2,
        display_name: f.display_name,
        category: Category::Import,
        contexts: ContextMask::SUBFLOW,
        inputs: vec![],
        outputs: vec![geometry_output()],
        params: params_with(f.display_name, specific),
        bypass: BypassBehavior::Mute,
        doc: f.doc,
        search_aliases: f.aliases,
        cook: cook_import,
        migrate: Some(f.migrate),
    }
}

const OBJ: Format = Format {
    type_id: "import_obj",
    display_name: "Import OBJ",
    accept: &[".obj"],
    doc: "Loads a Wavefront OBJ (MTL and textures via the asset resolver).",
    aliases: &["obj", "wavefront", "import"],
    extra: Vec::new,
    migrate: migrate_strip_rendering_group,
};
const GLTF: Format = Format {
    type_id: "import_gltf",
    display_name: "Import glTF",
    accept: &[".gltf", ".glb"],
    doc: "Loads a glTF or GLB model.",
    aliases: &["gltf", "glb", "import"],
    extra: gltf_extra,
    migrate: migrate_strip_rendering_group,
};
const STL: Format = Format {
    type_id: "import_stl",
    display_name: "Import STL",
    accept: &[".stl"],
    doc: "Loads a binary or ASCII STL mesh.",
    aliases: &["stl", "import", "print"],
    extra: stl_extra,
    migrate: migrate_strip_rendering_group,
};
const PLY: Format = Format {
    type_id: "import_ply",
    display_name: "Import PLY",
    accept: &[".ply"],
    doc: "Loads a binary or ASCII PLY mesh.",
    aliases: &["ply", "import", "scan"],
    extra: Vec::new,
    migrate: migrate_ply,
};

fn gltf_extra() -> Vec<ParamSpec> {
    vec![ParamSpec::new(
        "preserve_materials",
        "Preserve Materials",
        "object",
        ParamType::Bool,
        ParamValue::Bool(true),
    )]
}
fn stl_extra() -> Vec<ParamSpec> {
    vec![ParamSpec::new(
        "recompute_normals",
        "Recompute Normals",
        "object",
        ParamType::Bool,
        ParamValue::Bool(true),
    )]
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
        preserve_materials: matches!(format, "gltf" | "glb").then(|| p.bool("preserve_materials")),
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
}
