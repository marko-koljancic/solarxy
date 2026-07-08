//! Shared node-descriptor building blocks: the implicit `general` group,
//! the geometry `rendering` group, and the default geometry output port.
//! These factor out the catalog conventions (part II, section 11) so each
//! node file declares only what is specific to it.

use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::PortSpec;

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

/// The `rendering` group on geometry-producing subflow nodes: the
/// display-flag storage plus the shadow participation flags Minimystix
/// silently resolved to false.
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
        .doc("Whether this node's geometry is displayed."),
        ParamSpec::new(
            "cast_shadow",
            "Cast Shadow",
            "rendering",
            ParamType::Bool,
            ParamValue::Bool(true),
        )
        .doc("Whether this geometry casts shadows."),
        ParamSpec::new(
            "receive_shadow",
            "Receive Shadow",
            "rendering",
            ParamType::Bool,
            ParamValue::Bool(true),
        )
        .doc("Whether this geometry receives shadows."),
    ]
}

/// The default geometry output port, key `geometry` (every geometry node's
/// single default output).
#[must_use]
pub fn geometry_output() -> PortSpec {
    PortSpec::single("geometry", "Geometry", DataType::Geometry, false).default_port()
}

/// Assembles a node's full param list: `general`, then the node-specific
/// groups, then `rendering`.
#[must_use]
pub fn params_with(display_name: &str, specific: Vec<ParamSpec>) -> Vec<ParamSpec> {
    let mut params = general_params(display_name);
    params.extend(specific);
    params.extend(rendering_params());
    params
}
