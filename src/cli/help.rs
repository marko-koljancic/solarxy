use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
};

use super::tui::{kv_line, section_header};

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

pub fn about() -> Text<'static> {
    let version = env!("CARGO_PKG_VERSION");
    let description = env!("CARGO_PKG_DESCRIPTION");
    let repository = env!("CARGO_PKG_REPOSITORY");
    let license = env!("CARGO_PKG_LICENSE");

    Text::from(vec![
        blank(),
        section_header("SOLARXY"),
        blank(),
        kv_line("Version", version),
        prose(description),
        blank(),
        kv_line("Repository", repository),
        kv_line("License", license),
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
    Text::from(vec![
        blank(),
        section_header("VIEW MODE"),
        blank(),
        prose("Real-time 3D model viewer with Cook-Torrance PBR rendering,"),
        prose("shadow mapping, normal mapping, and a 3-light system."),
        blank(),
        prose("Launch with:  solarxy -m model.obj"),
        prose("The model path is optional — you can also drop a file onto the window."),
        blank(),
        blank(),
        section_header("DISPLAY"),
        blank(),
        shortcut_line(
            "W",
            "Cycle view mode (Shaded > Shaded+Wire > Wireframe > Shaded)",
        ),
        shortcut_line("S", "Switch to Shaded mode"),
        shortcut_line("X", "Toggle Ghosted mode"),
        shortcut_line(
            "Shift+W",
            "Cycle wireframe line weight (Light > Medium > Bold)",
        ),
        shortcut_line(
            "N",
            "Cycle normals display (Off > Face > Vertex > Face+Vertex)",
        ),
        shortcut_line("U", "Cycle UV overlay (Off > Gradient > Checker)"),
        shortcut_line(
            "B",
            "Cycle background (White > Gradient > Dark Gray > Black)",
        ),
        shortcut_line(
            "Shift+B",
            "Cycle bounding box display (Off > Model > Per Mesh)",
        ),
        shortcut_line("Shift+M", "Toggle bloom"),
        shortcut_line("G", "Toggle grid"),
        shortcut_line("A", "Toggle axis gizmo"),
        shortcut_line("V", "Toggle turntable auto-rotation"),
        blank(),
        blank(),
        section_header("CAMERA"),
        blank(),
        shortcut_line("P", "Perspective projection"),
        shortcut_line("O", "Orthographic projection"),
        shortcut_line("H", "Home — frame model in view"),
        shortcut_line("T", "Top view"),
        shortcut_line("F", "Front view"),
        shortcut_line("L", "Left view"),
        shortcut_line("R", "Right view"),
        shortcut_line("Arrow keys", "Camera movement"),
        blank(),
        blank(),
        section_header("OTHER"),
        blank(),
        shortcut_line("C", "Capture screenshot (PNG)"),
        shortcut_line("Shift+S", "Save current settings as preferences"),
        shortcut_line("Shift+L", "Toggle lights lock (lights follow camera)"),
        shortcut_line("?", "Toggle keyboard hints overlay"),
        shortcut_line("Esc", "Exit viewer"),
        blank(),
        blank(),
        section_header("MOUSE"),
        blank(),
        shortcut_line("Left drag", "Orbit camera"),
        shortcut_line("Middle drag", "Pan camera"),
        shortcut_line("Scroll wheel", "Zoom in / out"),
        blank(),
        blank(),
        section_header("VIEW MODES"),
        blank(),
        shortcut_line("Shaded", "Full PBR shading with materials and lighting"),
        shortcut_line("Shaded+Wire", "PBR shading with wireframe overlay"),
        shortcut_line("Wireframe", "Wireframe only, no shading"),
        shortcut_line("Ghosted", "Semi-transparent ghosted view"),
        blank(),
    ])
}

pub fn analyze_mode() -> Text<'static> {
    Text::from(vec![
        blank(),
        section_header("ANALYZE MODE"),
        blank(),
        prose("Model analysis and validation. Produces a structured report covering"),
        prose("geometry, meshes, materials, and validation issues."),
        blank(),
        blank(),
        section_header("USAGE"),
        blank(),
        prose("  Text report to terminal (auto-opens TUI):"),
        prose(""),
        prose("    solarxy -M analyze -m model.obj"),
        blank(),
        prose("  JSON report to file:"),
        prose(""),
        prose("    solarxy -M analyze -m model.obj -f json"),
        blank(),
        prose("  Text report to file:"),
        prose(""),
        prose("    solarxy -M analyze -m model.obj -o report.txt"),
        blank(),
        prose("  JSON to stdout (non-terminal / pipe):"),
        prose(""),
        prose("    solarxy -M analyze -m model.obj -f json | jq ."),
        blank(),
        blank(),
        section_header("TUI NAVIGATION"),
        blank(),
        prose("When running in a terminal without -o or -f json, the report opens"),
        prose("in an interactive TUI with four tabs:"),
        blank(),
        shortcut_line("Tab / 1-4", "Switch between tabs"),
        shortcut_line("Shift+Tab", "Previous tab"),
        shortcut_line("j / k", "Scroll down / up"),
        shortcut_line("g / G", "Jump to top / bottom"),
        shortcut_line("PgUp / PgDn", "Scroll 20 lines up / down"),
        shortcut_line("e", "Export text report to file"),
        shortcut_line("J", "Export JSON report to file"),
        shortcut_line("q / Esc", "Quit"),
        blank(),
        blank(),
        section_header("REPORT TABS"),
        blank(),
        shortcut_line(
            "1  Overview",
            "Model summary: vertices, faces, file size, bounds",
        ),
        shortcut_line("2  Meshes", "Per-mesh breakdown with vertex/face counts"),
        shortcut_line("3  Materials", "Material properties and texture references"),
        shortcut_line(
            "4  Validation",
            "Issues: errors, warnings, and info messages",
        ),
        blank(),
    ])
}

pub fn preferences() -> Text<'static> {
    Text::from(vec![
        blank(),
        section_header("PREFERENCES"),
        blank(),
        prose("Solarxy stores display and rendering preferences in a TOML config file."),
        prose("These are loaded at startup and applied to the viewer."),
        blank(),
        blank(),
        section_header("CONFIG FILE LOCATION"),
        blank(),
        kv_line("macOS", "~/Library/Application Support/solarxy/config.toml"),
        kv_line("Linux", "~/.config/solarxy/config.toml"),
        kv_line(
            "Windows",
            "C:\\Users\\<User>\\AppData\\Roaming\\solarxy\\config.toml",
        ),
        blank(),
        blank(),
        section_header("EDITING PREFERENCES"),
        blank(),
        prose("Interactive TUI editor:"),
        prose(""),
        prose("    solarxy -M preferences"),
        blank(),
        prose("Or save from the viewer with Shift+S — this writes the current display"),
        prose("state (view mode, background, grid, etc.) to the config file."),
        blank(),
        prose("You can also edit the TOML file directly in any text editor."),
        blank(),
        blank(),
        section_header("SETTINGS AND DEFAULTS"),
        blank(),
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
        ]),
        blank(),
        settings_row("background", "Gradient"),
        settings_row("view_mode", "Shaded"),
        settings_row("normals_mode", "Off"),
        settings_row("grid_visible", "true"),
        settings_row("axis_gizmo_visible", "false"),
        settings_row("bloom_enabled", "true"),
        settings_row("uv_mode", "Off"),
        settings_row("projection_mode", "Perspective"),
        settings_row("turntable_active", "false"),
        settings_row("wireframe_line_weight", "Medium"),
        settings_row("msaa_sample_count", "4"),
        settings_row("lighting.lock", "false"),
        blank(),
        blank(),
        section_header("TUI NAVIGATION"),
        blank(),
        shortcut_line("j / k", "Move selection down / up"),
        shortcut_line("Enter / Space", "Cycle setting forward"),
        shortcut_line("l / Right", "Cycle setting forward"),
        shortcut_line("h / Left", "Cycle setting backward"),
        shortcut_line("s", "Save preferences to disk"),
        shortcut_line("r", "Reset all to defaults (unsaved)"),
        shortcut_line("q / Esc", "Quit"),
        blank(),
    ])
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
