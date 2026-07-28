//! Tree walking evaluation.
//!
//! The evaluator is a leaf: it knows nothing about documents, cooks or
//! geometry. Everything it cannot compute from the tree alone arrives
//! through two narrow traits on [`EvalCtx`], which the engine implements.
//! That keeps `expr/` free of a dependency on `Document`,
//! `Registry` or `Inputs`, and it is what lets the same evaluator serve
//! both a parameter and, later, a wrangle body.

use super::ast::{BinaryOp, Expr, UnaryOp, Var};
use super::builtins;
use super::error::ExprError;
use super::value::{Value, map1, map2};
use std::ops::Range;

/// The scene clock as an expression sees it.
///
/// Zero while the runtime is stopped (which is every cook until F3 lands),
/// so golden captures, CLI cooks and `.slxy` reload stay reproducible.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneTime {
    pub seconds: f64,
    pub frame: f64,
    pub fps: f64,
}

impl Default for SceneTime {
    fn default() -> Self {
        // Stopped: $T and $F read zero. fps is still meaningful, so it
        // carries the default frame rate rather than a zero that would make
        // `$F / $FPS` divide by zero.
        Self {
            seconds: 0.0,
            frame: 0.0,
            fps: 24.0,
        }
    }
}

/// Cross-node parameter reads. Implemented by the engine, in [`crate::refs`].
pub trait ParamRefs {
    /// Reads the parameter at `path`, relative to the referring node.
    ///
    /// # Errors
    /// A path that does not resolve, is ambiguous, or closes a cycle.
    fn read(&self, path: &str) -> Result<Value, String>;
}

/// Queries against the node's own gathered geometry inputs. Implemented by
/// the engine, in [`crate::cook::geo_queries`].
pub trait GeoQueries {
    /// # Errors
    /// The queried input is not connected. Every query is fallible for the
    /// same reason `bbox` is: a node whose input is empty has no answer,
    /// and `0` is a plausible wrong number rather than a missing one.
    fn npoints(&self) -> Result<f64, String>;
    /// # Errors
    /// The queried input is not connected.
    fn nprims(&self) -> Result<f64, String>;
    /// # Errors
    /// The queried input is not connected.
    fn nmeshes(&self) -> Result<f64, String>;
    /// `field` is one of `xmin ymin zmin xmax ymax zmax size center`.
    ///
    /// # Errors
    /// An unrecognised field name, or the input is not connected.
    fn bbox(&self, field: &str) -> Result<Value, String>;
    /// # Errors
    /// The queried input is not connected.
    fn centroid(&self) -> Result<[f64; 3], String>;
}

/// Per-element reads inside a wrangle: `@attributes` and locals, both
/// addressed by the slot the statement parser assigned them.
///
/// Slots rather than names, so the per-element loop indexes an array
/// instead of hashing a string once per read.
pub trait ElementScope {
    /// # Errors
    /// The lane is absent from the input and nothing has assigned it yet,
    /// so it has no value to read.
    fn attr(&self, slot: usize) -> Result<Value, String>;
    /// # Errors
    /// The local has not been assigned on this element's path through the
    /// program.
    fn local(&self, slot: usize) -> Result<Value, String>;
}

/// Everything an expression can read beyond its own tree.
///
/// Every capability is optional because most resolve sites have none: a
/// light's `intensity` has no geometry inputs and no element scope, and an
/// absent capability must say so by name rather than quietly evaluating to
/// zero. That rule is not theoretical; the geometry queries shipped
/// answering `0` on an unconnected input and cooked an invisible box with
/// nothing badged.
#[derive(Default, Clone, Copy)]
pub struct EvalCtx<'a> {
    pub time: SceneTime,
    pub refs: Option<&'a dyn ParamRefs>,
    pub geo: Option<&'a dyn GeoQueries>,
    pub element: Option<&'a dyn ElementScope>,
}

impl std::fmt::Debug for EvalCtx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EvalCtx")
            .field("time", &self.time)
            .field("refs", &self.refs.is_some())
            .field("geo", &self.geo.is_some())
            .field("element", &self.element.is_some())
            .finish()
    }
}

impl<'a> EvalCtx<'a> {
    #[must_use]
    pub fn new(time: SceneTime) -> Self {
        Self {
            time,
            refs: None,
            geo: None,
            element: None,
        }
    }

    #[must_use]
    pub fn with_refs(mut self, refs: &'a dyn ParamRefs) -> Self {
        self.refs = Some(refs);
        self
    }

    #[must_use]
    pub fn with_geo(mut self, geo: &'a dyn GeoQueries) -> Self {
        self.geo = Some(geo);
        self
    }

    #[must_use]
    pub fn with_element(mut self, element: &'a dyn ElementScope) -> Self {
        self.element = Some(element);
        self
    }
}

/// Evaluates a parsed tree.
///
/// # Errors
/// Any type mismatch, arity error, unknown name, or unavailable capability.
pub fn eval(expr: &Expr, ctx: &EvalCtx) -> Result<Value, ExprError> {
    eval_inner(expr, ctx, 0..0)
}

fn eval_inner(expr: &Expr, ctx: &EvalCtx, span: Range<usize>) -> Result<Value, ExprError> {
    match expr {
        Expr::Number(v) => Ok(Value::Float(*v)),
        Expr::Str(_) => Err(ExprError::new(
            "a string is not a value here; it is only an argument to ch() or bbox()",
            span,
        )),
        Expr::Var(v) => Ok(Value::Float(match v {
            Var::Time => ctx.time.seconds,
            Var::Frame => ctx.time.frame,
            Var::Fps => ctx.time.fps,
            Var::Pi => std::f64::consts::PI,
            Var::E => std::f64::consts::E,
        })),
        Expr::Unary { op, rhs } => {
            let v = eval_inner(rhs, ctx, span.clone())?;
            match op {
                UnaryOp::Neg => map1(v, "unary `-`", &span, |x| -x),
                UnaryOp::Not => Ok(Value::Bool(!v.as_bool("`!`", &span)?)),
            }
        }
        Expr::Binary { op, lhs, rhs } => eval_binary(*op, lhs, rhs, ctx, span),
        Expr::Ternary {
            cond,
            then,
            otherwise,
        } => {
            let c = eval_inner(cond, ctx, span.clone())?;
            // Only the taken branch is evaluated, so a guarded division is
            // actually guarded: `d == 0 ? 0 : n / d` never divides.
            if c.as_bool("a `? :` condition", &span)? {
                eval_inner(then, ctx, span)
            } else {
                eval_inner(otherwise, ctx, span)
            }
        }
        Expr::Member { base, component } => {
            let v = eval_inner(base, ctx, span.clone())?;
            let idx = component.index();
            let Some(lanes) = v.lanes() else {
                return Err(ExprError::new(
                    format!("a {} has no components", v.type_name()),
                    span,
                ));
            };
            lanes.get(idx).map(|f| Value::Float(*f)).ok_or_else(|| {
                ExprError::new(
                    format!(
                        "a {} has no `.{}` component",
                        v.type_name(),
                        ["x", "y", "z", "w"][idx]
                    ),
                    span,
                )
            })
        }
        Expr::Call { name, args, span } => eval_call(name, args, ctx, span),
        // The parser only emits these two under a wrangle scope, so an
        // absent capability here is a bug rather than user input; it still
        // names itself instead of yielding a plausible zero.
        Expr::Attr(slot) => ctx
            .element
            .ok_or_else(|| {
                ExprError::new(
                    "an `@attribute` is only readable inside a wrangle",
                    span.clone(),
                )
            })?
            .attr(*slot)
            .map_err(|m| ExprError::new(m, span)),
        Expr::Local(slot) => ctx
            .element
            .ok_or_else(|| {
                ExprError::new("a local is only readable inside a wrangle", span.clone())
            })?
            .local(*slot)
            .map_err(|m| ExprError::new(m, span)),
    }
}

fn eval_binary(
    op: BinaryOp,
    lhs: &Expr,
    rhs: &Expr,
    ctx: &EvalCtx,
    span: Range<usize>,
) -> Result<Value, ExprError> {
    // Short-circuit before evaluating the right side, so `a != 0 && b / a`
    // behaves the way every language has taught people to expect.
    if matches!(op, BinaryOp::And | BinaryOp::Or) {
        let l = eval_inner(lhs, ctx, span.clone())?.as_bool("a logical operand", &span)?;
        return match (op, l) {
            (BinaryOp::And, false) => Ok(Value::Bool(false)),
            (BinaryOp::Or, true) => Ok(Value::Bool(true)),
            _ => Ok(Value::Bool(
                eval_inner(rhs, ctx, span.clone())?.as_bool("a logical operand", &span)?,
            )),
        };
    }

    let a = eval_inner(lhs, ctx, span.clone())?;
    let b = eval_inner(rhs, ctx, span.clone())?;
    match op {
        BinaryOp::Add => map2(a, b, "`+`", &span, |x, y| x + y),
        BinaryOp::Sub => map2(a, b, "`-`", &span, |x, y| x - y),
        BinaryOp::Mul => map2(a, b, "`*`", &span, |x, y| x * y),
        // IEEE division: x/0 is an infinity, not a cook failure. One bad
        // element must not blank a scene (the keep-last-good posture).
        BinaryOp::Div => map2(a, b, "`/`", &span, |x, y| x / y),
        BinaryOp::Rem => map2(a, b, "`%`", &span, |x, y| x % y),
        BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
            let x = a.as_float("a comparison", &span)?;
            let y = b.as_float("a comparison", &span)?;
            Ok(Value::Bool(match op {
                BinaryOp::Lt => x < y,
                BinaryOp::Le => x <= y,
                BinaryOp::Gt => x > y,
                _ => x >= y,
            }))
        }
        BinaryOp::Eq => Ok(Value::Bool(a == b)),
        BinaryOp::Ne => Ok(Value::Bool(a != b)),
        BinaryOp::And | BinaryOp::Or => unreachable!("handled above"),
    }
}

/// The literal path argument of a reference function.
fn path_arg(args: &[Expr], name: &str, span: &Range<usize>) -> Result<String, ExprError> {
    match args {
        [Expr::Str(s)] => Ok(s.clone()),
        _ => Err(ExprError::new(
            format!("{name}() takes exactly one quoted argument, e.g. {name}(\"...\")"),
            span.clone(),
        )),
    }
}

fn eval_call(
    name: &str,
    args: &[Expr],
    ctx: &EvalCtx,
    span: &Range<usize>,
) -> Result<Value, ExprError> {
    // Reference functions take a literal path and are handled before the
    // arguments are evaluated, because a string is not a value.
    match name {
        "ch" => {
            let path = path_arg(args, "ch", span)?;
            let Some(refs) = ctx.refs else {
                return Err(ExprError::new(
                    "ch() is not available here: this parameter is resolved outside the document",
                    span.clone(),
                ));
            };
            return refs
                .read(&path)
                .map_err(|message| ExprError::new(message, span.clone()));
        }
        "bbox" => {
            let field = path_arg(args, "bbox", span)?;
            let Some(geo) = ctx.geo else {
                return Err(ExprError::new(
                    "bbox() is only available on a node with geometry inputs",
                    span.clone(),
                ));
            };
            return geo
                .bbox(&field)
                .map_err(|message| ExprError::new(message, span.clone()));
        }
        "npoints" | "nprims" | "nmeshes" | "centroid" => {
            if !args.is_empty() {
                return Err(ExprError::new(
                    format!("{name}() takes no arguments"),
                    span.clone(),
                ));
            }
            let Some(geo) = ctx.geo else {
                return Err(ExprError::new(
                    format!("{name}() is only available on a node with geometry inputs"),
                    span.clone(),
                ));
            };
            let value = match name {
                "npoints" => geo.npoints().map(Value::Float),
                "nprims" => geo.nprims().map(Value::Float),
                "nmeshes" => geo.nmeshes().map(Value::Float),
                _ => geo.centroid().map(Value::Vec3),
            };
            return value.map_err(|message| ExprError::new(message, span.clone()));
        }
        _ => {}
    }

    let mut values = Vec::with_capacity(args.len());
    for a in args {
        values.push(eval_inner(a, ctx, span.clone())?);
    }
    match builtins::call(name, &values, span)? {
        Some(v) => Ok(v),
        None => Err(ExprError::new(
            format!("unknown function `{name}()`"),
            span.clone(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::parse;

    fn run(src: &str) -> Value {
        let parsed = parse(src).expect("parses");
        eval(&parsed.root, &EvalCtx::default()).expect("evaluates")
    }

    fn run_at(src: &str, time: SceneTime) -> Value {
        let parsed = parse(src).expect("parses");
        eval(&parsed.root, &EvalCtx::new(time)).expect("evaluates")
    }

    fn fail(src: &str) -> ExprError {
        let parsed = parse(src).expect("parses");
        eval(&parsed.root, &EvalCtx::default()).expect_err("should fail")
    }

    #[test]
    fn arithmetic_respects_precedence_end_to_end() {
        assert_eq!(run("1 + 2 * 3"), Value::Float(7.0));
        assert_eq!(run("(1 + 2) * 3"), Value::Float(9.0));
        assert_eq!(run("10 - 2 - 3"), Value::Float(5.0));
        assert_eq!(run("7 % 4"), Value::Float(3.0));
        assert_eq!(run("-2 * 3"), Value::Float(-6.0));
    }

    #[test]
    fn vectors_broadcast_and_select_components() {
        assert_eq!(run("set(1,2,3) * 2"), Value::Vec3([2.0, 4.0, 6.0]));
        assert_eq!(
            run("set(1,2,3) + set(10,20,30)"),
            Value::Vec3([11.0, 22.0, 33.0])
        );
        assert_eq!(run("set(1,2,3).y"), Value::Float(2.0));
        assert_eq!(run("set(1,2,3,4).w"), Value::Float(4.0));
    }

    #[test]
    fn selecting_a_component_a_value_does_not_have_says_so() {
        let e = fail("set(1,2).z");
        assert!(e.message.contains("no `.z` component"), "{e:?}");
    }

    #[test]
    fn comparisons_and_logic_produce_bools() {
        assert_eq!(run("1 < 2"), Value::Bool(true));
        assert_eq!(run("1 > 2"), Value::Bool(false));
        assert_eq!(run("1 < 2 && 3 > 2"), Value::Bool(true));
        assert_eq!(run("!(1 < 2)"), Value::Bool(false));
        assert_eq!(run("set(1,2) == set(1,2)"), Value::Bool(true));
    }

    #[test]
    fn logic_short_circuits_so_a_guard_actually_guards() {
        // The right side would be an unknown function; it must never run.
        assert_eq!(run("1 > 2 && nope()"), Value::Bool(false));
        assert_eq!(run("1 < 2 || nope()"), Value::Bool(true));
    }

    #[test]
    fn a_ternary_evaluates_only_the_taken_branch() {
        assert_eq!(run("1 < 2 ? 10 : nope()"), Value::Float(10.0));
        assert_eq!(run("1 > 2 ? nope() : 20"), Value::Float(20.0));
    }

    #[test]
    fn division_by_zero_is_an_infinity_not_a_failure() {
        // Keep-last-good: one bad element must not blank a scene.
        let Value::Float(v) = run("1 / 0") else {
            panic!()
        };
        assert!(v.is_infinite());
    }

    #[test]
    fn constants_and_the_stopped_clock() {
        let Value::Float(pi) = run("$PI") else {
            panic!()
        };
        assert!((pi - std::f64::consts::PI).abs() < 1e-12);
        // Stopped by default, so every cook is reproducible.
        assert_eq!(run("$T"), Value::Float(0.0));
        assert_eq!(run("$F"), Value::Float(0.0));
        assert_eq!(run("$FPS"), Value::Float(24.0));
    }

    #[test]
    fn a_running_clock_feeds_the_time_variables() {
        let t = SceneTime {
            seconds: 2.5,
            frame: 60.0,
            fps: 24.0,
        };
        assert_eq!(run_at("$T", t), Value::Float(2.5));
        assert_eq!(run_at("$F", t), Value::Float(60.0));
        assert_eq!(run_at("$F / $FPS", t), Value::Float(2.5));
    }

    #[test]
    fn an_unknown_function_names_itself() {
        let e = fail("wobble(1)");
        assert!(e.message.contains("unknown function `wobble()`"), "{e:?}");
    }

    #[test]
    fn reference_functions_say_why_they_are_unavailable() {
        // These four sites resolve outside a cook, so the capability is
        // genuinely absent; naming it beats evaluating to zero.
        let e = fail("ch(\"../a/b\")");
        assert!(e.message.contains("ch() is not available"), "{e:?}");
        let e = fail("npoints()");
        assert!(e.message.contains("geometry inputs"), "{e:?}");
        let e = fail("bbox(\"xmin\")");
        assert!(e.message.contains("geometry inputs"), "{e:?}");
    }

    #[test]
    fn reference_functions_reject_a_computed_path() {
        let e = fail("ch(1 + 2)");
        assert!(e.message.contains("one quoted argument"), "{e:?}");
    }

    struct FakeGeo;
    impl GeoQueries for FakeGeo {
        fn npoints(&self) -> Result<f64, String> {
            Ok(8.0)
        }
        fn nprims(&self) -> Result<f64, String> {
            Ok(12.0)
        }
        fn nmeshes(&self) -> Result<f64, String> {
            Ok(1.0)
        }
        fn bbox(&self, field: &str) -> Result<Value, String> {
            match field {
                "xmin" => Ok(Value::Float(-1.0)),
                "size" => Ok(Value::Vec3([2.0, 2.0, 2.0])),
                other => Err(format!("`{other}` is not a bbox field")),
            }
        }
        fn centroid(&self) -> Result<[f64; 3], String> {
            Ok([0.0, 1.0, 0.0])
        }
    }

    /// A connected-but-answerless input: every query fails, which is what
    /// `InputGeo` does when nothing is plugged into the queried port.
    struct EmptyGeo;
    impl GeoQueries for EmptyGeo {
        fn npoints(&self) -> Result<f64, String> {
            Err("npoints() has no geometry: this node's input is not connected".into())
        }
        fn nprims(&self) -> Result<f64, String> {
            Err("nprims() has no geometry: this node's input is not connected".into())
        }
        fn nmeshes(&self) -> Result<f64, String> {
            Err("nmeshes() has no geometry: this node's input is not connected".into())
        }
        fn bbox(&self, _field: &str) -> Result<Value, String> {
            Err("bbox() has no geometry: this node's input is not connected".into())
        }
        fn centroid(&self) -> Result<[f64; 3], String> {
            Err("centroid() has no geometry: this node's input is not connected".into())
        }
    }

    /// A failing query must surface as an expression error carrying the
    /// query's own message, not be swallowed into a number.
    #[test]
    fn an_empty_input_makes_every_geometry_query_an_error() {
        let geo = EmptyGeo;
        for src in [
            "npoints()",
            "nprims()",
            "nmeshes()",
            "centroid()",
            "bbox(\"size\")",
        ] {
            let parsed = parse(src).expect("parses");
            let ctx = EvalCtx::new(SceneTime::default()).with_geo(&geo);
            let err = eval(&parsed.root, &ctx).expect_err("should fail");
            assert!(err.message.contains("not connected"), "{src}: {err:?}");
        }
    }

    struct FakeRefs;
    impl ParamRefs for FakeRefs {
        fn read(&self, path: &str) -> Result<Value, String> {
            match path {
                "../box1/size" => Ok(Value::Float(4.0)),
                other => Err(format!("`{other}` does not resolve")),
            }
        }
    }

    #[test]
    fn geometry_queries_read_through_the_context() {
        let geo = FakeGeo;
        let ctx = EvalCtx::default().with_geo(&geo);
        let parsed = parse("npoints() * 2 + nprims()").expect("parses");
        assert_eq!(eval(&parsed.root, &ctx).unwrap(), Value::Float(28.0));
        let parsed = parse("bbox(\"size\").x").expect("parses");
        assert_eq!(eval(&parsed.root, &ctx).unwrap(), Value::Float(2.0));
        let parsed = parse("centroid().y").expect("parses");
        assert_eq!(eval(&parsed.root, &ctx).unwrap(), Value::Float(1.0));
    }

    #[test]
    fn an_unknown_bbox_field_reports_the_name() {
        let geo = FakeGeo;
        let ctx = EvalCtx::default().with_geo(&geo);
        let parsed = parse("bbox(\"nope\")").expect("parses");
        let e = eval(&parsed.root, &ctx).unwrap_err();
        assert!(e.message.contains("not a bbox field"), "{e:?}");
    }

    #[test]
    fn ch_reads_through_the_context_and_surfaces_its_errors() {
        let refs = FakeRefs;
        let ctx = EvalCtx::default().with_refs(&refs);
        let parsed = parse("ch(\"../box1/size\") * 2").expect("parses");
        assert_eq!(eval(&parsed.root, &ctx).unwrap(), Value::Float(8.0));
        let parsed = parse("ch(\"../nope/x\")").expect("parses");
        let e = eval(&parsed.root, &ctx).unwrap_err();
        assert!(e.message.contains("does not resolve"), "{e:?}");
    }

    #[test]
    fn a_realistic_driven_parameter_evaluates() {
        let refs = FakeRefs;
        let t = SceneTime {
            seconds: 0.5,
            frame: 12.0,
            fps: 24.0,
        };
        let ctx = EvalCtx::new(t).with_refs(&refs);
        let parsed = parse("ch(\"../box1/size\") * 2 + sin($T * $PI) * 0").expect("parses");
        assert_eq!(eval(&parsed.root, &ctx).unwrap(), Value::Float(8.0));
    }
}
