use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
};

use super::tui::{kv_line, section_header};

pub struct AppInfo {
    pub version: &'static str,
    pub description: &'static str,
    pub repository: &'static str,
    pub license: &'static str,
}

fn blank() -> Line<'static> {
    Line::raw("")
}

fn shortcut_line(key: &str, description: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!("{:<14}", key),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(description.to_string(), Style::default().fg(Color::White)),
    ])
}

fn prose(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::default().fg(Color::White),
    ))
}

fn settings_row(name: &str, default: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!("{:<28}", name),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(default.to_string(), Style::default().fg(Color::White)),
    ])
}

fn settings_table_header() -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {:<28}", "Setting"),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Default".to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

fn parse_help_text(text: &str) -> Text<'static> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    for line in text.lines() {
        if let Some(header) = line.strip_prefix("## ") {
            lines.push(section_header(header));
        } else if line.is_empty() {
            lines.push(blank());
        } else if line.starts_with('[') {
            if let Some(end) = line.find(']') {
                lines.push(shortcut_line(&line[1..end], line[end + 1..].trim()));
            } else {
                lines.push(prose(line));
            }
        } else if let Some(rest) = line.strip_prefix("::") {
            if let Some(sp) = rest.find(' ') {
                lines.push(kv_line(&rest[..sp], rest[sp + 1..].trim()));
            }
        } else if let Some(rest) = line.strip_prefix("==") {
            if rest.is_empty() {
                lines.push(settings_table_header());
            } else if let Some(sp) = rest.find(' ') {
                lines.push(settings_row(&rest[..sp], rest[sp + 1..].trim()));
            }
        } else {
            lines.push(prose(line));
        }
    }
    Text::from(lines)
}

pub fn about(info: &AppInfo) -> Text<'static> {
    Text::from(vec![
        blank(),
        section_header("SOLARXY"),
        blank(),
        kv_line("Version", info.version),
        prose(info.description),
        blank(),
        kv_line("Repository", info.repository),
        kv_line("License", info.license),
        kv_line("Contact", "https://koljam.com"),
        blank(),
        blank(),
        section_header("MODES"),
        blank(),
        shortcut_line(
            "view",
            "Real-time 3D model viewer with PBR rendering (default)",
        ),
        shortcut_line("analyze", "CLI/TUI model analysis and validation report"),
        shortcut_line("preferences", "Interactive settings editor (TUI)"),
        shortcut_line("docs", "This documentation viewer"),
        blank(),
        blank(),
        section_header("GETTING STARTED"),
        blank(),
        prose("  Open a model in the viewer:"),
        prose(""),
        prose("    solarxy -m model.obj"),
        blank(),
        prose("  Launch the viewer empty and drag a file onto the window:"),
        prose(""),
        prose("    solarxy"),
        blank(),
        prose("  Analyze a model:"),
        prose(""),
        prose("    solarxy -M analyze -m model.obj"),
        blank(),
        prose("  Export analysis as JSON:"),
        prose(""),
        prose("    solarxy -M analyze -m model.obj -f json"),
        blank(),
        prose("  Edit preferences:"),
        prose(""),
        prose("    solarxy -M preferences"),
        blank(),
        prose("  Open this documentation:"),
        prose(""),
        prose("    solarxy -M docs"),
        blank(),
        blank(),
        section_header("CLI OPTIONS"),
        blank(),
        shortcut_line("-m, --model", "Path to model file (optional in view mode)"),
        shortcut_line(
            "-M, --mode",
            "Operation mode: view, analyze, preferences, docs",
        ),
        shortcut_line(
            "-f, --format",
            "Output format: text or json (analyze mode only)",
        ),
        shortcut_line("-o, --output", "Write report to file (analyze mode only)"),
        shortcut_line("--about", "Print version and project info"),
        shortcut_line("--help", "Print help"),
        shortcut_line("--version", "Print version"),
        blank(),
    ])
}

pub fn view_mode() -> Text<'static> {
    parse_help_text(include_str!("../content/view_mode.txt"))
}

pub fn analyze_mode() -> Text<'static> {
    parse_help_text(include_str!("../content/analyze_mode.txt"))
}

pub fn preferences() -> Text<'static> {
    parse_help_text(include_str!("../content/preferences.txt"))
}

pub fn formats() -> Text<'static> {
    parse_help_text(include_str!("../content/formats.txt"))
}
