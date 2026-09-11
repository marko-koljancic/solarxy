//! The glyph a node wears, and the art behind it.
//!
//! **Which art exists is this shell's question**, which is why the table
//! is here rather than in the shared derivation: a rule that answers
//! "which silhouette" belongs there, and a path that answers "drawn how"
//! would have picked a toolkit. The rule this table is read through, the
//! family fallback in `solarxy_studio::types::category_glyph`, is shared.
//!
//! ## Two copies, one checked
//!
//! The browser holds the same seventy-six paths in `nodeVisual.ts`. That
//! is a duplication the release's own exit criteria sanction, on the
//! grounds that art is what a shell can draw; what they do not sanction
//! is the two quietly diverging. So the paths are held **verbatim** as
//! authored rather than pre-flattened into vertices, and
//! `the_glyph_art_matches_the_browsers` compares the two tables entry for
//! entry, key and path. A table of floats could not be compared that way,
//! which is the whole reason this file carries strings.
//!
//! Every glyph is a sixteen by sixteen stroke path with round caps and
//! joins, transplanted from the design source.

use egui::{Color32, Painter, Rect, Stroke, vec2};

use super::vector;

/// The glyph drawn when a node's declared key has no art and its category
/// fallback has none either. Never reached from the shipped registry,
/// which `every_declared_glyph_has_art` holds; it exists so a node type
/// added in Rust with a novel key and a novel category still draws.
pub(super) const FALLBACK_GLYPH: &str = "null";

/// The authored size every glyph path is written in.
const GLYPH_BOX: f32 = 16.0;

/// Glyph key to its sixteen by sixteen stroke path, sorted by key so the
/// lookup can bisect.
const GLYPH_PATHS: &[(&str, &str)] = &[
    (
        "ambient",
        "M11 8a3 3 0 1 1-6 0 3 3 0 1 1 6 0m-7.8-4.8l1 1m8.6-1l-1 1m-8.6 8.6l1-1m8.6 1l-1-1",
    ),
    (
        "array",
        "M2.5 9.5h4v4h-4z m3.5-3.5h4v4h-4z m3.5-3.5h4v4h-4z",
    ),
    ("attribute_copy", "M3 3h7v7h-7z M6 6h7v7h-7z"),
    (
        "attribute_create",
        "M 2.75 5 h 6 l 4.5 3 -4.5 3 h -6 z M 5.25 8 h 3 M 6.75 6.5 v 3",
    ),
    (
        "attribute_from_image",
        "M 3 2.75 h 10 v 6 h -10 z M 4.5 7.25 l 2 -2.4 1.5 1.7 1.2 -1.4 1.8 2.1 M 8 9.75 v 3.5 m -1.6 -1.6 L 8 13.25 l 1.6 -1.6",
    ),
    (
        "attribute_promote",
        "M8 2.5l2.7 3.6h-5.4z M8 10.6V7.4m-1.5 1.5L8 7.4l1.5 1.5 M4.9 12.4a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0 M8.9 12.4a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0 M12.9 12.4a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0",
    ),
    (
        "attribute_randomize",
        "M3 3h10v10h-10z M6.4 5.5a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0 M8.9 8a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0 M11.4 10.5a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0",
    ),
    (
        "attribute_wrangle",
        "M2.5 3h11v10h-11z M2.5 5.4h11 M5.2 8l1.6 1.6-1.6 1.6 M8.4 11.2h2.6",
    ),
    (
        "blur",
        "M10 8a2 2 0 1 1-4 0 2 2 0 1 1 4 0 M4.6 5.2a5 5 0 0 0 0 5.6 M11.4 5.2a5 5 0 0 1 0 5.6",
    ),
    (
        "bounds",
        "M3 5.5v-2.5h2.5m5 0h2.5v2.5m0 5v2.5h-2.5m-5 0h-2.5v-2.5",
    ),
    (
        "box",
        "M3 5.5l5-2.5 5 2.5v5l-5 2.5-5-2.5z m0 0l5 2.5 5-2.5m-5 2.5v5",
    ),
    (
        "brick",
        "M3 3.5h10v9h-10z M3 6.5h10 M3 9.5h10 M8 3.5v3 M5.5 6.5v3 M10.5 6.5v3 M8 9.5v3",
    ),
    (
        "brightness_contrast",
        "M13 8a5 5 0 1 1-10 0 5 5 0 1 1 10 0 M8 3v10 M10 6.2h1.6 M10 8h2.2 M10 9.8h1.6",
    ),
    (
        "camera",
        "M2.5 5.5h2.8l1.2-1.8h3l1.2 1.8h2.8v7h-11z M10.2 9a2.2 2.2 0 1 1-4.4 0 2.2 2.2 0 1 1 4.4 0",
    ),
    (
        "checker",
        "M3 3.5h10v9h-10z M8 3.5v9 M3 8h10 M3.7 4.2l3.6 3.6 M8.7 8.7l3.6 3.3",
    ),
    (
        "circle",
        "M9.8 3.66L12.34 6.2 12.34 9.8 9.8 12.34 6.2 12.34 3.66 9.8 3.66 6.2 6.2 3.66z",
    ),
    (
        "compute_normals",
        "M 2.5 12.5 c 2 -2 9 -2 11 0 m -5.5 -2.5 v -6.5 m 0 0 l -2 2 m 2 -2 l 2 2",
    ),
    (
        "cone",
        "M 8 2.65 l 4.5 8.3 m -4.5 -8.3 l -4.5 8.3 m 9.2 0.7 a 4.7 1.7 0 1 1 -9.4 0 4.7 1.7 0 1 1 9.4 0",
    ),
    (
        "constant",
        "M3 3.5h10v9h-10z m6.8 4.5a1.8 1.8 0 1 1-3.6 0 1.8 1.8 0 1 1 3.6 0",
    ),
    (
        "copnet",
        "M4.5 2.5h7a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2h-7a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2z M5 10.5l2.2-2.7 1.6 1.8 1.4-1.6 1.8 2.5",
    ),
    (
        "copy_to_points",
        "M 2.5 11.85 c 2 -2.5 9 -2.5 11 0 M 3.3 6.95 h 2.4 v 2.4 h -2.4 z M 6.8 4.15 h 2.4 v 2.4 h -2.4 z M 10.3 6.95 h 2.4 v 2.4 h -2.4 z",
    ),
    (
        "cylinder",
        "M 12.5 5.4 a 4.5 1.6 0 1 1 -9 0 4.5 1.6 0 1 1 9 0 m -9 0 v 6.8 m 9 -6.8 v 6.8 m 0 0 a 4.5 1.6 0 1 0 -9 0",
    ),
    (
        "delete",
        "M3 4.5h10m-6.5 0v-1.5h3v1.5m-5 0l0.7 8.5h5.6l0.7-8.5",
    ),
    (
        "directional",
        "M 5 3.5 v 5 m 0 0 l -1.6 -1.6 m 1.6 1.6 l 1.6 -1.6 m 2.9 -3.4 v 5 m 0 0 l -1.6 -1.6 m 1.6 1.6 l 1.6 -1.6 m -8.6 5.6 c 2.2 -1.6 8.8 -1.6 11 0",
    ),
    (
        "displace",
        "M 2.5 11.85 H 5 c 1 -3.5 5 -3.5 6 0 h 2.5 M 8 8.35 V 4.15 m -1.6 1.6 L 8 4.15 l 1.6 1.6",
    ),
    (
        "edges_to_geo",
        "M3.5 3.5h9v9h-9z M3.5 3.5l9 9 M12.5 3.5l-9 9",
    ),
    (
        "gamma",
        "M 4.5 3.15 c 0.8 3.2 2 5.2 3.6 6.8 m 3.4 -6.8 c -0.2 4.6 -1.6 7.6 -4 9.7",
    ),
    (
        "geo_export",
        "M4 2.5h5l3 3v8h-8z m5 0v3h3 M8 12v-4m-1.7 1.7L8 8l1.7 1.7",
    ),
    (
        "gradient",
        "M3 3.5h10v9h-10z M11 8a3 3 0 1 1-6 0 3 3 0 1 1 6 0 M9.3 8a1.3 1.3 0 1 1-2.6 0 1.3 1.3 0 1 1 2.6 0",
    ),
    (
        "height_to_normal",
        "M2.5 12.5c2-5 3.8-5 5.5-2s3.8 3 5.5-2 M11.5 8V3.5 m-1.7 1.7l1.7-1.7 1.7 1.7",
    ),
    (
        "hemisphere",
        "M 2.5 10.1 h 11 m -9.7 0 a 4.2 4.2 0 0 1 8.4 0",
    ),
    (
        "hue_saturation",
        "M13 8a5 5 0 1 1-10 0 5 5 0 1 1 10 0 M8 8V3 M8 8l-4.33 2.5 M8 8l4.33 2.5",
    ),
    (
        "image_export",
        "M 2.75 5.75 h 8 v 7.5 h -8 z M 4.05 11.45 l 2 -2.4 1.5 1.7 1.2 -1.3 1.7 2 M 10.25 2.75 h 3 v 3 m 0 -3 l -3.7 3.7",
    ),
    (
        "import_gltf",
        "M4 2.5h5l3 3v8h-8z m5 0v3h3m-4 1.5v4m-1.7-1.7l1.7 1.7 1.7-1.7",
    ),
    (
        "import_image",
        "M3 4h10v8h-10z m2.5 4.3a1.1 1.1 0 1 0 0-2.2m-2 5.4l3-3 2.3 2.3 2.2-2.3 2 2",
    ),
    (
        "import_obj",
        "M4 2.5h5l3 3v8h-8z m5 0v3h3m-4 1.5v4m-1.7-1.7l1.7 1.7 1.7-1.7",
    ),
    (
        "import_ply",
        "M4 2.5h5l3 3v8h-8z m5 0v3h3m-4 1.5v4m-1.7-1.7l1.7 1.7 1.7-1.7",
    ),
    (
        "import_stl",
        "M4 2.5h5l3 3v8h-8z m5 0v3h3m-4 1.5v4m-1.7-1.7l1.7 1.7 1.7-1.7",
    ),
    (
        "invert",
        "M3 5.5h8.5m0 0l-2-2m2 2l-2 2 M13 10.5h-8.5m0 0l2-2m-2 2l2 2",
    ),
    (
        "levels",
        "M 2.5 12.45 h 11 m -9 -0.2 v -4 m 2.6 4 v -7.5 m 2.6 7.5 v -5.3 m 2.6 5.3 v -8.7",
    ),
    (
        "line",
        "M2.5 13L13.5 3 M7.1 9.6a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0 M10.8 6.3a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0",
    ),
    (
        "matcap",
        "M13 8a5 5 0 1 1-10 0 5 5 0 1 1 10 0 M3.9 6.5a4.6 4.6 0 0 1 8.2 0",
    ),
    (
        "material",
        "M8 2.5c2.5 3 4 4.8 4 6.8a4 4 0 1 1-8 0c0-2 1.5-3.8 4-6.8z m-1.2 7a1.3 1.3 0 1 0 2.6 0",
    ),
    (
        "matnet",
        "M4.5 2.5h7a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2h-7a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2z M8 4.8c1.5 1.9 2.4 3 2.4 4.2a2.4 2.4 0 1 1-4.8 0c0-1.2 0.9-2.3 2.4-4.2z",
    ),
    (
        "merge",
        "M3 4h3.5l3.5 4h3m-10 4h3.5l3.5-4m3 0l-1.8-1.8m1.8 1.8l-1.8 1.8",
    ),
    ("mirror", "M8 2.5v11m-2.5-8.5l-2.5 3 2.5 3m5-6l2.5 3-2.5 3"),
    (
        "mix",
        "M9.4 8a3.4 3.4 0 1 1-6.8 0 3.4 3.4 0 1 1 6.8 0 M13.4 8a3.4 3.4 0 1 1-6.8 0",
    ),
    (
        "mix_material",
        "M 5.75 3.9 c 1.2 1.5 2 2.6 2 3.7 a 2 2 0 1 1 -4 0 c 0 -1.1 0.8 -2.2 2 -3.7 z M 10.25 6.4 c 1.2 1.5 2 2.6 2 3.7 a 2 2 0 1 1 -4 0 c 0 -1.1 0.8 -2.2 2 -3.7 z",
    ),
    (
        "noise",
        "M 4.7 4.35 a 0.9 0.9 0 1 1 -1.8 0 0.9 0.9 0 1 1 1.8 0 M 10.2 3.75 a 0.9 0.9 0 1 1 -1.8 0 0.9 0.9 0 1 1 1.8 0 M 13.1 7.25 a 0.9 0.9 0 1 1 -1.8 0 0.9 0.9 0 1 1 1.8 0 M 6.4 8.55 a 0.9 0.9 0 1 1 -1.8 0 0.9 0.9 0 1 1 1.8 0 M 10.8 11.25 a 0.9 0.9 0 1 1 -1.8 0 0.9 0.9 0 1 1 1.8 0 M 5.2 12.25 a 0.9 0.9 0 1 1 -1.8 0 0.9 0.9 0 1 1 1.8 0",
    ),
    ("note", "M3 3h10v6.5l-3.5 3.5h-6.5z m6.5 10v-3.5h3.5"),
    ("null", "M11 8a3 3 0 1 1-6 0 3 3 0 1 1 6 0"),
    (
        "pack_orm",
        "M 8 2.35 l 5.5 3 -5.5 3 -5.5 -3 z m -5.5 5.7 l 5.5 3 5.5 -3 m -11 2.6 l 5.5 3 5.5 -3",
    ),
    ("plane", "M2 11l4-6h8l-4 6z"),
    (
        "point",
        "M9.4 8a1.4 1.4 0 1 1-2.8 0 1.4 1.4 0 1 1 2.8 0m-1.4-5.2v2m0 6.4v2m-5.2-5.2h2m6.4 0h2m-8.9-3.7l1.4 1.4m4.6 4.6l1.4 1.4m0-7.4l-1.4 1.4m-4.6 4.6l-1.4 1.4",
    ),
    (
        "points_from_geo",
        "M 8 4 l 4.5 8 h -9 z M 8.9 4 a 0.9 0.9 0 1 1 -1.8 0 0.9 0.9 0 1 1 1.8 0 M 13.4 12 a 0.9 0.9 0 1 1 -1.8 0 0.9 0.9 0 1 1 1.8 0 M 4.4 12 a 0.9 0.9 0 1 1 -1.8 0 0.9 0.9 0 1 1 1.8 0",
    ),
    (
        "principled",
        "M13 8a5 5 0 1 1-10 0 5 5 0 1 1 10 0 M7.3 6a1.3 1.3 0 1 1-2.6 0 1.3 1.3 0 1 1 2.6 0",
    ),
    (
        "ramp",
        "M 2.5 4.25 h 11 v 3.5 h -11 z m 1.5 5.5 v 2 m 4 -2 v 2 m 4 -2 v 2",
    ),
    (
        "rect_area",
        "M 2.75 3 h 6 v 10 h -6 z m 8 2 h 2.5 m -2.5 3 h 2.5 m -2.5 3 h 2.5",
    ),
    (
        "render",
        "M13 8a5 5 0 1 1-10 0 5 5 0 1 1 10 0 M8 3.6l3.8 6.6h-7.6z",
    ),
    (
        "scatter",
        "M 2.5 12.25 c 2 -2.5 9 -2.5 11 0 M 5.4 9.05 a 0.9 0.9 0 1 1 -1.8 0 0.9 0.9 0 1 1 1.8 0 M 9 7.95 a 0.9 0.9 0 1 1 -1.8 0 0.9 0.9 0 1 1 1.8 0 M 12.6 9.25 a 0.9 0.9 0 1 1 -1.8 0 0.9 0.9 0 1 1 1.8 0 M 7 4.65 a 0.9 0.9 0 1 1 -1.8 0 0.9 0.9 0 1 1 1.8 0 M 11.2 5.15 a 0.9 0.9 0 1 1 -1.8 0 0.9 0.9 0 1 1 1.8 0",
    ),
    ("sharpen", "M3.5 12.5l4.5-9 4.5 9z M8 8.2v4.3"),
    (
        "sopnet",
        "M4.5 2.5h7a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2h-7a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2z m1 5.5h5",
    ),
    (
        "sphere",
        "M14 8a6 6 0 1 1-12 0 6 6 0 1 1 12 0m-11 2c1.5 1.1 8.5 1.1 10 0",
    ),
    (
        "spot",
        "M6.5 3h3l1.2 3.5h-5.4z m-2.5 10l2.3-6.5m5.7 6.5l-2.3-6.5m-5.7 6.5h8",
    ),
    ("subdivide", "M3 3h10v10h-10z m5 0v10m-5-5h10"),
    ("switch", "M 2.75 5 h 8 m -8 6 h 8 m 0 -6 l 2.5 3 -2.5 3"),
    (
        "tex_ref",
        "M 7 4 h 7 v 8 h -7 z M 8.5 10 l 1.8 -2.2 1.6 1.9 M 2 8 h 3.6 m -1.5 -1.6 L 5.8 8 l -1.7 1.6",
    ),
    ("text", "M3.5 2.5h9v11h-9z M5.5 5.5h5 M5.5 8h5 M5.5 10.5h3"),
    (
        "toon",
        "M13 8a5 5 0 1 1-10 0 5 5 0 1 1 10 0 M3.6 9.5h4.2l2-2.5h3 M5 11.8h3l1.8-2.2",
    ),
    (
        "torus",
        "M14 8a6 3.6 0 1 1-12 0 6 3.6 0 1 1 12 0m-3.5 0a2.5 1.3 0 1 1-5 0 2.5 1.3 0 1 1 5 0",
    ),
    (
        "torus_knot",
        "M10 8a3.5 3.5 0 1 1-7 0 3.5 3.5 0 1 1 7 0m3 0a3.5 3.5 0 1 1-7 0",
    ),
    (
        "transform",
        "M8 2v12m-6-6h12m-6-6l-1.8 1.8m1.8-1.8l1.8 1.8m4.2 4.2l-1.8-1.8m1.8 1.8l-1.8 1.8",
    ),
    ("unlit", "M13 8a5 5 0 1 1-10 0 5 5 0 1 1 10 0 M4.5 11.5l7-7"),
    (
        "uv_project",
        "M 2.89 6.5 h 7 v 7 h -7 z m 7 0 c 2.6 0 3.6 -2 3.1 -4 m -6.6 4 v -2 m -3.5 5.5 h 7",
    ),
    (
        "validate",
        "M8 2l5 2v4c0 3-2.2 4.8-5 6-2.8-1.2-5-3-5-6v-4z m-2.2 6.2l1.6 1.6 3-3.4",
    ),
    (
        "voronoi",
        "M3 3.5h10v9h-10z M8 8l1.5-4.5 M8 8l4 2 M8 8l-3 4.5",
    ),
];

/// The art for a declared glyph key, or `None` when this shell has none.
///
/// The caller falls back through `category_glyph`, which is the shared
/// rule, rather than this deciding a family for itself.
#[must_use]
pub(in crate::gui::panels) fn art(key: &str) -> Option<&'static str> {
    GLYPH_PATHS
        .binary_search_by_key(&key, |(k, _)| *k)
        .ok()
        .map(|i| GLYPH_PATHS[i].1)
}

/// Stroke a glyph into `rect`, scaled from its authored box and centred.
pub(super) fn paint(painter: &Painter, key: &str, rect: Rect, color: Color32, width: f32) {
    let Some(path) = art(key) else {
        return;
    };
    let map = vector::fit(vec2(GLYPH_BOX, GLYPH_BOX), rect);
    let stroke = Stroke::new(width, color);
    for sub in vector::flatten_path(path) {
        let mut points: Vec<_> = sub.points.into_iter().map(&map).collect();
        if let (true, Some(&first)) = (sub.closed, points.first()) {
            points.push(first);
        }
        painter.add(egui::Shape::line(points, stroke));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("the crate sits two levels under the workspace root")
            .to_path_buf()
    }

    /// The browser's copy of the same table, parsed out of its source.
    ///
    /// Entries are read across line breaks rather than line by line,
    /// because the formatter wraps a long path onto the line after its
    /// key. A line-by-line reader silently drops exactly those, which
    /// makes a lossy parse look like a divergence; `declared` counts the
    /// keys independently so the two failures cannot be confused.
    fn browser_glyphs() -> (BTreeMap<String, String>, usize) {
        let source = std::fs::read_to_string(workspace_root().join("web/src/flow/nodeVisual.ts"))
            .expect("nodeVisual.ts must exist");
        let block = source
            .split("export const GLYPH_PATHS")
            .nth(1)
            .and_then(|s| s.split("\n};").next())
            .expect("the glyph table is still a top-level const");

        let declared = block
            .lines()
            .filter(|line| {
                let trimmed = line.trim_start();
                line.len() - trimmed.len() == 2
                    && trimmed.split_once(':').is_some_and(|(key, _)| {
                        !key.is_empty()
                            && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    })
            })
            .count();

        // One join is all the wrapping amounts to: the formatter breaks
        // between a key and its value and nowhere else in this table.
        let joined = block.replace(":\n", ": ");
        let mut out = BTreeMap::new();
        for line in joined.lines() {
            let Some((key, rest)) = line.trim().split_once(':') else {
                continue;
            };
            if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                continue;
            }
            let Some(path) = rest
                .trim()
                .strip_prefix('"')
                .and_then(|p| p.split('"').next())
            else {
                continue;
            };
            out.insert(key.to_string(), path.to_string());
        }
        (out, declared)
    }

    /// **The reason this table holds strings rather than vertices.**
    ///
    /// Two shells draw the same seventy-six glyphs and the release's exit
    /// criteria sanction that, on the grounds that art is what a shell
    /// can draw. What they do not sanction is the two quietly diverging,
    /// and a redrawn glyph is exactly the kind of change that lands in
    /// one shell's pass and never opens the other's. Holding the authored
    /// path verbatim is what makes the comparison possible at all: a
    /// table of flattened points could be compared for length and for
    /// nothing that matters.
    #[test]
    fn the_glyph_art_matches_the_browsers() {
        let (browser, declared) = browser_glyphs();
        let ours: BTreeMap<String, String> = GLYPH_PATHS
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();

        assert_eq!(
            browser.len(),
            declared,
            "the reader above lost entries out of nodeVisual.ts, so it is this test that \
             is broken rather than the art"
        );
        assert_eq!(
            ours.keys().collect::<Vec<_>>(),
            browser.keys().collect::<Vec<_>>(),
            "the two shells draw different glyph keys"
        );
        for (key, path) in &ours {
            assert_eq!(
                path, &browser[key],
                "the two shells draw `{key}` differently"
            );
        }
    }

    /// A node whose declared glyph has no art falls back to its family,
    /// which is correct behaviour and a terrible default state: it
    /// renders something plausible and nobody notices. The browser has
    /// the same guard, and this is the half of it that speaks for the
    /// desktop.
    #[test]
    fn every_declared_glyph_has_art() {
        let registry: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(workspace_root().join("schemas/registry.json"))
                .expect("registry.json must exist"),
        )
        .expect("registry.json parses");
        let nodes = registry["nodes"]
            .as_array()
            .expect("the registry snapshot carries a node array");
        assert!(nodes.len() > 50, "the registry looks truncated");

        let missing: Vec<&str> = nodes
            .iter()
            .filter_map(|n| n["glyph"].as_str())
            .filter(|glyph| art(glyph).is_none())
            .collect();
        assert!(
            missing.is_empty(),
            "these declared glyph keys have no art on the desktop, so their nodes draw as \
             their family and look plausible while being wrong: {missing:?}"
        );
    }

    /// The lookup bisects, so the table has to be sorted. Unsorted, it
    /// would silently miss entries rather than fail.
    #[test]
    fn the_table_is_sorted_because_the_lookup_bisects() {
        let keys: Vec<&str> = GLYPH_PATHS.iter().map(|(k, _)| *k).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted);
        sorted.dedup();
        assert_eq!(sorted.len(), keys.len(), "a glyph key appears twice");
        for key in keys {
            assert!(art(key).is_some(), "{key} is in the table but not findable");
        }
    }

    /// The parser's subset is defined by what the art needs, so the art
    /// is what holds it: every path must yield at least one run of at
    /// least two points, or it draws as nothing at all.
    #[test]
    fn every_glyph_parses_into_something_drawable() {
        for (key, path) in GLYPH_PATHS {
            let subs = super::vector::flatten_path(path);
            assert!(!subs.is_empty(), "{key} parsed into nothing");
            for sub in subs {
                assert!(
                    sub.points.len() >= 2,
                    "{key} has a run of {} points, which draws nothing",
                    sub.points.len()
                );
                assert!(
                    sub.points
                        .iter()
                        .all(|p| p.x.is_finite() && p.y.is_finite()),
                    "{key} produced a non-finite point"
                );
            }
        }
    }

    /// The fallback exists for a node type this build has never heard of,
    /// so the one thing it must never be is absent.
    #[test]
    fn the_fallback_glyph_is_drawable() {
        assert!(art(FALLBACK_GLYPH).is_some());
    }
}
