use crossterm::event::{KeyCode, KeyEvent, MouseEventKind};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Margin},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Paragraph, Scrollbar, ScrollbarOrientation, Tabs, Wrap},
};
use solarxy_core::format_number;

use std::io;

use solarxy_core::json::report_to_json;
use solarxy_core::report::{AnalysisReport, Severity};

use super::tui::caps::{Capabilities, Glyphs};
use super::tui::scroll::{Extent, Scroll, rendered_rows};
use super::tui::prefs::TuiPrefs;
use super::tui::shell::{self, Flow, Input, Surface};
use super::tui::theme::ThemeSet;
use super::tui::{kv_line, section_header};
use super::tui_theme::TuiTheme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Overview = 0,
    Meshes = 1,
    Materials = 2,
    Validation = 3,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Overview, Tab::Meshes, Tab::Materials, Tab::Validation];

    fn title(self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Meshes => "Meshes",
            Tab::Materials => "Materials",
            Tab::Validation => "Validation",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

pub struct TerminalApp {
    exit: bool,
    report: AnalysisReport,
    model_path: String,
    active_tab: Tab,
    /// One scroll position per tab, so switching tabs returns to where the
    /// user left off.
    scrolls: [Scroll; 4],
    /// What each tab measured at its last draw, in rendered rows. A keystroke
    /// arrives between frames, so this is the most recent description of the
    /// content it can be clamped against.
    extents: [Extent; 4],
    export_input: Option<String>,
    export_json_input: Option<String>,
    status_message: Option<(String, bool)>,
    /// The shared palette, resolved once at construction and degraded to
    /// what the terminal can render. This shell draws no color that does
    /// not come from here.
    theme: TuiTheme,
    /// The glyph repertoire, resolved once alongside the theme.
    glyphs: Glyphs,
}

impl TerminalApp {
    pub fn new(report: AnalysisReport, model_path: String, requested_theme: Option<&str>) -> Self {
        let caps = Capabilities::detect();
        let (prefs, prefs_notices) = TuiPrefs::load();
        // The flag wins for this run; the file is what the reader chose last
        // time. Neither is an error when absent.
        let wanted = requested_theme.or(prefs.theme.as_deref());
        let (theme, notices) = ThemeSet::load().resolve(wanted, caps);

        // All of this lands before `ratatui::init` takes the screen, so it
        // reaches the normal terminal rather than smearing the alternate one.
        // This is how a reader confirms an override did what they asked, and
        // how a refused theme gets to say why.
        tracing::debug!(
            "resolved terminal capabilities: color={:?} glyphs={:?} theme={}",
            caps.color,
            caps.glyphs,
            theme.name
        );
        for notice in prefs_notices {
            tracing::warn!("{notice}");
        }
        for notice in notices {
            tracing::warn!("{notice}");
        }

        Self::with_capabilities_and_report(report, model_path, caps)
            .with_theme(TuiTheme::from_theme(&theme))
    }

    fn with_capabilities_and_report(
        report: AnalysisReport,
        model_path: String,
        caps: Capabilities,
    ) -> Self {
        Self {
            exit: false,
            report,
            model_path,
            active_tab: Tab::Overview,
            scrolls: [Scroll::default(); 4],
            extents: [Extent::default(); 4],
            export_input: None,
            export_json_input: None,
            status_message: None,
            theme: TuiTheme::for_capabilities(caps),
            glyphs: caps.glyphs(),
        }
    }

    /// Build against explicit capabilities rather than the environment.
    ///
    /// Tests pin a tier this way so they assert what a given terminal would
    /// see rather than what the machine running them happens to report.
    #[must_use]
    pub fn with_capabilities(mut self, caps: Capabilities) -> Self {
        self.theme = TuiTheme::for_capabilities(caps);
        self.glyphs = caps.glyphs();
        self
    }

    /// Build with an explicit theme, bypassing capability resolution.
    #[must_use]
    pub fn with_theme(mut self, theme: TuiTheme) -> Self {
        self.theme = theme;
        self
    }

    pub fn run(mut self) -> io::Result<()> {
        shell::run(&mut self)
    }

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(area);

        let title = Line::from(vec![
            Span::raw(" "),
            Span::styled(self.glyphs.sun, Style::default().fg(self.theme.accent)),
            Span::raw(" "),
            Span::styled(
                "Solarxy",
                Style::default()
                    .fg(self.theme.heading)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled("Model Analysis", Style::default().fg(self.theme.text)),
            Span::raw(" "),
        ]);

        let tab_titles: Vec<Line> = Tab::ALL.iter().map(|t| Line::raw(t.title())).collect();

        let tabs_widget = Tabs::new(tab_titles)
            .block(
                Block::bordered()
                    .title(title.centered())
                    .border_set(self.glyphs.border)
                    .border_style(Style::default().fg(self.theme.border)),
            )
            .select(self.active_tab.index())
            .highlight_style(
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )
            .style(Style::default().fg(self.theme.muted))
            .divider(format!(" {} ", self.glyphs.divider));
        frame.render_widget(tabs_widget, chunks[0]);

        let tab_idx = self.active_tab.index();
        let content_text = self.format_tab_content();

        // Measured in rendered rows, not in logical lines: the body below
        // wraps, and a count of lines under-reports it by however many rows
        // the wrapping added. The clamp, the counter and the scrollbar all
        // read this one figure.
        let extent = Extent::new(
            rendered_rows(&content_text, chunks[1].width.saturating_sub(2)),
            chunks[1].height.saturating_sub(2),
        );
        self.extents[tab_idx] = extent;
        let offset = self.scrolls[tab_idx].offset(extent);

        let (first_row, total_rows) = self.scrolls[tab_idx].position(extent);
        let position = format!(" [{}/{}] ", first_row, total_rows);

        let instructions = if let Some(ref path) = self.export_json_input {
            Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    "Export JSON to: ",
                    Style::default()
                        .fg(self.theme.label)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(path.clone(), Style::default().fg(self.theme.text)),
                Span::styled(self.glyphs.caret, Style::default().fg(self.theme.accent)),
                Span::raw("  "),
                Span::styled(
                    "Enter",
                    Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Save  "),
                Span::styled(
                    "Esc",
                    Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Cancel "),
            ])
        } else if let Some(ref path) = self.export_input {
            Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    "Export to: ",
                    Style::default()
                        .fg(self.theme.label)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(path.clone(), Style::default().fg(self.theme.text)),
                Span::styled(self.glyphs.caret, Style::default().fg(self.theme.accent)),
                Span::raw("  "),
                Span::styled(
                    "Enter",
                    Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Save  "),
                Span::styled(
                    "Esc",
                    Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Cancel "),
            ])
        } else if let Some((ref msg, success)) = self.status_message {
            let color = if success {
                self.theme.success
            } else {
                self.theme.error
            };
            Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    msg.clone(),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
            ])
        } else {
            Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    "Tab/1-4",
                    Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Switch  "),
                Span::styled(
                    "j/k",
                    Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Scroll  "),
                Span::styled(
                    "g/G",
                    Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Top/Bottom  "),
                Span::styled(
                    "e",
                    Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Export  "),
                Span::styled(
                    "J",
                    Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" JSON  "),
                Span::styled(
                    "q",
                    Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Quit "),
            ])
        };

        let content_block = Block::bordered()
            .title_bottom(instructions.left_aligned())
            .title_bottom(Line::from(position).centered())
            .title_bottom(
                validation_status_line(&self.report, &self.theme, &self.glyphs).right_aligned(),
            )
            .border_set(self.glyphs.border)
            .border_style(Style::default().fg(self.theme.border));

        let paragraph = Paragraph::new(content_text)
            .block(content_block)
            .scroll((offset, 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, chunks[1]);

        if extent.overflows() {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some(self.glyphs.scroll_up))
                .end_symbol(Some(self.glyphs.scroll_down));
            let mut scrollbar_state = self.scrolls[tab_idx].scrollbar(extent);
            frame.render_stateful_widget(
                scrollbar,
                chunks[1].inner(Margin {
                    vertical: 1,
                    horizontal: 0,
                }),
                &mut scrollbar_state,
            );
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if let Some(ref mut path) = self.export_json_input {
            match key_event.code {
                KeyCode::Enter => {
                    let path = path.clone();
                    self.export_json_report(&path);
                    self.export_json_input = None;
                }
                KeyCode::Esc => self.export_json_input = None,
                KeyCode::Char(c) => path.push(c),
                KeyCode::Backspace => {
                    path.pop();
                }
                _ => {}
            }
            return;
        }

        if let Some(ref mut path) = self.export_input {
            match key_event.code {
                KeyCode::Enter => {
                    let path = path.clone();
                    self.export_report(&path);
                    self.export_input = None;
                }
                KeyCode::Esc => self.export_input = None,
                KeyCode::Char(c) => path.push(c),
                KeyCode::Backspace => {
                    path.pop();
                }
                _ => {}
            }
            return;
        }

        self.status_message = None;

        let tab_idx = self.active_tab.index();
        let extent = self.extents[tab_idx];
        match key_event.code {
            KeyCode::Char('q') | KeyCode::Esc => self.exit = true,
            KeyCode::Char('e') => {
                self.export_input = Some(self.default_export_filename());
            }
            KeyCode::Char('J') => {
                self.export_json_input = Some(self.default_json_export_path());
            }
            KeyCode::Up | KeyCode::Char('k') => self.scrolls[tab_idx].up(1),
            KeyCode::Down | KeyCode::Char('j') => self.scrolls[tab_idx].down(1, extent),
            KeyCode::Char('g') => self.scrolls[tab_idx].first(),
            KeyCode::Char('G') => self.scrolls[tab_idx].last(extent),
            KeyCode::PageUp => self.scrolls[tab_idx].up(20),
            KeyCode::PageDown => self.scrolls[tab_idx].down(20, extent),
            KeyCode::Tab => {
                let next = (self.active_tab.index() + 1) % Tab::ALL.len();
                self.active_tab = Tab::ALL[next];
            }
            KeyCode::BackTab => {
                let prev = (self.active_tab.index() + Tab::ALL.len() - 1) % Tab::ALL.len();
                self.active_tab = Tab::ALL[prev];
            }
            KeyCode::Char('1') => self.active_tab = Tab::Overview,
            KeyCode::Char('2') => self.active_tab = Tab::Meshes,
            KeyCode::Char('3') => self.active_tab = Tab::Materials,
            KeyCode::Char('4') => self.active_tab = Tab::Validation,
            _ => {}
        }
    }

    fn default_export_filename(&self) -> String {
        let name = &self.report.model_name;
        match name.rsplit_once('.') {
            Some((stem, _)) => format!("{}_report.txt", stem),
            None => format!("{}_report.txt", name),
        }
    }

    fn default_json_export_path(&self) -> String {
        std::path::Path::new(&self.model_path)
            .with_extension("json")
            .to_string_lossy()
            .to_string()
    }

    fn export_report(&mut self, path: &str) {
        match std::fs::write(path, self.report.to_string()) {
            Ok(_) => self.status_message = Some((format!("Report saved to {}", path), true)),
            Err(e) => self.status_message = Some((format!("Export failed: {}", e), false)),
        }
    }

    fn export_json_report(&mut self, path: &str) {
        let json = match report_to_json(&self.report) {
            Ok(j) => j,
            Err(e) => {
                self.status_message = Some((format!("JSON serialization failed: {e}"), false));
                return;
            }
        };
        match std::fs::write(path, json) {
            Ok(_) => self.status_message = Some((format!("JSON report saved to {}", path), true)),
            Err(e) => self.status_message = Some((format!("JSON export failed: {}", e), false)),
        }
    }

    fn format_tab_content(&self) -> Text<'static> {
        match self.active_tab {
            Tab::Overview => self.format_overview(),
            Tab::Meshes => self.format_meshes(),
            Tab::Materials => self.format_materials(),
            Tab::Validation => self.format_validation(),
        }
    }

    fn format_overview(&self) -> Text<'static> {
        let mut lines = vec![
            section_header("MODEL OVERVIEW", &self.theme),
            Line::from(""),
            kv_line("Model Name", &self.report.model_name, &self.theme),
            kv_line(
                "Mesh Count",
                &self.report.mesh_count.to_string(),
                &self.theme,
            ),
            kv_line(
                "Material Count",
                &self.report.material_count.to_string(),
                &self.theme,
            ),
            kv_line(
                "Total Vertices",
                &format_number(self.report.total_vertices),
                &self.theme,
            ),
            kv_line(
                "Total Indices",
                &format_number(self.report.total_indices),
                &self.theme,
            ),
            kv_line(
                "Total Triangles",
                &format_number(self.report.total_triangles),
                &self.theme,
            ),
        ];

        if let Some(ref bounds) = self.report.bounds {
            lines.push(Line::from(""));
            lines.push(section_header("BOUNDING BOX", &self.theme));
            lines.push(Line::from(""));
            lines.push(kv_line(
                "Min",
                &format!(
                    "[{:.3}, {:.3}, {:.3}]",
                    bounds.min[0], bounds.min[1], bounds.min[2]
                ),
                &self.theme,
            ));
            lines.push(kv_line(
                "Max",
                &format!(
                    "[{:.3}, {:.3}, {:.3}]",
                    bounds.max[0], bounds.max[1], bounds.max[2]
                ),
                &self.theme,
            ));
            lines.push(kv_line(
                "Size",
                &format!(
                    "[{:.3}, {:.3}, {:.3}]",
                    bounds.size[0], bounds.size[1], bounds.size[2]
                ),
                &self.theme,
            ));
            lines.push(kv_line(
                "Center",
                &format!(
                    "[{:.3}, {:.3}, {:.3}]",
                    bounds.center[0], bounds.center[1], bounds.center[2]
                ),
                &self.theme,
            ));
            lines.push(kv_line(
                "Diagonal",
                &format!("{:.3}", bounds.diagonal),
                &self.theme,
            ));
        }

        Text::from(lines)
    }

    fn format_meshes(&self) -> Text<'static> {
        let mut lines = Vec::new();

        if self.report.meshes.is_empty() {
            lines.push(Line::from(Span::styled(
                "No meshes found",
                Style::default().fg(self.theme.label),
            )));
            return Text::from(lines);
        }

        lines.push(section_header("MESH DETAILS", &self.theme));
        lines.push(Line::from(""));

        for (i, mesh) in self.report.meshes.iter().enumerate() {
            lines.push(Line::from(Span::styled(
                format!("Mesh [{}]:", mesh.index),
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(kv_line(
                "  Vertices",
                &format_number(mesh.vertex_count),
                &self.theme,
            ));
            lines.push(kv_line(
                "  Indices",
                &format_number(mesh.index_count),
                &self.theme,
            ));
            lines.push(kv_line(
                "  Triangles",
                &format_number(mesh.triangle_count),
                &self.theme,
            ));

            let normal_indicator = if mesh.normal_count == mesh.vertex_count {
                self.glyphs.check
            } else {
                self.glyphs.warn
            };
            lines.push(kv_line(
                "  Normals",
                &format!("{} {}", format_number(mesh.normal_count), normal_indicator),
                &self.theme,
            ));

            let texcoord_indicator = if mesh.texcoord_count == mesh.vertex_count {
                self.glyphs.check
            } else if mesh.texcoord_count == 0 {
                self.glyphs.cross
            } else {
                self.glyphs.warn
            };
            lines.push(kv_line(
                "  Texture Coords",
                &format!(
                    "{} {}",
                    format_number(mesh.texcoord_count),
                    texcoord_indicator
                ),
                &self.theme,
            ));

            let mat_str = match (&mesh.material_name, mesh.material_id) {
                (Some(name), Some(id)) => format!("'{}' (ID: {})", name, id),
                (None, Some(id)) => format!("Invalid ID: {}", id),
                _ => "None".to_string(),
            };
            lines.push(kv_line("  Material", &mat_str, &self.theme));

            if i < self.report.meshes.len() - 1 {
                lines.push(Line::from(""));
            }
        }

        Text::from(lines)
    }

    fn format_materials(&self) -> Text<'static> {
        let mut lines = Vec::new();

        if self.report.materials.is_empty() {
            lines.push(section_header("MATERIALS", &self.theme));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "No materials found (.mtl file not provided or empty)",
                Style::default().fg(self.theme.label),
            )));
            return Text::from(lines);
        }

        lines.push(section_header("MATERIAL DETAILS", &self.theme));
        lines.push(Line::from(""));

        for (i, mat) in self.report.materials.iter().enumerate() {
            lines.push(Line::from(Span::styled(
                format!("Material [{}]: '{}'", mat.index, mat.name),
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(kv_line(
                "  Ambient",
                &format!(
                    "[{:.3}, {:.3}, {:.3}]",
                    mat.ambient[0], mat.ambient[1], mat.ambient[2]
                ),
                &self.theme,
            ));
            lines.push(kv_line(
                "  Diffuse",
                &format!(
                    "[{:.3}, {:.3}, {:.3}]",
                    mat.diffuse[0], mat.diffuse[1], mat.diffuse[2]
                ),
                &self.theme,
            ));
            lines.push(kv_line(
                "  Specular",
                &format!(
                    "[{:.3}, {:.3}, {:.3}]",
                    mat.specular[0], mat.specular[1], mat.specular[2]
                ),
                &self.theme,
            ));

            if let Some(shininess) = mat.shininess {
                lines.push(kv_line(
                    "  Shininess",
                    &format!("{:.3}", shininess),
                    &self.theme,
                ));
            }
            if let Some(dissolve) = mat.dissolve {
                lines.push(kv_line(
                    "  Dissolve (opacity)",
                    &format!("{:.3}", dissolve),
                    &self.theme,
                ));
            }
            if let Some(optical_density) = mat.optical_density {
                lines.push(kv_line(
                    "  Optical Density",
                    &format!("{:.3}", optical_density),
                    &self.theme,
                ));
            }

            lines.push(kv_line_label_only("  Textures", &self.theme));
            if mat.textures.is_empty() {
                lines.push(Line::from(Span::styled(
                    "    None",
                    Style::default().fg(self.theme.label),
                )));
            } else {
                for tex in &mat.textures {
                    let style = if tex.exists {
                        Style::default().fg(self.theme.text)
                    } else {
                        Style::default().fg(self.theme.error)
                    };
                    let missing = if tex.exists { "" } else { " [MISSING]" };
                    lines.push(Line::from(vec![
                        Span::raw("    "),
                        Span::styled(
                            format!("{}:", tex.slot),
                            Style::default()
                                .fg(self.theme.success)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(" "),
                        Span::styled(format!("'{}'", tex.path), style),
                        Span::styled(missing.to_string(), Style::default().fg(self.theme.error)),
                    ]));
                }
            }

            if i < self.report.materials.len() - 1 {
                lines.push(Line::from(""));
            }
        }

        Text::from(lines)
    }

    fn format_validation(&self) -> Text<'static> {
        let mut lines = Vec::new();

        lines.push(section_header("VALIDATION", &self.theme));
        lines.push(Line::from(""));

        if self.report.validation.is_clean() {
            lines.push(Line::from(Span::styled(
                format!("{} No issues found", self.glyphs.check),
                Style::default()
                    .fg(self.theme.success)
                    .add_modifier(Modifier::BOLD),
            )));
            return Text::from(lines);
        }

        let errors = self.report.validation.error_count();
        let warnings = self.report.validation.warning_count();
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} error(s)", errors),
                Style::default().fg(if errors > 0 {
                    self.theme.error
                } else {
                    self.theme.success
                }),
            ),
            Span::raw(", "),
            Span::styled(
                format!("{} warning(s)", warnings),
                Style::default().fg(if warnings > 0 {
                    self.theme.warning
                } else {
                    self.theme.success
                }),
            ),
        ]));
        lines.push(Line::from(""));

        for issue in &self.report.validation.issues {
            let (tag, color) = match issue.severity {
                Severity::Error => ("[ERROR]", self.theme.error),
                Severity::Warning => ("[WARN]", self.theme.warning),
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{} ", tag),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}", issue.scope),
                    Style::default().fg(self.theme.text),
                ),
                Span::raw(": "),
                Span::styled(issue.message.clone(), Style::default().fg(color)),
            ]));
        }

        Text::from(lines)
    }
}

fn kv_line_label_only(label: &str, theme: &TuiTheme) -> Line<'static> {
    Line::from(Span::styled(
        format!("{}:", label),
        Style::default()
            .fg(theme.success)
            .add_modifier(Modifier::BOLD),
    ))
}

/// Severity never rides colour alone: every chip carries a glyph and a
/// word, so it still reads at the monochrome tier.
fn validation_status_line(
    report: &AnalysisReport,
    theme: &TuiTheme,
    glyphs: &Glyphs,
) -> Line<'static> {
    let v = &report.validation;
    if v.is_clean() {
        Line::from(Span::styled(
            format!(" {} Clean ", glyphs.check),
            Style::default()
                .fg(theme.success)
                .add_modifier(Modifier::BOLD),
        ))
    } else {
        let mut spans = Vec::new();
        let errors = v.error_count();
        let warnings = v.warning_count();
        if errors > 0 {
            spans.push(Span::styled(
                format!(
                    " {} {} error{} ",
                    glyphs.cross,
                    errors,
                    if errors == 1 { "" } else { "s" }
                ),
                Style::default()
                    .fg(theme.error)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        if warnings > 0 {
            spans.push(Span::styled(
                format!(
                    " {} {} warning{} ",
                    glyphs.warn,
                    warnings,
                    if warnings == 1 { "" } else { "s" }
                ),
                Style::default()
                    .fg(theme.warning)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        Line::from(spans)
    }
}

/// The shipped analyze surface, driven by the shared terminal loop.
///
/// Routing this shell through the driver rather than leaving it on its own
/// blocking read is what actually fixes the defect: a resize now reflows when
/// it happens instead of waiting for the next keystroke, and the terminal
/// comes back usable after a panic.
impl Surface for TerminalApp {
    fn draw(&mut self, frame: &mut Frame) {
        TerminalApp::draw(self, frame);
    }

    fn handle(&mut self, input: Input) -> Flow {
        match input {
            Input::Key(key) => self.handle_key_event(key),
            // A resize needs no state change: the next draw measures the new
            // frame and re-clamps the scroll against it. The loop having
            // noticed at all is the whole fix.
            Input::Resize(..) | Input::Tick => {}
            Input::Mouse(mouse) => {
                let tab = self.active_tab.index();
                let extent = self.extents[tab];
                match mouse.kind {
                    MouseEventKind::ScrollDown => self.scrolls[tab].down(3, extent),
                    MouseEventKind::ScrollUp => self.scrolls[tab].up(3),
                    _ => {}
                }
            }
        }
        if self.exit {
            Flow::Quit
        } else {
            Flow::Continue
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use solarxy_core::report::AnalysisReport;
    use solarxy_core::theme::Palette;
    use solarxy_core::validation::ValidationReport;

    use super::*;

    fn report() -> AnalysisReport {
        AnalysisReport {
            model_name: "frog".to_string(),
            mesh_count: 3,
            material_count: 1,
            total_vertices: 12,
            total_indices: 36,
            total_triangles: 12,
            bounds: None,
            meshes: Vec::new(),
            materials: Vec::new(),
            validation: ValidationReport::default(),
            source_format: "obj".to_owned(),
            file_size_bytes: None,
            asset_category: None,
            triangle_budget: None,
        }
    }

    use super::super::tui::caps::{ColorTier, GlyphTier};

    /// The tier that describes the surface as it shipped before the
    /// capability model existed. Every invariant below that was written
    /// against that surface is asserted here, unchanged.
    const TIER1: Capabilities = Capabilities {
        color: ColorTier::Ansi16,
        glyphs: GlyphTier::Unicode,
    };

    /// Render the real widget tree and read the cells back.
    ///
    /// The TUI needs a tty, so this is the only way to assert what it
    /// actually paints rather than what we believe it paints. Capabilities
    /// are always explicit: reading the ambient environment here would make
    /// every assertion depend on the machine running the tests.
    fn render_with(caps: Capabilities, theme: Option<TuiTheme>) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("test terminal");
        let mut app =
            TerminalApp::with_capabilities_and_report(report(), "frog.obj".to_string(), caps);
        if let Some(theme) = theme {
            app = app.with_theme(theme);
        }
        terminal.draw(|f| app.draw(f)).expect("draw");
        terminal.backend().buffer().clone()
    }

    fn render_at(caps: Capabilities) -> ratatui::buffer::Buffer {
        render_with(caps, None)
    }

    fn fixture() -> TerminalApp {
        TerminalApp::with_capabilities_and_report(report(), "frog.obj".to_string(), TIER1)
    }

    /// Draw one frame, which is what populates the extent a keystroke is
    /// clamped against. The real loop draws before it reads, so a test that
    /// presses a key without drawing first is testing a state the shell never
    /// reaches.
    fn draw_at(app: &mut TerminalApp, width: u16, height: u16) {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        terminal.draw(|frame| app.draw(frame)).expect("draw");
    }

    fn send(app: &mut TerminalApp, codes: &[KeyCode]) {
        crate::tui::harness::press(|event| app.handle_key_event(event), codes);
    }

    fn render(theme: TuiTheme) -> ratatui::buffer::Buffer {
        render_with(TIER1, Some(theme))
    }

    /// Nothing the TUI paints may be a colour we invented for ink. Every
    /// foreground is either the terminal's own (Reset / a named ANSI slot)
    /// or a semantic hue from the shared palette.
    #[test]
    fn paints_only_terminal_ink_or_palette_hues() {
        let buffer = render(TuiTheme::resolve());
        let allowed = [
            ratatui::style::Color::Reset,
            ratatui::style::Color::DarkGray,
        ];
        let t = TuiTheme::resolve();
        let hues = [t.accent, t.success, t.warning, t.error];
        for cell in buffer.content() {
            assert!(
                allowed.contains(&cell.fg) || hues.contains(&cell.fg),
                "painted {:?}, which is neither terminal ink nor a palette hue",
                cell.fg
            );
        }
    }

    /// The regression the maintainer caught in a real terminal: selecting
    /// the GUI's light theme painted near-black ink into a dark terminal, so
    /// the report was invisible. No RGB ink may reach the screen at all.
    #[test]
    fn no_rgb_ink_reaches_the_screen() {
        let buffer = render(TuiTheme::from_palette(&Palette::light()));
        let light = Palette::light();
        for cell in buffer.content() {
            for role in [
                light.roles.ink_primary,
                light.roles.ink_strong,
                light.roles.ink_tertiary,
            ] {
                let ink = ratatui::style::Color::Rgb(role.rgb.r, role.rgb.g, role.rgb.b);
                assert_ne!(
                    cell.fg, ink,
                    "a light palette's ink was painted into a terminal"
                );
            }
        }
    }

    /// Body text must be the terminal's own foreground, which is the only
    /// value legible in every colour scheme.
    #[test]
    fn body_text_is_terminal_ink() {
        let buffer = render(TuiTheme::resolve());
        let resets = buffer
            .content()
            .iter()
            .filter(|c| c.fg == ratatui::style::Color::Reset && !c.symbol().trim().is_empty())
            .count();
        assert!(
            resets > 20,
            "expected the bulk of the report to be terminal ink, saw {resets}"
        );
    }

    /// At this tier and below, the TUI must never paint a background: the
    /// terminal's own ground shows through, which is what keeps RGB
    /// foregrounds legible whatever the user's terminal theme is. The
    /// richer tiers do own a ground, so this is deliberately tier-scoped.
    #[test]
    fn never_paints_a_background() {
        let buffer = render(TuiTheme::resolve());
        for cell in buffer.content() {
            assert_eq!(
                cell.bg,
                ratatui::style::Color::Reset,
                "the TUI painted a background, which fights the user's terminal"
            );
        }
    }

    fn every_pair() -> Vec<Capabilities> {
        let mut pairs = Vec::new();
        for color in [
            ColorTier::Mono,
            ColorTier::Ansi16,
            ColorTier::Ansi256,
            ColorTier::TrueColor,
        ] {
            for glyphs in [GlyphTier::Unicode, GlyphTier::Ascii] {
                pairs.push(Capabilities { color, glyphs });
            }
        }
        pairs
    }

    /// Every combination draws something. A tier that panics or paints an
    /// empty frame is worse than one that paints plainly.
    #[test]
    fn every_tier_and_glyph_pair_renders() {
        for caps in every_pair() {
            let buffer = render_at(caps);
            let painted = buffer
                .content()
                .iter()
                .filter(|cell| !cell.symbol().trim().is_empty())
                .count();
            assert!(painted > 50, "{caps:?} painted only {painted} cells");
        }
    }

    /// Tier 0 is for terminals that have no colour to give. Nothing may
    /// reach a cell but the terminal's own ink, which is why every severity
    /// also carries a glyph and a word.
    #[test]
    fn the_monochrome_tier_paints_no_colour() {
        for glyphs in [GlyphTier::Unicode, GlyphTier::Ascii] {
            let buffer = render_at(Capabilities {
                color: ColorTier::Mono,
                glyphs,
            });
            for cell in buffer.content() {
                assert_eq!(
                    cell.fg,
                    ratatui::style::Color::Reset,
                    "monochrome painted {:?} at {glyphs:?}",
                    cell.fg
                );
                assert_eq!(cell.bg, ratatui::style::Color::Reset);
            }
        }
    }

    /// Lose detail, keep meaning. At the ASCII tier nothing outside the
    /// repertoire may reach the screen, whatever the colour tier is doing.
    #[test]
    fn the_ascii_tier_paints_only_ascii() {
        for color in [
            ColorTier::Mono,
            ColorTier::Ansi16,
            ColorTier::Ansi256,
            ColorTier::TrueColor,
        ] {
            let buffer = render_at(Capabilities {
                color,
                glyphs: GlyphTier::Ascii,
            });
            for cell in buffer.content() {
                assert!(
                    cell.symbol().is_ascii(),
                    "{:?} reached the screen at the ASCII tier under {color:?}",
                    cell.symbol()
                );
            }
        }
    }

    /// The 256 tier renders exact palette entries rather than leaving the
    /// terminal to approximate an RGB triple it cannot show.
    #[test]
    fn the_256_tier_paints_indexed_colour_and_never_raw_rgb() {
        let buffer = render_at(Capabilities {
            color: ColorTier::Ansi256,
            glyphs: GlyphTier::Unicode,
        });
        let mut saw_indexed = false;
        for cell in buffer.content() {
            assert!(
                !matches!(cell.fg, ratatui::style::Color::Rgb(..)),
                "raw RGB reached a 256-colour terminal"
            );
            saw_indexed |= matches!(cell.fg, ratatui::style::Color::Indexed(_));
        }
        assert!(saw_indexed, "nothing was quantised at all");
    }

    /// The accent is the one hue the shell paints unconditionally, so it is
    /// the cheapest proof that a tier reached the screen at all.
    #[test]
    fn the_richer_tiers_still_carry_the_accent() {
        let truecolor = render_at(Capabilities {
            color: ColorTier::TrueColor,
            glyphs: GlyphTier::Unicode,
        });
        let accent = TuiTheme::resolve().accent;
        assert!(
            truecolor.content().iter().any(|cell| cell.fg == accent),
            "the authored accent never reached a truecolor terminal"
        );
    }

    // The bindings below pin what this shell does today, so the keymap that
    // replaces it is measured against shipped behaviour rather than against
    // memory. Twelve bindings exist and six are advertised, which is the
    // whole reason a generated footer is being built.

    #[test]
    fn the_tab_bindings_dispatch() {
        let mut app = fixture();
        send(&mut app, &[KeyCode::Tab]);
        assert_eq!(app.active_tab, Tab::Meshes);
        send(&mut app, &[KeyCode::BackTab]);
        assert_eq!(app.active_tab, Tab::Overview);
        send(&mut app, &[KeyCode::BackTab]);
        assert_eq!(app.active_tab, Tab::Validation, "the cycle wraps backwards");

        for (code, expected) in [
            (KeyCode::Char('1'), Tab::Overview),
            (KeyCode::Char('2'), Tab::Meshes),
            (KeyCode::Char('3'), Tab::Materials),
            (KeyCode::Char('4'), Tab::Validation),
        ] {
            send(&mut app, &[code]);
            assert_eq!(app.active_tab, expected, "{code:?}");
        }
    }

    #[test]
    fn the_scroll_bindings_dispatch() {
        let mut app = fixture();
        draw_at(&mut app, 60, 8);
        let extent = app.extents[0];
        assert!(
            extent.overflows(),
            "the fixture has to be taller than the viewport to scroll at all"
        );

        for down in [KeyCode::Char('j'), KeyCode::Down] {
            let mut app = fixture();
            draw_at(&mut app, 60, 8);
            send(&mut app, &[down]);
            assert_eq!(app.scrolls[0].offset(extent), 1, "{down:?}");
        }
        for up in [KeyCode::Char('k'), KeyCode::Up] {
            let mut app = fixture();
            draw_at(&mut app, 60, 8);
            send(&mut app, &[KeyCode::Char('j'), KeyCode::Char('j'), up]);
            assert_eq!(app.scrolls[0].offset(extent), 1, "{up:?}");
        }

        send(&mut app, &[KeyCode::PageDown]);
        assert!(app.scrolls[0].offset(extent) > 1, "PageDown moved nothing");
        send(&mut app, &[KeyCode::PageUp]);
        assert_eq!(app.scrolls[0].offset(extent), 0);

        send(&mut app, &[KeyCode::Char('G')]);
        assert_eq!(app.scrolls[0].offset(extent), extent.max_offset());
        send(&mut app, &[KeyCode::Char('g')]);
        assert_eq!(app.scrolls[0].offset(extent), 0);
    }

    /// Each tab keeps its own position, so switching away and back returns to
    /// where the reader was rather than to the top.
    #[test]
    fn a_scroll_position_is_per_tab() {
        let mut app = fixture();
        draw_at(&mut app, 60, 8);
        send(&mut app, &[KeyCode::Char('j'), KeyCode::Char('j')]);
        let overview = app.extents[0];
        assert_eq!(app.scrolls[0].offset(overview), 2);

        send(&mut app, &[KeyCode::Char('2')]);
        draw_at(&mut app, 60, 8);
        assert_eq!(app.scrolls[1].offset(app.extents[1]), 0, "a fresh tab");

        send(&mut app, &[KeyCode::Char('1')]);
        assert_eq!(app.scrolls[0].offset(overview), 2, "the position survived");
    }

    /// The shipped shell quits on Esc as well as on `q`. Pinned because the
    /// tiled workspace gives Esc a meaning of its own, so this is behaviour
    /// that changes rather than behaviour that persists.
    #[test]
    fn both_quit_keys_dispatch() {
        for code in [KeyCode::Char('q'), KeyCode::Esc] {
            let mut app = fixture();
            assert!(!app.exit);
            send(&mut app, &[code]);
            assert!(app.exit, "{code:?} did not quit");
        }
    }

    /// The JSON export is bound to a shift-only key that appears nowhere in
    /// the footer, which is the defect the generated help exists to close.
    #[test]
    fn the_export_bindings_open_their_buffers() {
        let mut app = fixture();
        send(&mut app, &[KeyCode::Char('e')]);
        assert_eq!(app.export_input.as_deref(), Some("frog_report.txt"));
        assert!(app.export_json_input.is_none());

        let mut app = fixture();
        send(&mut app, &[KeyCode::Char('J')]);
        assert_eq!(app.export_json_input.as_deref(), Some("frog.json"));
        assert!(app.export_input.is_none());
    }

    /// A text buffer takes the whole keyboard while it is open. Without that,
    /// typing a path containing `q` would quit and a path containing a digit
    /// would switch tabs underneath the prompt.
    #[test]
    fn text_entry_swallows_the_global_keys() {
        for opener in [KeyCode::Char('e'), KeyCode::Char('J')] {
            let mut app = fixture();
            send(&mut app, &[opener]);
            let before = app.active_tab;

            send(
                &mut app,
                &[
                    KeyCode::Backspace,
                    KeyCode::Char('q'),
                    KeyCode::Char('2'),
                    KeyCode::Char('j'),
                ],
            );

            let buffer = app
                .export_input
                .as_deref()
                .or(app.export_json_input.as_deref())
                .expect("the buffer stayed open");
            assert!(buffer.ends_with("q2j"), "{opener:?} lost the typed text");
            assert!(!app.exit, "{opener:?} quit while a path was being typed");
            assert_eq!(app.active_tab, before, "{opener:?} switched tab");
        }
    }

    /// Esc inside a text buffer cancels the entry and nothing more. It is the
    /// same key that quits at the top level, so the precedence is the whole
    /// behaviour.
    #[test]
    fn escape_cancels_text_entry_without_quitting() {
        for opener in [KeyCode::Char('e'), KeyCode::Char('J')] {
            let mut app = fixture();
            send(&mut app, &[opener, KeyCode::Esc]);
            assert!(app.export_input.is_none(), "{opener:?}");
            assert!(app.export_json_input.is_none(), "{opener:?}");
            assert!(!app.exit, "{opener:?} quit instead of cancelling");
        }
    }

    /// The loop stops when the surface says so and not before, which is what
    /// makes quitting a property of the surface rather than of the loop.
    #[test]
    fn the_surface_asks_the_loop_to_stop_only_after_a_quit_key() {
        let mut app = fixture();
        assert_eq!(
            app.handle(Input::Key(crate::tui::harness::key(KeyCode::Char('j')))),
            Flow::Continue
        );
        assert_eq!(
            app.handle(Input::Key(crate::tui::harness::key(KeyCode::Char('q')))),
            Flow::Quit
        );
    }

    /// A resize changes no state on purpose: the next draw measures the new
    /// frame and re-clamps against it. What was missing before was anything
    /// awake to notice the resize at all.
    #[test]
    fn a_resize_and_a_tick_change_nothing_and_do_not_stop_the_loop() {
        let mut app = fixture();
        draw_at(&mut app, 60, 8);
        let before = app.scrolls[0].offset(app.extents[0]);

        assert_eq!(app.handle(Input::Resize(80, 24)), Flow::Continue);
        assert_eq!(app.handle(Input::Tick), Flow::Continue);
        assert_eq!(app.scrolls[0].offset(app.extents[0]), before);
        assert!(!app.exit);
    }

    /// The wheel is the one mouse action this shell has a use for, and it goes
    /// through the same clamped scroll model the keys do.
    #[test]
    fn the_wheel_scrolls_within_the_measured_extent() {
        let wheel = |kind| {
            Input::Mouse(crossterm::event::MouseEvent {
                kind,
                column: 1,
                row: 1,
                modifiers: crossterm::event::KeyModifiers::NONE,
            })
        };

        let mut app = fixture();
        draw_at(&mut app, 60, 8);
        let extent = app.extents[0];

        app.handle(wheel(MouseEventKind::ScrollDown));
        assert_eq!(app.scrolls[0].offset(extent), 3);
        app.handle(wheel(MouseEventKind::ScrollUp));
        assert_eq!(app.scrolls[0].offset(extent), 0);

        // Clamped by the same model the keyboard uses, so the wheel cannot
        // run past the end where `j` could not.
        for _ in 0..50 {
            app.handle(wheel(MouseEventKind::ScrollDown));
        }
        assert_eq!(app.scrolls[0].offset(extent), extent.max_offset());
    }

    /// The scroll defect, at the shell rather than at the model: a body whose
    /// lines wrap is taller than its line count, and the shell has to measure
    /// the taller figure or the reader cannot reach the end of it.
    #[test]
    fn the_shell_measures_rendered_rows_not_logical_lines() {
        let mut wordy = report();
        wordy.model_name = "a-delivered-asset-with-a-very-long-file-name.obj".to_owned();
        let mut app =
            TerminalApp::with_capabilities_and_report(wordy, "frog.obj".to_string(), TIER1);

        draw_at(&mut app, 40, 10);
        let logical = app.format_tab_content().lines.len() as u16;
        let extent = app.extents[0];
        assert!(
            extent.rendered_rows > logical,
            "nothing wrapped at 40 columns, so this proves nothing: \
             {} rendered rows against {logical} lines",
            extent.rendered_rows
        );

        send(&mut app, &[KeyCode::Char('G')]);
        assert!(
            app.scrolls[0].offset(extent) > logical.saturating_sub(extent.viewport_rows),
            "jump-to-bottom stopped at the line count instead of the true end"
        );
    }
}
