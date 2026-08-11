//! Owning the terminal: the loop, the events, and giving it back.
//!
//! # Why the loop had to stop blocking
//!
//! The shipped analyze surface called `event::read()` with no poll and no
//! timeout. That single choice is why nothing in it could ever refresh, why a
//! resize was noticed only on the next keypress, and why there was no way to
//! show anything that changes on its own. A polled loop with a tick is not a
//! polish item; it is the precondition for everything that draws itself.
//!
//! # Giving the terminal back
//!
//! Setup and restore are paired in [`Session`], so no caller can take the
//! screen without also arranging to hand it back, and a panic hook restores
//! before it delegates. Without that hook a panic anywhere in a draw leaves the
//! reader at a shell that is still in raw mode with the alternate screen
//! showing: no echo, no line editing, and no obvious way out.
//!
//! A session rather than only a loop, because not every surface can be driven
//! by one. A surface that reports on work happening elsewhere is called by that
//! work and cannot also own the loop, so it takes the screen, draws when it is
//! told something, and gives the screen back. [`run`] is the same session for a
//! surface that has nothing else to do.
//!
//! # Why the screen is taken by hand rather than through `ratatui::init`
//!
//! That helper is hard-wired to standard output in three places: it enters the
//! alternate screen there, its restore leaves it there, and the panic hook it
//! installs writes the leave sequence there **for the rest of the process**. A
//! surface painting on standard error cannot use any of it without putting
//! escape sequences into the stream that carries data. Doing the four steps
//! here costs a dozen lines and makes the stream a parameter.
//!
//! It also removes a hook the old code could not have been right about: this
//! module used to take `ratatui`'s wrapper as the previous hook and put *that*
//! back on the way out, so every later panic in the process stayed wrapped in a
//! restore for a terminal nobody held. The hook captured below is the real one.

use std::io::{self, Write};
use std::sync::Arc;

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEvent, KeyEventKind, MouseEvent,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Frame;
use ratatui::backend::CrosstermBackend;

/// How long a quiet loop waits before looking up.
///
/// Long enough that an idle session costs nothing, short enough that a resize
/// or a finished piece of work reaches the screen before anyone notices the
/// delay.
pub const TICK: std::time::Duration = std::time::Duration::from_millis(250);

/// The smallest terminal this surface will take over.
///
/// Below it the plain text report is not a fallback, it is the better answer:
/// a stock 80 by 25 console cannot hold the arrangement, and squeezing it
/// would produce something worse than the thing it replaced.
pub const FLOOR_WIDTH: u16 = 100;
pub const FLOOR_HEIGHT: u16 = 30;

/// Opts mouse capture in.
///
/// Off by default on purpose. Grabbing the mouse wholesale breaks the
/// terminal's own text selection, which readers use constantly to copy a mesh
/// name out of a report, and every mouse action here has a keyboard path
/// anyway.
pub const MOUSE_ENV_VAR: &str = "SOLARXY_MOUSE";

/// Something the loop wants the surface to know about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
    /// Nothing happened, and the surface may repaint if it wants to.
    Tick,
}

/// Whether the loop keeps going.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Quit,
}

/// Anything the loop can drive.
pub trait Surface {
    fn draw(&mut self, frame: &mut Frame);
    fn handle(&mut self, input: Input) -> Flow;

    /// Whether a tick should cost a repaint.
    ///
    /// Defaults to no, so a surface with nothing moving stays completely
    /// quiet and a session left open overnight does no work.
    fn needs_redraw(&self) -> bool {
        false
    }
}

/// Turn a terminal event into something the surface cares about, or nothing.
///
/// Key events are filtered to presses. Terminals differ on whether they also
/// report releases and repeats, and a surface that counted all three would
/// scroll two lines per keystroke on some of them and one on others, which is
/// the kind of bug that only ever reproduces on someone else's machine.
pub fn classify(event: &Event, mouse_enabled: bool) -> Option<Input> {
    match *event {
        Event::Key(key) if key.kind == KeyEventKind::Press => Some(Input::Key(key)),
        Event::Resize(width, height) => Some(Input::Resize(width, height)),
        Event::Mouse(mouse) if mouse_enabled => Some(Input::Mouse(mouse)),
        _ => None,
    }
}

/// Whether the mouse was asked for.
pub fn mouse_requested(lookup: impl Fn(&str) -> Option<String>) -> bool {
    matches!(
        lookup(MOUSE_ENV_VAR)
            .map(|raw| raw.trim().to_ascii_lowercase())
            .as_deref(),
        Some("1" | "true" | "yes")
    )
}

/// The message for a terminal too small to take over, or `None` if it fits.
///
/// Names both the actual and the required size, because "too small" without
/// numbers leaves the reader guessing how much to drag. `surface` names which
/// one is refusing and `instead` says what happens now, because a reader who is
/// told only that something did not fit has been told the least useful half.
pub fn below_floor(width: u16, height: u16, surface: &str, instead: &str) -> Option<String> {
    (width < FLOOR_WIDTH || height < FLOOR_HEIGHT).then(|| {
        format!(
            "This terminal is {width}x{height} and the {surface} surface needs \
             {FLOOR_WIDTH}x{FLOOR_HEIGHT}. {instead}"
        )
    })
}

/// Which stream a surface paints on.
///
/// Standard output is where a terminal application conventionally draws, and
/// where the analyze surface draws. A progress surface draws on standard error
/// instead, because this release's rule is that standard output carries data: a
/// dashboard painted on it would put escape sequences into the stream a build
/// system parses, and the alternate screen's exit sequence would land there
/// after the surface had finished with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

impl Stream {
    /// A fresh handle to the stream.
    ///
    /// Boxed rather than made a type parameter of the session, because the only
    /// thing that varies is which descriptor the escape sequences reach, and a
    /// type parameter for that would spread through every signature touching a
    /// session for no gain a reader could name.
    fn writer(self) -> Box<dyn Write> {
        match self {
            Self::Stdout => Box::new(io::stdout()),
            Self::Stderr => Box::new(io::stderr()),
        }
    }

    /// How the stream is named to a person, for a message that has to say.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Stdout => "standard output",
            Self::Stderr => "standard error",
        }
    }
}

/// Give the screen back. Safe on a session that never finished taking it.
///
/// Free rather than a method, because the panic hook has to run it and cannot
/// hold the session: the session is what is unwinding.
fn restore(stream: Stream, mouse: bool) {
    let mut out = stream.writer();
    // Before leaving the screen, so the sequence lands where it was turned on.
    if mouse {
        let _ = execute!(out, DisableMouseCapture);
    }
    let _ = disable_raw_mode();
    let _ = execute!(out, LeaveAlternateScreen);
}

type PanicHook = Arc<dyn Fn(&std::panic::PanicHookInfo<'_>) + Sync + Send>;

/// The terminal, held for as long as a surface needs it.
///
/// Taking it and giving it back are one value rather than two calls, so a
/// caller cannot do the first without the second: the restore runs from `Drop`,
/// which covers the early return and the panic alike.
pub struct Session {
    terminal: ratatui::Terminal<CrosstermBackend<Box<dyn Write>>>,
    stream: Stream,
    mouse: bool,
    /// Put back when the session ends. `Option` only so `Drop` can take it.
    previous: Option<PanicHook>,
}

impl Session {
    /// Take the screen, on a stream.
    ///
    /// # Errors
    /// If the terminal will not go into raw mode or will not switch screens.
    pub fn enter(stream: Stream) -> io::Result<Self> {
        let mouse = mouse_requested(|key| std::env::var(key).ok());

        // The real hook, taken before anything else installs one. Shared rather
        // than moved because it is needed twice: from inside the wrapper, and
        // to put back afterwards.
        let previous: PanicHook = Arc::from(std::panic::take_hook());
        let inner = Arc::clone(&previous);
        // Restore before delegating, so a panic message lands on a usable
        // terminal. The previous hook still prints the message and the
        // backtrace; it just must not do it first, into a screen that is still
        // in raw mode.
        std::panic::set_hook(Box::new(move |info| {
            restore(stream, mouse);
            inner(info);
        }));

        // From here every way out unwinds through `Drop`, including the three
        // fallible steps below, so a screen half taken is still given back.
        let session = Self {
            terminal: ratatui::Terminal::new(CrosstermBackend::new(stream.writer()))?,
            stream,
            mouse,
            previous: Some(previous),
        };
        enable_raw_mode()?;
        let mut out = stream.writer();
        execute!(out, EnterAlternateScreen)?;
        if mouse {
            execute!(out, EnableMouseCapture)?;
        }
        Ok(session)
    }

    /// Which stream this session paints on.
    #[must_use]
    pub fn stream(&self) -> Stream {
        self.stream
    }

    /// Paint the surface once.
    ///
    /// # Errors
    /// If the terminal cannot be written to.
    pub fn draw(&mut self, surface: &mut impl Surface) -> io::Result<()> {
        self.terminal.draw(|frame| surface.draw(frame))?;
        Ok(())
    }

    /// Wait up to `timeout` for something the surface cares about.
    ///
    /// `Some(Input::Tick)` means the wait expired and `None` means an event
    /// arrived that this surface has no use for. The two are distinct because
    /// only the first is a reason to consider repainting. A caller driving its
    /// own work passes a zero timeout and gets one or the other at once.
    ///
    /// # Errors
    /// If the event stream cannot be read.
    pub fn poll(&mut self, timeout: std::time::Duration) -> io::Result<Option<Input>> {
        if event::poll(timeout)? {
            // A resize arrives here as an event rather than as a surprise,
            // because crossterm registers for the signal. What was missing
            // before was anything to wake up and notice.
            Ok(classify(&event::read()?, self.mouse))
        } else {
            Ok(Some(Input::Tick))
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        restore(self.stream, self.mouse);
        // Put the reader's own hook back rather than leaving every later panic
        // in this process wrapped in a restore for a terminal nobody holds any
        // more.
        if let Some(previous) = self.previous.take() {
            std::panic::set_hook(Box::new(move |info| previous(info)));
        }
    }
}

/// Take the terminal, run the surface until it quits, and give it back.
///
/// The loop for a surface that has nothing else to do. One driven by work
/// happening elsewhere holds a [`Session`] and calls it itself.
///
/// # Errors
/// If the terminal cannot be taken, drawn to, or read from.
pub fn run(surface: &mut impl Surface) -> io::Result<()> {
    let mut session = Session::enter(Stream::Stdout)?;
    let mut dirty = true;
    loop {
        if dirty {
            session.draw(surface)?;
            dirty = false;
        }

        let input = session.poll(TICK)?;

        let Some(input) = input else { continue };
        // A tick repaints only if the surface says so; anything else is a real
        // change and always does.
        dirty = !matches!(input, Input::Tick) || surface.needs_redraw();
        if surface.handle(input) == Flow::Quit {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers, MouseEventKind};

    fn key(kind: KeyEventKind) -> Event {
        Event::Key(KeyEvent {
            code: KeyCode::Char('j'),
            modifiers: KeyModifiers::NONE,
            kind,
            state: crossterm::event::KeyEventState::NONE,
        })
    }

    /// The defect this filter exists for: terminals differ on whether they
    /// report releases and repeats, so a surface counting all three scrolls
    /// twice per keystroke on some of them.
    #[test]
    fn only_key_presses_reach_the_surface() {
        assert!(matches!(
            classify(&key(KeyEventKind::Press), false),
            Some(Input::Key(_))
        ));
        assert_eq!(classify(&key(KeyEventKind::Release), false), None);
        assert_eq!(classify(&key(KeyEventKind::Repeat), false), None);
    }

    #[test]
    fn a_resize_is_an_event_rather_than_a_surprise() {
        assert_eq!(
            classify(&Event::Resize(120, 40), false),
            Some(Input::Resize(120, 40))
        );
    }

    /// Capture is off unless asked for, because grabbing the mouse breaks the
    /// terminal's own text selection.
    #[test]
    fn the_mouse_is_ignored_unless_it_was_asked_for() {
        let moved = MouseEvent {
            kind: MouseEventKind::Moved,
            column: 1,
            row: 1,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(classify(&Event::Mouse(moved), false), None);
        assert!(matches!(
            classify(&Event::Mouse(moved), true),
            Some(Input::Mouse(_))
        ));
    }

    #[test]
    fn the_mouse_override_reads_like_the_others() {
        let env = |value: Option<&str>| {
            let owned = value.map(str::to_owned);
            move |key: &str| {
                if key == MOUSE_ENV_VAR {
                    owned.clone()
                } else {
                    None
                }
            }
        };
        assert!(mouse_requested(env(Some("1"))));
        assert!(mouse_requested(env(Some("yes"))));
        assert!(mouse_requested(env(Some(" TRUE "))));
        assert!(!mouse_requested(env(Some("0"))));
        assert!(!mouse_requested(env(None)));
    }

    /// Paste and focus events are real and would otherwise be handed to a
    /// keymap that has no idea what to do with them.
    #[test]
    fn events_the_surface_has_no_use_for_are_dropped() {
        assert_eq!(classify(&Event::FocusGained, true), None);
        assert_eq!(classify(&Event::FocusLost, true), None);
        assert_eq!(classify(&Event::Paste("x".to_owned()), true), None);
    }

    const ANALYZE_INSTEAD: &str =
        "Printing the report instead; use --format text to ask for it directly.";

    fn analyze_floor(width: u16, height: u16) -> Option<String> {
        below_floor(width, height, "analyze", ANALYZE_INSTEAD)
    }

    /// "Too small" without numbers leaves the reader guessing how far to drag.
    #[test]
    fn the_floor_message_names_both_sizes() {
        assert_eq!(analyze_floor(FLOOR_WIDTH, FLOOR_HEIGHT), None);
        assert_eq!(analyze_floor(200, 60), None);

        let message = analyze_floor(80, 25).expect("below the floor");
        assert!(message.contains("80x25"), "{message}");
        assert!(message.contains("100x30"), "{message}");
        assert!(message.contains("--format text"), "{message}");
    }

    /// The surface refusing and what happens now are both the caller's words,
    /// so a second surface says its own thing rather than the analyzer's.
    #[test]
    fn the_floor_message_is_the_callers_own() {
        let message = below_floor(80, 25, "render", "Reporting a plain line instead.")
            .expect("below the floor");
        assert!(message.contains("the render surface needs"), "{message}");
        assert!(
            message.ends_with("Reporting a plain line instead."),
            "{message}"
        );
        assert!(!message.contains("--format text"), "{message}");
    }

    #[test]
    fn either_dimension_alone_is_enough_to_be_below_the_floor() {
        assert!(analyze_floor(FLOOR_WIDTH - 1, FLOOR_HEIGHT).is_some());
        assert!(analyze_floor(FLOOR_WIDTH, FLOOR_HEIGHT - 1).is_some());
    }
}
