//! One named point lane resolved on one mesh, whatever storage holds it.
//!
//! Producers split between two storages: `uv_project`, `compute_normals`
//! and the importers write the fixed buffers, `scatter` and the attribute
//! nodes write the map. Every consumer of a named lane resolves through
//! here rather than reading the map alone, so the reserved names answer
//! whichever way they were written. Both shells' attribute channel and the
//! engine's own nodes read lanes this way, which is why it lives with the
//! geometry rather than with the engine.

use crate::set::{AttributeData, KernelMesh, reserved};

/// The declared type of an attribute buffer, in the vocabulary the
/// registry and the attribute table speak.
#[must_use]
pub fn type_name(data: &AttributeData) -> &'static str {
    match data {
        AttributeData::Float(_) => "float",
        AttributeData::Vec2(_) => "vec2",
        AttributeData::Vec3(_) => "vec3",
        AttributeData::Vec4(_) => "vec4",
    }
}

/// One point-domain lane resolved by name on one mesh: a MAP lane, or a
/// fixed reserved buffer exposed as a pseudo-lane (`N` = `mesh.normals`,
/// `uv` = `mesh.tex_coords`).
#[derive(Clone, Copy)]
pub enum LaneRef<'a> {
    Map(&'a AttributeData),
    Normals(&'a [[f32; 3]]),
    Uvs(&'a [[f32; 2]]),
}

/// Resolves `name` against `mesh`'s POINT domain: the map lane when
/// present (the map shadows the fixed buffers on a name collision), else
/// the matching fixed buffer for the reserved names.
#[must_use]
pub fn resolve_lane<'a>(mesh: &'a KernelMesh, name: &str) -> Option<LaneRef<'a>> {
    if let Some(data) = mesh.attributes.get(name) {
        return Some(LaneRef::Map(data));
    }
    if name == reserved::NORMAL {
        return mesh.normals.as_deref().map(|n| LaneRef::Normals(n));
    }
    if name == reserved::UV {
        return mesh.tex_coords.as_deref().map(|uv| LaneRef::Uvs(uv));
    }
    None
}

impl LaneRef<'_> {
    #[must_use]
    pub fn ty(&self) -> &'static str {
        match self {
            LaneRef::Map(data) => type_name(data),
            LaneRef::Normals(_) => "vec3",
            LaneRef::Uvs(_) => "vec2",
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            LaneRef::Map(data) => data.len(),
            LaneRef::Normals(v) => v.len(),
            LaneRef::Uvs(v) => v.len(),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Component `c` of element `i` under the declared column type `ty`;
    /// `None` on a type conflict (the page's null-not-zero rule) or out
    /// of range.
    #[must_use]
    pub fn component(&self, ty: &str, i: usize, c: usize) -> Option<f64> {
        if self.ty() != ty {
            return None;
        }
        match self {
            LaneRef::Map(AttributeData::Float(v)) => v.get(i).map(|x| f64::from(*x)),
            LaneRef::Map(AttributeData::Vec2(v)) => v.get(i).map(|x| f64::from(x[c])),
            LaneRef::Map(AttributeData::Vec3(v)) => v.get(i).map(|x| f64::from(x[c])),
            LaneRef::Map(AttributeData::Vec4(v)) => v.get(i).map(|x| f64::from(x[c])),
            LaneRef::Normals(v) => v.get(i).map(|x| f64::from(x[c])),
            LaneRef::Uvs(v) => v.get(i).map(|x| f64::from(x[c])),
        }
    }

    /// Every component of element `i` (the pin value labels).
    #[must_use]
    pub fn components(&self, i: usize) -> Option<Vec<f32>> {
        match self {
            LaneRef::Map(AttributeData::Float(v)) => v.get(i).map(|x| vec![*x]),
            LaneRef::Map(AttributeData::Vec2(v)) => v.get(i).map(|x| x.to_vec()),
            LaneRef::Map(AttributeData::Vec3(v)) => v.get(i).map(|x| x.to_vec()),
            LaneRef::Map(AttributeData::Vec4(v)) => v.get(i).map(|x| x.to_vec()),
            LaneRef::Normals(v) => v.get(i).map(|x| x.to_vec()),
            LaneRef::Uvs(v) => v.get(i).map(|x| x.to_vec()),
        }
    }

    /// The xyz arrow direction of element `i`: `Some` for vec3 lanes and
    /// vec4 lanes (w dropped); `None` for float/vec2 (no spatial reading).
    #[must_use]
    pub fn direction(&self, i: usize) -> Option<[f32; 3]> {
        match self {
            LaneRef::Map(AttributeData::Vec3(v)) => v.get(i).copied(),
            LaneRef::Map(AttributeData::Vec4(v)) => v.get(i).map(|x| [x[0], x[1], x[2]]),
            LaneRef::Normals(v) => v.get(i).copied(),
            LaneRef::Map(AttributeData::Float(_) | AttributeData::Vec2(_)) | LaneRef::Uvs(_) => {
                None
            }
        }
    }
}
