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

use std::io;

use crate::calc::json::report_to_json;
use crate::calc::report::{AnalysisReport, Severity};

use super::tui::{kv_line, section_header};

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

fn kv_line_label_only(label: &str) -> Line<'static> {
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
