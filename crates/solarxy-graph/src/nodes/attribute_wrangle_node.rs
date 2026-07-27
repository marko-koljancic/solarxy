//! The `attribute_wrangle` modifier: runs a small program once per element,
//! reading and writing attributes by name.
//!
//! This is the node the attribute substrate was built for. 0.8.0 shipped
//! typed, domained lanes and two authoring nodes that write a constant and a
//! seeded random; neither can compute a lane *from* another lane, or from a
//! point's own position. A wrangle can, which is what turns attributes from
//! storage into a medium.

use solarxy_kernel::AttributeDomain;

use super::common::{geometry_output, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::expr::{Runner, parse_program};
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};

/// Elements above which the node warns that a re-cook has stopped being
/// instant.
///
/// **Measured, not guessed** (`cargo run --release -p solarxy-graph
/// --example wrangle_cost`). Native release, per element: a trivial
/// assignment costs 0.05 to 0.19 us, the colour-by-position default 0.30 to
/// 0.87 us, and a local-plus-trig-plus-displace program 0.44 to 0.51 us. So
/// one 60fps frame buys roughly 20,000 to 56,000 elements for a realistic
/// program, and 66,049 points already costs 19.6 ms on the default and
/// 29.1 ms on the heavy one.
///
/// The ceiling sits at 50,000, *below* the native measurement rather than
/// at it, for two reasons. The cook is single-threaded and on web runs in
/// the browser's main wasm instance, which is slower than native by a ratio
/// that is not fixed. And a parameter drag re-cooks continuously, so a
/// wrangle upstream of a dragged param has to fit in a frame repeatedly,
/// not once.
pub const ELEMENT_WARN_CEILING: usize = 50_000;

const DEFAULT_PROGRAM: &str = "@Cd = set(@P.x + 0.5, @P.y + 0.5, @P.z + 0.5);";

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "attribute_wrangle",
        version: 1,
        display_name: "Attribute Wrangle",
        category: Category::Attribute,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
                .default_port()
                .doc("The geometry the program runs over."),
        ],
        outputs: vec![geometry_output()],
        params: params_with(
            "Attribute Wrangle",
            vec![
                ParamSpec::new(
                    "domain",
                    "Run Over",
                    "wrangle",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("point", "Points"),
                            EnumVariant::new("primitive", "Primitives"),
                        ],
                    },
                    ParamValue::Enum("point".into()),
                )
                .doc(
                    "Which elements the program runs once for. Points is the \
                     usual choice and the only domain that can move geometry, \
                     because `@P` is a point attribute. Primitives runs once \
                     per triangle, segment or point primitive and reaches the \
                     primitive lanes `attribute_promote` writes.",
                ),
                ParamSpec::new(
                    "program",
                    "Program",
                    "wrangle",
                    ParamType::Snippet,
                    ParamValue::Text(DEFAULT_PROGRAM.into()),
                )
                .doc(
                    "Statements separated by `;`, each assigning to an \
                     `@attribute` or a local. Reads the same maths the \
                     expression language offers: around thirty builtins, \
                     `$T` for scene time, `ch(\"box1/width\")` to read another \
                     node's parameter, and `npoints()` or `bbox(\"size\")` for \
                     the incoming geometry.\n\n\
                     The element scope is `@P` (position), `@N` (normal), \
                     `@Cd` (colour), `@uv`, plus `@ptnum` / `@numpt` (or \
                     `@primnum` / `@numprim`) and any lane on the input. \
                     Declare locals with `float`, `vector2`, `vector` or \
                     `vector4`.\n\n\
                     There is no `if` and no `for`; use the `? :` conditional \
                     for a branching value. A lane the input does not carry is \
                     created at the width of its first assignment.",
                ),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Runs a small program once per point or per primitive, reading \
              and writing attributes by name. This is the general-purpose \
              attribute tool: where `attribute_create` writes a constant and \
              `attribute_randomize` writes noise, a wrangle computes a lane \
              from whatever else is on the geometry.\n\n\
              `@Cd = set(@P.x + 0.5, @P.y + 0.5, @P.z + 0.5);` colours the \
              geometry by position and shows immediately, because `@Cd` is the \
              reserved colour lane the viewport already displays. \
              `@P = set(@P.x, @P.y + sin(@P.x * 4 + $T), @P.z);` ripples the \
              surface, and animates once playback is running.\n\n\
              A parse error names the line and column and badges the node. An \
              arithmetic edge such as division by zero is not an error: it \
              yields the IEEE result, so one bad element cannot blank a scene.",
        search_aliases: &[
            "wrangle",
            "attribute",
            "vex",
            "expression",
            "snippet",
            "code",
            "script",
        ],
        glyph: "attribute_wrangle",
        role: NodeRole::Standard,
        cook,
        migrate: None,
    }
}

fn cook(p: &ResolvedParams, inputs: &Inputs, cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let Some(input) = inputs.geometry("geometry") else {
        return Ok(CookOutcome::Done(Outputs::geometry(
            solarxy_kernel::GeometrySet::empty(),
        )));
    };

    let source = p.text("program");
    if source.trim().is_empty() {
        cx.warn("attribute_wrangle has no program; the input passes through unchanged");
        return Ok(CookOutcome::Done(Outputs::geometry((**input).clone())));
    }

    // Parsed once per cook, never per element.
    let program = parse_program(source).map_err(|e| {
        let (line, col) = e.line_col(source);
        CookError::Params(format!("line {line}, column {col}: {}", e.message))
    })?;

    if !program.writes_anything() {
        cx.warn(
            "this program assigns nothing, so the geometry passes through unchanged; \
             assign an attribute such as `@Cd`",
        );
    }

    let domain = match p.enum_key("domain") {
        "primitive" => AttributeDomain::Primitive,
        _ => AttributeDomain::Point,
    };

    let elements: usize = input
        .meshes
        .iter()
        .map(|m| solarxy_kernel::wrangle::element_count(m, domain))
        .sum();
    if elements > ELEMENT_WARN_CEILING {
        cx.warn(format!(
            "running over {elements} elements, above the {ELEMENT_WARN_CEILING} where a \
             re-cook stops feeling instant: the cook is single-threaded, and dragging a \
             parameter upstream of this node re-runs the whole program every frame. \
             Reduce the geometry upstream, or expect the wait."
        ));
    }

    let runner = Runner::new(&program, cx.eval, source);
    let out = solarxy_kernel::wrangle::wrangle(input, domain, &program.lane_bindings(), &runner)
        .map_err(|e| CookError::Params(e.to_string()))?;
    Ok(CookOutcome::Done(Outputs::geometry(out)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook::InputSlot;
    use crate::params::ParamSource;
    use crate::registry::coerce::Value;
    use solarxy_kernel::primitives::generate_box;
    use solarxy_kernel::{AttributeData, GeometrySet};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn run_with(
        stored: BTreeMap<String, ParamSource>,
    ) -> (Result<CookOutcome, CookError>, Vec<String>) {
        let resolved =
            crate::registry::resolve::resolve_params(&stored, &descriptor().params).unwrap();
        let mut slots = BTreeMap::new();
        slots.insert(
            "geometry".to_string(),
            InputSlot::Single(Value::Geometry(Arc::new(GeometrySet::from_mesh(
                generate_box(1.0, 1.0, 1.0, 1, 1, 1),
            )))),
        );
        let inputs = Inputs::new(slots);
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let outcome = cook(&resolved, &inputs, &mut cx);
        (outcome, cx.take_warnings())
    }

    fn program_of(src: &str) -> BTreeMap<String, ParamSource> {
        let mut stored = BTreeMap::new();
        stored.insert(
            "program".to_string(),
            ParamSource::Literal(ParamValue::Text(src.into())),
        );
        stored
    }

    fn set_of(out: &Outputs) -> &Arc<GeometrySet> {
        let Some(Value::Geometry(set)) = out.get("geometry") else {
            panic!("outputs geometry");
        };
        set
    }

    fn done(outcome: Result<CookOutcome, CookError>) -> Outputs {
        match outcome.expect("cooks") {
            CookOutcome::Done(out) => out,
            CookOutcome::Pending(_) => panic!("cooks synchronously"),
        }
    }

    #[test]
    fn the_default_program_colours_by_position() {
        let (outcome, warnings) = run_with(BTreeMap::new());
        let out = done(outcome);
        let lane = set_of(&out).meshes[0]
            .attributes
            .get(solarxy_kernel::reserved::COLOR)
            .expect("the colour lane");
        assert!(matches!(lane, AttributeData::Vec4(_)));
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn a_parse_error_is_a_cook_error_naming_line_and_column() {
        let (outcome, _) = run_with(program_of("@Cd = 1;\n@P = ;"));
        let err = outcome.expect_err("a bad program fails the cook");
        let message = err.to_string();
        assert!(message.contains("line 2"), "{message}");
    }

    #[test]
    fn an_empty_program_passes_through_with_a_warning() {
        let (outcome, warnings) = run_with(program_of("   "));
        let out = done(outcome);
        assert!(set_of(&out).meshes[0].attributes.is_empty());
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn a_program_that_only_reads_warns_that_it_writes_nothing() {
        let (outcome, warnings) = run_with(program_of("float t = @P.x;"));
        done(outcome);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("assigns nothing"), "{warnings:?}");
    }

    #[test]
    fn the_primitive_domain_writes_the_primitive_map() {
        let mut stored = program_of("@pid = @primnum;");
        stored.insert(
            "domain".to_string(),
            ParamSource::Literal(ParamValue::Enum("primitive".into())),
        );
        let (outcome, _) = run_with(stored);
        let out = done(outcome);
        assert!(
            set_of(&out).meshes[0]
                .primitive_attributes
                .contains_key("pid")
        );
    }

    #[test]
    fn assigning_p_moves_the_geometry() {
        let (outcome, _) = run_with(program_of("@P = set(@P.x, @P.y + 1, @P.z);"));
        let out = done(outcome);
        let before = generate_box(1.0, 1.0, 1.0, 1, 1, 1);
        for (a, b) in set_of(&out).meshes[0]
            .positions
            .iter()
            .zip(before.positions.iter())
        {
            assert!((a[1] - (b[1] + 1.0)).abs() < 1e-6);
        }
    }

    #[test]
    fn a_small_input_does_not_warn_about_cost() {
        // The ceiling has to be far enough above ordinary geometry that the
        // warning means something when it does fire.
        let (outcome, warnings) = run_with(program_of("@Cd = set(1, 0, 0);"));
        done(outcome);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn a_large_input_warns_with_the_measured_ceiling() {
        let big = solarxy_kernel::primitives::generate_plane(2.0, 2.0, 400, 400);
        let points = big.positions.len();
        assert!(points > ELEMENT_WARN_CEILING, "fixture must cross it");

        let resolved = crate::registry::resolve::resolve_params(
            &program_of("@Cd = set(1, 0, 0);"),
            &descriptor().params,
        )
        .unwrap();
        let mut slots = BTreeMap::new();
        slots.insert(
            "geometry".to_string(),
            InputSlot::Single(Value::Geometry(Arc::new(GeometrySet::from_mesh(big)))),
        );
        let inputs = Inputs::new(slots);
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        done(cook(&resolved, &inputs, &mut cx));
        let warnings = cx.take_warnings();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains(&ELEMENT_WARN_CEILING.to_string()),
            "{warnings:?}"
        );
    }

    #[test]
    fn time_reads_zero_so_a_stopped_cook_is_reproducible() {
        // The node's own cook builds no clock; the driver supplies one. A
        // default context is stopped, which is what keeps golden captures
        // and CLI cooks deterministic.
        let (outcome, _) = run_with(program_of("@t = $T;"));
        let out = done(outcome);
        let Some(AttributeData::Float(lane)) = set_of(&out).meshes[0].attributes.get("t") else {
            panic!("float lane");
        };
        assert!(lane.iter().all(|v| *v == 0.0));
    }
}
