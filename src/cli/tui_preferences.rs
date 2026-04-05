use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span, Text},
    widgets::{Block, Paragraph},
    DefaultTerminal, Frame,
};
use solarxy::preferences::{
    self, BackgroundMode, IblMode, LineWeight, NormalsMode, Preferences, ProjectionMode, ToneMode,
    UvMode, ViewMode, MAX_WINDOW_HEIGHT, MAX_WINDOW_WIDTH, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH,
};

use std::io;

const PREF_FIELD_COUNT: usize = 18;

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
            "Tone Mode",
            "Exposure",
            "Wireframe Line Weight",
            "MSAA Sample Count",
            "Lighting Lock",
            "Window Width",
            "Window Height",
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
            format!("{}", self.preferences.display.tone_mode),
            format!("{:.1}", self.preferences.display.exposure),
            format!("{}", self.preferences.rendering.wireframe_line_weight),
            format!("{}", self.preferences.rendering.msaa_sample_count),
            format!("{}", self.preferences.lighting.lock),
            format!("{}", self.preferences.window.window_width),
            format!("{}", self.preferences.window.window_height),
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
            format!("{}", self.original.display.tone_mode),
            format!("{:.1}", self.original.display.exposure),
            format!("{}", self.original.rendering.wireframe_line_weight),
            format!("{}", self.original.rendering.msaa_sample_count),
            format!("{}", self.original.lighting.lock),
            format!("{}", self.original.window.window_width),
            format!("{}", self.original.window.window_height),
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
                self.preferences.display.tone_mode = if forward {
                    self.preferences.display.tone_mode.next()
                } else {
                    cycle_back_tone_mode(self.preferences.display.tone_mode)
                };
            }
            12 => {
                let step = if forward { 0.5 } else { -0.5 };
                self.preferences.display.exposure =
                    (self.preferences.display.exposure + step).clamp(0.1, 10.0);
            }
            13 => {
                self.preferences.rendering.wireframe_line_weight = if forward {
                    self.preferences.rendering.wireframe_line_weight.next()
                } else {
                    cycle_back_line_weight(self.preferences.rendering.wireframe_line_weight)
                };
            }
            14 => {
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
            15 => self.preferences.lighting.lock = !self.preferences.lighting.lock,
            16 => {
                let step: i32 = if forward { 160 } else { -160 };
                let new_val = (self.preferences.window.window_width as i32 + step)
                    .clamp(MIN_WINDOW_WIDTH as i32, MAX_WINDOW_WIDTH as i32)
                    as u32;
                self.preferences.window.window_width = new_val;
            }
            17 => {
                let step: i32 = if forward { 160 } else { -160 };
                let new_val = (self.preferences.window.window_height as i32 + step)
                    .clamp(MIN_WINDOW_HEIGHT as i32, MAX_WINDOW_HEIGHT as i32)
                    as u32;
                self.preferences.window.window_height = new_val;
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

fn cycle_back_tone_mode(mode: ToneMode) -> ToneMode {
    match mode {
        ToneMode::None => ToneMode::AcesFilmic,
        ToneMode::Linear => ToneMode::None,
        ToneMode::Reinhard => ToneMode::Linear,
        ToneMode::AcesFilmic => ToneMode::Reinhard,
    }
}
