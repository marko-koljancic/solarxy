//! Per-element program execution over a [`GeometrySet`].
//!
//! This module owns the *mechanics* of running a program once per point or
//! per primitive: binding the lanes a program touches to fixed slots,
//! gathering one element's values into a scratch register file, and writing
//! back only what the program actually assigned. It deliberately owns no
//! language at all.
//!
//! The language lives in `solarxy-graph`'s `expr` module, because an
//! expression can read another node's parameter and this crate has no
//! concept of a node. `solarxy-graph` depends on `solarxy-kernel` and never
//! the reverse, so the split runs the only way it can: the caller supplies
//! an [`ElementFn`], this module supplies the geometry.
//!
//! That is the same shape `deform_ops::displace_mesh` already uses, where
//! the node decides intent and the kernel does the per-point work.
//!
//! **Slots, not names.** Lane names resolve to slot indices once per
//! wrangle call, so the inner loop indexes an array instead of hashing a
//! string. Together with a register file reused across elements, that is
//! what keeps the per-element cost free of allocation.

use std::sync::Arc;

use crate::attribute_ops::LaneType;
use crate::error::KernelError;
use crate::set::{AttributeData, AttributeDomain, AttributeMap, GeometrySet, KernelMesh, reserved};

/// The widest lane this module carries. Vec4 is the largest
/// [`AttributeData`] variant, so four doubles hold any lane value.
const MAX_WIDTH: usize = 4;

/// One lane a program touches, resolved to a slot before the element loop.
///
/// `ty` is `None` for a lane the program only *reads*: its type comes from
/// the input. A written lane that is absent from the input is created at the
/// type inferred from its first assignment, which the caller records here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneBinding {
    /// The attribute name, without the leading `@`.
    pub name: String,
    /// Whether the program ever assigns this lane.
    pub written: bool,
}

impl LaneBinding {
    #[must_use]
    pub fn read(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            written: false,
        }
    }

    #[must_use]
    pub fn written(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            written: true,
        }
    }
}

/// One slot in the register file: a lane's value for the current element.
///
/// `width` is authoritative. A slot the input did not supply starts
/// `present: false` with width 0, and the program's first assignment both
/// sets the width and decides the created lane's type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slot {
    pub value: [f64; MAX_WIDTH],
    pub width: u8,
    /// The input carried this lane for this element.
    pub present: bool,
    /// The program assigned this slot while running the current element.
    pub assigned: bool,
}

impl Default for Slot {
    fn default() -> Self {
        Self {
            value: [0.0; MAX_WIDTH],
            width: 0,
            present: false,
            assigned: false,
        }
    }
}

/// The element scope handed to a program: the register file plus the two
/// counters every wrangle language exposes.
#[derive(Debug)]
pub struct Element<'a> {
    /// Slot-indexed, parallel to the `lanes` passed to [`wrangle`].
    pub slots: &'a mut [Slot],
    /// This element's index in its domain (`@ptnum` / `@primnum`).
    pub index: usize,
    /// How many elements the domain holds (`@numpt` / `@numprim`).
    pub count: usize,
}

/// A program run once per element. Implemented in `solarxy-graph`.
pub trait ElementFn {
    /// # Errors
    /// A type mismatch the statement layer can only detect with a value in
    /// hand. Arithmetic conditions such as division by zero are *not*
    /// errors: they yield the IEEE result, so one bad element cannot blank
    /// a scene (decision M-22).
    fn run(&self, element: &mut Element) -> Result<(), String>;
}

/// Runs `program` over every element of `domain` on every mesh in `set`.
///
/// Only lanes marked written in `lanes` are written back. Every buffer the
/// program does not assign rides through by `Arc` refcount bump, per the
/// sharing model [`crate::set`] documents.
///
/// # Errors
/// [`KernelError::Wrangle`] when the program fails on an element, naming
/// the mesh and element index, or when an assignment's width disagrees with
/// a lane the input already carries under a contractual type.
pub fn wrangle(
    set: &GeometrySet,
    domain: AttributeDomain,
    lanes: &[LaneBinding],
    program: &dyn ElementFn,
) -> Result<GeometrySet, KernelError> {
    let mut meshes = Vec::with_capacity(set.meshes.len());
    for mesh in &set.meshes {
        meshes.push(wrangle_mesh(mesh, domain, lanes, program)?);
    }
    Ok(GeometrySet::from_parts(meshes, set.materials.clone()))
}

/// How many elements `domain` has on `mesh`.
#[must_use]
pub fn element_count(mesh: &KernelMesh, domain: AttributeDomain) -> usize {
    match domain {
        AttributeDomain::Point => mesh.positions.len(),
        AttributeDomain::Primitive => mesh.primitive_count(),
    }
}

fn wrangle_mesh(
    mesh: &KernelMesh,
    domain: AttributeDomain,
    lanes: &[LaneBinding],
    program: &dyn ElementFn,
) -> Result<KernelMesh, KernelError> {
    let count = element_count(mesh, domain);
    let mut out = mesh.clone();
    if count == 0 || lanes.is_empty() {
        return Ok(out);
    }

    // Resolve every lane to a source once, before the loop. `None` means the
    // input does not carry it; a written lane in that state is created.
    let sources: Vec<Option<Source>> = lanes
        .iter()
        .map(|l| Source::resolve(mesh, domain, &l.name))
        .collect();

    // The write-back buffers, allocated once per mesh. A read-only lane gets
    // no buffer at all.
    let mut sinks: Vec<Option<Sink>> = lanes
        .iter()
        .map(|l| l.written.then(|| Sink::new(count)))
        .collect();

    let mut slots = vec![Slot::default(); lanes.len()];

    for index in 0..count {
        for (slot, source) in slots.iter_mut().zip(&sources) {
            *slot = Slot::default();
            if let Some(src) = source {
                src.read_into(index, slot);
            }
        }

        let mut element = Element {
            slots: &mut slots,
            index,
            count,
        };
        program.run(&mut element).map_err(|message| {
            KernelError::Wrangle(format!(
                "{} (mesh '{}', element {index})",
                message, mesh.name
            ))
        })?;

        for (i, slot) in slots.iter().enumerate() {
            if let Some(sink) = sinks[i].as_mut()
                && slot.assigned
            {
                sink.push(index, slot, &lanes[i].name, &mesh.name)?;
            }
        }
    }

    // Only assigned lanes are written back; everything else keeps its Arc.
    for (i, sink) in sinks.into_iter().enumerate() {
        let Some(sink) = sink else { continue };
        sink.commit(&mut out, domain, &lanes[i].name)?;
    }
    Ok(out)
}

/// Where one lane's values come from for this mesh.
enum Source<'a> {
    Positions(&'a Arc<Vec<[f32; 3]>>),
    Normals(&'a Arc<Vec<[f32; 3]>>),
    Uvs(&'a Arc<Vec<[f32; 2]>>),
    Lane(&'a AttributeData),
}

impl<'a> Source<'a> {
    fn resolve(mesh: &'a KernelMesh, domain: AttributeDomain, name: &str) -> Option<Self> {
        // The fixed buffers are point-domain by construction, so they only
        // answer in the point domain. In the primitive domain `@P` is not a
        // thing the input carries, and the lane map is the only source.
        if domain == AttributeDomain::Point {
            match name {
                "P" => return Some(Source::Positions(&mesh.positions)),
                reserved::NORMAL => {
                    if let Some(n) = &mesh.normals {
                        return Some(Source::Normals(n));
                    }
                }
                reserved::UV => {
                    if let Some(uv) = &mesh.tex_coords {
                        return Some(Source::Uvs(uv));
                    }
                }
                _ => {}
            }
        }
        let map = match domain {
            AttributeDomain::Point => &mesh.attributes,
            AttributeDomain::Primitive => &mesh.primitive_attributes,
        };
        map.get(name).map(Source::Lane)
    }

    fn read_into(&self, index: usize, slot: &mut Slot) {
        match self {
            Source::Positions(v) | Source::Normals(v) => {
                let Some(p) = v.get(index) else { return };
                slot.value[..3].copy_from_slice(&[
                    f64::from(p[0]),
                    f64::from(p[1]),
                    f64::from(p[2]),
                ]);
                slot.width = 3;
                slot.present = true;
            }
            Source::Uvs(v) => {
                let Some(p) = v.get(index) else { return };
                slot.value[..2].copy_from_slice(&[f64::from(p[0]), f64::from(p[1])]);
                slot.width = 2;
                slot.present = true;
            }
            Source::Lane(data) => {
                let width = match data {
                    AttributeData::Float(b) => {
                        let Some(v) = b.get(index) else { return };
                        slot.value[0] = f64::from(*v);
                        1
                    }
                    AttributeData::Vec2(b) => {
                        let Some(v) = b.get(index) else { return };
                        for (dst, src) in slot.value.iter_mut().zip(v) {
                            *dst = f64::from(*src);
                        }
                        2
                    }
                    AttributeData::Vec3(b) => {
                        let Some(v) = b.get(index) else { return };
                        for (dst, src) in slot.value.iter_mut().zip(v) {
                            *dst = f64::from(*src);
                        }
                        3
                    }
                    AttributeData::Vec4(b) => {
                        let Some(v) = b.get(index) else { return };
                        for (dst, src) in slot.value.iter_mut().zip(v) {
                            *dst = f64::from(*src);
                        }
                        4
                    }
                };
                slot.width = width;
                slot.present = true;
            }
        }
    }
}

/// The accumulating write-back buffer for one assigned lane.
///
/// The lane's type is fixed by the first element that assigns it, and every
/// later element must agree. A program whose branches assign different
/// widths is a type error naming both, not a silently ragged lane.
struct Sink {
    values: Vec<[f64; MAX_WIDTH]>,
    /// `None` until the first assignment decides it.
    width: Option<u8>,
    count: usize,
}

impl Sink {
    fn new(count: usize) -> Self {
        Self {
            values: vec![[0.0; MAX_WIDTH]; count],
            width: None,
            count,
        }
    }

    fn push(
        &mut self,
        index: usize,
        slot: &Slot,
        lane: &str,
        mesh: &str,
    ) -> Result<(), KernelError> {
        match self.width {
            None => self.width = Some(slot.width),
            Some(w) if w == slot.width => {}
            Some(w) => {
                let got = slot.width;
                return Err(KernelError::Wrangle(format!(
                    "`@{lane}` is assigned a {got}-component value at element {index} \
                     but a {w}-component value earlier (mesh '{mesh}'); a lane has \
                     one type"
                )));
            }
        }
        self.values[index] = slot.value;
        Ok(())
    }

    /// Writes the lane onto `mesh`, or does nothing when no element
    /// assigned it (a program guarded by a condition nothing satisfied).
    fn commit(
        self,
        mesh: &mut KernelMesh,
        domain: AttributeDomain,
        lane: &str,
    ) -> Result<(), KernelError> {
        let Some(width) = self.width else {
            return Ok(());
        };
        // `@P`, `@N` and `@uv` rebuild the fixed buffers rather than
        // creating a lane that shadows them.
        if domain == AttributeDomain::Point {
            match lane {
                "P" => {
                    require_width(width, 3, lane, "position")?;
                    mesh.positions = Arc::new(self.values.iter().map(to_f32x3).collect());
                    return Ok(());
                }
                reserved::NORMAL => {
                    require_width(width, 3, lane, "normal")?;
                    mesh.normals = Some(Arc::new(self.values.iter().map(to_f32x3).collect()));
                    return Ok(());
                }
                reserved::UV => {
                    require_width(width, 2, lane, "texture coordinate")?;
                    mesh.tex_coords = Some(Arc::new(self.values.iter().map(to_f32x2).collect()));
                    return Ok(());
                }
                _ => {}
            }
        }
        // The colour lane is contractually Vec4 linear RGBA, but every
        // wrangle language writes colour as three components. Widening with
        // an opaque alpha is what makes `@Cd = set(1, 0, 0)` mean red
        // instead of failing a contract the user never saw.
        if lane == reserved::COLOR && width == 3 {
            let rgba = self
                .values
                .iter()
                .map(|v| [v[0] as f32, v[1] as f32, v[2] as f32, 1.0])
                .collect();
            attribute_map(mesh, domain)
                .insert(lane.to_string(), AttributeData::Vec4(Arc::new(rgba)));
            return Ok(());
        }
        if lane == reserved::COLOR && width != 4 {
            return Err(KernelError::Wrangle(format!(
                "`@Cd` is the colour lane and holds 3 or 4 components, but the program \
                 assigns {width}"
            )));
        }

        let data = match width {
            1 => AttributeData::Float(Arc::new(self.values.iter().map(|v| v[0] as f32).collect())),
            2 => AttributeData::Vec2(Arc::new(self.values.iter().map(to_f32x2).collect())),
            3 => AttributeData::Vec3(Arc::new(self.values.iter().map(to_f32x3).collect())),
            _ => AttributeData::Vec4(Arc::new(self.values.iter().map(to_f32x4).collect())),
        };
        debug_assert_eq!(data.len(), self.count, "sink length tracks the domain");
        attribute_map(mesh, domain).insert(lane.to_string(), data);
        Ok(())
    }
}

/// The lane map for a domain.
fn attribute_map(mesh: &mut KernelMesh, domain: AttributeDomain) -> &mut AttributeMap {
    match domain {
        AttributeDomain::Point => &mut mesh.attributes,
        AttributeDomain::Primitive => &mut mesh.primitive_attributes,
    }
}

/// A reserved lane keeps its contractual type; a mismatched assignment
/// names both rather than corrupting a buffer the renderer trusts.
fn require_width(got: u8, want: u8, lane: &str, what: &str) -> Result<(), KernelError> {
    if got == want {
        return Ok(());
    }
    Err(KernelError::Wrangle(format!(
        "`@{lane}` is the {what} and holds {want} components, but the program \
         assigns {got}"
    )))
}

fn to_f32x2(v: &[f64; MAX_WIDTH]) -> [f32; 2] {
    [v[0] as f32, v[1] as f32]
}

fn to_f32x3(v: &[f64; MAX_WIDTH]) -> [f32; 3] {
    [v[0] as f32, v[1] as f32, v[2] as f32]
}

fn to_f32x4(v: &[f64; MAX_WIDTH]) -> [f32; 4] {
    [v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32]
}

/// The lane type a width writes, for callers that report what a program
/// would create before running it.
#[must_use]
pub fn lane_type_for_width(width: u8) -> Option<LaneType> {
    match width {
        1 => Some(LaneType::Float),
        2 => Some(LaneType::Vec2),
        3 => Some(LaneType::Vec3),
        4 => Some(LaneType::Vec4),
        _ => None,
    }
}

/// The reserved names the element scope exposes as fixed buffers rather
/// than free-form lanes, so callers can document and validate them.
#[must_use]
pub fn is_fixed_buffer_lane(name: &str) -> bool {
    matches!(name, "P" | reserved::NORMAL | reserved::UV)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::generate_box;

    /// A program built from a closure, so a test states its intent inline.
    struct Fn_<F>(F);
    impl<F: Fn(&mut Element) -> Result<(), String>> ElementFn for Fn_<F> {
        fn run(&self, element: &mut Element) -> Result<(), String> {
            (self.0)(element)
        }
    }

    fn a_box() -> GeometrySet {
        GeometrySet::from_parts(vec![generate_box(1.0, 1.0, 1.0, 1, 1, 1)], Vec::new())
    }

    fn assign(slot: &mut Slot, lanes: &[f64]) {
        slot.value = [0.0; MAX_WIDTH];
        slot.value[..lanes.len()].copy_from_slice(lanes);
        slot.width = u8::try_from(lanes.len()).expect("width fits");
        slot.assigned = true;
    }

    #[test]
    fn a_program_that_assigns_nothing_leaves_every_buffer_arc_identical() {
        let set = a_box();
        let lanes = [LaneBinding::read("P")];
        let out = wrangle(
            &set,
            AttributeDomain::Point,
            &lanes,
            &Fn_(|_: &mut Element| Ok(())),
        )
        .expect("runs");

        for (before, after) in set.meshes.iter().zip(&out.meshes) {
            assert!(
                Arc::ptr_eq(&before.positions, &after.positions),
                "positions were rebuilt despite no assignment"
            );
            assert!(Arc::ptr_eq(&before.indices, &after.indices));
            match (&before.normals, &after.normals) {
                (Some(a), Some(b)) => assert!(Arc::ptr_eq(a, b), "normals were rebuilt"),
                (None, None) => {}
                _ => panic!("normal presence changed"),
            }
        }
    }

    #[test]
    fn assigning_p_rebuilds_positions_per_element() {
        let set = a_box();
        let lanes = [LaneBinding::written("P")];
        let out = wrangle(
            &set,
            AttributeDomain::Point,
            &lanes,
            &Fn_(|el: &mut Element| {
                let p = el.slots[0].value;
                assign(&mut el.slots[0], &[p[0] * 2.0, p[1], p[2]]);
                Ok(())
            }),
        )
        .expect("runs");

        for (before, after) in set.meshes.iter().zip(&out.meshes) {
            for (b, a) in before.positions.iter().zip(after.positions.iter()) {
                assert!((a[0] - b[0] * 2.0).abs() < 1e-6, "x doubled");
                assert!((a[1] - b[1]).abs() < 1e-6, "y untouched");
            }
        }
    }

    #[test]
    fn an_absent_lane_is_created_at_the_width_its_first_assignment_uses() {
        let set = a_box();
        let lanes = [LaneBinding::written("heat")];
        let out = wrangle(
            &set,
            AttributeDomain::Point,
            &lanes,
            &Fn_(|el: &mut Element| {
                assert!(!el.slots[0].present, "the input carries no `heat`");
                let t = el.index as f64 / el.count.max(1) as f64;
                assign(&mut el.slots[0], &[t]);
                Ok(())
            }),
        )
        .expect("runs");

        let lane = out.meshes[0].attributes.get("heat").expect("lane created");
        assert!(matches!(lane, AttributeData::Float(_)), "inferred Float");
        assert_eq!(lane.len(), out.meshes[0].positions.len());
    }

    #[test]
    fn a_lane_assigned_two_different_widths_is_an_error_naming_both() {
        let set = a_box();
        let lanes = [LaneBinding::written("mixed")];
        let err = wrangle(
            &set,
            AttributeDomain::Point,
            &lanes,
            &Fn_(|el: &mut Element| {
                if el.index == 0 {
                    assign(&mut el.slots[0], &[1.0]);
                } else {
                    assign(&mut el.slots[0], &[1.0, 2.0, 3.0]);
                }
                Ok(())
            }),
        )
        .unwrap_err();
        let message = err.to_string();
        assert!(message.contains("3-component"), "{message}");
        assert!(message.contains("1-component"), "{message}");
    }

    #[test]
    fn a_reserved_lane_keeps_its_contractual_width() {
        let set = a_box();
        let lanes = [LaneBinding::written(reserved::NORMAL)];
        let err = wrangle(
            &set,
            AttributeDomain::Point,
            &lanes,
            &Fn_(|el: &mut Element| {
                assign(&mut el.slots[0], &[0.5, 0.5]);
                Ok(())
            }),
        )
        .unwrap_err();
        assert!(err.to_string().contains("normal"), "{err}");
    }

    #[test]
    fn the_primitive_domain_runs_once_per_primitive_not_once_per_point() {
        let set = a_box();
        let prims = set.meshes[0].primitive_count();
        let points = set.meshes[0].positions.len();
        assert_ne!(prims, points, "the fixture must distinguish the two");

        let lanes = [LaneBinding::written("id")];
        let out = wrangle(
            &set,
            AttributeDomain::Primitive,
            &lanes,
            &Fn_(|el: &mut Element| {
                assign(&mut el.slots[0], &[el.count as f64]);
                Ok(())
            }),
        )
        .expect("runs");

        let lane = out.meshes[0]
            .primitive_attributes
            .get("id")
            .expect("primitive lane");
        assert_eq!(lane.len(), prims);
    }

    #[test]
    fn an_element_failure_names_the_mesh_and_the_element() {
        let set = a_box();
        let lanes = [LaneBinding::read("P")];
        let err = wrangle(
            &set,
            AttributeDomain::Point,
            &lanes,
            &Fn_(|el: &mut Element| {
                if el.index == 2 {
                    return Err("something specific went wrong".into());
                }
                Ok(())
            }),
        )
        .unwrap_err();
        let message = err.to_string();
        assert!(message.contains("element 2"), "{message}");
        assert!(message.contains("something specific"), "{message}");
    }

    #[test]
    fn ptnum_and_numpt_are_the_element_index_and_domain_size() {
        let set = a_box();
        let expected = set.meshes[0].positions.len();
        let lanes = [LaneBinding::written("seen")];
        let out = wrangle(
            &set,
            AttributeDomain::Point,
            &lanes,
            &Fn_(move |el: &mut Element| {
                assert_eq!(el.count, expected);
                assign(&mut el.slots[0], &[el.index as f64]);
                Ok(())
            }),
        )
        .expect("runs");

        let AttributeData::Float(seen) = out.meshes[0].attributes.get("seen").expect("lane") else {
            panic!("expected a Float lane");
        };
        assert_eq!(seen.len(), expected);
        assert!(
            seen.iter()
                .enumerate()
                .all(|(i, v)| (*v - i as f32).abs() < 1e-6),
            "each element saw its own index"
        );
    }
}
