use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    buffer::Buffer,
    layout::{Rect},
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span, Text},
    widgets::{Block, Paragraph, Widget, Wrap},
    DefaultTerminal, Frame,
};

use std::io;

#[derive(Debug, Default)]
pub struct TerminalApp {
    exit: bool,
    analysis_report: String,
}

impl TerminalApp {
    pub fn new(analysis_report: String) -> Self {
        Self {
            exit: false,
            analysis_report,
        }
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn draw(&self, frame: &mut Frame) {
        frame.render_widget(self, frame.area());
    }

    fn handle_events(&mut self) -> io::Result<()> {
        match event::read()? {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => self.handle_key_event(key_event),
            _ => {}
        };
        Ok(())
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Char('q') | KeyCode::Esc => self.exit(),
            _ => {}
        }
    }

    fn exit(&mut self) {
        self.exit = true;
    }

    fn format_report(&self) -> Text<'_> {
        let mut lines = Vec::new();

        for line in self.analysis_report.lines() {
            if line.trim().is_empty() {
                lines.push(Line::from(""));
                continue;
            }

            if line
                .trim()
                .chars()
                .all(|c| c.is_uppercase() || c.is_whitespace() || c.is_ascii_punctuation())
                && line.trim().len() > 3
                && !line.contains('[')
            {
                lines.push(Line::from(vec![Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                )]));
            } else if line.contains("Mesh [") || line.contains("Material [") {
                lines.push(Line::from(vec![Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                )]));
            } else if line.contains(':') {
                let parts: Vec<&str> = line.splitn(2, ':').collect();
                if parts.len() == 2 {
                    let indent = line.chars().take_while(|c| c.is_whitespace()).collect::<String>();
                    let label = parts[0].trim();
                    let value = parts[1].trim();

                    lines.push(Line::from(vec![
                        Span::raw(indent),
                        Span::styled(
                            format!("{}:", label),
                            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(" "),
                        Span::styled(value.to_string(), Style::default().fg(Color::White)),
                    ]));
                } else {
                    lines.push(Line::from(line.to_string()));
                }
            } else {
                lines.push(Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::Gray),
                )));
            }
        }

        Text::from(lines)
    }
}

impl Widget for &TerminalApp {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let title = Line::from(vec![
            Span::raw(" "),
            Span::styled("☀", Style::default().fg(Color::Yellow)),
            Span::raw(" "),
            Span::styled("Solarxy", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
            Span::styled("Model Analysis", Style::default().fg(Color::White)),
            Span::raw(" "),
        ]);

        let instructions = Line::from(vec![
            Span::raw(" Quit: "),
            Span::styled("<Q>", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" / "),
            Span::styled("<ESC>", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
        ]);

        let block = Block::bordered()
            .title(title.centered())
            .title_bottom(instructions.left_aligned())
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(Color::Cyan));

        let formatted_text = self.format_report();

        Paragraph::new(formatted_text)
            .block(block)
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }
}
