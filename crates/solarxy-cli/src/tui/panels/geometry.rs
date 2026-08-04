//! Identity and counts, plus the budget the analyzer already resolves.
//!
//! The budget and the asset category were computed on every run before 0.8.2,
//! used to raise one issue, and then thrown away. This panel is where they
//! finally land. The category is always named beside the meter, because a
//! budget without its category is a number with no reason attached: 12,480 of
//! 20,000 means nothing until you know it was measured as a hero asset.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use solarxy_core::format_number;

use super::super::widgets;
use super::{Ctx, Panel};

pub struct Geometry;

/// Wide enough for the longest label the panel uses.
const LABEL: u16 = 10;

impl Panel for Geometry {
    fn draw(&mut self, frame: &mut Frame, area: Rect, ctx: &Ctx<'_>) {
        let report = ctx.report;
        let mut lines: Vec<Line> = Vec::new();

        let format = if report.source_format.is_empty() {
            "unknown".to_owned()
        } else {
            report.source_format.to_uppercase()
        };
        let size = report
            .file_size_bytes
            .map_or_else(|| "size unknown".to_owned(), human_size);
        let mut first = widgets::field("format", &format, LABEL, ctx.theme);
        first.push(ratatui::text::Span::raw("   "));
        first.extend(widgets::field("", &size, 0, ctx.theme));
        lines.push(Line::from(first));
        lines.push(Line::raw(""));

        let uv_sets = usize::from(report.meshes.iter().any(|m| m.texcoord_count > 0));
        for (label, value) in [
            ("meshes", format_number(report.mesh_count)),
            ("materials", format_number(report.material_count)),
            ("vertices", format_number(report.total_vertices)),
            ("triangles", format_number(report.total_triangles)),
            ("indices", format_number(report.total_indices)),
            ("uv sets", uv_sets.to_string()),
        ] {
            lines.push(Line::from(widgets::field(label, &value, LABEL, ctx.theme)));
        }

        if let Some(budget) = report.triangle_budget {
            lines.push(Line::raw(""));
            let used = report.total_triangles as f64 / f64::from(budget).max(1.0);
            // Over budget is the state this panel exists to make obvious, so
            // the meter changes hue rather than only running full.
            let ink = if report.total_triangles as u64 > u64::from(budget) {
                ctx.theme.error
            } else {
                ctx.theme.success
            };
            let percent = format!("{:.0}%", used * 100.0);
            lines.push(widgets::meter(
                used,
                meter_cells(area.width),
                &percent,
                widgets::Paint {
                    ink,
                    theme: ctx.theme,
                    glyphs: ctx.glyphs,
                },
            ));
            let category = report
                .asset_category
                .map_or_else(|| "unclassified".to_owned(), |c| c.to_string());
            lines.push(Line::from(widgets::field(
                "",
                &format!(
                    "{category}, {} of {}",
                    format_number(report.total_triangles),
                    format_number(budget as usize)
                ),
                LABEL,
                ctx.theme,
            )));
        }

        frame.render_widget(Paragraph::new(lines), area);
    }
}

/// Leave room for the label the meter carries beside it.
fn meter_cells(width: u16) -> u16 {
    width.saturating_sub(LABEL + 12).clamp(4, 40)
}

fn human_size(bytes: u64) -> String {
    const STEPS: [(u64, &str); 3] = [(1 << 30, "GB"), (1 << 20, "MB"), (1 << 10, "kB")];
    for (scale, suffix) in STEPS {
        if bytes >= scale {
            return format!("{:.1} {suffix}", bytes as f64 / scale as f64);
        }
    }
    format!("{bytes} B")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_size_reads_in_the_unit_a_person_would_use() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2_516_582), "2.4 MB");
        assert_eq!(human_size(1536), "1.5 kB");
        assert_eq!(human_size(3_221_225_472), "3.0 GB");
    }

    /// The meter has to survive a panel at the minimum width without
    /// collapsing to nothing or overflowing the frame.
    #[test]
    fn the_meter_fits_every_panel_width() {
        for width in [24u16, 40, 47, 140] {
            let cells = meter_cells(width);
            assert!(cells >= 4, "{width} gave a {cells}-cell meter");
            assert!(cells <= 40);
        }
    }
}
