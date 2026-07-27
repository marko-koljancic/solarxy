//! The value lattice and its arithmetic.
//!
//! Five variants, chosen to land inside what
//! [`crate::registry::resolve::conform_value`] already knows how to
//! conform: an evaluated result becomes a [`crate::params::ParamValue`] of
//! the spec's type and then travels the same conform, clamp and
//! degrees-to-radians path a literal does. There is deliberately no string
//! type (decision M-3), so a path argument is a literal recognised by the
//! parser rather than a value that can be computed.
//!
//! Vector arithmetic is component-wise, and a scalar mixed with a vector
//! broadcasts. Mixing two vectors of different width is an error rather
//! than a silent truncation, because silently dropping `z` is the kind of
//! bug a user never finds.

use super::error::ExprError;
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Float(f64),
    Vec2([f64; 2]),
    Vec3([f64; 3]),
    Vec4([f64; 4]),
    Bool(bool),
}

impl Value {
    /// How the value reads in an error message.
    #[must_use]
    pub fn type_name(self) -> &'static str {
        match self {
            Value::Float(_) => "float",
            Value::Vec2(_) => "vec2",
            Value::Vec3(_) => "vec3",
            Value::Vec4(_) => "vec4",
            Value::Bool(_) => "bool",
        }
    }

    /// The numeric lanes, or `None` for a bool.
    #[must_use]
    pub fn lanes(self) -> Option<Vec<f64>> {
        match self {
            Value::Float(v) => Some(vec![v]),
            Value::Vec2(v) => Some(v.to_vec()),
            Value::Vec3(v) => Some(v.to_vec()),
            Value::Vec4(v) => Some(v.to_vec()),
            Value::Bool(_) => None,
        }
    }

    /// Rebuilds a value of the same width from lanes.
    #[must_use]
    pub fn from_lanes(lanes: &[f64]) -> Option<Value> {
        match lanes {
            [a] => Some(Value::Float(*a)),
            [a, b] => Some(Value::Vec2([*a, *b])),
            [a, b, c] => Some(Value::Vec3([*a, *b, *c])),
            [a, b, c, d] => Some(Value::Vec4([*a, *b, *c, *d])),
            _ => None,
        }
    }

    /// The scalar value, for contexts that need one number.
    pub fn as_float(self, what: &str, span: &Range<usize>) -> Result<f64, ExprError> {
        match self {
            Value::Float(v) => Ok(v),
            Value::Bool(b) => Ok(if b { 1.0 } else { 0.0 }),
            other => Err(ExprError::new(
                format!("{what} expects a number, found a {}", other.type_name()),
                span.clone(),
            )),
        }
    }

    /// Truthiness, for conditions: a non-zero number, or the bool itself.
    pub fn as_bool(self, what: &str, span: &Range<usize>) -> Result<bool, ExprError> {
        match self {
            Value::Bool(b) => Ok(b),
            Value::Float(v) => Ok(v != 0.0),
            other => Err(ExprError::new(
                format!("{what} expects a condition, found a {}", other.type_name()),
                span.clone(),
            )),
        }
    }
}

/// Applies `f` to every lane of a numeric value.
pub fn map1(
    v: Value,
    what: &str,
    span: &Range<usize>,
    f: impl Fn(f64) -> f64,
) -> Result<Value, ExprError> {
    let Some(lanes) = v.lanes() else {
        return Err(ExprError::new(
            format!("{what} expects a number, found a {}", v.type_name()),
            span.clone(),
        ));
    };
    let mapped: Vec<f64> = lanes.into_iter().map(f).collect();
    Value::from_lanes(&mapped)
        .ok_or_else(|| ExprError::new(format!("{what} produced no value"), span.clone()))
}

/// Applies `f` lane-wise across two values, broadcasting a scalar against
/// a vector. Two vectors of differing width are an error.
pub fn map2(
    a: Value,
    b: Value,
    what: &str,
    span: &Range<usize>,
    f: impl Fn(f64, f64) -> f64,
) -> Result<Value, ExprError> {
    let (Some(la), Some(lb)) = (a.lanes(), b.lanes()) else {
        return Err(ExprError::new(
            format!(
                "{what} expects numbers, found a {} and a {}",
                a.type_name(),
                b.type_name()
            ),
            span.clone(),
        ));
    };
    let width = match (la.len(), lb.len()) {
        (x, y) if x == y => x,
        (1, y) => y,
        (x, 1) => x,
        (x, y) => {
            return Err(ExprError::new(
                format!(
                    "{what} cannot combine a {x}-component and a {y}-component value; \
                     widen one with set(...)"
                ),
                span.clone(),
            ));
        }
    };
    let at = |lanes: &[f64], i: usize| if lanes.len() == 1 { lanes[0] } else { lanes[i] };
    let mapped: Vec<f64> = (0..width).map(|i| f(at(&la, i), at(&lb, i))).collect();
    Value::from_lanes(&mapped)
        .ok_or_else(|| ExprError::new(format!("{what} produced no value"), span.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span() -> Range<usize> {
        0..1
    }

    #[test]
    fn map1_is_component_wise() {
        let v = map1(Value::Vec3([1.0, -2.0, 3.0]), "abs", &span(), f64::abs).unwrap();
        assert_eq!(v, Value::Vec3([1.0, 2.0, 3.0]));
    }

    #[test]
    fn map2_broadcasts_a_scalar_against_a_vector_either_way() {
        let a = map2(
            Value::Vec3([1.0, 2.0, 3.0]),
            Value::Float(2.0),
            "*",
            &span(),
            |x, y| x * y,
        )
        .unwrap();
        assert_eq!(a, Value::Vec3([2.0, 4.0, 6.0]));
        let b = map2(
            Value::Float(2.0),
            Value::Vec2([1.0, 2.0]),
            "*",
            &span(),
            |x, y| x * y,
        )
        .unwrap();
        assert_eq!(b, Value::Vec2([2.0, 4.0]));
    }

    #[test]
    fn mismatched_widths_are_an_error_not_a_truncation() {
        let e = map2(
            Value::Vec3([1.0, 2.0, 3.0]),
            Value::Vec2([1.0, 2.0]),
            "+",
            &span(),
            |x, y| x + y,
        )
        .unwrap_err();
        assert!(e.message.contains("cannot combine"), "{e:?}");
    }

    #[test]
    fn a_bool_is_not_a_number_for_arithmetic() {
        let e = map1(Value::Bool(true), "abs", &span(), f64::abs).unwrap_err();
        assert!(e.message.contains("expects a number"), "{e:?}");
    }

    #[test]
    fn truthiness_accepts_a_number_or_a_bool() {
        assert!(Value::Float(0.5).as_bool("?", &span()).unwrap());
        assert!(!Value::Float(0.0).as_bool("?", &span()).unwrap());
        assert!(Value::Bool(true).as_bool("?", &span()).unwrap());
        assert!(Value::Vec3([1.0; 3]).as_bool("?", &span()).is_err());
    }
}
