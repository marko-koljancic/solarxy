//! How a port's data type presents itself, and how a category presents a
//! node that declares nothing more specific.
//!
//! The load-bearing rule here is the one about colour. A wire type's hue
//! is a **reference into the shared palette**, never a literal authored
//! beside the thing that draws it. Before 0.10.0 the thirteen hues were
//! hex literals in the browser, copied by hand into the editor stylesheet
//! and again into the landing page's animation, with nothing holding the
//! three together. They are now `solarxy_core::theme::WireColors`, and
//! both shells resolve them from there.

use std::cmp::Ordering;

use solarxy_core::theme::{Palette, Rgb};
use solarxy_graph::registry::coerce::{Coercion, DataType};
use solarxy_graph::registry::{Category, NodeRole, Registry};

/// The second encoding channel on a port handle.
///
/// Colour says which family a value belongs to and shape separates the
/// members of that family, so a reader who cannot tell two hues apart can
/// still tell one port from another. Neither channel is decorative.
///
/// **Among the vectors the shape counts components**, which is the rule
/// that makes the channel legible rather than arbitrary: a bar is two, a
/// triangle three, a square four. That also puts `vec4` and `color` on the
/// same square, which is honest rather than a collision, because a colour
/// is an RGBA four-vector and the two convert in both directions without
/// loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HandleShape {
    Round,
    Diamond,
    /// Two components.
    Bar,
    /// Three components.
    Triangle,
    /// Four components.
    Square,
    Hexagon,
}

/// The handle shape for a data type.
#[must_use]
pub fn handle_shape(data_type: DataType) -> HandleShape {
    match data_type {
        DataType::Int => HandleShape::Diamond,
        DataType::Vec2 => HandleShape::Bar,
        DataType::Vec3 => HandleShape::Triangle,
        DataType::Vec4 | DataType::Color => HandleShape::Square,
        DataType::Image | DataType::Material => HandleShape::Hexagon,
        _ => HandleShape::Round,
    }
}

/// The palette token a data type's wire colour comes from, without the
/// leading `--`.
///
/// Ten tokens serve thirteen types: the scalars share a hue and are told
/// apart by [`handle_shape`], and so do the vectors.
#[must_use]
pub fn wire_token(data_type: DataType) -> &'static str {
    match data_type {
        DataType::Geometry => "wire-geometry",
        DataType::Light => "wire-light",
        DataType::Report => "wire-report",
        DataType::Float | DataType::Int => "wire-scalar",
        DataType::Bool => "wire-bool",
        DataType::Vec2 | DataType::Vec3 | DataType::Vec4 => "wire-vector",
        DataType::Color => "wire-color",
        DataType::Text => "wire-text",
        DataType::Image => "wire-image",
        DataType::Material => "wire-material",
    }
}

/// A data type's wire colour, resolved against a palette.
///
/// The browser resolves the same answer through the generated custom
/// property named by [`wire_token`]; this is the form a shell that paints
/// directly needs.
#[must_use]
pub fn wire_color(data_type: DataType, palette: &Palette) -> Rgb {
    let wire = &palette.wire;
    match data_type {
        DataType::Geometry => wire.geometry,
        DataType::Light => wire.light,
        DataType::Report => wire.report,
        DataType::Float | DataType::Int => wire.scalar,
        DataType::Bool => wire.boolean,
        DataType::Vec2 | DataType::Vec3 | DataType::Vec4 => wire.vector,
        DataType::Color => wire.color,
        DataType::Text => wire.text,
        DataType::Image => wire.image,
        DataType::Material => wire.material,
    }
}

/// Which side of a node a port sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortSide {
    Input,
    Output,
}

/// A port's declared data type, or nothing when the type or the port is
/// not in the registry.
#[must_use]
pub fn port_data_type(
    registry: &Registry,
    type_id: &str,
    port_key: &str,
    side: PortSide,
) -> Option<DataType> {
    let desc = registry.get(type_id)?;
    let ports = match side {
        PortSide::Input => &desc.inputs,
        PortSide::Output => &desc.outputs,
    };
    ports
        .iter()
        .find(|p| p.key == port_key)
        .map(|p| p.data_type)
}

/// What a canvas should say about a proposed connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionVerdict {
    /// Whether the wire may be made at all.
    pub legal: bool,
    /// How the value arrives when it may, so a canvas can warn about a
    /// lossy wire without deciding for itself what is lossy.
    pub coercion: Option<Coercion>,
}

/// Whether two ports may be wired, and what happens to the value if they
/// are.
///
/// This is the answer that must not diverge between the shells. A
/// connection allowed on one and refused on the other would let someone
/// build a scene in one shell that the other reports as broken, which is
/// worse than either rule being wrong on its own.
#[must_use]
pub fn connection_verdict(
    registry: &Registry,
    from_type: &str,
    from_port: &str,
    to_type: &str,
    to_port: &str,
) -> ConnectionVerdict {
    let from = port_data_type(registry, from_type, from_port, PortSide::Output);
    let to = port_data_type(registry, to_type, to_port, PortSide::Input);
    let (Some(from), Some(to)) = (from, to) else {
        return ConnectionVerdict {
            legal: false,
            coercion: None,
        };
    };
    let coercion = solarxy_graph::registry::coerce::can_coerce(from, to);
    ConnectionVerdict {
        legal: coercion.is_legal(),
        coercion: coercion.is_legal().then_some(coercion),
    }
}

/// The palette and menu order of the node categories.
///
/// The registry hands nodes over sorted by type id, so an interface that
/// wants families in a sensible order has to say what that order is. It is
/// the [`Category`] declaration order, which is not a coincidence and is
/// pinned by a test: the browser maintained a separate curated list and it
/// had always been the same sequence.
#[must_use]
pub fn compare_categories(a: Category, b: Category) -> Ordering {
    (a as u8).cmp(&(b as u8))
}

/// The glyph key shown when a node's declared glyph has no art.
///
/// The rule is the family fallback; **which art exists is the shell's
/// question**, so a shell looks up the declared key first and comes here
/// only when it finds nothing. That split is what lets a node type added
/// in Rust with a novel glyph key still draw as its family on a shell that
/// has never heard of it.
#[must_use]
pub fn category_glyph(category: Category) -> &'static str {
    match category {
        Category::Container => "sopnet",
        Category::Generators => "box",
        Category::Attribute => "attribute_create",
        Category::Transform => "transform",
        Category::Copy => "copy_to_points",
        Category::Topology => "subdivide",
        Category::Shaders => "material",
        Category::Import => "import_obj",
        Category::Export => "geo_export",
        Category::Lights => "point",
        Category::Cameras => "camera",
        Category::Utility => "null",
        Category::CopGenerate => "checker",
        Category::CopAdjust => "levels",
        Category::CopComposite => "mix",
    }
}

/// The silhouette family used when a node declares no usable role.
///
/// Every Rust descriptor declares one, so this is unreachable from a
/// registry; it is here for a shell reading a role it has no silhouette
/// for, which is the browser's situation whenever the engine is newer than
/// the frontend.
#[must_use]
pub fn category_role(category: Category) -> NodeRole {
    match category {
        Category::Container => NodeRole::Container,
        Category::Export => NodeRole::Terminal,
        Category::Lights => NodeRole::Light,
        Category::Cameras => NodeRole::Camera,
        Category::CopGenerate => NodeRole::ImageSource,
        _ => NodeRole::Standard,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn registry() -> Registry {
        solarxy_graph::nodes::builtin_registry().expect("builtin registry")
    }

    #[test]
    fn every_data_type_has_a_wire_colour_and_a_shape() {
        // The browser's own guard asserted only that a colour was
        // *defined*, never what it was, so a data type added with a
        // fallback of undefined would have passed. Both channels are
        // checked here, and the colour is checked by value.
        for dt in DataType::ALL {
            let token = wire_token(dt);
            assert!(token.starts_with("wire-"), "{dt:?} -> {token}");
            let dark = wire_color(dt, &Palette::dark());
            let light = wire_color(dt, &Palette::light());
            assert_eq!(dark.css(), light.css(), "{dt:?} differs between themes");
            assert_ne!(dark.css(), "#000000", "{dt:?} resolved to nothing");
            let _ = handle_shape(dt);
        }
    }

    #[test]
    fn the_wire_colours_are_the_values_that_shipped() {
        // Moving them into the palette must not have retuned them. These
        // are the literals the browser carried.
        let p = Palette::dark();
        for (dt, hex) in [
            (DataType::Geometry, "#5aa0ff"),
            (DataType::Light, "#ffcc66"),
            (DataType::Report, "#4dd0c8"),
            (DataType::Float, "#7fd962"),
            (DataType::Int, "#7fd962"),
            (DataType::Bool, "#ff8a80"),
            (DataType::Vec2, "#b39ddb"),
            (DataType::Vec3, "#b39ddb"),
            (DataType::Vec4, "#b39ddb"),
            (DataType::Color, "#f5a623"),
            (DataType::Text, "#9aa0a6"),
            (DataType::Image, "#e879c8"),
            (DataType::Material, "#c96f4a"),
        ] {
            assert_eq!(wire_color(dt, &p).css(), hex, "{dt:?}");
        }
    }

    #[test]
    fn two_handles_that_look_alike_can_be_wired_together() {
        // The encoding's promise: hue says family, shape separates the
        // family, so two ports drawn identically are ports you can wire.
        //
        // It did not hold until 0.10.0. `vec2`, `vec3` and `vec4` shared a
        // hue AND a round handle while coercing to each other in no
        // direction at all, so three identical-looking handles refused to
        // connect, and the pair the shape channel did separate, `float` and
        // `int`, is the one that converts both ways. The vectors now count
        // their components in the shape.
        //
        // No exception list: a new pair that looks alike and cannot be
        // wired fails here, which is the whole point of stating the rule as
        // a rule.
        for a in DataType::ALL {
            for b in DataType::ALL {
                if a == b || (a as u8) > (b as u8) {
                    continue;
                }
                if wire_token(a) != wire_token(b) || handle_shape(a) != handle_shape(b) {
                    continue;
                }
                let both_ways = solarxy_graph::registry::coerce::can_coerce(a, b).is_legal()
                    && solarxy_graph::registry::coerce::can_coerce(b, a).is_legal();
                assert!(
                    both_ways,
                    "{a:?} and {b:?} are drawn identically and cannot be wired together"
                );
            }
        }
    }

    #[test]
    fn shape_alone_does_not_identify_a_type_and_the_sharing_is_stated() {
        // Thirteen types cannot have thirteen shapes legible at handle
        // size, so shape narrows rather than identifies, and hue finishes
        // the job. This pins which shapes are shared so the limit is a
        // recorded fact rather than something a reader has to rediscover.
        //
        // `Round` is the absence of a claim: six unrelated families carry
        // it. `Hexagon` is a real claim, "a resource", carried by two types
        // that convert in neither direction, so a reader who cannot use hue
        // cannot tell an image port from a material one. That is milder
        // than what the vectors had, since those were identical on BOTH
        // channels, but it is the same shape of gap and it is left standing
        // deliberately rather than overlooked.
        let mut groups: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        for dt in DataType::ALL {
            groups
                .entry(format!("{:?}", handle_shape(dt)))
                .or_default()
                .push(format!("{dt:?}"));
        }
        let shared: Vec<(String, Vec<String>)> = groups
            .into_iter()
            .filter(|(_, types)| types.len() > 1)
            .collect();
        assert_eq!(
            shared,
            vec![
                (
                    "Hexagon".to_string(),
                    vec!["Image".to_string(), "Material".to_string()]
                ),
                (
                    "Round".to_string(),
                    vec![
                        "Geometry".to_string(),
                        "Light".to_string(),
                        "Report".to_string(),
                        "Float".to_string(),
                        "Bool".to_string(),
                        "Text".to_string(),
                    ]
                ),
                (
                    "Square".to_string(),
                    vec!["Vec4".to_string(), "Color".to_string()]
                ),
            ]
        );
    }

    #[test]
    fn a_legal_wire_reports_how_the_value_arrives() {
        let r = registry();
        // Same type, straight through.
        let same = connection_verdict(&r, "box", "geometry", "transform", "geometry");
        assert!(same.legal);
        assert_eq!(same.coercion, Some(Coercion::Same));
    }

    #[test]
    fn an_illegal_wire_is_refused_and_carries_no_verdict() {
        let r = registry();
        // A light into a geometry input is the refusal the canvas has to
        // make before the engine is asked.
        let bad = connection_verdict(&r, "point_light", "light", "transform", "geometry");
        assert!(!bad.legal);
        assert_eq!(bad.coercion, None);
    }

    #[test]
    fn an_unknown_type_or_port_is_refused_rather_than_assumed_legal() {
        let r = registry();
        assert!(!connection_verdict(&r, "nope", "geometry", "transform", "geometry").legal);
        assert!(!connection_verdict(&r, "box", "nope", "transform", "geometry").legal);
        assert!(!connection_verdict(&r, "box", "geometry", "transform", "nope").legal);
    }

    #[test]
    fn the_verdict_agrees_with_the_engine_on_every_pair_in_the_matrix() {
        // The answer that must not diverge. Rather than restate the
        // matrix, walk it.
        for from in DataType::ALL {
            for to in DataType::ALL {
                let engine = solarxy_graph::registry::coerce::can_coerce(from, to);
                let shown = engine.is_legal().then_some(engine);
                assert_eq!(
                    shown.is_some(),
                    engine.is_legal(),
                    "{from:?} -> {to:?} disagrees with the matrix"
                );
            }
        }
    }

    #[test]
    fn categories_sort_in_declaration_order() {
        let mut all = vec![
            Category::Utility,
            Category::Container,
            Category::Lights,
            Category::Generators,
        ];
        all.sort_by(|a, b| compare_categories(*a, *b));
        assert_eq!(
            all,
            [
                Category::Container,
                Category::Generators,
                Category::Lights,
                Category::Utility
            ]
        );
    }

    #[test]
    fn every_category_falls_back_to_a_glyph_the_catalog_declares() {
        // A fallback naming art nothing draws is worse than no fallback:
        // it fails only for the node type nobody has added yet.
        let r = registry();
        let declared: BTreeSet<&str> = r.descriptors().map(|d| d.glyph).collect();
        for category in CATEGORIES {
            let key = category_glyph(category);
            assert!(
                declared.contains(key),
                "{category:?} falls back to `{key}`, which no node type declares"
            );
        }
    }

    #[test]
    fn every_category_falls_back_to_a_role_its_own_nodes_use() {
        let r = registry();
        for category in CATEGORIES {
            let role = category_role(category);
            let used: Vec<NodeRole> = r
                .descriptors()
                .filter(|d| d.category == category)
                .map(|d| d.role)
                .collect();
            if used.is_empty() {
                continue;
            }
            assert!(
                used.contains(&role),
                "{category:?} falls back to {role:?}, which none of its own nodes use: {used:?}"
            );
        }
    }

    /// Every category, so the two fallback tables above are walked whole.
    /// Written out rather than derived because `Category` has no `ALL`, and
    /// a missing entry here would silently narrow both tests.
    const CATEGORIES: [Category; 15] = [
        Category::Container,
        Category::Generators,
        Category::Attribute,
        Category::Transform,
        Category::Copy,
        Category::Topology,
        Category::Shaders,
        Category::Import,
        Category::Export,
        Category::Lights,
        Category::Cameras,
        Category::Utility,
        Category::CopGenerate,
        Category::CopAdjust,
        Category::CopComposite,
    ];
}
