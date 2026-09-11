//! The node info card: what a node is, what it did, and what it is wired to.
//!
//! **It follows the node it was opened on, not the selection**, so a user
//! can open it on one node, select another, and compare; a card that
//! tracked the selection would be a second parameter panel header. It is
//! modeless, movable and resizable, because it is read beside the graph
//! rather than instead of it, and it closes on Escape, on its own control,
//! and when the node it describes is gone.
//!
//! **Every line is the shared derivation**, so the card says what the
//! browser's says: the kind and the doc, the status with its bypassed and
//! stale suffixes, the geometry counts, the bounds, the cooks, the wiring,
//! the two stamps, the warnings and the validation counts, then the ports
//! and the parameters straight off the registry, so a node added in Rust
//! documents itself here with no change on this side. The report's
//! reading is `solarxy_studio::node::node_report_text`, which the browser
//! host reads too. The one host-bound line is the absolute date: it needs
//! the reader's locale and clock, and the shared rule deliberately takes
//! neither.
//!
//! Opened from the radial's Info spoke, from the canvas toolbar's Info
//! button on the selected node, and from `I` over the canvas.

use std::fmt::Write as _;

use solarxy_graph::cook::state::{CookState, NodeCookStats};
use solarxy_graph::document::{Document, GraphContext, NodeId};
use solarxy_graph::engine::snapshot::{ParamSnapshot, PortSnapshot};
use solarxy_graph::registry::Registry;
use solarxy_graph::registry::param_spec::Unit;
use solarxy_studio::node::NodeReportText;

use super::seed::{CanvasState, NodeCook};
use crate::gui::theme::Theme;

/// What the state layer gathers for the card's node, once per frame
/// while the card is up.
#[derive(Debug, Clone, Default)]
pub(crate) struct NodeInfoView {
    pub report: Option<NodeReportText>,
    pub stats: Option<NodeCookStats>,
    pub warnings: Vec<String>,
    /// Errors and warnings in the node's validation report, when it has one.
    pub validation: Option<(usize, usize)>,
}

/// A span of a rendered doc string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DocSpan {
    pub text: String,
    pub code: bool,
    pub bold: bool,
}

/// A doc string as paragraphs of spans: blank-line paragraphs with
/// backtick code and double-star bold, which is all the catalogue's docs
/// use and all the browser's `renderDoc` reads.
pub(super) fn render_doc(doc: &str) -> Vec<Vec<DocSpan>> {
    doc.split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|para| {
            let mut spans = Vec::new();
            let mut rest = para;
            while !rest.is_empty() {
                let code = rest
                    .find('`')
                    .and_then(|a| rest[a + 1..].find('`').map(|b| (a, a + 1 + b)));
                let bold = rest
                    .find("**")
                    .and_then(|a| rest[a + 2..].find("**").map(|b| (a, a + 2 + b)));
                let next = match (code, bold) {
                    (Some(c), Some(b)) if b.0 < c.0 => Some((b, true)),
                    (Some(c), _) => Some((c, false)),
                    (None, Some(b)) => Some((b, true)),
                    (None, None) => None,
                };
                let Some(((start, end), is_bold)) = next else {
                    spans.push(DocSpan {
                        text: rest.to_string(),
                        code: false,
                        bold: false,
                    });
                    break;
                };
                if start > 0 {
                    spans.push(DocSpan {
                        text: rest[..start].to_string(),
                        code: false,
                        bold: false,
                    });
                }
                let width = if is_bold { 2 } else { 1 };
                spans.push(DocSpan {
                    text: rest[start + width..end].to_string(),
                    code: !is_bold,
                    bold: is_bold,
                });
                rest = &rest[end + width..];
            }
            spans
        })
        .collect()
}

/// The status line: the browser's states, with its two suffixes.
pub(super) fn status_text(cook: Option<&NodeCook>, bypassed: bool) -> String {
    let base = match cook {
        Some(c) if c.error.is_some() => {
            format!("error: {}", c.error.as_deref().unwrap_or_default())
        }
        Some(c) => match c.state {
            // An asynchronous cook in flight. The browser's fifth state, a
            // parse loading in its worker, has no counterpart here: a native
            // import parses inside the cook.
            CookState::Pending(_) => "cooking...".to_string(),
            CookState::Clean | CookState::Dirty if c.last_us > 0 => {
                let mut s = format!("cooked in {:.1} ms", c.last_us as f64 / 1000.0);
                if c.state == CookState::Dirty {
                    s.push_str(" (stale)");
                }
                s
            }
            CookState::Clean | CookState::Dirty => "not cooked yet".to_string(),
        },
        None => "not cooked yet".to_string(),
    };
    if bypassed {
        format!("{base} (bypassed)")
    } else {
        base
    }
}

/// A stamp as the card prints it: the date in the local clock, then the
/// relative phrase, or "unknown" when the document carries none.
pub(super) fn format_timestamp(ms: Option<f64>, relative: &str) -> String {
    let Some(ms) = ms else {
        return "unknown".to_string();
    };
    #[allow(clippy::cast_possible_truncation)]
    let nanos = (ms * 1_000_000.0) as i128;
    let Ok(stamp) = time::OffsetDateTime::from_unix_timestamp_nanos(nanos) else {
        return "unknown".to_string();
    };
    let local = time::UtcOffset::current_local_offset().map_or(stamp, |o| stamp.to_offset(o));
    let date = local
        .format(&time::macros::format_description!(
            "[year]-[month]-[day] [hour]:[minute]"
        ))
        .unwrap_or_else(|_| "unknown".to_string());
    if relative.is_empty() {
        date
    } else {
        format!("{date} ({relative})")
    }
}

fn unit_name(unit: Unit) -> Option<&'static str> {
    match unit {
        Unit::None => None,
        Unit::Degrees => Some("degrees"),
        Unit::Meters => Some("meters"),
        Unit::Normalized => Some("normalized"),
    }
}

/// Draw the card for the node the canvas opened it on, if any.
pub(super) fn draw_info(
    ui: &egui::Ui,
    doc: &Document,
    registry: &Registry,
    ctx: GraphContext,
    cook: &std::collections::BTreeMap<NodeId, NodeCook>,
    view: Option<&NodeInfoView>,
    state: &mut CanvasState,
    theme: Theme,
) {
    let Some(node) = state.info else {
        return;
    };
    let Ok(graph) = doc.graph(ctx) else {
        state.info = None;
        return;
    };
    let Some(data) = graph.node(node) else {
        state.info = None;
        return;
    };
    let Some(desc) = registry.get(&data.type_id) else {
        state.info = None;
        return;
    };
    if ui
        .ctx()
        .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        state.info = None;
        return;
    }

    let title = solarxy_graph::naming::node_name(data, registry);
    let kind = solarxy_studio::node::describe_kind(desc);
    let status = status_text(cook.get(&node), data.bypassed);
    let mut open = true;
    egui::Window::new(format!("{title}  {} v{}", desc.type_id, desc.version))
        .id(ui.id().with(("node-info", node)))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .movable(true)
        .default_width(340.0)
        .show(ui.ctx(), |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let row = |ui: &mut egui::Ui, label: &str, value: &str| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(label).color(theme.muted).size(10.0));
                        ui.label(egui::RichText::new(value).size(11.0));
                    });
                };
                row(ui, "Kind", &kind);
                draw_doc(ui, desc.doc, theme);
                ui.separator();
                row(ui, "Status", &status);
                if let Some(stats) = view.and_then(|v| v.stats) {
                    let mesh = if stats.meshes == 1 { "mesh" } else { "meshes" };
                    row(
                        ui,
                        "Geometry",
                        &format!(
                            "{} points, {} prims, {} {mesh}",
                            stats.points, stats.prims, stats.meshes
                        ),
                    );
                }
                if let Some(report) = view.and_then(|v| v.report.as_ref()) {
                    if let Some(bounds) = &report.bounds {
                        row(ui, "Bounds", bounds);
                    }
                    if report.cook_count > 0 {
                        let mut cooks = format!(
                            "{} this session, {} total",
                            report.cook_count, report.total_cook
                        );
                        if let Some(average) = &report.average_cook {
                            let _ = write!(cooks, " \u{b7} {average} average");
                        }
                        let _ = write!(cooks, " \u{b7} {} last", report.last_cook);
                        row(ui, "Cooks", &cooks);
                    }
                    let wiring = &report.connections;
                    if !wiring.inputs.is_empty() || !wiring.outputs.is_empty() {
                        let mut lines = Vec::new();
                        for port in &wiring.inputs {
                            lines.push(format!("{} <- {}", port.port, port.nodes.join(", ")));
                        }
                        for port in &wiring.outputs {
                            lines.push(format!("{} -> {}", port.port, port.nodes.join(", ")));
                        }
                        row(ui, "Wired", &lines.join("\n"));
                    }
                    row(
                        ui,
                        "Created",
                        &format_timestamp(report.created.ms, &report.created.relative),
                    );
                    row(
                        ui,
                        "Modified",
                        &format_timestamp(report.modified.ms, &report.modified.relative),
                    );
                }
                if let Some(warnings) = view.map(|v| &v.warnings).filter(|w| !w.is_empty()) {
                    row(ui, "Warnings", &warnings.join("\n"));
                }
                if let Some((errors, warnings)) = view.and_then(|v| v.validation) {
                    row(
                        ui,
                        "Validation",
                        &format!("{errors} error(s), {warnings} warning(s)"),
                    );
                }

                let ports: Vec<(&str, PortSnapshot)> = desc
                    .inputs
                    .iter()
                    .map(|p| ("in", PortSnapshot::from(p)))
                    .chain(desc.outputs.iter().map(|p| ("out", PortSnapshot::from(p))))
                    .collect();
                if !ports.is_empty() {
                    egui::CollapsingHeader::new(format!("Ports ({})", ports.len()))
                        .default_open(true)
                        .show(ui, |ui| {
                            for (dir, port) in &ports {
                                let mut meta = format!(
                                    "{dir} \u{b7} {}",
                                    serde_json::to_value(port.data_type)
                                        .ok()
                                        .and_then(|v| v.as_str().map(String::from))
                                        .unwrap_or_default()
                                );
                                if port.variadic {
                                    meta.push_str(" \u{b7} variadic");
                                }
                                if port.required {
                                    meta.push_str(" \u{b7} required");
                                }
                                definition(ui, &port.label, &meta, &port.doc, theme);
                            }
                        });
                }
                let params: Vec<(
                    &solarxy_graph::registry::param_spec::ParamSpec,
                    ParamSnapshot,
                )> = desc
                    .params
                    .iter()
                    .filter(|p| p.group != "general")
                    .map(|p| (p, ParamSnapshot::from(p)))
                    .collect();
                if !params.is_empty() {
                    egui::CollapsingHeader::new(format!("Parameters ({})", params.len()))
                        .default_open(false)
                        .show(ui, |ui| {
                            for (spec, snap) in &params {
                                let mut meta = snap.param_type.clone();
                                if let Some(unit) = unit_name(spec.unit) {
                                    let _ = write!(meta, " \u{b7} {unit}");
                                }
                                if let Some((lo, hi)) = snap.hard {
                                    let _ = write!(meta, " \u{b7} {lo} to {hi}");
                                }
                                definition(ui, &snap.label, &meta, &snap.doc, theme);
                            }
                        });
                }
            });
        });
    if !open {
        state.info = None;
    }
}

/// One definition: the term, its meta line, and its doc or the absence.
fn definition(ui: &mut egui::Ui, term: &str, meta: &str, doc: &str, theme: Theme) {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(term).strong().size(11.0));
        ui.label(egui::RichText::new(meta).color(theme.muted).size(10.0));
    });
    if doc.trim().is_empty() {
        ui.label(
            egui::RichText::new("No description.")
                .italics()
                .color(theme.muted)
                .size(10.0),
        );
    } else {
        draw_doc(ui, doc, theme);
    }
}

/// A doc string as paragraphs, code in the monospace face and bold strong.
fn draw_doc(ui: &mut egui::Ui, doc: &str, theme: Theme) {
    for para in render_doc(doc) {
        let mut job = egui::text::LayoutJob::default();
        for span in para {
            let font = if span.code {
                egui::TextStyle::Monospace.resolve(ui.style())
            } else {
                egui::FontId::proportional(11.0)
            };
            job.append(
                &span.text,
                0.0,
                egui::TextFormat {
                    font_id: font,
                    color: if span.bold { theme.fg } else { theme.muted },
                    ..Default::default()
                },
            );
        }
        job.wrap.max_width = ui.available_width();
        ui.label(job);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(text: &str, code: bool, bold: bool) -> DocSpan {
        DocSpan {
            text: text.to_string(),
            code,
            bold,
        }
    }

    /// Paragraphs split on a blank line, code and bold are spans, and a
    /// paragraph with neither is one plain span; the browser's renderDoc.
    #[test]
    fn a_doc_renders_as_paragraphs_of_code_and_bold_spans() {
        let doc = "Plain text.\n\nUses `width` and **must** be set.\n\n\n";
        let paras = render_doc(doc);
        assert_eq!(paras.len(), 2);
        assert_eq!(paras[0], vec![span("Plain text.", false, false)]);
        assert_eq!(
            paras[1],
            vec![
                span("Uses ", false, false),
                span("width", true, false),
                span(" and ", false, false),
                span("must", false, true),
                span(" be set.", false, false),
            ]
        );
        assert!(render_doc("").is_empty());
    }

    /// The states and the two suffixes are the browser's.
    #[test]
    fn the_status_line_is_the_browsers() {
        let cook = |state: CookState, last_us: u64, error: Option<&str>| NodeCook {
            state,
            last_us,
            error: error.map(str::to_string),
            errors: 0,
            warnings: 0,
        };
        assert_eq!(status_text(None, false), "not cooked yet");
        assert_eq!(
            status_text(Some(&cook(CookState::Clean, 1_234, None)), false),
            "cooked in 1.2 ms"
        );
        assert_eq!(
            status_text(Some(&cook(CookState::Dirty, 1_234, None)), false),
            "cooked in 1.2 ms (stale)"
        );
        assert_eq!(
            status_text(Some(&cook(CookState::Dirty, 0, None)), false),
            "not cooked yet"
        );
        assert_eq!(
            status_text(Some(&cook(CookState::Pending(1), 0, None)), false),
            "cooking..."
        );
        assert_eq!(
            status_text(Some(&cook(CookState::Clean, 5, Some("bad"))), true),
            "error: bad (bypassed)"
        );
    }

    /// A missing stamp is "unknown", never an epoch date; a present one
    /// carries the date and the phrase beside it.
    #[test]
    fn a_missing_stamp_is_unknown_and_a_present_one_carries_its_phrase() {
        assert_eq!(format_timestamp(None, "5 minutes ago"), "unknown");
        let stamp = format_timestamp(Some(1_700_000_000_000.0), "5 minutes ago");
        assert!(stamp.ends_with(" (5 minutes ago)"), "{stamp}");
        assert!(
            stamp.starts_with("2023-11-1"),
            "a real date in the local clock: {stamp}"
        );
        let bare = format_timestamp(Some(1_700_000_000_000.0), "");
        assert!(!bare.contains('('), "no phrase, no brackets: {bare}");
    }
}
