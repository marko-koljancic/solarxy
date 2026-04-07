use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Margin},
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span, Text},
    widgets::{Block, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Tabs, Wrap},
    DefaultTerminal, Frame,
};

use std::io;

use super::help::{self, AppInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocsTab {
    About = 0,
    ViewMode = 1,
    AnalyzeMode = 2,
    Formats = 3,
    Preferences = 4,
}

impl DocsTab {
    const ALL: [DocsTab; 5] = [
        DocsTab::About,
        DocsTab::ViewMode,
        DocsTab::AnalyzeMode,
        DocsTab::Formats,
        DocsTab::Preferences,
    ];

    fn title(self) -> &'static str {
        match self {
            DocsTab::About => "About",
            DocsTab::ViewMode => "View Mode",
            DocsTab::AnalyzeMode => "Analyze Mode",
            DocsTab::Formats => "Formats",
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
    scroll_offsets: [u16; 5],
    content_heights: [u16; 5],
    app_info: AppInfo,
}

impl DocsApp {
    pub fn new(app_info: AppInfo) -> Self {
        Self {
            exit: false,
            active_tab: DocsTab::About,
            scroll_offsets: [0; 5],
            content_heights: [0; 5],
            app_info,
        }
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
                "Tab/1-5",
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
            DocsTab::About => help::about(&self.app_info),
            DocsTab::ViewMode => help::view_mode(),
            DocsTab::AnalyzeMode => help::analyze_mode(),
            DocsTab::Formats => help::formats(),
            DocsTab::Preferences => help::preferences(),
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
            KeyCode::Char('4') => self.active_tab = DocsTab::Formats,
            KeyCode::Char('5') => self.active_tab = DocsTab::Preferences,
            _ => {}
        }
    }
}
