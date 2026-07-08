//! The param resolver: the single chokepoint between stored params and
//! compute bodies (node catalog part I, sections 4 and 5).
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
use crate::params::{AssetId, ParamSource, ParamValue};

/// Why a node's params could not resolve (a cook error, not a command
/// error: the node badges and refuses to cook).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveFailure {
    /// Decision 26: v1 refuses to evaluate expressions and badges the node.
    ExpressionUnsupported { key: String },
}

impl std::fmt::Display for ResolveFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveFailure::ExpressionUnsupported { key } => {
                write!(f, "param '{key}' uses an expression; not supported in v1")
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
}

/// The chokepoint. Reads each spec'd param from `stored` (default when
/// unset), conforms it to the spec type, clamps to the hard range, and
/// applies the unit conversion. Fails only on an expression source (v1).
pub fn resolve_params(
    stored: &BTreeMap<String, ParamSource>,
    specs: &[ParamSpec],
) -> Result<ResolvedParams, ResolveFailure> {
    let mut values = BTreeMap::new();
    for spec in specs {
        let raw = match stored.get(&spec.key) {
            Some(ParamSource::Literal(v)) => v.clone(),
            Some(ParamSource::Expression { .. }) => {
                return Err(ResolveFailure::ExpressionUnsupported {
                    key: spec.key.clone(),
                });
            }
            None => spec.default.clone(),
        };
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
        (ParamValue::Text(v), ParamType::Text) => Ok(ParamValue::Text(v.clone())),
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
        ParamType::Text => ParamValue::Text(
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

    #[test]
    fn expression_source_refuses_to_resolve() {
        let specs = [spec_float("radius")];
        let mut stored = BTreeMap::new();
        stored.insert(
            "radius".to_string(),
            ParamSource::Expression {
                expr: "$F * 2".to_string(),
            },
        );
        let err = resolve_params(&stored, &specs).unwrap_err();
        assert_eq!(
            err,
            ResolveFailure::ExpressionUnsupported {
                key: "radius".to_string()
            }
        );
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
}
