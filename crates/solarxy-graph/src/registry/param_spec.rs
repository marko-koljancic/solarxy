//! Declarative parameter schemas (node catalog part I, section 4).
//!
//! `ParamSpec` is what the parameter panel interprets and what the
//! resolver enforces. Conventions frozen here: hard range = validity
//! (engine-clamped in the resolver), soft range = the slider/drag range
//! (UI only); every angle is degrees in the document and UI with the
//! resolver converting to radians (`Unit::Degrees` drives both the suffix
//! and the conversion); `group` is presentation metadata, storage stays
//! flat.

use crate::params::ParamValue;

/// One selectable variant of an [`ParamType::Enum`] param.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumVariant {
    pub key: String,
    pub label: String,
}

impl EnumVariant {
    #[must_use]
    pub fn new(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
        }
    }
}

/// The parameter-editor vocabulary. Adding a variant is a deliberate
/// frontend change (a new editor widget must exist); everything else about
/// a node is zero-frontend-change by construction.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamType {
    Float,
    Int,
    Bool,
    Text,
    Vec2,
    Vec3,
    Vec4,
    Color,
    Enum {
        variants: Vec<EnumVariant>,
    },
    /// A staged-asset reference with an extension accept-list
    /// (e.g. `[".stl"]`).
    AssetRef {
        accept: Vec<String>,
    },
}

/// Unit annotation: drives the UI suffix and the resolver conversion
/// (degrees to radians) from one declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Unit {
    #[default]
    None,
    Degrees,
    Meters,
    Normalized,
}

/// Hard = validity (resolver-clamped); soft = the slider range (UI only).
/// Typed values may exceed soft but never hard.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamRange {
    pub hard: (f64, f64),
    pub soft: Option<(f64, f64)>,
}

/// Predicate grammar for conditional visibility. Deliberately tiny
/// (no OR, no expressions) until a node needs more.
#[derive(Debug, Clone, PartialEq)]
pub enum Pred {
    Truthy,
    Eq(ParamValue),
    Neq(ParamValue),
    In(Vec<ParamValue>),
}

/// One visibility condition; a spec's `show_if` list ANDs them.
#[derive(Debug, Clone, PartialEq)]
pub struct ShowIf {
    pub param: String,
    pub pred: Pred,
}

/// One parameter's declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamSpec {
    /// Storage key, unique across the whole node (groups do not scope
    /// keys).
    pub key: String,
    pub label: String,
    /// Two-level presentation grouping (general, geometry, transform, ...).
    pub group: String,
    pub ty: ParamType,
    pub default: ParamValue,
    pub range: Option<ParamRange>,
    pub step: Option<f64>,
    pub unit: Unit,
    /// AND semantics; empty means always visible.
    pub show_if: Vec<ShowIf>,
    /// Per-param documentation (hover popover; wiki reference).
    pub doc: String,
}

impl ParamSpec {
    /// A spec with the required fields; chain the builders for the rest.
    #[must_use]
    pub fn new(
        key: impl Into<String>,
        label: impl Into<String>,
        group: impl Into<String>,
        ty: ParamType,
        default: ParamValue,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            group: group.into(),
            ty,
            default,
            range: None,
            step: None,
            unit: Unit::None,
            show_if: Vec::new(),
            doc: String::new(),
        }
    }

    /// Sets the hard range (doubles as the slider range when no soft
    /// range is given, matching the catalog convention).
    #[must_use]
    pub fn hard(mut self, min: f64, max: f64) -> Self {
        self.range = Some(ParamRange {
            hard: (min, max),
            soft: self.range.and_then(|r| r.soft),
        });
        self
    }

    /// Sets the soft (slider) range; requires a hard range.
    #[must_use]
    pub fn soft(mut self, min: f64, max: f64) -> Self {
        if let Some(range) = &mut self.range {
            range.soft = Some((min, max));
        } else {
            debug_assert!(false, "soft range without a hard range on '{}'", self.key);
        }
        self
    }

    #[must_use]
    pub fn step(mut self, step: f64) -> Self {
        self.step = Some(step);
        self
    }

    #[must_use]
    pub fn unit(mut self, unit: Unit) -> Self {
        self.unit = unit;
        self
    }

    #[must_use]
    pub fn show_if(mut self, param: impl Into<String>, pred: Pred) -> Self {
        self.show_if.push(ShowIf {
            param: param.into(),
            pred,
        });
        self
    }

    #[must_use]
    pub fn doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = doc.into();
        self
    }
}
