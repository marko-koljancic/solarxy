//! Shared node-descriptor building blocks: the implicit `general` group,
//! the geo container's `rendering` group, the default geometry output
//! port, and the Phase 8 silent-strip migrations. These factor out the
//! catalog conventions (part II, section 11) so each node file declares
//! only what is specific to it.

use solarxy_kernel::transform::RotateOrder;

use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::{ParamSource, ParamValue};
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType};
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

/// The six Euler orders. Shared by `transform` and `geo` so the two cannot
/// drift apart again: they had, with `geo` hardcoding ZYX in its world matrix
/// while `transform` defaulted to XYZ, so the same angles typed into each
/// produced two different orientations.
#[must_use]
pub fn rotate_order_variants() -> Vec<EnumVariant> {
    ["xyz", "xzy", "yxz", "yzx", "zxy", "zyx"]
        .into_iter()
        .map(|k| EnumVariant::new(k, k.to_uppercase()))
        .collect()
}

/// The `rotate_order` param, identical on every node that composes a rotation.
#[must_use]
pub fn rotate_order_param() -> ParamSpec {
    ParamSpec::new(
        "rotate_order",
        "Rotate Order",
        "transform",
        ParamType::Enum {
            variants: rotate_order_variants(),
        },
        ParamValue::Enum("xyz".to_string()),
    )
}

/// Maps the stored variant key onto the kernel's order. An unknown key falls
/// back to the default rather than failing a cook.
#[must_use]
pub fn rotate_order_from_key(key: &str) -> RotateOrder {
    match key {
        "xzy" => RotateOrder::Xzy,
        "yxz" => RotateOrder::Yxz,
        "yzx" => RotateOrder::Yzx,
        "zxy" => RotateOrder::Zxy,
        "zyx" => RotateOrder::Zyx,
        _ => RotateOrder::Xyz,
    }
}

/// The `geo` container's migrations.
///
/// v1 -> v2: `receive_shadow` is dropped (never wired; re-adding it is an
/// instance-flags backlog note), silently.
///
/// v2 -> v3: `geo` gains `rotate_order`, and its world matrix moves from a
/// hardcoded `T * Rz * Ry * Rx * S` onto the kernel's `compose_trs`, which
/// defaults to XYZ. Left alone, that would silently re-orient every existing
/// geo. So a v2 geo whose `rotate` has two or more nonzero lanes -- the only
/// case where the order is observable at all -- gets `zyx` stamped explicitly,
/// preserving its appearance exactly. A geo with one or zero nonzero lanes
/// rotates identically under either order, so it keeps the new default and the
/// document stays clean.
pub fn migrate_geo(
    from: u32,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), MigrateError> {
    if from == 1 {
        strip_keys(params, &["receive_shadow"]);
    }
    if from == 2 && !params.contains_key("rotate_order") && geo_rotate_order_is_observable(params) {
        let value = serde_json::to_value(ParamSource::Literal(ParamValue::Enum("zyx".to_string())))
            .map_err(|e| MigrateError {
                from,
                reason: format!("could not encode rotate_order: {e}"),
            })?;
        params.insert("rotate_order".to_string(), value);
    }
    Ok(())
}

/// Whether a stored `rotate` has two or more nonzero lanes, which is exactly
/// when the composition order changes the resulting orientation. Deserialized
/// through the real `ParamSource` rather than by hand, so it cannot rot against
/// the serde shape.
fn geo_rotate_order_is_observable(params: &serde_json::Map<String, serde_json::Value>) -> bool {
    let Some(raw) = params.get("rotate") else {
        return false;
    };
    let Ok(ParamSource::Literal(ParamValue::Vec3(rotate))) =
        serde_json::from_value::<ParamSource>(raw.clone())
    else {
        // An expression, or something unreadable: leave it at the default
        // rather than guess. An expression cannot be evaluated in v1 anyway.
        return false;
    };
    rotate.iter().filter(|a| **a != 0.0).count() >= 2
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
