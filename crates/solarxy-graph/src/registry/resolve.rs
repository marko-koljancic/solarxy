//! The param resolver: the single chokepoint between stored params and
//! compute bodies (sections 4 and 5).
//!
//! `resolve_params` runs before every compute. It reads literals (v1
//! refuses expressions), conforms values to the spec type, clamps to the
//! hard range, and converts units (degrees to radians, element-wise for
//! vectors). Compute bodies only ever see [`ResolvedParams`] typed getters
//! with values already in radians and SI; no compute body ever touches raw
//! params. This is the single seam expressions later drop into.
//!
//! The JSON conversions implement the schema-v1 commitment: literals
//! serialize as plain JSON values, expressions as `{"$expr": "..."}`.
//! Values are schema-typed, not self-describing, so parsing requires the
//! spec (`ParamSpec.ty` disambiguates Int vs Float vs Enum vs Text vs
//! Asset); migration hooks run on the raw JSON before this typing.

use std::collections::BTreeMap;

use serde_json::Value as Json;

use super::param_spec::{ParamSpec, ParamType, Unit};
use super::scalar;
use crate::expr::{EvalCtx, ExprError, Value as ExprValue};
use crate::params::{AssetId, ParamSource, ParamValue};

/// Why a node's params could not resolve (a cook error, not a command
/// error: the node badges and refuses to cook).
///
/// Every variant names the param, because a node with twenty params and a
/// bare "syntax error" is a scavenger hunt. The parse and eval variants
/// carry the [`ExprError`] whole so the editor can underline the offending
/// span rather than re-parsing to find it (decision M-17: a bad expression
/// is a value the user can fix by editing, so it badges rather than being
/// refused at `SetParam`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveFailure {
    /// The expression could not be parsed.
    ExpressionParse { key: String, error: ExprError },
    /// It parsed, but evaluating it failed (an unknown function, a type
    /// mismatch, an unresolvable `ch()` path).
    ExpressionEval { key: String, error: ExprError },
    /// It evaluated, but the result cannot become this param's type.
    ExpressionType { key: String, reason: String },
}

impl ResolveFailure {
    /// The param this failure is about.
    #[must_use]
    pub fn key(&self) -> &str {
        match self {
            ResolveFailure::ExpressionParse { key, .. }
            | ResolveFailure::ExpressionEval { key, .. }
            | ResolveFailure::ExpressionType { key, .. } => key,
        }
    }

    /// The source span to underline, when there is one.
    #[must_use]
    pub fn span(&self) -> Option<std::ops::Range<usize>> {
        match self {
            ResolveFailure::ExpressionParse { error, .. }
            | ResolveFailure::ExpressionEval { error, .. } => Some(error.span.clone()),
            ResolveFailure::ExpressionType { .. } => None,
        }
    }
}

impl std::fmt::Display for ResolveFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Parse and eval read the same to a user: the param, then what
            // went wrong. The variants stay distinct because the editor and
            // the tests care which stage failed.
            ResolveFailure::ExpressionParse { key, error }
            | ResolveFailure::ExpressionEval { key, error } => {
                write!(f, "param '{key}': {error}")
            }
            ResolveFailure::ExpressionType { key, reason } => {
                write!(f, "param '{key}': {reason}")
            }
        }
    }
}

/// Fully resolved, spec-conformed, unit-converted parameter values. The
/// getters are infallible by construction: every declared key is present
/// (filled from the spec default when unset) and spec-typed. A missing or
/// mistyped read is a node-authoring bug, caught by `debug_assert` and
/// answered with the type's zero value in release.
#[derive(Debug, Clone, Default)]
pub struct ResolvedParams {
    values: BTreeMap<String, ParamValue>,
}

impl ResolvedParams {
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&ParamValue> {
        self.values.get(key)
    }

    #[must_use]
    pub fn f64(&self, key: &str) -> f64 {
        match self.values.get(key) {
            Some(ParamValue::Float(v)) => *v,
            other => {
                debug_assert!(false, "param '{key}' is not a Float: {other:?}");
                0.0
            }
        }
    }

    #[must_use]
    pub fn f32(&self, key: &str) -> f32 {
        self.f64(key) as f32
    }

    #[must_use]
    pub fn i64(&self, key: &str) -> i64 {
        match self.values.get(key) {
            Some(ParamValue::Int(v)) => *v,
            other => {
                debug_assert!(false, "param '{key}' is not an Int: {other:?}");
                0
            }
        }
    }

    /// Segment-count convenience: the Int value clamped into `u32`.
    #[must_use]
    pub fn u32(&self, key: &str) -> u32 {
        self.i64(key).clamp(0, i64::from(u32::MAX)) as u32
    }

    #[must_use]
    pub fn bool(&self, key: &str) -> bool {
        match self.values.get(key) {
            Some(ParamValue::Bool(v)) => *v,
            other => {
                debug_assert!(false, "param '{key}' is not a Bool: {other:?}");
                false
            }
        }
    }

    #[must_use]
    pub fn text(&self, key: &str) -> &str {
        match self.values.get(key) {
            Some(ParamValue::Text(v)) => v,
            other => {
                debug_assert!(false, "param '{key}' is not Text: {other:?}");
                ""
            }
        }
    }

    #[must_use]
    pub fn vec2(&self, key: &str) -> [f64; 2] {
        match self.values.get(key) {
            Some(ParamValue::Vec2(v)) => *v,
            other => {
                debug_assert!(false, "param '{key}' is not a Vec2: {other:?}");
                [0.0; 2]
            }
        }
    }

    #[must_use]
    pub fn vec3(&self, key: &str) -> [f64; 3] {
        match self.values.get(key) {
            Some(ParamValue::Vec3(v)) => *v,
            other => {
                debug_assert!(false, "param '{key}' is not a Vec3: {other:?}");
                [0.0; 3]
            }
        }
    }

    /// `vec3` narrowed to `f32` lanes (the kernel's native width).
    #[must_use]
    pub fn vec3_f32(&self, key: &str) -> [f32; 3] {
        let v = self.vec3(key);
        [v[0] as f32, v[1] as f32, v[2] as f32]
    }

    #[must_use]
    pub fn vec4(&self, key: &str) -> [f64; 4] {
        match self.values.get(key) {
            Some(ParamValue::Vec4(v)) => *v,
            other => {
                debug_assert!(false, "param '{key}' is not a Vec4: {other:?}");
                [0.0; 4]
            }
        }
    }

    #[must_use]
    pub fn color(&self, key: &str) -> [f32; 4] {
        match self.values.get(key) {
            Some(ParamValue::Color(v)) => *v,
            other => {
                debug_assert!(false, "param '{key}' is not a Color: {other:?}");
                [0.0; 4]
            }
        }
    }

    /// The selected enum variant's key.
    #[must_use]
    pub fn enum_key(&self, key: &str) -> &str {
        match self.values.get(key) {
            Some(ParamValue::Enum(v)) => v,
            other => {
                debug_assert!(false, "param '{key}' is not an Enum: {other:?}");
                ""
            }
        }
    }

    /// The staged-asset reference, if one is set (an empty digest counts
    /// as unset).
    #[must_use]
    pub fn asset(&self, key: &str) -> Option<&AssetId> {
        match self.values.get(key) {
            Some(ParamValue::Asset(id)) if !id.0.is_empty() => Some(id),
            _ => None,
        }
    }

    /// The cross-context node reference, if one is set.
    #[must_use]
    pub fn node_ref(&self, key: &str) -> Option<crate::document::NodeId> {
        match self.values.get(key) {
            Some(ParamValue::NodeRef(id)) => *id,
            _ => None,
        }
    }
}

/// The chokepoint, with no evaluation context.
///
/// Equivalent to [`resolve_params_with`] against a stopped clock, no
/// document and no geometry: arithmetic and the maths builtins evaluate,
/// while `ch()` and the geometry queries report that they are unavailable
/// here rather than quietly reading as zero. Every production call site
/// passes a real context; this exists for the node unit tests, which build
/// a param map directly and have no engine to borrow one from.
pub fn resolve_params(
    stored: &BTreeMap<String, ParamSource>,
    specs: &[ParamSpec],
) -> Result<ResolvedParams, ResolveFailure> {
    resolve_params_with(stored, specs, &EvalCtx::default())
}

/// The chokepoint. Reads each spec'd param from `stored` (default when
/// unset), evaluates it if it is an expression, conforms it to the spec
/// type, clamps to the hard range, and applies the unit conversion.
///
/// The ordering is the whole point: an expression result rejoins the
/// literal path *before* conform, clamp and the degrees-to-radians
/// conversion, so it is subject to exactly the same validity rules as a
/// typed value and cannot smuggle an out-of-range or wrongly-typed result
/// past the resolver.
pub fn resolve_params_with(
    stored: &BTreeMap<String, ParamSource>,
    specs: &[ParamSpec],
    ctx: &EvalCtx,
) -> Result<ResolvedParams, ResolveFailure> {
    let mut values = BTreeMap::new();
    for spec in specs {
        let raw = raw_value_of(stored, spec, ctx)?;
        // Conform (a mistyped stored value falls back to the default; the
        // SetParam command conforms on write, so this is a backstop).
        let mut value = conform_value(&raw, &spec.ty).unwrap_or_else(|_| spec.default.clone());
        // Hard clamp.
        if let Some(range) = &spec.range {
            value = clamp_value(value, range.hard);
        }
        // Unit conversion: the one place degrees become radians.
        if spec.unit == Unit::Degrees {
            value = map_numeric(value, f64::to_radians);
        }
        values.insert(spec.key.clone(), value);
    }
    Ok(ResolvedParams { values })
}

/// Lowers an evaluated expression result into the param's declared type.
///
/// This is where decision M-3 is enforced in the resolver: only the seven
/// numeric types accept an expression. `SetParam` refuses the others up
/// front (W1g), so reaching this arm means a hand-edited document, and
/// naming the type beats resolving to a default.
///
/// The lowering deliberately produces the spec's exact type where it can,
/// leaving `conform_value` a passthrough, except for Int, where handing
/// back a Float lets the existing half-away-from-zero rounding apply
/// rather than duplicating it here.
fn value_to_param(value: ExprValue, ty: &ParamType) -> Result<ParamValue, String> {
    let type_name = value.type_name();
    match (value, ty) {
        (ExprValue::Float(v), ParamType::Float | ParamType::Int) => Ok(ParamValue::Float(v)),
        (ExprValue::Float(v), ParamType::Bool) => Ok(ParamValue::Bool(v != 0.0)),
        (ExprValue::Bool(b), ParamType::Bool) => Ok(ParamValue::Bool(b)),
        (ExprValue::Bool(b), ParamType::Float | ParamType::Int) => {
            Ok(ParamValue::Float(if b { 1.0 } else { 0.0 }))
        }
        (ExprValue::Vec2(v), ParamType::Vec2) => Ok(ParamValue::Vec2(v)),
        (ExprValue::Vec3(v), ParamType::Vec3) => Ok(ParamValue::Vec3(v)),
        (ExprValue::Vec4(v), ParamType::Vec4) => Ok(ParamValue::Vec4(v)),
        (ExprValue::Vec4(v), ParamType::Color) => Ok(ParamValue::Color([
            v[0] as f32,
            v[1] as f32,
            v[2] as f32,
            v[3] as f32,
        ])),
        // A three-component colour is opaque: alpha 1 is the only useful
        // reading, and requiring set(r,g,b,1) for every tint would be noise.
        (ExprValue::Vec3(v), ParamType::Color) => Ok(ParamValue::Color([
            v[0] as f32,
            v[1] as f32,
            v[2] as f32,
            1.0,
        ])),
        (_, ParamType::Float | ParamType::Int) => Err(format!(
            "expected a number, the expression produced a {type_name}"
        )),
        (_, ParamType::Bool) => Err(format!(
            "expected a condition, the expression produced a {type_name}"
        )),
        (_, ParamType::Vec2) => Err(format!(
            "expected 2 components, the expression produced a {type_name}; use set(x, y)"
        )),
        (_, ParamType::Vec3) => Err(format!(
            "expected 3 components, the expression produced a {type_name}; use set(x, y, z)"
        )),
        (_, ParamType::Vec4 | ParamType::Color) => Err(format!(
            "expected 4 components, the expression produced a {type_name}; use set(x, y, z, w)"
        )),
        (_, other) => Err(format!(
            "{other:?} params cannot be driven by an expression"
        )),
    }
}

/// One param's stored value, with an expression evaluated but nothing
/// else applied yet.
fn raw_value_of(
    stored: &BTreeMap<String, ParamSource>,
    spec: &ParamSpec,
    ctx: &EvalCtx,
) -> Result<ParamValue, ResolveFailure> {
    match stored.get(&spec.key) {
        Some(ParamSource::Literal(v)) => Ok(v.clone()),
        Some(ParamSource::Expression { expr }) => {
            // Parsed per resolve rather than cached. A parameter
            // expression is short and this costs a couple of microseconds;
            // the wrangle, which runs a program per element, is what
            // parses once and reuses.
            let parsed =
                crate::expr::parse(expr).map_err(|error| ResolveFailure::ExpressionParse {
                    key: spec.key.clone(),
                    error,
                })?;
            let value = crate::expr::eval(&parsed.root, ctx).map_err(|error| {
                ResolveFailure::ExpressionEval {
                    key: spec.key.clone(),
                    error,
                }
            })?;
            value_to_param(value, &spec.ty).map_err(|reason| ResolveFailure::ExpressionType {
                key: spec.key.clone(),
                reason,
            })
        }
        None => Ok(spec.default.clone()),
    }
}

/// Resolves ONE param, in the space the user authored it in.
///
/// The parameter panel asks per row, so this is deliberately not
/// `resolve_params_with` filtered down: that fails the whole node on the
/// first bad expression, which would blank the readout of every other
/// param because one of them has a typo.
///
/// Conformed and hard-clamped but NOT unit-converted, for the same reason
/// [`conform_and_clamp`] is: a Degrees param is authored and displayed in
/// degrees, and a readout showing 1.571 under a field the user typed
/// `45 * 2` into would be worse than no readout.
pub fn resolve_one_authored(
    stored: &BTreeMap<String, ParamSource>,
    spec: &ParamSpec,
    ctx: &EvalCtx,
) -> Result<ParamValue, ResolveFailure> {
    let raw = raw_value_of(stored, spec, ctx)?;
    Ok(conform_and_clamp(&raw, spec).unwrap_or_else(|_| spec.default.clone()))
}

/// A stored value as a **reference** sees it: conformed to the spec type
/// and hard-clamped, but deliberately **not** unit-converted.
///
/// This is the one place the reference path diverges from the cook path,
/// and the reason is the documented 57x trap. A `Unit::Degrees` param
/// stores and displays degrees; the resolver converts to radians for the
/// cook body. If `ch()` handed back radians, then
/// `geo2.rotate = ch("../geo1/rotate")` would store radians into a field
/// that means degrees and convert a second time, landing 57x off. Reading
/// in the authoring space makes copying a rotation round-trip, at the cost
/// that `sin(ch(...))` on a degrees param needs an explicit `radians(...)`
/// -- which is already true of typing the number by hand.
///
/// The clamp *is* applied, so a reader can never observe a value the
/// target's own cook does not use.
pub fn conform_and_clamp(value: &ParamValue, spec: &ParamSpec) -> Result<ParamValue, String> {
    let mut out = conform_value(value, &spec.ty)?;
    if let Some(range) = &spec.range {
        out = clamp_value(out, range.hard);
    }
    Ok(out)
}

/// Conforms a value to a spec type, applying the shared scalar coercions
/// (Int accepts integral or rounded floats; Float accepts ints). Returns
/// the reason on a hopeless mismatch.
pub fn conform_value(value: &ParamValue, ty: &ParamType) -> Result<ParamValue, String> {
    match (value, ty) {
        (ParamValue::Float(v), ParamType::Float) => Ok(ParamValue::Float(*v)),
        (ParamValue::Int(v), ParamType::Float) => Ok(ParamValue::Float(*v as f64)),
        (ParamValue::Int(v), ParamType::Int) => Ok(ParamValue::Int(*v)),
        // Rounds half away from zero, matching the wire matrix.
        (ParamValue::Float(v), ParamType::Int) => Ok(ParamValue::Int(scalar::f64_to_i64(*v))),
        (ParamValue::Bool(v), ParamType::Bool) => Ok(ParamValue::Bool(*v)),
        (
            ParamValue::Text(v),
            ParamType::Text
            | ParamType::MultilineText
            | ParamType::AttributeName
            | ParamType::Snippet,
        ) => Ok(ParamValue::Text(v.clone())),
        (ParamValue::Vec2(v), ParamType::Vec2) => Ok(ParamValue::Vec2(*v)),
        (ParamValue::Vec3(v), ParamType::Vec3) => Ok(ParamValue::Vec3(*v)),
        (ParamValue::Vec4(v), ParamType::Vec4) => Ok(ParamValue::Vec4(*v)),
        (ParamValue::Color(v), ParamType::Color) => Ok(ParamValue::Color(*v)),
        (ParamValue::Enum(key), ParamType::Enum { variants }) => {
            if variants.iter().any(|v| v.key == *key) {
                Ok(ParamValue::Enum(key.clone()))
            } else {
                Err(format!("'{key}' is not a variant of this enum"))
            }
        }
        (ParamValue::Asset(id), ParamType::AssetRef { .. }) => Ok(ParamValue::Asset(id.clone())),
        (ParamValue::NodeRef(id), ParamType::NodePath { .. }) => Ok(ParamValue::NodeRef(*id)),
        // Action params are inert; any stored bool conforms.
        (ParamValue::Bool(_), ParamType::Action) => Ok(ParamValue::Bool(false)),
        (other, ty) => Err(format!("{other:?} does not conform to {ty:?}")),
    }
}

/// Component-wise hard clamp on the numeric types; passthrough otherwise.
fn clamp_value(value: ParamValue, (lo, hi): (f64, f64)) -> ParamValue {
    match value {
        ParamValue::Float(v) => ParamValue::Float(v.clamp(lo, hi)),
        ParamValue::Int(v) => {
            ParamValue::Int(v.clamp(scalar::f64_to_i64(lo), scalar::f64_to_i64(hi)))
        }
        ParamValue::Vec2(mut v) => {
            for c in &mut v {
                *c = c.clamp(lo, hi);
            }
            ParamValue::Vec2(v)
        }
        ParamValue::Vec3(mut v) => {
            for c in &mut v {
                *c = c.clamp(lo, hi);
            }
            ParamValue::Vec3(v)
        }
        ParamValue::Vec4(mut v) => {
            for c in &mut v {
                *c = c.clamp(lo, hi);
            }
            ParamValue::Vec4(v)
        }
        other => other,
    }
}

/// Element-wise numeric map (the Degrees conversion covers vectors too).
fn map_numeric(value: ParamValue, f: impl Fn(f64) -> f64) -> ParamValue {
    match value {
        ParamValue::Float(v) => ParamValue::Float(f(v)),
        ParamValue::Vec2(v) => ParamValue::Vec2(v.map(&f)),
        ParamValue::Vec3(v) => ParamValue::Vec3(v.map(&f)),
        ParamValue::Vec4(v) => ParamValue::Vec4(v.map(&f)),
        other => other,
    }
}

// Schema-v1 JSON conversions.

/// A literal as its plain JSON value (`"radius": 1.5`, `"size": [1,2,1]`,
/// `"preset": "studio"`).
#[must_use]
pub fn param_value_to_json(value: &ParamValue) -> Json {
    match value {
        ParamValue::Float(v) => serde_json::json!(v),
        ParamValue::Int(v) => serde_json::json!(v),
        ParamValue::Bool(v) => serde_json::json!(v),
        ParamValue::Text(v) => serde_json::json!(v),
        ParamValue::Vec2(v) => serde_json::json!(v),
        ParamValue::Vec3(v) => serde_json::json!(v),
        ParamValue::Vec4(v) => serde_json::json!(v),
        ParamValue::Color(v) => serde_json::json!(v),
        ParamValue::Enum(key) => serde_json::json!(key),
        ParamValue::Asset(id) => serde_json::json!(id.0),
        // The plain form is the raw id number (or null when unset), so a
        // reference reads naturally in `document.json`.
        ParamValue::NodeRef(id) => match id {
            Some(n) => serde_json::json!(n.0),
            None => Json::Null,
        },
    }
}

/// A source as schema-v1 JSON: literals plain, expressions as
/// `{"$expr": "..."}`.
#[must_use]
pub fn param_source_to_json(source: &ParamSource) -> Json {
    match source {
        ParamSource::Literal(v) => param_value_to_json(v),
        ParamSource::Expression { expr } => serde_json::json!({ "$expr": expr }),
    }
}

/// Parses schema-v1 JSON under a spec type. The `{"$expr": ...}` object
/// maps to `Expression`; anything else is typed by `ty`. Errors name the
/// mismatch (the caller decides between defaulting-with-warning on load
/// and rejecting on `SetParam`).
pub fn param_source_from_json(json: &Json, ty: &ParamType) -> Result<ParamSource, String> {
    if let Json::Object(map) = json
        && let Some(expr) = map.get("$expr")
    {
        let expr = expr
            .as_str()
            .ok_or_else(|| "$expr must be a string".to_string())?;
        return Ok(ParamSource::Expression {
            expr: expr.to_string(),
        });
    }
    let value = match ty {
        ParamType::Float => ParamValue::Float(
            json.as_f64()
                .ok_or_else(|| format!("expected a number, got {json}"))?,
        ),
        ParamType::Int => {
            if let Some(i) = json.as_i64() {
                ParamValue::Int(i)
            } else if let Some(f) = json.as_f64() {
                // Rounds half away from zero, the one rounding model.
                ParamValue::Int(scalar::f64_to_i64(f))
            } else {
                return Err(format!("expected an integer, got {json}"));
            }
        }
        ParamType::Bool => ParamValue::Bool(
            json.as_bool()
                .ok_or_else(|| format!("expected a bool, got {json}"))?,
        ),
        ParamType::Text
        | ParamType::MultilineText
        | ParamType::AttributeName
        | ParamType::Snippet => ParamValue::Text(
            json.as_str()
                .ok_or_else(|| format!("expected a string, got {json}"))?
                .to_string(),
        ),
        ParamType::Vec2 => ParamValue::Vec2(json_array(json)?),
        ParamType::Vec3 => ParamValue::Vec3(json_array(json)?),
        ParamType::Vec4 => ParamValue::Vec4(json_array(json)?),
        ParamType::Color => {
            let v: [f64; 4] = json_array(json)?;
            ParamValue::Color(v.map(|c| c as f32))
        }
        ParamType::Enum { variants } => {
            let key = json
                .as_str()
                .ok_or_else(|| format!("expected an enum key string, got {json}"))?;
            if !variants.iter().any(|v| v.key == key) {
                return Err(format!("'{key}' is not a variant of this enum"));
            }
            ParamValue::Enum(key.to_string())
        }
        ParamType::AssetRef { .. } => ParamValue::Asset(AssetId(
            json.as_str()
                .ok_or_else(|| format!("expected an asset digest string, got {json}"))?
                .to_string(),
        )),
        ParamType::Action => ParamValue::Bool(false),
        ParamType::NodePath { .. } => {
            if json.is_null() {
                ParamValue::NodeRef(None)
            } else {
                let id = json
                    .as_u64()
                    .ok_or_else(|| format!("expected a node id number or null, got {json}"))?;
                ParamValue::NodeRef(Some(crate::document::NodeId(id)))
            }
        }
    };
    Ok(ParamSource::Literal(value))
}

/// An exact-length JSON number array.
fn json_array<const N: usize>(json: &Json) -> Result<[f64; N], String> {
    let arr = json
        .as_array()
        .ok_or_else(|| format!("expected an array of {N} numbers, got {json}"))?;
    if arr.len() != N {
        return Err(format!("expected {N} components, got {}", arr.len()));
    }
    let mut out = [0.0; N];
    for (i, v) in arr.iter().enumerate() {
        out[i] = v
            .as_f64()
            .ok_or_else(|| format!("component {i} is not a number: {v}"))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests

    use super::*;
    use crate::registry::param_spec::EnumVariant;

    fn spec_float(key: &str) -> ParamSpec {
        ParamSpec::new(key, key, "test", ParamType::Float, ParamValue::Float(1.0))
    }

    #[test]
    fn defaults_fill_unset_params() {
        let specs = [spec_float("radius").hard(0.001, 100.0)];
        let stored = BTreeMap::new();
        let p = resolve_params(&stored, &specs).unwrap();
        assert_eq!(p.f64("radius"), 1.0);
    }

    #[test]
    fn hard_clamp_applies_in_the_resolver() {
        let specs = [spec_float("radius").hard(0.001, 10.0)];
        let mut stored = BTreeMap::new();
        stored.insert(
            "radius".to_string(),
            ParamSource::Literal(ParamValue::Float(9999.0)),
        );
        let p = resolve_params(&stored, &specs).unwrap();
        assert_eq!(p.f64("radius"), 10.0);
    }

    #[test]
    fn degrees_convert_element_wise_for_vectors() {
        let specs = [
            ParamSpec::new(
                "rotate",
                "Rotate",
                "transform",
                ParamType::Vec3,
                ParamValue::Vec3([0.0; 3]),
            )
            .unit(Unit::Degrees),
            ParamSpec::new(
                "angle",
                "Angle",
                "light",
                ParamType::Float,
                ParamValue::Float(45.0),
            )
            .hard(1.0, 89.0)
            .unit(Unit::Degrees),
            // Normalized must NOT convert (the 57x silent-error trap).
            ParamSpec::new(
                "penumbra",
                "Penumbra",
                "light",
                ParamType::Float,
                ParamValue::Float(0.5),
            )
            .hard(0.0, 1.0)
            .unit(Unit::Normalized),
        ];
        let mut stored = BTreeMap::new();
        stored.insert(
            "rotate".to_string(),
            ParamSource::Literal(ParamValue::Vec3([90.0, 180.0, -90.0])),
        );
        let p = resolve_params(&stored, &specs).unwrap();
        let r = p.vec3("rotate");
        assert!((r[0] - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert!((r[1] - std::f64::consts::PI).abs() < 1e-12);
        assert!((r[2] + std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        // Default 45 degrees resolved to radians.
        assert!((p.f64("angle") - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
        // Normalized untouched.
        assert_eq!(p.f64("penumbra"), 0.5);
    }

    #[test]
    fn clamp_happens_in_degrees_before_conversion() {
        let specs = [ParamSpec::new(
            "angle",
            "Angle",
            "light",
            ParamType::Float,
            ParamValue::Float(45.0),
        )
        .hard(1.0, 89.0)
        .unit(Unit::Degrees)];
        let mut stored = BTreeMap::new();
        stored.insert(
            "angle".to_string(),
            ParamSource::Literal(ParamValue::Float(170.0)),
        );
        let p = resolve_params(&stored, &specs).unwrap();
        // Clamped to 89 degrees, then converted.
        assert!((p.f64("angle") - 89.0_f64.to_radians()).abs() < 1e-12);
    }

    /// A single-param store holding one expression.
    fn expr_store(key: &str, expr: &str) -> BTreeMap<String, ParamSource> {
        let mut stored = BTreeMap::new();
        stored.insert(
            key.to_string(),
            ParamSource::Expression {
                expr: expr.to_string(),
            },
        );
        stored
    }

    #[test]
    fn an_expression_resolves_to_its_value() {
        let specs = [spec_float("radius").hard(0.0, 100.0)];
        let p = resolve_params(&expr_store("radius", "2 * 3 + 1"), &specs).unwrap();
        assert_eq!(p.f64("radius"), 7.0);
    }

    #[test]
    fn time_reads_zero_while_the_runtime_is_stopped() {
        // Every cook is reproducible until F3 starts a clock: golden
        // captures and CLI cooks depend on this.
        let specs = [spec_float("radius").hard(-10.0, 100.0)];
        let p = resolve_params(&expr_store("radius", "$F * 2 + $T"), &specs).unwrap();
        assert_eq!(p.f64("radius"), 0.0);
    }

    #[test]
    fn an_expression_result_is_hard_clamped_exactly_as_a_literal_is() {
        // The whole point of rejoining the literal path before the clamp:
        // an expression must not be able to smuggle an out-of-range value
        // past the resolver.
        let specs = [spec_float("radius").hard(0.0, 10.0)];
        let p = resolve_params(&expr_store("radius", "999"), &specs).unwrap();
        assert_eq!(p.f64("radius"), 10.0);
        let p = resolve_params(&expr_store("radius", "0 - 999"), &specs).unwrap();
        assert_eq!(p.f64("radius"), 0.0);
    }

    #[test]
    fn an_expression_result_is_converted_from_degrees_like_a_literal() {
        // And in the same order: clamp first, then convert. An expression
        // that skipped this would reopen the documented 57x gizmo trap.
        let specs = [ParamSpec::new(
            "angle",
            "angle",
            "test",
            ParamType::Float,
            ParamValue::Float(0.0),
        )
        .hard(0.0, 89.0)
        .unit(Unit::Degrees)];
        let p = resolve_params(&expr_store("angle", "45 * 4"), &specs).unwrap();
        assert!((p.f64("angle") - 89.0_f64.to_radians()).abs() < 1e-12);
    }

    #[test]
    fn an_expression_result_rounds_into_an_int_param() {
        let specs = [ParamSpec::new(
            "segments",
            "segments",
            "test",
            ParamType::Int,
            ParamValue::Int(3),
        )];
        let p = resolve_params(&expr_store("segments", "7 / 2"), &specs).unwrap();
        assert_eq!(
            p.i64("segments"),
            4,
            "half away from zero, as for a literal"
        );
    }

    #[test]
    fn a_vector_expression_fills_a_vector_param() {
        let specs = [ParamSpec::new(
            "size",
            "size",
            "test",
            ParamType::Vec3,
            ParamValue::Vec3([1.0; 3]),
        )];
        let p = resolve_params(&expr_store("size", "set(1, 2, 3) * 2"), &specs).unwrap();
        assert_eq!(p.vec3("size"), [2.0, 4.0, 6.0]);
    }

    #[test]
    fn a_three_component_expression_fills_a_colour_as_opaque() {
        let specs = [ParamSpec::new(
            "tint",
            "tint",
            "test",
            ParamType::Color,
            ParamValue::Color([1.0; 4]),
        )];
        let p = resolve_params(&expr_store("tint", "set(1, 0, 0)"), &specs).unwrap();
        assert_eq!(p.color("tint"), [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn a_parse_error_names_the_param_and_keeps_its_span() {
        let specs = [spec_float("radius")];
        let err = resolve_params(&expr_store("radius", "1 +"), &specs).unwrap_err();
        assert_eq!(err.key(), "radius");
        assert!(err.to_string().contains("radius"), "{err}");
        assert!(
            err.span().is_some(),
            "the editor needs somewhere to underline"
        );
        assert!(matches!(err, ResolveFailure::ExpressionParse { .. }));
    }

    #[test]
    fn an_evaluation_error_names_the_param() {
        let specs = [spec_float("radius")];
        let err = resolve_params(&expr_store("radius", "wobble(1)"), &specs).unwrap_err();
        assert!(matches!(err, ResolveFailure::ExpressionEval { .. }));
        assert!(err.to_string().contains("unknown function"), "{err}");
    }

    #[test]
    fn a_wrongly_typed_result_says_what_it_produced() {
        let specs = [spec_float("radius")];
        let err = resolve_params(&expr_store("radius", "set(1, 2, 3)"), &specs).unwrap_err();
        assert!(matches!(err, ResolveFailure::ExpressionType { .. }));
        assert!(err.to_string().contains("produced a vec3"), "{err}");
    }

    #[test]
    fn a_reference_without_a_document_reports_that_rather_than_reading_zero() {
        // The context-free wrapper has no document, and silently resolving
        // to a default would be a wrong number rather than an error.
        let specs = [spec_float("radius")];
        let err = resolve_params(&expr_store("radius", "ch(\"../a/b\")"), &specs).unwrap_err();
        assert!(err.to_string().contains("not available"), "{err}");
    }

    #[test]
    fn an_expression_on_a_non_numeric_param_is_refused_by_type() {
        // SetParam refuses these up front (M-3), so reaching here means a
        // hand-edited document; naming the type beats silently defaulting.
        let specs = [ParamSpec::new(
            "label",
            "label",
            "test",
            ParamType::Text,
            ParamValue::Text("x".into()),
        )];
        let err = resolve_params(&expr_store("label", "1 + 1"), &specs).unwrap_err();
        assert!(matches!(err, ResolveFailure::ExpressionType { .. }));
        assert!(err.to_string().contains("cannot be driven"), "{err}");
    }

    #[test]
    fn int_params_accept_rounded_floats() {
        let specs = [ParamSpec::new(
            "segments",
            "Segments",
            "geometry",
            ParamType::Int,
            ParamValue::Int(4),
        )
        .hard(1.0, 512.0)];
        let mut stored = BTreeMap::new();
        stored.insert(
            "segments".to_string(),
            ParamSource::Literal(ParamValue::Float(7.5)),
        );
        let p = resolve_params(&stored, &specs).unwrap();
        assert_eq!(p.i64("segments"), 8);
        assert_eq!(p.u32("segments"), 8);
    }

    #[test]
    fn invalid_enum_falls_back_to_default() {
        let specs = [ParamSpec::new(
            "order",
            "Order",
            "transform",
            ParamType::Enum {
                variants: vec![
                    EnumVariant::new("xyz", "XYZ"),
                    EnumVariant::new("zyx", "ZYX"),
                ],
            },
            ParamValue::Enum("xyz".to_string()),
        )];
        let mut stored = BTreeMap::new();
        stored.insert(
            "order".to_string(),
            ParamSource::Literal(ParamValue::Enum("bogus".to_string())),
        );
        let p = resolve_params(&stored, &specs).unwrap();
        assert_eq!(p.enum_key("order"), "xyz");
    }

    #[test]
    fn schema_v1_json_round_trip() {
        let ty_vec3 = ParamType::Vec3;
        let src = ParamSource::Literal(ParamValue::Vec3([1.0, 2.0, 3.0]));
        let json = param_source_to_json(&src);
        assert_eq!(json, serde_json::json!([1.0, 2.0, 3.0]));
        assert_eq!(param_source_from_json(&json, &ty_vec3).unwrap(), src);

        // Expressions round-trip through the reserved object form.
        let src = ParamSource::Expression {
            expr: "$F".to_string(),
        };
        let json = param_source_to_json(&src);
        assert_eq!(json, serde_json::json!({ "$expr": "$F" }));
        assert_eq!(param_source_from_json(&json, &ty_vec3).unwrap(), src);

        // Schema-typing disambiguates: the same JSON number is Int or
        // Float depending on the spec.
        let json = serde_json::json!(2);
        assert_eq!(
            param_source_from_json(&json, &ParamType::Float).unwrap(),
            ParamSource::Literal(ParamValue::Float(2.0))
        );
        assert_eq!(
            param_source_from_json(&json, &ParamType::Int).unwrap(),
            ParamSource::Literal(ParamValue::Int(2))
        );

        // A float into an Int param rounds (half away from zero).
        let json = serde_json::json!(1.5);
        assert_eq!(
            param_source_from_json(&json, &ParamType::Int).unwrap(),
            ParamSource::Literal(ParamValue::Int(2))
        );

        // Wrong shapes are named errors.
        assert!(param_source_from_json(&serde_json::json!([1, 2]), &ParamType::Vec3).is_err());
        assert!(param_source_from_json(&serde_json::json!("x"), &ParamType::Bool).is_err());
    }

    #[test]
    fn color_json_uses_four_f32_lanes() {
        let src = ParamSource::Literal(ParamValue::Color([0.25, 0.5, 0.75, 1.0]));
        let json = param_source_to_json(&src);
        let back = param_source_from_json(&json, &ParamType::Color).unwrap();
        assert_eq!(back, src);
    }

    #[test]
    fn attribute_name_stores_and_loads_as_plain_text() {
        // The widget variant, not the storage, is what AttributeName
        // declares: a pre-variant document's plain string literal types
        // straight into it, so no migration exists or is needed.
        let json = serde_json::json!("color");
        assert_eq!(
            param_source_from_json(&json, &ParamType::AttributeName).unwrap(),
            ParamSource::Literal(ParamValue::Text("color".to_string()))
        );
        let src = ParamSource::Literal(ParamValue::Text("mask".to_string()));
        assert_eq!(param_source_to_json(&src), serde_json::json!("mask"));
        // conform accepts the Text value under the AttributeName spec.
        assert_eq!(
            conform_value(
                &ParamValue::Text("mask".to_string()),
                &ParamType::AttributeName
            )
            .unwrap(),
            ParamValue::Text("mask".to_string())
        );
    }
}
