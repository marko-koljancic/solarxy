//! Parameter values and the expression reserve (node catalog part I,
//! section 5).
//!
//! These are the **typed in-memory** forms. They deliberately do not derive
//! serde: the schema-v1 on-disk shape is *schema-typed, not
//! self-describing* (a literal serializes as its plain JSON value, an
//! expression as `{"$expr": "..."}`), so JSON conversion requires the
//! registry's `ParamSpec.ty` to disambiguate Int vs Float vs Enum vs Text
//! vs Asset and lives next to the registry (where migration hooks run on
//! the raw JSON before typing).

/// Identity of a staged asset: its content-addressed SHA-256 hex digest
/// (decision: file identity is content, not name + mtime + size).
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct AssetId(pub String);

/// One typed parameter value.
///
/// The serde form here is the **self-describing** (tagged) representation
/// used on the Command/Event boundary and in the registry snapshot. The
/// separate schema-typed plain form used in `.slxy` files is produced by
/// `crate::registry::resolve::param_value_to_json` (Phase 5), which the
/// registry's `ParamSpec.ty` disambiguates on read.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum ParamValue {
    Float(f64),
    Int(i64),
    Bool(bool),
    Text(String),
    Vec2([f64; 2]),
    Vec3([f64; 3]),
    Vec4([f64; 4]),
    /// Linear RGBA.
    Color([f32; 4]),
    /// The selected variant's key.
    Enum(String),
    Asset(AssetId),
}

/// Where a parameter's value comes from (decision 26: values are tagged so
/// expressions arrive additively post-beta).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ParamSource {
    Literal(ParamValue),
    /// Reserved: v1 refuses to evaluate and badges the node.
    Expression {
        expr: String,
    },
}

impl ParamSource {
    /// The literal value, if this source is a literal.
    #[must_use]
    pub fn literal(&self) -> Option<&ParamValue> {
        match self {
            ParamSource::Literal(v) => Some(v),
            ParamSource::Expression { .. } => None,
        }
    }
}

impl From<ParamValue> for ParamSource {
    fn from(v: ParamValue) -> Self {
        ParamSource::Literal(v)
    }
}
