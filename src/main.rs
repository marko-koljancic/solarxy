use clap::Parser;
use crate::cli::parser::{Args, OperationMode};
use crate::calc::analyize::ModelAnalyzer;
use solarxy::{run_viewer};
use std::fs;

use std::io;

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

mod calc;
mod cli;

fn main() -> io::Result<()> {
    let args = Args::parse();
    println!("::: Solarxy starting :::");
    println!("Model path >>> {}", args.model_path.display());
    println!("Operation mode >>> {:?}", args.mode);

    let model_path_buff = fs::canonicalize(&args.model_path).expect("Failed to canonicalize the model path");
    let model_path = model_path_buff.to_string_lossy().to_string();

    match args.mode {
        OperationMode::View => {
            println!("Launching viewer for model at path: {}", model_path);
            run_viewer(model_path).unwrap();
            Ok(())
        }
        OperationMode::Analyze => {
            let analyzer = ModelAnalyzer::new(&model_path).expect("Failed to load model for analysis");
            let mut terminal = ratatui::init();
            let app_result = TerminalApp::new(analyzer.generate_report()).run(&mut terminal);
            ratatui::restore();
            app_result
        }
    }
}

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
