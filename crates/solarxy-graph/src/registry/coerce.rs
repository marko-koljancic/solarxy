//! The `DataType` system and the wire-level coercion matrix (node catalog
//! part I, section 2).
//!
//! A connection is legal iff the endpoint types are the same or a listed
//! coercion exists; the engine applies coercions at value-gather time
//! during the cook. `Geometry`, `Light`, `Report`, and `Image` coerce to
//! and from nothing (a Color-to-Image constant fill would require a
//! synthesis cook; backlog note). Lossy cells (`Float -> Int` rounds half
//! away from zero,
//! `Color -> Vec3` drops alpha) are allowed per decision 28 and get
//! distinct handle UX in the frontend.
//!
//! The matrix is a table-driven function locked by an exhaustive N-by-N
//! snapshot test below: any change to a cell is a visible diff in this
//! file, never an accident.

use std::sync::Arc;

use solarxy_core::scene::LightDef;
use solarxy_core::{RawImageData, ValidationReport};
use solarxy_kernel::GeometrySet;

use super::scalar;

/// The wire-type vocabulary. There is deliberately no `Object` and no
/// `Any` type in v1; `GeometrySet` subsumes the group case, and `Any`
/// stays out until a concrete node needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DataType {
    /// `Arc<GeometrySet>`: meshes + materials + attributes.
    Geometry,
    /// Reserved: no MVP node has a Light port (lights cook at root,
    /// portless). Frozen into the matrix and handle palette now.
    Light,
    /// `Arc<ValidationReport>`: the validate node's second output.
    Report,
    Float,
    Int,
    Bool,
    Vec2,
    Vec3,
    Vec4,
    /// `[f32; 4]` linear RGBA.
    Color,
    Text,
    /// `Arc<RawImageData>`: decoded RGBA8 pixels plus content hash. The
    /// wire value between `import_image` and the `material` node's map
    /// ports (Phase 13).
    Image,
    /// `Arc<RawMaterialData>`: a built material description. The wire
    /// value INSIDE material networks only (phase 20, decision C-2);
    /// across contexts materials travel by path reference, never by
    /// wire. Coerces to and from nothing.
    Material,
}

impl DataType {
    /// Every variant, in matrix row/column order.
    pub const ALL: [DataType; 13] = [
        DataType::Geometry,
        DataType::Light,
        DataType::Report,
        DataType::Float,
        DataType::Int,
        DataType::Bool,
        DataType::Vec2,
        DataType::Vec3,
        DataType::Vec4,
        DataType::Color,
        DataType::Text,
        DataType::Image,
        DataType::Material,
    ];
}

/// One matrix cell's verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coercion {
    /// Identical types.
    Same,
    /// Allowed, no information lost (plain handle ring in the UI).
    Lossless,
    /// Allowed, information lost (filled warning ring naming the
    /// conversion).
    Lossy,
    /// Rejected with visible feedback.
    Forbidden,
}

impl Coercion {
    /// Whether a wire between these types may exist at all.
    #[must_use]
    pub fn is_legal(self) -> bool {
        !matches!(self, Coercion::Forbidden)
    }
}

/// The table-driven matrix. Exhaustive over `DataType x DataType`; the
/// snapshot test renders every cell.
// One arm per matrix cell is the documentation; do not merge same-body
// arms.
#[allow(clippy::match_same_arms)]
#[must_use]
pub fn can_coerce(from: DataType, to: DataType) -> Coercion {
    use Coercion::{Forbidden, Lossless, Lossy, Same};
    use DataType as D;

    if from == to {
        return Same;
    }
    match (from, to) {
        // Float row.
        (D::Float, D::Int) => Lossy, // rounds half away from zero
        (D::Float, D::Vec2 | D::Vec3 | D::Vec4) => Lossless, // splat
        // Int row.
        (D::Int, D::Float) => Lossless,
        (D::Int, D::Vec2 | D::Vec3 | D::Vec4) => Lossless, // splat
        // Bool row.
        (D::Bool, D::Float | D::Int) => Lossless, // 0 or 1
        // Vec3 row.
        (D::Vec3, D::Color) => Lossless, // rgb, alpha 1
        // Vec4 row.
        (D::Vec4, D::Color) => Lossless,
        // Color row.
        (D::Color, D::Vec3) => Lossy, // drops alpha
        (D::Color, D::Vec4) => Lossless,
        // Geometry, Light, Report, Image, Text, Vec2: nothing beyond Same.
        _ => Forbidden,
    }
}

/// A value on a wire. Payload variants mirror [`DataType`] one to one;
/// [`Value::data_type`] is the mapping.
#[derive(Debug, Clone)]
pub enum Value {
    Geometry(Arc<GeometrySet>),
    Light(LightDef),
    Report(Arc<ValidationReport>),
    Float(f64),
    Int(i64),
    Bool(bool),
    Vec2([f64; 2]),
    Vec3([f64; 3]),
    Vec4([f64; 4]),
    Color([f32; 4]),
    Text(String),
    Image(Arc<RawImageData>),
    Material(Arc<solarxy_core::RawMaterialData>),
}

impl Value {
    #[must_use]
    pub fn data_type(&self) -> DataType {
        match self {
            Value::Geometry(_) => DataType::Geometry,
            Value::Light(_) => DataType::Light,
            Value::Report(_) => DataType::Report,
            Value::Float(_) => DataType::Float,
            Value::Int(_) => DataType::Int,
            Value::Bool(_) => DataType::Bool,
            Value::Vec2(_) => DataType::Vec2,
            Value::Vec3(_) => DataType::Vec3,
            Value::Vec4(_) => DataType::Vec4,
            Value::Color(_) => DataType::Color,
            Value::Text(_) => DataType::Text,
            Value::Image(_) => DataType::Image,
            Value::Material(_) => DataType::Material,
        }
    }

    /// The geometry payload, if this is a Geometry value.
    #[must_use]
    pub fn as_geometry(&self) -> Option<&Arc<GeometrySet>> {
        match self {
            Value::Geometry(g) => Some(g),
            _ => None,
        }
    }

    /// The report payload, if this is a Report value.
    #[must_use]
    pub fn as_report(&self) -> Option<&Arc<ValidationReport>> {
        match self {
            Value::Report(r) => Some(r),
            _ => None,
        }
    }

    /// The image payload, if this is an Image value.
    #[must_use]
    pub fn as_image(&self) -> Option<&Arc<RawImageData>> {
        match self {
            Value::Image(i) => Some(i),
            _ => None,
        }
    }

    /// The material payload, if this is a Material value.
    #[must_use]
    pub fn as_material(&self) -> Option<&Arc<solarxy_core::RawMaterialData>> {
        match self {
            Value::Material(m) => Some(m),
            _ => None,
        }
    }
}

/// Applies the matrix to a runtime value at gather time. Returns `None`
/// exactly when [`can_coerce`] says [`Coercion::Forbidden`].
#[must_use]
pub fn coerce_value(value: &Value, to: DataType) -> Option<Value> {
    if value.data_type() == to {
        return Some(value.clone());
    }
    let coerced = match (value, to) {
        (Value::Float(v), DataType::Int) => Value::Int(scalar::f64_to_i64(*v)),
        (Value::Float(v), DataType::Vec2) => Value::Vec2(scalar::splat(*v)),
        (Value::Float(v), DataType::Vec3) => Value::Vec3(scalar::splat(*v)),
        (Value::Float(v), DataType::Vec4) => Value::Vec4(scalar::splat(*v)),
        (Value::Int(v), DataType::Float) => Value::Float(*v as f64),
        (Value::Int(v), DataType::Vec2) => Value::Vec2(scalar::splat(*v as f64)),
        (Value::Int(v), DataType::Vec3) => Value::Vec3(scalar::splat(*v as f64)),
        (Value::Int(v), DataType::Vec4) => Value::Vec4(scalar::splat(*v as f64)),
        (Value::Bool(v), DataType::Float) => Value::Float(f64::from(u8::from(*v))),
        (Value::Bool(v), DataType::Int) => Value::Int(i64::from(*v)),
        (Value::Vec3(v), DataType::Color) => {
            Value::Color([v[0] as f32, v[1] as f32, v[2] as f32, 1.0])
        }
        (Value::Vec4(v), DataType::Color) => {
            Value::Color([v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32])
        }
        (Value::Color(c), DataType::Vec3) => {
            Value::Vec3([f64::from(c[0]), f64::from(c[1]), f64::from(c[2])])
        }
        (Value::Color(c), DataType::Vec4) => Value::Vec4([
            f64::from(c[0]),
            f64::from(c[1]),
            f64::from(c[2]),
            f64::from(c[3]),
        ]),
        _ => return None,
    };
    debug_assert!(can_coerce(value.data_type(), to).is_legal());
    Some(coerced)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests

    use super::*;

    /// The exhaustive N-by-N snapshot (catalog section 2). Any matrix
    /// change is a visible diff in this expected text, never an accident.
    #[test]
    fn coercion_matrix_snapshot() {
        let mut grid = String::new();
        for from in DataType::ALL {
            for to in DataType::ALL {
                let cell = match can_coerce(from, to) {
                    Coercion::Same => '=',
                    Coercion::Lossless => '+',
                    Coercion::Lossy => '~',
                    Coercion::Forbidden => '.',
                };
                grid.push(cell);
            }
            grid.push('\n');
        }
        // Columns and rows in DataType::ALL order:
        // Geometry Light Report Float Int Bool Vec2 Vec3 Vec4 Color Text
        // Image Material
        let expected = "\
=............\n\
.=...........\n\
..=..........\n\
...=~.+++....\n\
...+=.+++....\n\
...++=.......\n\
......=......\n\
.......=.+...\n\
........=+...\n\
.......~+=...\n\
..........=..\n\
...........=.\n\
............=\n";
        assert_eq!(
            grid, expected,
            "coercion matrix changed; update the catalog first"
        );
    }

    #[test]
    fn geometry_light_report_image_material_coerce_to_and_from_nothing() {
        for hard in [
            DataType::Geometry,
            DataType::Light,
            DataType::Report,
            DataType::Image,
            DataType::Material,
        ] {
            for other in DataType::ALL {
                if other == hard {
                    continue;
                }
                assert_eq!(can_coerce(hard, other), Coercion::Forbidden);
                assert_eq!(can_coerce(other, hard), Coercion::Forbidden);
            }
        }
    }

    #[test]
    fn value_coercions_match_matrix_legality() {
        // A representative value per type.
        let samples: Vec<Value> = vec![
            Value::Geometry(Arc::new(GeometrySet::empty())),
            Value::Float(1.5),
            Value::Int(2),
            Value::Bool(true),
            Value::Vec2([1.0, 2.0]),
            Value::Vec3([1.0, 2.0, 3.0]),
            Value::Vec4([1.0, 2.0, 3.0, 4.0]),
            Value::Color([0.5, 0.25, 0.125, 0.75]),
            Value::Text("t".to_string()),
            Value::Image(Arc::new(RawImageData::new(vec![0, 0, 0, 255], 1, 1))),
        ];
        for v in &samples {
            for to in DataType::ALL {
                let legal = can_coerce(v.data_type(), to).is_legal();
                assert_eq!(
                    coerce_value(v, to).is_some(),
                    legal,
                    "{:?} -> {to:?}",
                    v.data_type()
                );
            }
        }
    }

    #[test]
    fn lossy_and_splat_semantics() {
        // Float -> Int rounds half away from zero.
        assert!(matches!(
            coerce_value(&Value::Float(-0.5), DataType::Int),
            Some(Value::Int(-1))
        ));
        // Splat fills every lane.
        assert!(matches!(
            coerce_value(&Value::Int(3), DataType::Vec3),
            Some(Value::Vec3([3.0, 3.0, 3.0]))
        ));
        // Vec3 -> Color gains alpha 1.
        let Some(Value::Color(c)) = coerce_value(&Value::Vec3([0.25, 0.5, 1.0]), DataType::Color)
        else {
            panic!("expected a color");
        };
        assert_eq!(c, [0.25, 0.5, 1.0, 1.0]);
        // Color -> Vec3 drops alpha.
        let Some(Value::Vec3(v)) =
            coerce_value(&Value::Color([0.1, 0.2, 0.3, 0.9]), DataType::Vec3)
        else {
            panic!("expected a vec3");
        };
        assert!((v[0] - 0.1).abs() < 1e-6);
        assert_eq!(v.len(), 3);
        // Bool -> Float/Int.
        assert!(matches!(
            coerce_value(&Value::Bool(true), DataType::Float),
            Some(Value::Float(v)) if v == 1.0
        ));
    }
}
