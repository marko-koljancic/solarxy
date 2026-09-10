//! How an attribute table reads: which node it watches, what a header
//! column is called, and how a cell prints.
//!
//! The virtualization window is **not** here. Deciding which rows to
//! materialize from a scroll offset and a viewport height is a fact about
//! a scrolling container, not about a document, and the two shells scroll
//! with different machinery. What is here is everything a reader would
//! notice differing between them.

use solarxy_graph::engine::attr_table::AttrColumn;
use solarxy_graph::document::NodeId;

/// The node the pane watches: the first selected node, else whichever
/// node carries the display flag, else nothing.
#[must_use]
pub fn watched_node(selection: &[NodeId], active_output: Option<NodeId>) -> Option<NodeId> {
    selection.first().copied().or(active_output)
}

/// One cell's text: four decimal places, with a missing lane shown as a
/// plain hyphen rather than a fabricated zero.
///
/// Negative zero prints as zero. It is a real `f64` a reader has no use
/// for, and a column that shows `-0.0000` beside `0.0000` reads as a
/// difference that is not there.
#[must_use]
pub fn cell_text(value: Option<f64>) -> String {
    match value {
        None => "-".to_string(),
        Some(v) if v.is_nan() => "-".to_string(),
        Some(v) => {
            let fixed = format!("{v:.4}");
            if fixed == "-0.0000" {
                "0.0000".to_string()
            } else {
                fixed
            }
        }
    }
}

/// The flat header row for a column set.
///
/// A single-component lane keeps its name; a vector lane fans out into one
/// heading per component, because the rows are flat and a reader counting
/// across needs the headings to count with them.
#[must_use]
pub fn header_cells(columns: &[AttrColumn]) -> Vec<String> {
    const SUFFIX: [&str; 4] = ["x", "y", "z", "w"];
    let mut out = Vec::new();
    for column in columns {
        if column.components == 1 {
            out.push(column.key.clone());
            continue;
        }
        for i in 0..column.components as usize {
            let suffix = SUFFIX.get(i).copied().unwrap_or("?");
            out.push(format!("{}.{suffix}", column.key));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(key: &str, ty: &'static str, components: u8) -> AttrColumn {
        AttrColumn {
            key: key.to_string(),
            ty,
            components,
        }
    }

    #[test]
    fn prefers_the_first_selected_node() {
        assert_eq!(
            watched_node(&[NodeId(7), NodeId(9)], Some(NodeId(3))),
            Some(NodeId(7))
        );
    }

    #[test]
    fn falls_back_to_the_display_flag_node() {
        assert_eq!(watched_node(&[], Some(NodeId(3))), Some(NodeId(3)));
    }

    #[test]
    fn watches_nothing_with_neither() {
        assert_eq!(watched_node(&[], None), None);
    }

    #[test]
    fn renders_four_fixed_decimals() {
        assert_eq!(cell_text(Some(1.0)), "1.0000");
        assert_eq!(cell_text(Some(-0.25)), "-0.2500");
    }

    #[test]
    fn normalizes_negative_zero() {
        // A column showing `-0.0000` beside `0.0000` reads as a difference
        // that is not there.
        assert_eq!(cell_text(Some(-0.000_001)), "0.0000");
        assert_eq!(cell_text(Some(-0.0)), "0.0000");
    }

    #[test]
    fn renders_missing_lanes_as_a_hyphen() {
        assert_eq!(cell_text(None), "-");
        assert_eq!(cell_text(Some(f64::NAN)), "-");
    }

    #[test]
    fn keeps_scalar_lanes_flat_and_fans_vectors_out_by_component() {
        assert_eq!(
            header_cells(&[
                column("P", "vec3", 3),
                column("mask", "float", 1),
                column("color", "vec4", 4),
            ]),
            [
                "P.x", "P.y", "P.z", "mask", "color.x", "color.y", "color.z", "color.w"
            ]
        );
    }
}
