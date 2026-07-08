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

use solarxy_formats::AssetResolver;
use solarxy_kernel::GeometrySet;
use solarxy_kernel::transform::{RotateOrder, bake_transform, compose_trs};

use super::common::{geometry_output, params_with};
use crate::assets::AssetTable;
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, JobRequest, Outputs};
use crate::params::ParamValue;
use crate::registry::param_spec::{ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextMask, NodeTypeDescriptor};

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
}

fn descriptor_for(f: &Format) -> NodeTypeDescriptor {
    let mut specific = common_params(f.accept);
    specific.extend((f.extra)());
    NodeTypeDescriptor {
        type_id: f.type_id,
        version: 1,
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
        migrate: None,
    }
}

const OBJ: Format = Format {
    type_id: "import_obj",
    display_name: "Import OBJ",
    accept: &[".obj"],
    doc: "Loads a Wavefront OBJ (MTL and textures via the asset resolver).",
    aliases: &["obj", "wavefront", "import"],
    extra: Vec::new,
};
const GLTF: Format = Format {
    type_id: "import_gltf",
    display_name: "Import glTF",
    accept: &[".gltf", ".glb"],
    doc: "Loads a glTF or GLB model.",
    aliases: &["gltf", "glb", "import"],
    extra: gltf_extra,
};
const STL: Format = Format {
    type_id: "import_stl",
    display_name: "Import STL",
    accept: &[".stl"],
    doc: "Loads a binary or ASCII STL mesh.",
    aliases: &["stl", "import", "print"],
    extra: stl_extra,
};
const PLY: Format = Format {
    type_id: "import_ply",
    display_name: "Import PLY",
    accept: &[".ply"],
    doc: "Loads a binary or ASCII PLY mesh.",
    aliases: &["ply", "import", "scan"],
    extra: ply_extra,
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
fn ply_extra() -> Vec<ParamSpec> {
    vec![ParamSpec::new(
        "vertex_colors",
        "Vertex Colors",
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

/// The shared import cook. Dispatches by the node's format (from the
/// resolved `file` accept-list is not available here, so the format is
/// carried via the type id, recovered from the `scale`-independent path).
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

    // On the web, hand heavy parsing to the import worker.
    if cx.async_jobs {
        return Ok(CookOutcome::Pending(JobRequest::ParseModel {
            asset: asset.clone(),
            format,
        }));
    }

    // Native / test path: parse inline.
    let bytes = (*entry.bytes).clone();
    let name = entry.name.clone();
    let raw = parse_bytes(&format, &bytes, &name, cx.assets)
        .map_err(|message| CookError::Failed { message })?;
    let mut set = GeometrySet::from_raw(raw);
    apply_import_options(&mut set, p);
    Ok(CookOutcome::Done(Outputs::geometry(set)))
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

/// Applies the common `scale` and `center_to_origin` options by baking a
/// transform (scale about the origin, then recenter). Failures (a zero
/// scale is excluded by the hard range) fall back to the unscaled set.
fn apply_import_options(set: &mut GeometrySet, p: &ResolvedParams) {
    let scale = p.f32("scale");
    let center = p.bool("center_to_origin");
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
        let id = assets.stage("tri.stl", TRI_STL.as_bytes().to_vec());

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
        let id = assets.stage("tri.stl", TRI_STL.as_bytes().to_vec());
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
        let id = assets.stage("bad.ply", b"not a ply file".to_vec());
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
}
