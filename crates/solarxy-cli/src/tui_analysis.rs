use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Margin},
    style::{Modifier, Style},
    symbols::border,
    text::{Line, Span, Text},
    widgets::{Block, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Tabs, Wrap},
    DefaultTerminal, Frame,
};
use solarxy_core::format_number;

use std::io;

use solarxy_core::json::report_to_json;
use solarxy_core::report::{AnalysisReport, Severity};

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
    scroll_offsets: [u16; 4],
    content_heights: [u16; 4],
    export_input: Option<String>,
    export_json_input: Option<String>,
    status_message: Option<(String, bool)>,
    /// The shared palette, resolved once at construction. This shell draws
    /// no color that does not come from here.
    theme: TuiTheme,
}

impl TerminalApp {
    pub fn new(report: AnalysisReport, model_path: String) -> Self {
        Self {
            exit: false,
            report,
            model_path,
            active_tab: Tab::Overview,
            scroll_offsets: [0; 4],
            content_heights: [0; 4],
            export_input: None,
            export_json_input: None,
            status_message: None,
            theme: TuiTheme::resolve(),
        }
    }

    /// Build with an explicit theme, bypassing the persisted preference.
    #[must_use]
    pub fn with_theme(mut self, theme: TuiTheme) -> Self {
        self.theme = theme;
        self
    }

    pub fn run(mut self) -> io::Result<()> {
        let mut terminal = ratatui::init();
        let result = self.run_inner(&mut terminal);
        ratatui::restore();
        result
    }

    fn run_inner(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(area);

        let title = Line::from(vec![
            Span::raw(" "),
            Span::styled("\u{2600}", Style::default().fg(self.theme.accent)),
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
                    .border_set(border::ROUNDED)
                    .border_style(Style::default().fg(self.theme.border)),
            )
            .select(self.active_tab.index())
            .highlight_style(
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )
            .style(Style::default().fg(self.theme.muted))
            .divider(" \u{2502} ");
        frame.render_widget(tabs_widget, chunks[0]);

        let tab_idx = self.active_tab.index();
        let content_text = self.format_tab_content();
        self.content_heights[tab_idx] = content_text.lines.len() as u16;

        let inner_height = chunks[1].height.saturating_sub(2);
        self.scroll_offsets[tab_idx] = self.scroll_offsets[tab_idx]
            .min(self.content_heights[tab_idx].saturating_sub(inner_height));

        let position = format!(
            " [{}/{}] ",
            self.scroll_offsets[tab_idx]
                .saturating_add(1)
                .min(self.content_heights[tab_idx]),
            self.content_heights[tab_idx]
        );

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
                Span::styled("\u{2588}", Style::default().fg(self.theme.accent)),
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
                Span::styled("\u{2588}", Style::default().fg(self.theme.accent)),
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
            .title_bottom(validation_status_line(&self.report, &self.theme).right_aligned())
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(self.theme.border));

        let paragraph = Paragraph::new(content_text)
            .block(content_block)
            .scroll((self.scroll_offsets[tab_idx], 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, chunks[1]);

        if self.content_heights[tab_idx] > inner_height {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("\u{2191}"))
                .end_symbol(Some("\u{2193}"));
            let mut scrollbar_state = ScrollbarState::new(self.content_heights[tab_idx] as usize)
                .position(self.scroll_offsets[tab_idx] as usize)
                .viewport_content_length(inner_height as usize);
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

    fn handle_events(&mut self) -> io::Result<()> {
        match event::read()? {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                self.handle_key_event(key_event)
            }
            _ => {}
        };
        Ok(())
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
        match key_event.code {
            KeyCode::Char('q') | KeyCode::Esc => self.exit = true,
            KeyCode::Char('e') => {
                self.export_input = Some(self.default_export_filename());
            }
            KeyCode::Char('J') => {
                self.export_json_input = Some(self.default_json_export_path());
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_offsets[tab_idx] = self.scroll_offsets[tab_idx].saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll_offsets[tab_idx] = self.scroll_offsets[tab_idx].saturating_add(1);
            }
            KeyCode::Char('g') => self.scroll_offsets[tab_idx] = 0,
            KeyCode::Char('G') => self.scroll_offsets[tab_idx] = u16::MAX,
            KeyCode::PageUp => {
                self.scroll_offsets[tab_idx] = self.scroll_offsets[tab_idx].saturating_sub(20);
            }
            KeyCode::PageDown => {
                self.scroll_offsets[tab_idx] = self.scroll_offsets[tab_idx].saturating_add(20);
            }
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
                "\u{2713}"
            } else {
                "\u{26a0}"
            };
            lines.push(kv_line(
                "  Normals",
                &format!("{} {}", format_number(mesh.normal_count), normal_indicator),
                &self.theme,
            ));

            let texcoord_indicator = if mesh.texcoord_count == mesh.vertex_count {
                "\u{2713}"
            } else if mesh.texcoord_count == 0 {
                "\u{2717}"
            } else {
                "\u{26a0}"
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
                "\u{2713} No issues found",
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

fn validation_status_line(report: &AnalysisReport, theme: &TuiTheme) -> Line<'static> {
    let v = &report.validation;
    if v.is_clean() {
        Line::from(Span::styled(
            " \u{2713} Clean ".to_string(),
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
                    " \u{2717} {} error{} ",
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
                    " \u{26a0} {} warning{} ",
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
        }
    }

    /// Render the real widget tree and read the cells back.
    ///
    /// The TUI needs a tty, so this is the only way to assert what it
    /// actually paints rather than what we believe it paints.
    fn render(theme: TuiTheme) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("test terminal");
        let mut app = TerminalApp::new(report(), "frog.obj".to_string()).with_theme(theme);
        terminal.draw(|f| app.draw(f)).expect("draw");
        terminal.backend().buffer().clone()
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

    /// The TUI must never paint a background: the terminal's own ground
    /// shows through, which is what keeps RGB foregrounds legible whatever
    /// the user's terminal theme is.
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
}
