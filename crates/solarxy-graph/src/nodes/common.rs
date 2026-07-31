//! Shared node-descriptor building blocks: the implicit `general` group,
//! the geo container's `rendering` group, the default geometry output
//! port, and the silent-strip migrations. These factor out the
//! catalog conventions so each node file declares
//! only what is specific to it.

use solarxy_kernel::copy::CopyMode;
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
        // Multi-line since 0.8.1: a description is prose, and a
        // single-line input made anything past a few words unreadable
        // while you typed it. Storage is unchanged (`ParamValue::Text`),
        // so this is a widget change, not a migration.
        ParamSpec::new(
            "description",
            "Description",
            "general",
            ParamType::MultilineText,
            ParamValue::Text(String::new()),
        )
        .doc("Free-text notes shown as the node subtitle."),
    ]
}

/// The `rendering` group on the `geo` container only (root render flags,
/// both wired end to end): additive visibility and per-object
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
pub(super) fn strip_keys(params: &mut serde_json::Map<String, serde_json::Value>, keys: &[&str]) {
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
///
/// Documented here rather than at each call site, so `transform` and the
/// `geo` container cannot drift into describing the same control two ways.
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
    .doc(
        "The order the three Euler angles are applied in. Rotations do not \
         commute, so the same three numbers land the object differently \
         depending on this: XYZ rotates about X first, then Y, then Z.\n\n\
         Change it only when matching another package's convention, or when \
         an axis you are animating has ended up gimbal-locked against \
         another. If a rotation is behaving unintuitively, this is usually \
         the reason.",
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

/// Resolves an instanced input into real geometry, for the operations that
/// cannot carry placements through.
///
/// `GeometrySet::instances` states the rule this enforces: an operation that
/// cannot carry placements must bake first rather than drop them, because
/// silently losing the list deletes every copy but one with no error
/// anywhere. Most operations are in that position, either because they need
/// the copies to exist (scatter, bounds, validate, the exports) or because
/// their meaning is per copy (a wrangle over `@ptnum`, a delete by angle, a
/// mirror across a plane the copies straddle).
///
/// The bake is announced. It can turn a scene that ran on one prototype into
/// one carrying ten thousand real copies, and a cliff that steep should not
/// arrive without a word: the warning names the count so the reader can see
/// which node did it and how much it cost. Uninstanced input, which is
/// almost all input, borrows straight through and warns about nothing.
///
/// # Errors
/// Propagates the bake's ceiling error, whose message already names the way
/// out.
pub(super) fn baked_input<'a>(
    set: &'a solarxy_kernel::GeometrySet,
    cx: &mut CookCtx,
) -> Result<std::borrow::Cow<'a, solarxy_kernel::GeometrySet>, CookError> {
    if !set.is_instanced() {
        return Ok(std::borrow::Cow::Borrowed(set));
    }
    let count = set.instance_count();
    let baked = set
        .baked()
        .map_err(|message| CookError::Failed { message })?;
    cx.warn(format!(
        "this node cannot work on instanced geometry, so the {count} \
         placements from upstream were baked into real copies here"
    ));
    Ok(baked)
}

/// The `copy_mode` param, identical on both copy operations.
///
/// Documented here rather than at each call site for the same reason
/// [`rotate_order_param`] is: `copy_to_points` and `array` offer the same
/// choice, and two copies of the explanation would drift into describing one
/// control two ways. Only the presentation group differs, so it is passed in.
///
/// The key is `copy_mode` rather than `mode` because `array` already carries a
/// `mode` holding Linear and Radial, and because it is what the kernel calls
/// the argument.
#[must_use]
pub fn copy_mode_param(group: &str) -> ParamSpec {
    ParamSpec::new(
        "copy_mode",
        "Copy Mode",
        group,
        ParamType::Enum {
            variants: vec![
                EnumVariant::new("instance", "Instance"),
                EnumVariant::new("bake", "Bake"),
            ],
        },
        ParamValue::Enum("instance".to_string()),
    )
    .doc(
        "Whether the copies are real geometry or placements of one \
         prototype.\n\n\
         Instance keeps the input once and carries a transform per copy. Ten \
         thousand copies of a five-thousand-triangle rock cost five thousand \
         triangles rather than fifty million, so the copy count stops being \
         the number you budget against.\n\n\
         What it costs is what the rest of the graph can see. Downstream \
         nodes are handed the prototype and the placements, never the \
         individual copies, so there is no per-copy attribute edit, no \
         boolean against one copy, and no deleting the third one from the \
         left: those copies do not exist as geometry. This is the difference \
         between copying and instancing rather than a fast path and a slow \
         one.\n\n\
         Bake makes every copy real, which is what you choose when the \
         copies have to be edited afterwards. It is no harder to author, and \
         it answers a different question rather than an outdated one.",
    )
}

/// Maps the stored variant key onto the kernel's mode. An unknown key falls
/// back to the default rather than failing a cook, matching
/// [`rotate_order_from_key`].
#[must_use]
pub fn copy_mode_from_key(key: &str) -> CopyMode {
    match key {
        "bake" => CopyMode::Bake,
        _ => CopyMode::Instance,
    }
}

/// v1 -> v2 for the two copy operations (`copy_to_points` and `array`): both
/// gain [`copy_mode_param`], whose default is Instance.
///
/// Every v1 node was authored against an engine that could only bake, so Bake
/// is not a preference those nodes expressed; it is the behavior their authors
/// saw and built the rest of the graph around. Left to inherit the new
/// default, a saved scene would open with its copies collapsed into one
/// prototype and every downstream node reading something different, with
/// nothing on screen to say why. So the value is written in explicitly.
///
/// A node that somehow already carries the key keeps it, so re-running the
/// step is a no-op rather than an overwrite.
///
/// The value written is the BARE enum key, because migrations run on the raw
/// stored JSON, which holds a plain value per param (the one object form is
/// `{"$expr": ...}`) rather than a serialized `ParamSource`. Writing the
/// wrapped form instead fails to type on the way back in.
#[allow(clippy::unnecessary_wraps)] // signature matches MigrateFn
pub fn migrate_pin_copy_mode_to_bake(
    from: u32,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), MigrateError> {
    if from == 1 && !params.contains_key("copy_mode") {
        params.insert(
            "copy_mode".to_string(),
            serde_json::Value::String("bake".to_string()),
        );
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

/// Warns when a lane is written under a reserved attribute name with a
/// type other than its contractual one: the write succeeds (free-form
/// lanes are legal), but every consumer of the reserved name will ignore
/// it, which is surprising enough to say out loud. `written` is the
/// node's type enum key (`float`/`vec2`/`vec3`/`vec4`).
pub(super) fn warn_reserved_lane_mismatch(cx: &mut CookCtx, name: &str, written: &str) {
    use solarxy_kernel::reserved;
    let contractual = if name == reserved::COLOR {
        Some("vec4")
    } else if name == reserved::NORMAL {
        Some("vec3")
    } else if name == reserved::UV {
        Some("vec2")
    } else if name == reserved::PSCALE {
        Some("float")
    } else {
        None
    };
    if let Some(expected) = contractual
        && expected != written
    {
        cx.warn(format!(
            "the reserved attribute `{name}` carries {expected} by contract; \
             a {written} lane under that name is ignored by its consumers"
        ));
    }
}

/// Warns when the lane a node is about to write replaces (or shadows, for
/// the fixed `N`/`uv` buffers) an INPUT lane of a different type: the
/// write is legal, but downstream consumers keyed to the old type stop
/// matching, which deserves saying out loud. `written` is the node's type
/// enum key. Resolution is first-seen across meshes, the `attr_table`
/// convention.
pub(super) fn warn_input_lane_type_replaced(
    cx: &mut CookCtx,
    input: &solarxy_kernel::GeometrySet,
    name: &str,
    written: &str,
) {
    let existing = input
        .meshes
        .iter()
        .find_map(|m| crate::engine::attr_table::resolve_lane(m, name))
        .map(|l| l.ty());
    if let Some(existing) = existing
        && existing != written
    {
        cx.warn(format!(
            "`{name}` on the input is {existing}; this cook replaces it              with a {written} lane"
        ));
    }
}

/// The default geometry output port, key `geometry` (every geometry node's
/// single default output).
#[must_use]
pub fn geometry_output() -> PortSpec {
    PortSpec::single("geometry", "Geometry", DataType::Geometry, false)
        .default_port()
        .doc(
            "The cooked geometry. Being the default output, a drag from the \
             node's body wires from here, and a bypass passes the input \
             through it.",
        )
}

/// Assembles a node's full param list: `general`, then the node-specific
/// groups. (The `rendering` group is geo-container-only;
/// `geo_node` appends it explicitly.)
#[must_use]
pub fn params_with(display_name: &str, specific: Vec<ParamSpec>) -> Vec<ParamSpec> {
    let mut params = general_params(display_name);
    params.extend(specific);
    params
}
