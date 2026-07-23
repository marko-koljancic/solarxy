//! The `uv_project` node: planar, box, cylindrical, or
//! spherical UV projection over the whole input set, normalized against
//! its AABB. High QA value: texel density needs UVs, and imports often
//! lack them. The kernel lives in `solarxy_kernel::uv_project`; box mode
//! rebuilds meshes non-indexed (see the kernel doc), which the validate
//! node's topology counts will reflect.

use solarxy_kernel::uv_project::{UvAxis, UvProjection, uv_project};

use super::common::{geometry_output, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "uv_project",
        version: 1,
        display_name: "UV Project",
        category: Category::Attribute,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
                .default_port()
                .doc(
                    "The geometry to unwrap. Every mesh in the set is projected, and any \
                     UVs a mesh already carried are overwritten rather than kept.",
                ),
        ],
        outputs: vec![geometry_output()],
        params: params_with(
            "UV Project",
            vec![
                ParamSpec::new(
                    "mode",
                    "Mode",
                    "projection",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("planar", "Planar"),
                            EnumVariant::new("box", "Box"),
                            EnumVariant::new("cylindrical", "Cylindrical"),
                            EnumVariant::new("spherical", "Spherical"),
                        ],
                    },
                    ParamValue::Enum("planar".into()),
                )
                .doc(
                    "Which shape maps position onto UV. Planar flattens the geometry \
                     along one axis and is the honest choice for anything roughly flat. \
                     Box gives each triangle the planar mapping of whichever axis its \
                     face normal is closest to, so a hard-surface model gets a sensible \
                     mapping on all six sides at once. Cylindrical turns the angle \
                     around the axis into u and the height along it into v. Spherical \
                     uses longitude and latitude about the axis. Cylindrical and \
                     Spherical wrap, and a triangle that straddles the wrap seam smears \
                     the whole texture across itself.",
                ),
                ParamSpec::new(
                    "axis",
                    "Axis",
                    "projection",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("x", "X"),
                            EnumVariant::new("y", "Y"),
                            EnumVariant::new("z", "Z"),
                        ],
                    },
                    ParamValue::Enum("y".into()),
                )
                .doc(
                    "The axis the projection is built around: Planar projects along it, \
                     Cylindrical wraps about it, Spherical takes it as the pole. Box \
                     uses all three axes by construction, so this does nothing at all in \
                     that mode.",
                ),
                ParamSpec::new(
                    "scale",
                    "Scale",
                    "projection",
                    ParamType::Vec2,
                    ParamValue::Vec2([1.0, 1.0]),
                )
                .step(0.1)
                .doc(
                    "Multiplies the normalized UVs. 1 fits the geometry's bounds into \
                     0..1 exactly; 2 tiles the texture twice across it; 0.5 uses half of \
                     it. Applied before Offset.",
                ),
                ParamSpec::new(
                    "offset",
                    "Offset",
                    "projection",
                    ParamType::Vec2,
                    ParamValue::Vec2([0.0, 0.0]),
                )
                .step(0.05)
                .doc(
                    "Slides the UVs after Scale, in UV units, so 1 shifts by a full \
                     tile. Use it to line a texture up on the surface, not to resize it.",
                ),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Writes a fresh UV set onto every mesh in the input using one of \
              four projections, normalized over the whole set's bounding box \
              so that a scale of 1 lands the geometry inside 0..1.\n\n\
              Imports very often arrive with no UVs at all, and texel density, \
              the checker pattern, and any textured material all need them. \
              This is the node that gives them something to read: it goes \
              after the import or the primitive and before the material. It is \
              a projection rather than an unwrap, so treat it as the fast way \
              to usable UVs, not as a substitute for a real layout.\n\n\
              Three things to expect. Existing UVs are replaced, not merged. \
              The normalization is against the bounds of the whole input set, \
              so several meshes share one consistent mapping and a `transform` \
              upstream drags the UVs along with the bounds. And Box mode \
              rebuilds each mesh non-indexed, three points per triangle, so \
              the point count jumps and validate's topology counts will \
              reflect it. Projection is a surface operation: point clouds \
              and polylines pass through untouched with a warning.",
        search_aliases: &["uv", "unwrap", "project", "texture", "mapping"],
        glyph: "uv_project",
        role: NodeRole::Standard,
        cook: cook_uv_project,
        migrate: None,
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook_uv_project(
    p: &ResolvedParams,
    inputs: &Inputs,
    cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let Some(input) = inputs.geometry("geometry") else {
        return Ok(CookOutcome::Done(Outputs::geometry(
            solarxy_kernel::GeometrySet::empty(),
        )));
    };

    if input.has_non_triangle_meshes() {
        cx.warn(
            "uv_project applies to triangle meshes; line and point meshes pass \
             through unchanged",
        );
    }

    let mode = match p.enum_key("mode") {
        "box" => UvProjection::Box,
        "cylindrical" => UvProjection::Cylindrical,
        "spherical" => UvProjection::Spherical,
        _ => UvProjection::Planar,
    };
    let axis = match p.enum_key("axis") {
        "x" => UvAxis::X,
        "z" => UvAxis::Z,
        _ => UvAxis::Y,
    };
    let scale2 = p.vec2("scale");
    let offset2 = p.vec2("offset");
    let scale = [scale2[0] as f32, scale2[1] as f32];
    let offset = [offset2[0] as f32, offset2[1] as f32];

    Ok(CookOutcome::Done(Outputs::geometry(uv_project(
        input, mode, axis, scale, offset,
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook::InputSlot;
    use crate::registry::coerce::Value;
    use solarxy_kernel::GeometrySet;
    use solarxy_kernel::primitives::generate_box;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    #[test]
    fn cooks_at_defaults_and_writes_uvs() {
        let resolved =
            crate::registry::resolve::resolve_params(&BTreeMap::new(), &descriptor().params)
                .unwrap();
        let mut mesh = generate_box(1.0, 1.0, 1.0, 1, 1, 1);
        mesh.tex_coords = None; // simulate an import without UVs
        let set = GeometrySet::from_mesh(mesh);
        let inputs = Inputs::new(
            [(
                "geometry".to_string(),
                InputSlot::Single(Value::Geometry(Arc::new(set))),
            )]
            .into_iter()
            .collect(),
        );
        let assets = crate::assets::AssetTable::default();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(outputs) = cook_uv_project(&resolved, &inputs, &mut cx).unwrap()
        else {
            panic!("synchronous cook");
        };
        let out = outputs.get("geometry").unwrap().as_geometry().unwrap();
        assert!(out.meshes[0].tex_coords.is_some(), "UVs written");
    }

    #[test]
    fn a_point_cloud_warns_and_passes_through_without_uvs() {
        let resolved =
            crate::registry::resolve::resolve_params(&BTreeMap::new(), &descriptor().params)
                .unwrap();
        let set = GeometrySet::from_mesh(solarxy_kernel::KernelMesh::points(
            "p",
            vec![[0.0; 3], [1.0; 3]],
        ));
        let inputs = Inputs::new(
            [(
                "geometry".to_string(),
                InputSlot::Single(Value::Geometry(Arc::new(set))),
            )]
            .into_iter()
            .collect(),
        );
        let assets = crate::assets::AssetTable::default();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(outputs) = cook_uv_project(&resolved, &inputs, &mut cx).unwrap()
        else {
            panic!("synchronous cook");
        };
        let out = outputs.get("geometry").unwrap().as_geometry().unwrap();
        assert!(out.meshes[0].tex_coords.is_none(), "no UVs invented");
        assert_eq!(out.meshes[0].vertex_count(), 2);
        let warns = cx.take_warnings();
        assert_eq!(warns.len(), 1);
        assert!(warns[0].contains("pass"), "got: {}", warns[0]);
    }
}
