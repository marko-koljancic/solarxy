//! Geometry queries for expressions, over a node's own gathered inputs.
//!
//! `npoints()`, `bbox("size")` and friends read cook output, unlike `ch()`
//! which reads document state. That sounds like it needs a new ordering
//! guarantee and does not: a node's inputs are exactly what the wire
//! topology already cooked before it, so the values are there by the time
//! `resolve_params` runs (gather happens first in `cook_one`).
//!
//! Only the **default geometry input** is queried. A merge node with three
//! inputs would otherwise make `npoints()` ambiguous, and picking "the
//! first one" silently is the kind of rule nobody remembers.

use crate::cook::Inputs;
use crate::expr::{GeoQueries, Value};
use solarxy_kernel::GeometrySet;

/// Wraps the gathered inputs so the evaluator can query them without
/// knowing what a `GeometrySet` is.
///
/// It holds a counted reference rather than borrowing the gathered inputs,
/// which costs one refcount bump per cook and buys two things. The driver
/// stays free to resolve instanced input on the ports that cannot carry it
/// without fighting a borrow this would otherwise hold for the whole cook;
/// and the decoupling is the honest shape, because these queries answer
/// over the geometry the wires delivered, which is deliberately not the
/// geometry a baking node's body goes on to work with.
pub struct InputGeo {
    set: Option<std::sync::Arc<GeometrySet>>,
}

impl InputGeo {
    /// Reads the default geometry input, if the node has one connected.
    #[must_use]
    pub fn new(inputs: &Inputs, port: &str) -> Self {
        Self {
            set: inputs.geometry(port).map(std::sync::Arc::clone),
        }
    }

    /// The connected input, or the error every query returns without one.
    ///
    /// A node with nothing plugged in has no answer, and `0` is a
    /// plausible wrong number rather than a visibly missing one: a box
    /// whose width is `npoints()` would silently cook at the hard-clamp
    /// floor instead of badging. This is the same stance
    /// [`crate::expr::EvalCtx`] takes for an absent capability, applied to
    /// a capability that is present but empty.
    fn connected(&self, query: &str) -> Result<&GeometrySet, String> {
        self.set
            .as_deref()
            .ok_or_else(|| format!("{query}() has no geometry: this node's input is not connected"))
    }
}

impl GeoQueries for InputGeo {
    fn npoints(&self) -> Result<f64, String> {
        Ok(self.connected("npoints")?.point_count() as f64)
    }

    fn nprims(&self) -> Result<f64, String> {
        // Primitive count is topology-aware (triangles, segments, points),
        // which is what `nprims` means everywhere else in the product.
        Ok(self
            .connected("nprims")?
            .meshes
            .iter()
            .map(solarxy_kernel::KernelMesh::primitive_count)
            .sum::<usize>() as f64)
    }

    fn nmeshes(&self) -> Result<f64, String> {
        Ok(f64::from(self.connected("nmeshes")?.mesh_count()))
    }

    fn bbox(&self, field: &str) -> Result<Value, String> {
        let set = self.connected("bbox")?;
        let b = &set.bounds;
        let min = [f64::from(b.min.x), f64::from(b.min.y), f64::from(b.min.z)];
        let max = [f64::from(b.max.x), f64::from(b.max.y), f64::from(b.max.z)];
        Ok(match field {
            "xmin" => Value::Float(min[0]),
            "ymin" => Value::Float(min[1]),
            "zmin" => Value::Float(min[2]),
            "xmax" => Value::Float(max[0]),
            "ymax" => Value::Float(max[1]),
            "zmax" => Value::Float(max[2]),
            "size" => Value::Vec3([max[0] - min[0], max[1] - min[1], max[2] - min[2]]),
            "center" => Value::Vec3([
                f64::midpoint(min[0], max[0]),
                f64::midpoint(min[1], max[1]),
                f64::midpoint(min[2], max[2]),
            ]),
            other => {
                return Err(format!(
                    "`{other}` is not a bbox field; use xmin, ymin, zmin, xmax, ymax, zmax, \
                     size or center"
                ));
            }
        })
    }

    fn centroid(&self) -> Result<[f64; 3], String> {
        // The bounds centre, not the average vertex: it is what "centroid"
        // means for a bounding box and it costs nothing, where averaging
        // every point would run per resolve.
        let b = &self.connected("centroid")?.bounds;
        Ok([
            f64::midpoint(f64::from(b.min.x), f64::from(b.max.x)),
            f64::midpoint(f64::from(b.min.y), f64::from(b.max.y)),
            f64::midpoint(f64::from(b.min.z), f64::from(b.max.z)),
        ])
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests
    use super::*;
    use crate::cook::{InputSlot, Value as CookValue};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn inputs_with(set: GeometrySet) -> Inputs {
        let mut slots = BTreeMap::new();
        slots.insert(
            "geometry".to_string(),
            InputSlot::Single(CookValue::Geometry(Arc::new(set))),
        );
        Inputs::new(slots)
    }

    fn box_set() -> GeometrySet {
        GeometrySet::from_mesh(solarxy_kernel::primitives::generate_box(
            2.0, 2.0, 2.0, 1, 1, 1,
        ))
    }

    #[test]
    fn counts_read_the_connected_geometry() {
        let inputs = inputs_with(box_set());
        let geo = InputGeo::new(&inputs, "geometry");
        assert!(geo.npoints().unwrap() > 0.0);
        assert!(geo.nprims().unwrap() > 0.0);
        assert_eq!(geo.nmeshes().unwrap(), 1.0);
    }

    #[test]
    fn bbox_fields_describe_a_two_unit_box() {
        let inputs = inputs_with(box_set());
        let geo = InputGeo::new(&inputs, "geometry");
        assert_eq!(geo.bbox("size").unwrap(), Value::Vec3([2.0, 2.0, 2.0]));
        assert_eq!(geo.bbox("center").unwrap(), Value::Vec3([0.0, 0.0, 0.0]));
        assert_eq!(geo.bbox("xmin").unwrap(), Value::Float(-1.0));
        assert_eq!(geo.bbox("ymax").unwrap(), Value::Float(1.0));
    }

    #[test]
    fn an_unknown_bbox_field_lists_the_real_ones() {
        let inputs = inputs_with(box_set());
        let geo = InputGeo::new(&inputs, "geometry");
        let err = geo.bbox("middle").unwrap_err();
        assert!(err.contains("xmin"), "{err}");
    }

    #[test]
    fn every_query_on_an_unconnected_input_names_the_problem() {
        // This deliberately replaces an earlier rule that let the counting
        // queries answer 0 while only `bbox` errored. Three things broke
        // under it, all found by driving the browser:
        //
        // 1. The parameter panel disagreed with the cook. `resolved_param`
        //    evaluates with no `GeoQueries` at all, so the field went red
        //    while the node cooked green off a silent 0.
        // 2. The 0 does not survive the resolver. `width = npoints()` on an
        //    unconnected node clamps to the hard floor and cooks a box you
        //    cannot see, with nothing badged to say why.
        // 3. A `box` has no geometry port at all, so there is no empty
        //    input to count: 0 is a made-up answer, not a measured one.
        let inputs = Inputs::new(BTreeMap::new());
        let geo = InputGeo::new(&inputs, "geometry");
        for err in [
            geo.npoints().unwrap_err(),
            geo.nprims().unwrap_err(),
            geo.nmeshes().unwrap_err(),
            geo.bbox("size").unwrap_err(),
            geo.centroid().unwrap_err(),
        ] {
            assert!(err.contains("not connected"), "{err}");
        }
    }
}
