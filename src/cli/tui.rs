use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Margin},
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span, Text},
    widgets::{Block, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Tabs, Wrap},
    DefaultTerminal, Frame,
};
use solarxy::format_number;
use solarxy::preferences::{
    self, BackgroundMode, IblMode, LineWeight, NormalsMode, Preferences, ProjectionMode, UvMode,
    ViewMode, MAX_WINDOW_HEIGHT, MAX_WINDOW_WIDTH, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH,
};

use std::io;

use crate::calc::json::report_to_json;
use crate::calc::report::{AnalysisReport, Severity};

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
        }
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
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
            Span::styled("\u{2600}", Style::default().fg(Color::Yellow)),
            Span::raw(" "),
            Span::styled(
                "Solarxy",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled("Model Analysis", Style::default().fg(Color::White)),
            Span::raw(" "),
        ]);

        let tab_titles: Vec<Line> = Tab::ALL.iter().map(|t| Line::raw(t.title())).collect();

        let tabs_widget = Tabs::new(tab_titles)
            .block(
                Block::bordered()
                    .title(title.centered())
                    .border_set(border::ROUNDED)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .select(self.active_tab.index())
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )
            .style(Style::default().fg(Color::DarkGray))
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
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(path.clone(), Style::default().fg(Color::White)),
                Span::styled("\u{2588}", Style::default().fg(Color::Yellow)),
                Span::raw("  "),
                Span::styled(
                    "Enter",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Save  "),
                Span::styled(
                    "Esc",
                    Style::default()
                        .fg(Color::Yellow)
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
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(path.clone(), Style::default().fg(Color::White)),
                Span::styled("\u{2588}", Style::default().fg(Color::Yellow)),
                Span::raw("  "),
                Span::styled(
                    "Enter",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Save  "),
                Span::styled(
                    "Esc",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Cancel "),
            ])
        } else if let Some((ref msg, success)) = self.status_message {
            let color = if success { Color::Green } else { Color::Red };
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
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Switch  "),
                Span::styled(
                    "j/k",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Scroll  "),
                Span::styled(
                    "g/G",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Top/Bottom  "),
                Span::styled(
                    "e",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Export  "),
                Span::styled(
                    "J",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" JSON  "),
                Span::styled(
                    "q",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Quit "),
            ])
        };

        let content_block = Block::bordered()
            .title_bottom(instructions.left_aligned())
            .title_bottom(Line::from(position).centered())
            .title_bottom(validation_status_line(&self.report).right_aligned())
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(Color::Cyan));

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
        let json = report_to_json(&self.report);
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
            section_header("MODEL OVERVIEW"),
            Line::from(""),
            kv_line("Model Name", &self.report.model_name),
            kv_line("Mesh Count", &self.report.mesh_count.to_string()),
            kv_line("Material Count", &self.report.material_count.to_string()),
            kv_line("Total Vertices", &format_number(self.report.total_vertices)),
            kv_line("Total Indices", &format_number(self.report.total_indices)),
            kv_line(
                "Total Triangles",
                &format_number(self.report.total_triangles),
            ),
        ];

        if let Some(ref bounds) = self.report.bounds {
            lines.push(Line::from(""));
            lines.push(section_header("BOUNDING BOX"));
            lines.push(Line::from(""));
            lines.push(kv_line(
                "Min",
                &format!(
                    "[{:.3}, {:.3}, {:.3}]",
                    bounds.min[0], bounds.min[1], bounds.min[2]
                ),
            ));
            lines.push(kv_line(
                "Max",
                &format!(
                    "[{:.3}, {:.3}, {:.3}]",
                    bounds.max[0], bounds.max[1], bounds.max[2]
                ),
            ));
            lines.push(kv_line(
                "Size",
                &format!(
                    "[{:.3}, {:.3}, {:.3}]",
                    bounds.size[0], bounds.size[1], bounds.size[2]
                ),
            ));
            lines.push(kv_line(
                "Center",
                &format!(
                    "[{:.3}, {:.3}, {:.3}]",
                    bounds.center[0], bounds.center[1], bounds.center[2]
                ),
            ));
            lines.push(kv_line("Diagonal", &format!("{:.3}", bounds.diagonal)));
        }

        Text::from(lines)
    }

    fn format_meshes(&self) -> Text<'static> {
        let mut lines = Vec::new();

        if self.report.meshes.is_empty() {
            lines.push(Line::from(Span::styled(
                "No meshes found",
                Style::default().fg(Color::Gray),
            )));
            return Text::from(lines);
        }

        lines.push(section_header("MESH DETAILS"));
        lines.push(Line::from(""));

        for (i, mesh) in self.report.meshes.iter().enumerate() {
            lines.push(Line::from(Span::styled(
                format!("Mesh [{}]:", mesh.index),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(kv_line("  Vertices", &format_number(mesh.vertex_count)));
            lines.push(kv_line("  Indices", &format_number(mesh.index_count)));
            lines.push(kv_line("  Triangles", &format_number(mesh.triangle_count)));

            let normal_indicator = if mesh.normal_count == mesh.vertex_count {
                "\u{2713}"
            } else {
                "\u{26a0}"
            };
            lines.push(kv_line(
                "  Normals",
                &format!("{} {}", format_number(mesh.normal_count), normal_indicator),
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
            ));

            let mat_str = match (&mesh.material_name, mesh.material_id) {
                (Some(name), Some(id)) => format!("'{}' (ID: {})", name, id),
                (None, Some(id)) => format!("Invalid ID: {}", id),
                _ => "None".to_string(),
            };
            lines.push(kv_line("  Material", &mat_str));

            if i < self.report.meshes.len() - 1 {
                lines.push(Line::from(""));
            }
        }

        Text::from(lines)
    }

    fn format_materials(&self) -> Text<'static> {
        let mut lines = Vec::new();

        if self.report.materials.is_empty() {
            lines.push(section_header("MATERIALS"));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "No materials found (.mtl file not provided or empty)",
                Style::default().fg(Color::Gray),
            )));
            return Text::from(lines);
        }

        lines.push(section_header("MATERIAL DETAILS"));
        lines.push(Line::from(""));

        for (i, mat) in self.report.materials.iter().enumerate() {
            lines.push(Line::from(Span::styled(
                format!("Material [{}]: '{}'", mat.index, mat.name),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(kv_line(
                "  Ambient",
                &format!(
                    "[{:.3}, {:.3}, {:.3}]",
                    mat.ambient[0], mat.ambient[1], mat.ambient[2]
                ),
            ));
            lines.push(kv_line(
                "  Diffuse",
                &format!(
                    "[{:.3}, {:.3}, {:.3}]",
                    mat.diffuse[0], mat.diffuse[1], mat.diffuse[2]
                ),
            ));
            lines.push(kv_line(
                "  Specular",
                &format!(
                    "[{:.3}, {:.3}, {:.3}]",
                    mat.specular[0], mat.specular[1], mat.specular[2]
                ),
            ));

            if let Some(shininess) = mat.shininess {
                lines.push(kv_line("  Shininess", &format!("{:.3}", shininess)));
            }
            if let Some(dissolve) = mat.dissolve {
                lines.push(kv_line("  Dissolve (opacity)", &format!("{:.3}", dissolve)));
            }
            if let Some(optical_density) = mat.optical_density {
                lines.push(kv_line(
                    "  Optical Density",
                    &format!("{:.3}", optical_density),
                ));
            }

            lines.push(kv_line_label_only("  Textures"));
            if mat.textures.is_empty() {
                lines.push(Line::from(Span::styled(
                    "    None",
                    Style::default().fg(Color::Gray),
                )));
            } else {
                for tex in &mat.textures {
                    let style = if tex.exists {
                        Style::default().fg(Color::White)
                    } else {
                        Style::default().fg(Color::Red)
                    };
                    let missing = if tex.exists { "" } else { " [MISSING]" };
                    lines.push(Line::from(vec![
                        Span::raw("    "),
                        Span::styled(
                            format!("{}:", tex.slot),
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(" "),
                        Span::styled(format!("'{}'", tex.path), style),
                        Span::styled(missing.to_string(), Style::default().fg(Color::Red)),
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

        lines.push(section_header("VALIDATION"));
        lines.push(Line::from(""));

        if self.report.validation.is_clean() {
            lines.push(Line::from(Span::styled(
                "\u{2713} No issues found",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )));
            return Text::from(lines);
        }

        let errors = self.report.validation.error_count();
        let warnings = self.report.validation.warning_count();
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} error(s)", errors),
                Style::default().fg(if errors > 0 { Color::Red } else { Color::Green }),
            ),
            Span::raw(", "),
            Span::styled(
                format!("{} warning(s)", warnings),
                Style::default().fg(if warnings > 0 {
                    Color::Yellow
                } else {
                    Color::Green
                }),
            ),
        ]));
        lines.push(Line::from(""));

        for issue in &self.report.validation.issues {
            let (tag, color) = match issue.severity {
                Severity::Error => ("[ERROR]", Color::Red),
                Severity::Warning => ("[WARN]", Color::Yellow),
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{} ", tag),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}", issue.scope),
                    Style::default().fg(Color::White),
                ),
                Span::raw(": "),
                Span::styled(issue.message.clone(), Style::default().fg(color)),
            ]));
        }

        Text::from(lines)
    }
}

pub(crate) fn section_header(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ))
}

pub(crate) fn kv_line(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{}:", label),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(value.to_string(), Style::default().fg(Color::White)),
    ])
}

pub(crate) fn kv_line_label_only(label: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!("{}:", label),
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    ))
}

fn validation_status_line(report: &AnalysisReport) -> Line<'static> {
    let v = &report.validation;
    if v.is_clean() {
        Line::from(Span::styled(
            " \u{2713} Clean ".to_string(),
            Style::default()
                .fg(Color::Green)
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
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
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
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        Line::from(spans)
    }
}

const PREF_FIELD_COUNT: usize = 17;

pub struct PreferencesApp {
    exit: bool,
    preferences: Preferences,
    original: Preferences,
    selected_field: usize,
    status_message: Option<(String, bool)>,
}

impl PreferencesApp {
    pub fn new(preferences: Preferences) -> Self {
        let original = preferences.clone();
        Self {
            exit: false,
            preferences,
            original,
            selected_field: 0,
            status_message: None,
        }
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn is_dirty(&self) -> bool {
        self.preferences != self.original
    }

    fn draw(&self, frame: &mut Frame) {
        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(3),
            ])
            .split(area);

        let dirty_marker = if self.is_dirty() { " *" } else { "" };
        let title = Line::from(vec![
            Span::raw(" "),
            Span::styled("\u{2600}", Style::default().fg(Color::Yellow)),
            Span::raw(" "),
            Span::styled(
                "Solarxy",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled("Preferences", Style::default().fg(Color::White)),
            Span::styled(dirty_marker.to_string(), Style::default().fg(Color::Yellow)),
            Span::raw(" "),
        ]);
        let title_block = Block::bordered()
            .title(title.centered())
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(Color::Cyan));
        frame.render_widget(title_block, chunks[0]);

        let content = self.format_fields();
        let content_block = Block::bordered()
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(Color::DarkGray));
        let paragraph = Paragraph::new(content).block(content_block);
        frame.render_widget(paragraph, chunks[1]);

        let bottom_line = if let Some((ref msg, is_success)) = self.status_message {
            let color = if is_success { Color::Green } else { Color::Red };
            Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    msg.clone(),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
            ])
        } else {
            Line::from(vec![
                Span::raw(" "),
                Span::styled("\u{2191}\u{2193}", Style::default().fg(Color::Yellow)),
                Span::styled(" Navigate  ", Style::default().fg(Color::DarkGray)),
                Span::styled("Enter/Space", Style::default().fg(Color::Yellow)),
                Span::styled(" Toggle  ", Style::default().fg(Color::DarkGray)),
                Span::styled("s", Style::default().fg(Color::Yellow)),
                Span::styled(" Save  ", Style::default().fg(Color::DarkGray)),
                Span::styled("r", Style::default().fg(Color::Yellow)),
                Span::styled(" Reset  ", Style::default().fg(Color::DarkGray)),
                Span::styled("q", Style::default().fg(Color::Yellow)),
                Span::styled(" Quit", Style::default().fg(Color::DarkGray)),
            ])
        };
        let bottom_block = Block::bordered()
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(Color::DarkGray));
        let bottom = Paragraph::new(bottom_line).block(bottom_block);
        frame.render_widget(bottom, chunks[2]);
    }

    fn format_fields(&self) -> Text<'static> {
        let labels = [
            "Background",
            "View Mode",
            "Normals Mode",
            "Grid Visible",
            "Axis Gizmo Visible",
            "Bloom Enabled",
            "SSAO Enabled",
            "UV Mode",
            "Projection Mode",
            "Turntable Active",
            "IBL Mode",
            "Wireframe Line Weight",
            "MSAA Sample Count",
            "Lighting Lock",
            "Window Width",
            "Window Height",
            "Start Maximized",
        ];

        let values: [String; PREF_FIELD_COUNT] = [
            format!("{}", self.preferences.display.background),
            format!("{}", self.preferences.display.view_mode),
            format!("{}", self.preferences.display.normals_mode),
            format!("{}", self.preferences.display.grid_visible),
            format!("{}", self.preferences.display.axis_gizmo_visible),
            format!("{}", self.preferences.display.bloom_enabled),
            format!("{}", self.preferences.display.ssao_enabled),
            format!("{}", self.preferences.display.uv_mode),
            format!("{}", self.preferences.display.projection_mode),
            format!("{}", self.preferences.display.turntable_active),
            format!("{}", self.preferences.display.ibl_mode),
            format!("{}", self.preferences.rendering.wireframe_line_weight),
            format!("{}", self.preferences.rendering.msaa_sample_count),
            format!("{}", self.preferences.lighting.lock),
            format!("{}", self.preferences.window.window_width),
            format!("{}", self.preferences.window.window_height),
            format!("{}", self.preferences.window.start_maximized),
        ];

        let original_values: [String; PREF_FIELD_COUNT] = [
            format!("{}", self.original.display.background),
            format!("{}", self.original.display.view_mode),
            format!("{}", self.original.display.normals_mode),
            format!("{}", self.original.display.grid_visible),
            format!("{}", self.original.display.axis_gizmo_visible),
            format!("{}", self.original.display.bloom_enabled),
            format!("{}", self.original.display.ssao_enabled),
            format!("{}", self.original.display.uv_mode),
            format!("{}", self.original.display.projection_mode),
            format!("{}", self.original.display.turntable_active),
            format!("{}", self.original.display.ibl_mode),
            format!("{}", self.original.rendering.wireframe_line_weight),
            format!("{}", self.original.rendering.msaa_sample_count),
            format!("{}", self.original.lighting.lock),
            format!("{}", self.original.window.window_width),
            format!("{}", self.original.window.window_height),
            format!("{}", self.original.window.start_maximized),
        ];

        let mut lines = Vec::new();
        lines.push(Line::from(""));

        for i in 0..PREF_FIELD_COUNT {
            let is_selected = i == self.selected_field;
            let is_changed = values[i] != original_values[i];

            let marker = if is_selected { "\u{25b6} " } else { "  " };
            let value_color = if is_changed {
                Color::Yellow
            } else {
                Color::White
            };
            let label_style = if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            };

            let value_style = if is_selected {
                Style::default()
                    .fg(value_color)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(value_color)
            };

            lines.push(Line::from(vec![
                Span::styled(marker.to_string(), Style::default().fg(Color::Cyan)),
                Span::styled(format!("{:<24}", labels[i]), label_style),
                Span::styled(format!("  {}", values[i]), value_style),
            ]));
        }

        Text::from(lines)
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
        self.status_message = None;

        match key_event.code {
            KeyCode::Char('q') | KeyCode::Esc => self.exit = true,
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_field > 0 {
                    self.selected_field -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected_field < PREF_FIELD_COUNT - 1 {
                    self.selected_field += 1;
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Right | KeyCode::Char('l') => {
                self.cycle_field(true);
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.cycle_field(false);
            }
            KeyCode::Char('s') => self.save(),
            KeyCode::Char('r') => self.reset_to_defaults(),
            _ => {}
        }
    }

    fn cycle_field(&mut self, forward: bool) {
        match self.selected_field {
            0 => {
                self.preferences.display.background = if forward {
                    self.preferences.display.background.next()
                } else {
                    cycle_back_background(self.preferences.display.background)
                };
            }
            1 => {
                self.preferences.display.view_mode = if forward {
                    cycle_all_view_modes(self.preferences.display.view_mode)
                } else {
                    cycle_back_view_mode(self.preferences.display.view_mode)
                };
            }
            2 => {
                self.preferences.display.normals_mode = if forward {
                    self.preferences.display.normals_mode.next()
                } else {
                    cycle_back_normals(self.preferences.display.normals_mode)
                };
            }
            3 => self.preferences.display.grid_visible = !self.preferences.display.grid_visible,
            4 => {
                self.preferences.display.axis_gizmo_visible =
                    !self.preferences.display.axis_gizmo_visible
            }
            5 => self.preferences.display.bloom_enabled = !self.preferences.display.bloom_enabled,
            6 => self.preferences.display.ssao_enabled = !self.preferences.display.ssao_enabled,
            7 => {
                self.preferences.display.uv_mode = if forward {
                    self.preferences.display.uv_mode.next()
                } else {
                    cycle_back_uv_mode(self.preferences.display.uv_mode)
                };
            }
            8 => {
                self.preferences.display.projection_mode = if forward {
                    self.preferences.display.projection_mode.next()
                } else {
                    cycle_back_projection_mode(self.preferences.display.projection_mode)
                };
            }
            9 => {
                self.preferences.display.turntable_active =
                    !self.preferences.display.turntable_active
            }
            10 => {
                self.preferences.display.ibl_mode = if forward {
                    match self.preferences.display.ibl_mode {
                        IblMode::Off => IblMode::Diffuse,
                        IblMode::Diffuse => IblMode::Full,
                        IblMode::Full => IblMode::Off,
                    }
                } else {
                    match self.preferences.display.ibl_mode {
                        IblMode::Off => IblMode::Full,
                        IblMode::Diffuse => IblMode::Off,
                        IblMode::Full => IblMode::Diffuse,
                    }
                };
            }
            11 => {
                self.preferences.rendering.wireframe_line_weight = if forward {
                    self.preferences.rendering.wireframe_line_weight.next()
                } else {
                    cycle_back_line_weight(self.preferences.rendering.wireframe_line_weight)
                };
            }
            12 => {
                self.preferences.rendering.msaa_sample_count = if forward {
                    match self.preferences.rendering.msaa_sample_count {
                        1 => 2,
                        2 => 4,
                        _ => 1,
                    }
                } else {
                    match self.preferences.rendering.msaa_sample_count {
                        4 => 2,
                        2 => 1,
                        _ => 4,
                    }
                };
            }
            13 => self.preferences.lighting.lock = !self.preferences.lighting.lock,
            14 => {
                let step: i32 = if forward { 160 } else { -160 };
                let new_val = (self.preferences.window.window_width as i32 + step)
                    .clamp(MIN_WINDOW_WIDTH as i32, MAX_WINDOW_WIDTH as i32)
                    as u32;
                self.preferences.window.window_width = new_val;
            }
            15 => {
                let step: i32 = if forward { 160 } else { -160 };
                let new_val = (self.preferences.window.window_height as i32 + step)
                    .clamp(MIN_WINDOW_HEIGHT as i32, MAX_WINDOW_HEIGHT as i32)
                    as u32;
                self.preferences.window.window_height = new_val;
            }
            16 => {
                self.preferences.window.start_maximized = !self.preferences.window.start_maximized
            }
            _ => {}
        }
    }

    fn save(&mut self) {
        match preferences::save(&self.preferences) {
            Ok(()) => {
                self.original = self.preferences.clone();
                self.status_message = Some(("Preferences saved".to_string(), true));
            }
            Err(e) => {
                self.status_message = Some((format!("Save failed: {}", e), false));
            }
        }
    }

    fn reset_to_defaults(&mut self) {
        self.preferences = Preferences::default();
        self.status_message = Some(("Reset to defaults (unsaved)".to_string(), true));
    }
}

fn cycle_all_view_modes(mode: ViewMode) -> ViewMode {
    match mode {
        ViewMode::Shaded => ViewMode::ShadedWireframe,
        ViewMode::ShadedWireframe => ViewMode::WireframeOnly,
        ViewMode::WireframeOnly => ViewMode::Ghosted,
        ViewMode::Ghosted => ViewMode::Shaded,
    }
}

fn cycle_back_view_mode(mode: ViewMode) -> ViewMode {
    match mode {
        ViewMode::Shaded => ViewMode::Ghosted,
        ViewMode::ShadedWireframe => ViewMode::Shaded,
        ViewMode::WireframeOnly => ViewMode::ShadedWireframe,
        ViewMode::Ghosted => ViewMode::WireframeOnly,
    }
}

fn cycle_back_background(mode: BackgroundMode) -> BackgroundMode {
    match mode {
        BackgroundMode::White => BackgroundMode::Black,
        BackgroundMode::Gradient => BackgroundMode::White,
        BackgroundMode::DarkGray => BackgroundMode::Gradient,
        BackgroundMode::Black => BackgroundMode::DarkGray,
    }
}

fn cycle_back_normals(mode: NormalsMode) -> NormalsMode {
    match mode {
        NormalsMode::Off => NormalsMode::FaceAndVertex,
        NormalsMode::Face => NormalsMode::Off,
        NormalsMode::Vertex => NormalsMode::Face,
        NormalsMode::FaceAndVertex => NormalsMode::Vertex,
    }
}

fn cycle_back_line_weight(weight: LineWeight) -> LineWeight {
    match weight {
        LineWeight::Light => LineWeight::Bold,
        LineWeight::Medium => LineWeight::Light,
        LineWeight::Bold => LineWeight::Medium,
    }
}

fn cycle_back_uv_mode(mode: UvMode) -> UvMode {
    match mode {
        UvMode::Off => UvMode::Checker,
        UvMode::Gradient => UvMode::Off,
        UvMode::Checker => UvMode::Gradient,
    }
}

fn cycle_back_projection_mode(mode: ProjectionMode) -> ProjectionMode {
    match mode {
        ProjectionMode::Perspective => ProjectionMode::Orthographic,
        ProjectionMode::Orthographic => ProjectionMode::Perspective,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocsTab {
    About = 0,
    ViewMode = 1,
    AnalyzeMode = 2,
    Preferences = 3,
}

impl DocsTab {
    const ALL: [DocsTab; 4] = [
        DocsTab::About,
        DocsTab::ViewMode,
        DocsTab::AnalyzeMode,
        DocsTab::Preferences,
    ];

    fn title(self) -> &'static str {
        match self {
            DocsTab::About => "About",
            DocsTab::ViewMode => "View Mode",
            DocsTab::AnalyzeMode => "Analyze Mode",
            DocsTab::Preferences => "Preferences",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

pub struct DocsApp {
    exit: bool,
    active_tab: DocsTab,
    scroll_offsets: [u16; 4],
    content_heights: [u16; 4],
}

impl DocsApp {
    pub fn new() -> Self {
        Self {
            exit: false,
            active_tab: DocsTab::About,
            scroll_offsets: [0; 4],
            content_heights: [0; 4],
        }
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
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
            Span::styled("\u{2600}", Style::default().fg(Color::Yellow)),
            Span::raw(" "),
            Span::styled(
                "Solarxy",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled("Documentation", Style::default().fg(Color::White)),
            Span::raw(" "),
        ]);

        let tab_titles: Vec<Line> = DocsTab::ALL.iter().map(|t| Line::raw(t.title())).collect();

        let tabs_widget = Tabs::new(tab_titles)
            .block(
                Block::bordered()
                    .title(title.centered())
                    .border_set(border::ROUNDED)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .select(self.active_tab.index())
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )
            .style(Style::default().fg(Color::DarkGray))
            .divider(" \u{2502} ");
        frame.render_widget(tabs_widget, chunks[0]);

        let tab_idx = self.active_tab.index();
        let content_text = self.tab_content();
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

        let instructions = Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "Tab/1-4",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Switch  "),
            Span::styled(
                "j/k",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Scroll  "),
            Span::styled(
                "g/G",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Top/Bottom  "),
            Span::styled(
                "q",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Quit "),
        ]);

        let content_block = Block::bordered()
            .title_bottom(instructions.left_aligned())
            .title_bottom(Line::from(position).centered())
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(Color::Cyan));

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

    fn tab_content(&self) -> Text<'static> {
        match self.active_tab {
            DocsTab::About => super::help::about(),
            DocsTab::ViewMode => super::help::view_mode(),
            DocsTab::AnalyzeMode => super::help::analyze_mode(),
            DocsTab::Preferences => super::help::preferences(),
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
        let tab_idx = self.active_tab.index();
        match key_event.code {
            KeyCode::Char('q') | KeyCode::Esc => self.exit = true,
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
                let next = (tab_idx + 1) % DocsTab::ALL.len();
                self.active_tab = DocsTab::ALL[next];
            }
            KeyCode::BackTab => {
                let prev = (tab_idx + DocsTab::ALL.len() - 1) % DocsTab::ALL.len();
                self.active_tab = DocsTab::ALL[prev];
            }
            KeyCode::Char('1') => self.active_tab = DocsTab::About,
            KeyCode::Char('2') => self.active_tab = DocsTab::ViewMode,
            KeyCode::Char('3') => self.active_tab = DocsTab::AnalyzeMode,
            KeyCode::Char('4') => self.active_tab = DocsTab::Preferences,
            _ => {}
        }
    }
}
