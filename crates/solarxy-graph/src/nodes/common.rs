//! Shared node-descriptor building blocks: the implicit `general` group,
//! the geo container's `rendering` group, the default geometry output
//! port, and the Phase 8 silent-strip migrations. These factor out the
//! catalog conventions (part II, section 11) so each node file declares
//! only what is specific to it.

use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{MigrateError, PortSpec};

/// The cook for a node that produces no wire output: containers (`geo`),
/// annotations (`note`), and the light nodes (whose `LightDef` is resolved
/// by the scene builder from their params, not carried on a wire). Emits
/// empty outputs, so the cook driver records it as cooked-clean without
/// geometry.
#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
pub fn passive_cook(
    _p: &ResolvedParams,
    _in: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    Ok(CookOutcome::Done(Outputs::empty()))
}

/// The implicit `general` group every node carries: `name` (shown as the
/// node title, defaulting to the display name) and `description`. Passed
/// the display name so `name`'s default matches.
#[must_use]
pub fn general_params(display_name: &str) -> Vec<ParamSpec> {
    vec![
        ParamSpec::new(
            "name",
            "Name",
            "general",
            ParamType::Text,
            ParamValue::Text(display_name.to_string()),
        )
        .doc("The node's title on the canvas."),
        ParamSpec::new(
            "description",
            "Description",
            "general",
            ParamType::Text,
            ParamValue::Text(String::new()),
        )
        .doc("Free-text notes shown as the node subtitle."),
    ]
}

/// The `rendering` group on the `geo` container only (root render flags,
/// both wired end to end since Phase 8): additive visibility and per-object
/// shadow participation. Subflow geometry nodes carry no copy: per-object
/// flags are geo-level concepts, and the display flag is graph-level.
/// `receive_shadow` was dropped in the same phase (no per-object channel
/// into the PBR shadow term exists; see the expansion doc's backlog note).
#[must_use]
pub fn rendering_params() -> Vec<ParamSpec> {
    vec![
        ParamSpec::new(
            "visible",
            "Visible",
            "rendering",
            ParamType::Bool,
            ParamValue::Bool(true),
        )
        .doc(
            "Whether this object is displayed. Hidden objects stay cooked, \
              so re-show is instant.",
        ),
        ParamSpec::new(
            "cast_shadow",
            "Cast Shadow",
            "rendering",
            ParamType::Bool,
            ParamValue::Bool(true),
        )
        .doc("Whether this object is drawn into the shadow map."),
    ]
}

/// Removes raw param keys before the registry-default migration sees them,
/// so a deliberate drop produces no load warning (the dropped params never
/// did anything; a toast about them would be noise).
fn strip_keys(params: &mut serde_json::Map<String, serde_json::Value>, keys: &[&str]) {
    for key in keys {
        params.remove(*key);
    }
}

/// v1 -> v2 for the subflow geometry nodes: their whole `rendering` group
/// was dead by design (the display flag is graph-level; per-object flags
/// are geo-level) and is silently stripped.
#[allow(clippy::unnecessary_wraps)] // signature matches MigrateFn
pub fn migrate_strip_rendering_group(
    from: u32,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), MigrateError> {
    if from == 1 {
        strip_keys(params, &["visible", "cast_shadow", "receive_shadow"]);
    }
    Ok(())
}

/// v1 -> v2 for the `geo` container: `receive_shadow` is dropped (never
/// wired; re-adding it is an instance-flags backlog note), silently.
#[allow(clippy::unnecessary_wraps)] // signature matches MigrateFn
pub fn migrate_strip_receive_shadow(
    from: u32,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), MigrateError> {
    if from == 1 {
        strip_keys(params, &["receive_shadow"]);
    }
    Ok(())
}

/// v1 -> v2 for `rect_area_light`: the v1 soft point-light approximation
/// never read `rotate` / `scale` / `uniform_scale`; they return with a
/// real LTC area-light model (backlog note). Silently stripped.
#[allow(clippy::unnecessary_wraps)] // signature matches MigrateFn
pub fn migrate_strip_rect_area_transform(
    from: u32,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), MigrateError> {
    if from == 1 {
        strip_keys(params, &["rotate", "scale", "uniform_scale"]);
    }
    Ok(())
}

/// The default geometry output port, key `geometry` (every geometry node's
/// single default output).
#[must_use]
pub fn geometry_output() -> PortSpec {
    PortSpec::single("geometry", "Geometry", DataType::Geometry, false).default_port()
}

/// Assembles a node's full param list: `general`, then the node-specific
/// groups. (The `rendering` group is geo-container-only since Phase 8;
/// `geo_node` appends it explicitly.)
#[must_use]
pub fn params_with(display_name: &str, specific: Vec<ParamSpec>) -> Vec<ParamSpec> {
    let mut params = general_params(display_name);
    params.extend(specific);
    params
}
