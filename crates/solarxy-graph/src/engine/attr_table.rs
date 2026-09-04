//! Read-only attribute inspection over a node's cooked geometry: the lane
//! summary feeding the attribute-name pickers and the paged value table
//! feeding the Attributes pane. Pure views over `GeometrySet` (the `Arc`
//! itself never leaves the engine); a shell serializes the returned DTOs
//! across its boundary, so every field is camelCase like the rest of the
//! engine contract.

use serde::Serialize;
use solarxy_kernel::{AttributeData, AttributeDomain, GeometrySet, KernelMesh};

/// The hard ceiling on one page. Small enough that a page is a few KB of
/// JSON; a virtualized table fetches windows, never the whole geometry.
pub const ATTR_PAGE_LIMIT: u32 = 256;

/// One named lane in one domain: its declared type and element count
/// (summed across meshes; a lane missing on some meshes counts only where
/// it exists, so `len` can trail the domain's element total).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttrLane {
    pub name: String,
    /// `"float" | "vec2" | "vec3" | "vec4"`.
    pub ty: &'static str,
    pub len: u64,
}

/// The lane inventory of a node's cooked geometry, both domains at once
/// (the pickers want names without paging values).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributeSummary {
    pub points: u64,
    /// Triangle count, matching the cook-stats convention (line and point
    /// meshes contribute zero).
    pub prims: u64,
    /// Primitive-DOMAIN element count (topology-aware: triangles,
    /// segments, or point primitives). The primitive table's row count;
    /// distinct from `prims` for non-triangle meshes by design.
    pub primitive_elements: u64,
    pub meshes: u32,
    pub point: Vec<AttrLane>,
    pub primitive: Vec<AttrLane>,
}

/// One value column of the paged table. `P` (positions, point domain only)
/// leads; attribute lanes follow in `BTreeMap` (name) order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttrColumn {
    pub key: String,
    pub ty: &'static str,
    pub components: u8,
}

/// One window of element rows. A row concatenates every column's
/// components in order; a lane missing on the element's mesh (or carrying
/// a different type there) yields `null`s, never fabricated zeros.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributePage {
    /// Total element count in the domain (the scrollbar's extent).
    pub total: u64,
    pub offset: u32,
    pub columns: Vec<AttrColumn>,
    pub rows: Vec<Vec<Option<f64>>>,
}

fn lane_ty(data: &AttributeData) -> &'static str {
    match data {
        AttributeData::Float(_) => "float",
        AttributeData::Vec2(_) => "vec2",
        AttributeData::Vec3(_) => "vec3",
        AttributeData::Vec4(_) => "vec4",
    }
}

fn ty_components(ty: &str) -> u8 {
    match ty {
        "float" => 1,
        "vec2" => 2,
        "vec3" => 3,
        _ => 4,
    }
}

/// Element count of one mesh in one domain.
fn domain_len(mesh: &KernelMesh, domain: AttributeDomain) -> usize {
    match domain {
        AttributeDomain::Point => mesh.vertex_count(),
        AttributeDomain::Primitive => mesh.primitive_count(),
    }
}

/// One point-domain lane resolved by name on one mesh: a MAP lane, or a
/// fixed reserved buffer exposed as a pseudo-lane (`N` = `mesh.normals`,
/// `uv` = `mesh.tex_coords`). Producers split between the two storages
/// (`uv_project`/`compute_normals`/imports write the fixed buffers,
/// `scatter` and the attribute nodes write the map), so every consumer of
/// named lanes resolves through here rather than reading the map alone.
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
    if name == solarxy_kernel::reserved::NORMAL {
        return mesh.normals.as_deref().map(|n| LaneRef::Normals(n));
    }
    if name == solarxy_kernel::reserved::UV {
        return mesh.tex_coords.as_deref().map(|uv| LaneRef::Uvs(uv));
    }
    None
}

impl LaneRef<'_> {
    #[must_use]
    pub fn ty(&self) -> &'static str {
        match self {
            LaneRef::Map(data) => lane_ty(data),
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

/// The fixed-buffer pseudo-lanes of one mesh (Point domain only), skipping
/// any name a map lane shadows.
fn pseudo_lanes(mesh: &KernelMesh) -> impl Iterator<Item = (&'static str, &'static str, usize)> {
    let n = mesh
        .normals
        .as_ref()
        .filter(|_| {
            !mesh
                .attributes
                .contains_key(solarxy_kernel::reserved::NORMAL)
        })
        .map(|v| (solarxy_kernel::reserved::NORMAL, "vec3", v.len()));
    let uv = mesh
        .tex_coords
        .as_ref()
        .filter(|_| !mesh.attributes.contains_key(solarxy_kernel::reserved::UV))
        .map(|v| (solarxy_kernel::reserved::UV, "vec2", v.len()));
    n.into_iter().chain(uv)
}

fn lanes(set: &GeometrySet, domain: AttributeDomain) -> Vec<AttrLane> {
    let mut merged: std::collections::BTreeMap<&str, (&'static str, u64)> =
        std::collections::BTreeMap::new();
    for mesh in &set.meshes {
        for (name, data) in mesh.domain_attributes(domain) {
            let entry = merged.entry(name).or_insert((lane_ty(data), 0));
            // The first-seen type names the lane; a conflicting mesh still
            // counts its elements (the page shows nulls there instead).
            entry.1 += data.len() as u64;
        }
        if domain == AttributeDomain::Point {
            for (name, ty, len) in pseudo_lanes(mesh) {
                let entry = merged.entry(name).or_insert((ty, 0));
                entry.1 += len as u64;
            }
        }
    }
    merged
        .into_iter()
        .map(|(name, (ty, len))| AttrLane {
            name: name.to_string(),
            ty,
            len,
        })
        .collect()
}

/// The lane inventory of a cooked set.
#[must_use]
pub fn attribute_summary(set: &GeometrySet) -> AttributeSummary {
    AttributeSummary {
        points: set.point_count(),
        prims: set.triangle_count(),
        primitive_elements: set.meshes.iter().map(|m| m.primitive_count() as u64).sum(),
        meshes: u32::try_from(set.meshes.len()).unwrap_or(u32::MAX),
        point: lanes(set, AttributeDomain::Point),
        primitive: lanes(set, AttributeDomain::Primitive),
    }
}

/// One window of the value table, elements running across meshes in mesh
/// order. `offset` past the end yields an empty `rows`, never an error;
/// `limit` clamps to [`ATTR_PAGE_LIMIT`].
#[must_use]
pub fn attribute_page(
    set: &GeometrySet,
    domain: AttributeDomain,
    offset: u32,
    limit: u32,
) -> AttributePage {
    let limit = limit.min(ATTR_PAGE_LIMIT) as usize;
    let total: u64 = set
        .meshes
        .iter()
        .map(|m| domain_len(m, domain) as u64)
        .sum();

    let mut columns = Vec::new();
    if domain == AttributeDomain::Point {
        columns.push(AttrColumn {
            key: "P".to_string(),
            ty: "vec3",
            components: 3,
        });
    }
    for lane in lanes(set, domain) {
        columns.push(AttrColumn {
            key: lane.name,
            ty: lane.ty,
            components: ty_components(lane.ty),
        });
    }

    let mut rows = Vec::new();
    // Walk meshes tracking the global element index; emit the window.
    let mut global = 0usize;
    let start = offset as usize;
    let end = start.saturating_add(limit);
    for mesh in &set.meshes {
        let len = domain_len(mesh, domain);
        if global + len <= start {
            global += len;
            continue;
        }
        let attrs = mesh.domain_attributes(domain);
        let from = start.saturating_sub(global);
        let to = len.min(end - global);
        for local in from..to {
            let mut row: Vec<Option<f64>> = Vec::new();
            for col in &columns {
                if col.key == "P" && domain == AttributeDomain::Point {
                    let p = mesh.positions.get(local);
                    for c in 0..3 {
                        row.push(p.map(|p| f64::from(p[c])));
                    }
                    continue;
                }
                // Point columns resolve through LaneRef (map lanes plus
                // the fixed-buffer pseudo-lanes); the primitive domain has
                // no pseudo-lanes and stays a plain map read.
                let lane = match domain {
                    AttributeDomain::Point => resolve_lane(mesh, &col.key),
                    AttributeDomain::Primitive => attrs.get(&col.key).map(LaneRef::Map),
                };
                match lane {
                    Some(lane) => {
                        for c in 0..col.components as usize {
                            row.push(lane.component(col.ty, local, c));
                        }
                    }
                    None => row.extend(std::iter::repeat_n(None, col.components as usize)),
                }
            }
            rows.push(row);
        }
        global += len;
        if global >= end {
            break;
        }
    }

    AttributePage {
        total,
        offset,
        columns,
        rows,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests

    use super::*;
    use std::sync::Arc;

    fn pts(name: &str, positions: Vec<[f32; 3]>) -> KernelMesh {
        KernelMesh::points(name, positions)
    }

    #[test]
    fn summary_lists_lanes_across_meshes_in_name_order() {
        let mut a = pts("a", vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
        a.attributes.insert(
            "mask".into(),
            AttributeData::Float(Arc::new(vec![0.5, 1.0])),
        );
        let mut b = pts("b", vec![[2.0, 0.0, 0.0]]);
        b.attributes.insert(
            "color".into(),
            AttributeData::Vec4(Arc::new(vec![[1.0, 0.0, 0.0, 1.0]])),
        );
        b.primitive_attributes
            .insert("area".into(), AttributeData::Float(Arc::new(vec![0.25])));
        let set = GeometrySet {
            meshes: vec![a, b],
            ..GeometrySet::empty()
        };

        let s = attribute_summary(&set);
        assert_eq!(s.points, 3);
        assert_eq!(s.meshes, 2);
        assert_eq!(
            s.point
                .iter()
                .map(|l| (l.name.as_str(), l.ty, l.len))
                .collect::<Vec<_>>(),
            vec![("color", "vec4", 1), ("mask", "float", 2)],
        );
        assert_eq!(
            s.primitive
                .iter()
                .map(|l| (l.name.as_str(), l.ty))
                .collect::<Vec<_>>(),
            vec![("area", "float")],
        );
    }

    #[test]
    fn a_page_windows_across_mesh_boundaries_with_nulls_for_missing_lanes() {
        let mut a = pts("a", vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
        a.attributes.insert(
            "mask".into(),
            AttributeData::Float(Arc::new(vec![0.5, 1.0])),
        );
        let b = pts("b", vec![[2.0, 0.0, 0.0], [3.0, 0.0, 0.0]]);
        let set = GeometrySet {
            meshes: vec![a, b],
            ..GeometrySet::empty()
        };

        // Window [1, 3): the second point of mesh a, the first of mesh b.
        let page = attribute_page(&set, AttributeDomain::Point, 1, 2);
        assert_eq!(page.total, 4);
        assert_eq!(page.offset, 1);
        assert_eq!(
            page.columns
                .iter()
                .map(|c| (c.key.as_str(), c.components))
                .collect::<Vec<_>>(),
            vec![("P", 3), ("mask", 1)],
        );
        assert_eq!(
            page.rows,
            vec![
                vec![Some(1.0), Some(0.0), Some(0.0), Some(1.0)],
                vec![Some(2.0), Some(0.0), Some(0.0), None],
            ],
        );
    }

    #[test]
    fn the_primitive_domain_pages_without_a_p_column() {
        let mut m = KernelMesh::new(
            "t",
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        );
        m.primitive_attributes
            .insert("area".into(), AttributeData::Float(Arc::new(vec![0.5])));
        let set = GeometrySet::from_mesh(m);

        let page = attribute_page(&set, AttributeDomain::Primitive, 0, 16);
        assert_eq!(page.total, 1);
        assert_eq!(page.columns.len(), 1);
        assert_eq!(page.columns[0].key, "area");
        assert_eq!(page.rows, vec![vec![Some(0.5)]]);
    }

    #[test]
    fn offset_past_the_end_and_oversized_limits_stay_calm() {
        let set = GeometrySet::from_mesh(pts("a", vec![[0.0, 0.0, 0.0]]));
        let page = attribute_page(&set, AttributeDomain::Point, 99, 10_000);
        assert_eq!(page.total, 1);
        assert!(page.rows.is_empty());
    }

    #[test]
    fn a_type_conflicted_lane_reads_null_on_the_conflicting_mesh() {
        // Mesh a declares `w` float, mesh b declares `w` vec2: the column
        // takes the first-seen type and b's values read as nulls.
        let mut a = pts("a", vec![[0.0, 0.0, 0.0]]);
        a.attributes
            .insert("w".into(), AttributeData::Float(Arc::new(vec![7.0])));
        let mut b = pts("b", vec![[1.0, 0.0, 0.0]]);
        b.attributes
            .insert("w".into(), AttributeData::Vec2(Arc::new(vec![[1.0, 2.0]])));
        let set = GeometrySet {
            meshes: vec![a, b],
            ..GeometrySet::empty()
        };

        let page = attribute_page(&set, AttributeDomain::Point, 0, 8);
        let w_col = page.columns.iter().position(|c| c.key == "w").unwrap();
        assert_eq!(page.columns[w_col].ty, "float");
        // Row 0 (mesh a): the float value. Row 1 (mesh b): null.
        assert_eq!(page.rows[0][3], Some(7.0));
        assert_eq!(page.rows[1][3], None);
    }

    /// A mesh with fixed normals + UVs and one map lane, the producer mix
    /// `uv_project` / `compute_normals` / `attribute_create` leave behind.
    fn fixed_buffer_mesh() -> KernelMesh {
        let mut m = pts("f", vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
        m.normals = Some(Arc::new(vec![[0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]));
        m.tex_coords = Some(Arc::new(vec![[0.0, 0.0], [1.0, 1.0]]));
        m.attributes.insert(
            "mask".into(),
            AttributeData::Float(Arc::new(vec![0.5, 1.0])),
        );
        m
    }

    #[test]
    fn fixed_normals_and_uvs_resolve_as_typed_pseudo_lanes() {
        let m = fixed_buffer_mesh();
        let n = resolve_lane(&m, "N").expect("normals resolve");
        assert_eq!(n.ty(), "vec3");
        assert_eq!(n.len(), 2);
        assert_eq!(n.direction(1), Some([0.0, 0.0, 1.0]));
        let uv = resolve_lane(&m, "uv").expect("uvs resolve");
        assert_eq!(uv.ty(), "vec2");
        assert_eq!(uv.components(1), Some(vec![1.0, 1.0]));
        assert_eq!(uv.direction(0), None, "vec2 has no spatial reading");
        assert!(resolve_lane(&m, "nonesuch").is_none());
    }

    #[test]
    fn resolve_lane_prefers_the_map_lane_over_the_fixed_buffer() {
        // Scatter writes an `N` MAP lane; a mesh may also carry fixed
        // normals. The map wins (it is the cooked, intentional lane).
        let mut m = fixed_buffer_mesh();
        m.attributes.insert(
            "N".into(),
            AttributeData::Vec3(Arc::new(vec![[1.0, 0.0, 0.0], [1.0, 0.0, 0.0]])),
        );
        let n = resolve_lane(&m, "N").expect("resolves");
        assert!(matches!(n, LaneRef::Map(_)));
        assert_eq!(n.direction(0), Some([1.0, 0.0, 0.0]));
    }

    #[test]
    fn lanes_merge_pseudo_lanes_name_sorted_with_correct_lens() {
        let set = GeometrySet {
            meshes: vec![fixed_buffer_mesh(), pts("bare", vec![[2.0, 0.0, 0.0]])],
            ..GeometrySet::empty()
        };
        let s = attribute_summary(&set);
        // Byte order: "N" < "mask" < "uv". The bare mesh contributes to no
        // lane (no buffers, no map), so lens count only the first mesh.
        assert_eq!(
            s.point
                .iter()
                .map(|l| (l.name.as_str(), l.ty, l.len))
                .collect::<Vec<_>>(),
            vec![("N", "vec3", 2), ("mask", "float", 2), ("uv", "vec2", 2)],
        );
    }

    #[test]
    fn pseudo_lanes_stay_out_of_the_primitive_domain() {
        let set = GeometrySet::from_mesh(fixed_buffer_mesh());
        let s = attribute_summary(&set);
        assert!(s.primitive.is_empty());
        let page = attribute_page(&set, AttributeDomain::Primitive, 0, 8);
        assert!(page.columns.iter().all(|c| c.key != "N" && c.key != "uv"));
    }

    #[test]
    fn a_page_reads_fixed_buffers_and_shadowing_map_lanes() {
        let set = GeometrySet::from_mesh(fixed_buffer_mesh());
        let page = attribute_page(&set, AttributeDomain::Point, 0, 8);
        // Columns: P (3) then N (3), mask (1), uv (2).
        assert_eq!(
            page.columns
                .iter()
                .map(|c| (c.key.as_str(), c.components))
                .collect::<Vec<_>>(),
            vec![("P", 3), ("N", 3), ("mask", 1), ("uv", 2)],
        );
        // Row 1: P=(1,0,0), N=(0,0,1), mask=1, uv=(1,1).
        assert_eq!(
            page.rows[1],
            vec![
                Some(1.0),
                Some(0.0),
                Some(0.0),
                Some(0.0),
                Some(0.0),
                Some(1.0),
                Some(1.0),
                Some(1.0),
                Some(1.0),
            ],
        );
    }

    #[test]
    fn direction_reads_vec3_and_the_xyz_of_vec4_lanes() {
        let mut m = pts("v", vec![[0.0, 0.0, 0.0]]);
        m.attributes.insert(
            "velocity".into(),
            AttributeData::Vec4(Arc::new(vec![[0.1, 0.2, 0.3, 1.0]])),
        );
        let lane = resolve_lane(&m, "velocity").expect("resolves");
        assert_eq!(lane.ty(), "vec4");
        assert_eq!(lane.direction(0), Some([0.1, 0.2, 0.3]));
        assert_eq!(lane.direction(9), None, "out of range");
    }
}
