//! The render dashboard: the same progress stream, as a terminal surface.
//!
//! # The loop belongs to the render
//!
//! Every other terminal surface in this crate is driven by
//! [`crate::tui::shell::run`], which owns the loop and calls the surface when
//! something happens. This one cannot be: the render owns the loop, and the
//! surface is called by it. So the dashboard holds a
//! [`crate::tui::shell::Session`] and does the two halves of that loop itself,
//! polling with no timeout at all and drawing under a throttle.
//!
//! The throttle is not a nicety. The drive loop does not yield while a tile's
//! readback is pending, so the sink is called as fast as the processor allows,
//! and a redraw at that rate would spend more time painting the terminal than
//! rendering the picture.
//!
//! # Standard error, and what happens when it will not do
//!
//! This paints on standard error, because that is where this release puts
//! progress and standard output carries data. It is what lets `--tui` and
//! `--json` be given together. When standard error is not a terminal, or is
//! smaller than the surface needs, the plain line takes over and says so: the
//! analyze surface's own answer to a terminal it cannot use.

pub mod panels;
pub mod picture;
pub mod state;

use std::io::IsTerminal;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Block;

use solarxy_render::{Preview, PreviewFormat, RenderProgress, RenderSink};

use crate::tui::caps::{Capabilities, Glyphs};
use crate::tui::layout::{Layout, PanelKind};
use crate::tui::panels::{Ctx, Panel};
use crate::tui::shell::{self, Flow, Input, Session, Stream, Surface};
use crate::tui::theme::{Theme, ThemeSet};

use panels::{DashPanel, Readout};
use state::{Picture, RenderView, Request, Stage};

/// The arrangement, as an encoding rather than as tree-building calls.
///
/// The same form the analyze presets take, and for the same reason: a layout
/// written as one line can be read against the design, and building the tree by
/// hand cannot. Fixed rather than rearrangeable, because a render lasts a
/// minute and there is nothing here to arrange for.
///
/// The picture takes most of the right half, because it is the only panel
/// whose value grows with the room it gets. Everything a reader checks in a
/// glance is stacked down the left, in the order they check it.
///
/// Every ratio is chosen against the minimum panel size rather than for looks:
/// a tree that asks for a panel the tree will not give is not refused at
/// decode, it is silently replaced at solve by one panel filling the pane,
/// which is a dashboard with five of its six readouts missing. The test below
/// asserts the arrangement survives the solve at both sizes this runs at.
const ARRANGEMENT: &str =
    "V0.515(H0.273(progress,H0.375(tiles,H0.500(stages,throughput))),H0.727(picture,render))";

/// The least time between two repaints.
///
/// Four a second is faster than a person reads and slow enough that the render
/// keeps the processor.
const REDRAW_INTERVAL: Duration = Duration::from_millis(250);

/// How the run ended, in the words the held frame states.
///
/// Carried into the hold by the caller rather than derived from the progress
/// stream, because the stream's failure names only the stage that failed: the
/// reason lives in the error the render returned, and only the caller holds
/// that.
pub enum Ending {
    Rendered,
    Failed(String),
    Cancelled,
}

/// The surface: an arrangement over one render.
pub struct Dashboard {
    view: RenderView,
    layout: Layout<DashPanel>,
    theme: Theme,
    glyphs: Glyphs,
    caps: Capabilities,
    quit: bool,
    /// Present once the render has returned and the frame is being held.
    ending: Option<Ending>,
}

impl Dashboard {
    /// Build the surface over what the reader asked for.
    pub fn new(request: Request, theme: Theme, caps: Capabilities) -> Self {
        Self {
            view: RenderView::new(request),
            layout: Layout::decode(ARRANGEMENT)
                .unwrap_or_else(|_| Layout::single(DashPanel::Progress)),
            theme,
            glyphs: caps.glyphs(),
            caps,
            quit: false,
            ending: None,
        }
    }

    /// What the surface is showing, for a test that wants to drive it.
    pub fn view_mut(&mut self) -> &mut RenderView {
        &mut self.view
    }
}

impl Surface for Dashboard {
    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let body = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1));

        for placement in self.layout.solve(body, None) {
            let block = Block::bordered()
                .title(Line::from(Span::styled(
                    placement.panel.name(),
                    Style::default()
                        .fg(self.theme.slots.ink_dim)
                        .add_modifier(Modifier::BOLD),
                )))
                .border_set(self.glyphs.border)
                .border_style(Style::default().fg(self.theme.slots.border));
            let inner = block.inner(placement.rect);
            frame.render_widget(block, placement.rect);

            let ctx = Ctx {
                subject: &self.view,
                theme: &self.theme.slots,
                glyphs: &self.glyphs,
                caps: self.caps,
                // Nothing here takes focus, and saying one panel has it would
                // draw an accent that means nothing.
                focused: false,
            };
            Readout(placement.panel).draw(frame, inner, &ctx);
        }

        frame.render_widget(
            Line::from(footer(&self.view, &self.theme, self.ending.as_ref())),
            Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1),
        );
    }

    fn handle(&mut self, input: Input) -> Flow {
        if let Input::Key(key) = input
            && matches!(key.code, KeyCode::Char('q' | 'Q') | KeyCode::Esc)
        {
            // Not a quit: the render is what ends, and it ends by being asked
            // to. Quitting here would leave it running with nothing watching.
            self.view.cancelling = true;
            self.quit = true;
        }
        Flow::Continue
    }
}

/// The one row along the bottom.
///
/// While the render runs: the key that stops it, and the stage's last word.
/// While the frame is held: the key that dismisses it and how the run ended,
/// with the output path on success, because that line is the reason the frame
/// is held at all.
fn footer(view: &RenderView, theme: &Theme, ending: Option<&Ending>) -> Vec<Span<'static>> {
    let key = |label: &'static str| {
        Span::styled(
            label,
            Style::default()
                .fg(theme.slots.accent)
                .add_modifier(Modifier::BOLD),
        )
    };
    if let Some(ending) = ending {
        let mut spans = vec![
            key(" q"),
            Span::styled("  dismiss", Style::default().fg(theme.slots.ink_dim)),
        ];
        match ending {
            Ending::Rendered => {
                spans.push(Span::styled(
                    "   rendered ",
                    Style::default().fg(theme.slots.success),
                ));
                spans.push(Span::styled(
                    view.request.output.clone(),
                    Style::default().fg(theme.slots.ink),
                ));
            }
            Ending::Failed(reason) => {
                spans.push(Span::styled(
                    "   failed: ",
                    Style::default().fg(theme.slots.error),
                ));
                // The first line only: the full error is printed onto the
                // restored terminal after dismissal, where it can be copied.
                spans.push(Span::styled(
                    reason.lines().next().unwrap_or_default().to_owned(),
                    Style::default().fg(theme.slots.ink),
                ));
            }
            Ending::Cancelled => spans.push(Span::styled(
                "   cancelled",
                Style::default().fg(theme.slots.warning),
            )),
        }
        return spans;
    }
    let mut spans = vec![
        key(" q"),
        Span::styled("  cancel", Style::default().fg(theme.slots.ink_dim)),
    ];
    match view.stage {
        Stage::Done => spans.push(Span::styled(
            "   rendered",
            Style::default().fg(theme.slots.success),
        )),
        Stage::Failed => spans.push(Span::styled(
            "   failed",
            Style::default().fg(theme.slots.error),
        )),
        _ => {}
    }
    spans
}

/// The dashboard, driven by the render that reports to it.
///
/// Holds the terminal for as long as the render runs and gives it back when
/// this is dropped, which is what makes an early return or a panic safe.
pub struct DashboardSink {
    session: Session,
    dashboard: Dashboard,
    cancel: Arc<AtomicBool>,
    painted: Instant,
}

impl DashboardSink {
    /// Take the terminal and put the surface on it.
    ///
    /// `cancel` is the same flag the interrupt handler sets, so the quit key
    /// and a signal stop the render by one path rather than two.
    ///
    /// # Errors
    /// If the terminal cannot be taken.
    pub fn new(
        request: Request,
        theme: Theme,
        caps: Capabilities,
        cancel: Arc<AtomicBool>,
    ) -> std::io::Result<Self> {
        Ok(Self {
            session: Session::enter(Stream::Stderr)?,
            dashboard: Dashboard::new(request, theme, caps),
            cancel,
            // A whole interval ago, so the first event paints. Checked
            // because a monotonic clock that has not yet run for a quarter of
            // a second cannot answer it, which is a real case on a fresh boot.
            painted: Instant::now()
                .checked_sub(REDRAW_INTERVAL)
                .unwrap_or_else(Instant::now),
        })
    }

    /// Hold the completion state until the reader dismisses it.
    ///
    /// Called after the render returns, whichever way it returned. Without it
    /// the final frame is torn down microseconds after it is painted, and
    /// nothing about a finished render survives on screen or in scrollback.
    /// The dismiss key is stated in the footer; a fresh interrupt dismisses
    /// too, the same escape the window's hold honours.
    ///
    /// This never runs on a piped invocation: the dashboard only exists when
    /// standard error is a terminal, which [`unavailable`] decides, and no
    /// second gate is added here that could disagree with it.
    pub fn hold(&mut self, ending: Ending) {
        self.dashboard.ending = Some(ending);
        // The interrupt or quit key that ended the render was answered by
        // ending it. The hold listens for the next one, so the flag starts
        // cleared: a cancelled run shows its completion state like any other
        // rather than flashing past it.
        self.cancel.store(false, Ordering::Relaxed);
        let _ = self.session.draw(&mut self.dashboard);
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                return;
            }
            match self.session.poll(Duration::from_millis(50)) {
                Ok(Some(Input::Key(key)))
                    if matches!(key.code, KeyCode::Char('q' | 'Q') | KeyCode::Esc) =>
                {
                    return;
                }
                // A resize deserves a repaint; a tick and an ignored event do
                // not, since nothing in the held frame changes on its own.
                Ok(Some(Input::Resize(..))) => {
                    let _ = self.session.draw(&mut self.dashboard);
                }
                Ok(Some(_)) | Ok(None) => {}
                // A terminal that cannot be read cannot be held.
                Err(_) => return,
            }
        }
    }

    /// Read the keyboard and repaint, at most as often as the throttle allows.
    fn pump(&mut self, force: bool) {
        while let Ok(Some(input)) = self.session.poll(Duration::ZERO) {
            if matches!(input, Input::Tick) {
                break;
            }
            if self.dashboard.handle(input) == Flow::Quit {
                break;
            }
        }
        if self.dashboard.quit {
            self.cancel.store(true, Ordering::Relaxed);
        }
        let now = Instant::now();
        if force || now.saturating_duration_since(self.painted) >= REDRAW_INTERVAL {
            let _ = self.session.draw(&mut self.dashboard);
            self.painted = now;
        }
    }
}

impl RenderSink for DashboardSink {
    fn report(&mut self, progress: &RenderProgress) {
        self.dashboard.view.observe(progress, Instant::now());
        // The last word of a render is always drawn, whatever the throttle
        // says, or a reader is left looking at the frame before the end.
        let ending = matches!(
            progress,
            RenderProgress::Done { .. } | RenderProgress::Failed { .. }
        );
        self.pump(ending);
    }

    fn preview(&mut self, image: &Preview<'_>) {
        // The first tile is also the first moment anything knows how big the
        // picture is, since the render node decides that and the command line
        // only overrides it.
        self.dashboard.view.request.size = Some((image.width, image.height));
        self.dashboard.view.picture = Some(match image.format {
            PreviewFormat::Rgba8 => Picture::from_rgba8(image.width, image.height, image.pixels),
            PreviewFormat::Rgba32F => {
                Picture::from_rgba32f(image.width, image.height, image.pixels)
            }
        });
        // A tile landing is the only moment the picture changes, so it is worth
        // a frame of its own rather than waiting for the next report.
        self.pump(true);
    }
}

/// Why the dashboard could not be opened, in the words a reader gets.
///
/// A refusal rather than a failure: the plain line does the same job, and a
/// render that would not start because a terminal was piped would be a worse
/// tool than one that says what it did instead.
pub fn unavailable() -> Option<String> {
    if !std::io::stderr().is_terminal() {
        return Some(
            "The dashboard needs a terminal on standard error, and this is not one. \
             Reporting progress as a plain line instead."
                .to_owned(),
        );
    }
    let (width, height) = crossterm::terminal::size().ok()?;
    shell::below_floor(
        width,
        height,
        "render",
        "Reporting progress as a plain line instead.",
    )
}

/// The theme the dashboard paints with, and anything loading it wants said.
///
/// The reader's own choice, resolved through the same door the analyze surface
/// uses, because a person who picked a terminal theme picked it for their
/// terminal rather than for one surface.
pub fn theme_for(caps: Capabilities, requested: Option<&str>) -> (Theme, Vec<String>) {
    let (prefs, prefs_notices) = crate::tui::prefs::TuiPrefs::load();
    let wanted = requested.or(prefs.theme.as_deref());
    let (theme, theme_notices) = ThemeSet::load().resolve(wanted, caps);
    let mut notices: Vec<String> = prefs_notices.iter().map(ToString::to_string).collect();
    notices.extend(theme_notices.iter().map(ToString::to_string));
    (theme, notices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::harness;
    use crate::tui::theme::DEFAULT_THEME;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    const WIDTH: u16 = 140;
    const HEIGHT: u16 = 45;

    fn request() -> Request {
        Request {
            input: "scene.slxy".into(),
            output: "render.png".into(),
            engine: Some("path traced".into()),
            size: Some((1920, 1080)),
            samples: Some(64),
            bounces: Some(6),
            seed: Some(11),
        }
    }

    fn sampling(tile: u32, sample: u32) -> RenderProgress {
        RenderProgress::Sampling {
            tile,
            tiles: 12,
            columns: 4,
            rows: 3,
            sample,
            samples: 64,
            elapsed_ms: 4200,
            drawn: u64::from(tile) * 64 + u64::from(sample),
            total: 12 * 64,
        }
    }

    /// A dashboard part way through a render, drawn at a capability pair.
    fn drawn(caps: Capabilities, name: &str, width: u16, height: u16) -> Buffer {
        let set = ThemeSet::bundled();
        let slots = set
            .slots_for(name)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let theme = Theme::resolve(caps, name, &slots);
        let mut dashboard = Dashboard::new(request(), theme, caps);
        let now = Instant::now();
        dashboard.view.observe(&RenderProgress::Loading, now);
        dashboard.view.observe(
            &RenderProgress::BuildingHierarchy { triangles: 345_678 },
            now + Duration::from_millis(80),
        );
        dashboard
            .view
            .observe(&sampling(5, 32), now + Duration::from_millis(400));
        dashboard
            .view
            .observe(&sampling(5, 48), now + Duration::from_millis(900));
        dashboard.view.picture = Some(Picture {
            width: 64,
            height: 48,
            luma: (0..64 * 48).map(|i| (i % 256) as u8).collect(),
        });
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal.draw(|frame| dashboard.draw(frame)).expect("draw");
        terminal.backend().buffer().clone()
    }

    fn screen(buffer: &Buffer, width: u16, height: u16) -> String {
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The authored arrangement survives the solve, at both sizes.
    ///
    /// A tree asking for a panel below the minimum is not refused when it is
    /// decoded; it is replaced at solve time by the focused panel filling the
    /// pane. That failure looks like a dashboard with one readout and says
    /// nothing about why, so it is asserted directly rather than left to be
    /// noticed in the strings the other tests happen to look for.
    #[test]
    fn the_authored_arrangement_holds_at_every_size_this_runs_at() {
        let layout = Layout::<DashPanel>::decode(ARRANGEMENT).expect("the arrangement parses");
        for (width, height) in [
            (WIDTH, HEIGHT - 1),
            (shell::FLOOR_WIDTH, shell::FLOOR_HEIGHT - 1),
        ] {
            let placed = layout.solve(Rect::new(0, 0, width, height), None);
            assert_eq!(
                placed.len(),
                DashPanel::choosable().len(),
                "at {width}x{height} the solve fell back to {} panel(s)",
                placed.len()
            );
        }
    }

    /// The board's own list: a grid, a gauge, timings and an estimate.
    #[test]
    fn the_six_readouts_put_their_facts_on_the_screen() {
        let text = screen(
            &drawn(harness::every_pair()[7], DEFAULT_THEME, WIDTH, HEIGHT),
            WIDTH,
            HEIGHT,
        );
        for want in [
            "progress",
            "tiles",
            "stages",
            "render",
            "throughput",
            "picture",
        ] {
            assert!(text.contains(want), "no {want} panel:\n{text}");
        }
        for want in [
            "tile 6 of 12", // the grid says where it is
            "elapsed",      // and how long
            "remaining",    // and how much longer
            "hierarchy",    // a stage that finished, timed
            "scene.slxy",   // what is being rendered
            "path traced",  // with which engine
            "1920x1080",    // at what size
        ] {
            assert!(text.contains(want), "missing {want}:\n{text}");
        }
    }

    /// The colour rules, through the same helpers the analyze surface's own
    /// drawing is held to. Every tier, every glyph repertoire, every theme
    /// that ships.
    #[test]
    fn the_dashboard_obeys_the_shipped_colour_rules() {
        for name in ThemeSet::bundled().names() {
            for caps in harness::lower_tiers() {
                let buffer = drawn(caps, name, WIDTH, HEIGHT);
                let where_ = format!("{name} at {caps:?}");
                // The resolved slots, not the file's: at these tiers the theme
                // is deliberately ignored, so the hues a cell is allowed to
                // carry are the ones resolution produced.
                let slots = ThemeSet::bundled()
                    .slots_for(name)
                    .expect("a bundled theme");
                harness::assert_only_terminal_ink_or_palette_hues(
                    &buffer,
                    &Theme::resolve(caps, name, &slots).slots,
                    &where_,
                );
                harness::assert_body_text_is_terminal_ink(&buffer, &where_);
                harness::assert_no_background(&buffer, &where_);
            }
        }
    }

    /// Monochrome paints no colour at all, and the ASCII repertoire is not
    /// left behind by the picture or the grid, which are the two things here
    /// that reach for block elements.
    #[test]
    fn the_lower_tiers_keep_their_repertoire() {
        for caps in harness::every_pair() {
            let buffer = drawn(caps, DEFAULT_THEME, WIDTH, HEIGHT);
            let where_ = format!("{caps:?}");
            if caps.color == crate::tui::caps::ColorTier::Mono {
                harness::assert_no_colour(&buffer, &where_);
            }
            if caps.glyphs == crate::tui::caps::GlyphTier::Ascii {
                harness::assert_only_ascii(&buffer, &where_);
            }
        }
    }

    /// The whole surface still fits where the analyze one does, which is the
    /// size the fallback is measured against.
    #[test]
    fn the_surface_works_at_the_floor_size() {
        let (w, h) = (shell::FLOOR_WIDTH, shell::FLOOR_HEIGHT);
        let text = screen(&drawn(harness::every_pair()[7], DEFAULT_THEME, w, h), w, h);
        // The elision sheds the picture first and keeps the three that answer
        // whether the render is progressing.
        assert!(text.contains("progress"), "{text}");
        assert!(text.contains("tiles"), "{text}");
        assert!(text.contains("stages"), "{text}");
    }

    /// The quit key asks the render to stop rather than ending the loop, since
    /// the loop is the render's.
    #[test]
    fn the_quit_key_asks_the_render_to_stop() {
        let set = ThemeSet::bundled();
        let slots = set.slots_for(DEFAULT_THEME).expect("the default");
        let caps = harness::every_pair()[7];
        let mut dashboard =
            Dashboard::new(request(), Theme::resolve(caps, DEFAULT_THEME, &slots), caps);
        assert!(!dashboard.view.cancelling);
        assert_eq!(
            dashboard.handle(Input::Key(harness::key(KeyCode::Char('q')))),
            Flow::Continue,
            "the surface ended the loop instead of asking the render to"
        );
        assert!(
            dashboard.view.cancelling,
            "the key did not reach the render"
        );
        assert!(dashboard.quit);
    }

    /// The held frame states how the run ended, the way out, and on success
    /// the output path, because a static bar at one hundred percent cannot
    /// tell success from a failure that happened at the end.
    #[test]
    fn the_held_frame_states_the_outcome_and_the_way_out() {
        let set = ThemeSet::bundled();
        let slots = set.slots_for(DEFAULT_THEME).expect("the default");
        let caps = harness::every_pair()[7];
        let cases: [(Ending, Vec<&str>, Vec<&str>); 3] = [
            (
                Ending::Rendered,
                vec!["dismiss", "rendered", "render.png"],
                vec![],
            ),
            (
                Ending::Failed("the device fell off\nand the detail follows".into()),
                vec!["dismiss", "failed", "the device fell off"],
                // The full error prints onto the restored terminal after
                // dismissal; the footer carries only the first line.
                vec!["and the detail follows"],
            ),
            (Ending::Cancelled, vec!["dismiss", "cancelled"], vec![]),
        ];
        for (ending, wants, spurns) in cases {
            let mut dashboard =
                Dashboard::new(request(), Theme::resolve(caps, DEFAULT_THEME, &slots), caps);
            dashboard.ending = Some(ending);
            let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("terminal");
            terminal.draw(|frame| dashboard.draw(frame)).expect("draw");
            let text = screen(&terminal.backend().buffer().clone(), WIDTH, HEIGHT);
            for want in wants {
                assert!(text.contains(want), "missing {want}:\n{text}");
            }
            for spurn in spurns {
                assert!(!text.contains(spurn), "{spurn} leaked into the footer");
            }
        }
    }
}
