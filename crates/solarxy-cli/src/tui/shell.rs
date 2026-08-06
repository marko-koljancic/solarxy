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
//! Setup and restore are paired here so no caller can take the screen without
//! also arranging to hand it back, and a panic hook restores before it
//! delegates. Without that hook a panic anywhere in a draw leaves the reader
//! at a shell that is still in raw mode with the alternate screen showing:
//! no echo, no line editing, and no obvious way out.
//!
//! `ratatui::restore` covers raw mode and the alternate screen and nothing
//! else, so anything this module turns on it must turn off itself. Today that
//! is mouse capture.

use std::io;
use std::sync::Arc;

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEvent, KeyEventKind, MouseEvent,
};
use crossterm::execute;
use ratatui::Frame;

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
/// numbers leaves the reader guessing how much to drag.
pub fn below_floor(width: u16, height: u16) -> Option<String> {
    (width < FLOOR_WIDTH || height < FLOOR_HEIGHT).then(|| {
        format!(
            "This terminal is {width}x{height} and the analyze surface needs \
             {FLOOR_WIDTH}x{FLOOR_HEIGHT}. Printing the report instead; \
             use --format text to ask for it directly."
        )
    })
}

/// Take the terminal, run the surface, and give it back.
///
/// The restore path runs whatever the loop did, including panicking, which is
/// the whole reason setup and teardown live together in one function rather
/// than at two call sites that could drift.
pub fn run(surface: &mut impl Surface) -> io::Result<()> {
    let mouse = mouse_requested(|key| std::env::var(key).ok());
    let mut terminal = ratatui::init();
    if mouse {
        execute!(io::stdout(), EnableMouseCapture)?;
    }
    let previous = install_panic_hook(mouse);

    let result = pump(surface, &mut terminal, mouse);

    if mouse {
        let _ = execute!(io::stdout(), DisableMouseCapture);
    }
    ratatui::restore();
    // Put the reader's own hook back rather than leaving every later panic in
    // this process wrapped in a restore for a terminal nobody holds any more.
    std::panic::set_hook(Box::new(move |info| previous(info)));
    result
}

/// Restore before delegating, so a panic message lands on a usable terminal.
///
/// The previous hook is what prints the message and the backtrace, so it still
/// has to run; it just must not run first, into a screen that is still in raw
/// mode. It is shared rather than moved because it is needed twice: once from
/// inside the wrapper and once to put back afterwards.
fn install_panic_hook(mouse: bool) -> PanicHook {
    let previous: PanicHook = Arc::from(std::panic::take_hook());
    let inner = Arc::clone(&previous);
    std::panic::set_hook(Box::new(move |info| {
        if mouse {
            let _ = execute!(io::stdout(), DisableMouseCapture);
        }
        ratatui::restore();
        inner(info);
    }));
    previous
}

type PanicHook = Arc<dyn Fn(&std::panic::PanicHookInfo<'_>) + Sync + Send>;

fn pump(
    surface: &mut impl Surface,
    terminal: &mut ratatui::DefaultTerminal,
    mouse: bool,
) -> io::Result<()> {
    let mut dirty = true;
    loop {
        if dirty {
            terminal.draw(|frame| surface.draw(frame))?;
            dirty = false;
        }

        let input = if event::poll(TICK)? {
            // A resize arrives here as an event rather than as a surprise,
            // because crossterm registers for the signal. What was missing
            // before was anything to wake up and notice.
            classify(&event::read()?, mouse)
        } else {
            Some(Input::Tick)
        };

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

    /// "Too small" without numbers leaves the reader guessing how far to drag.
    #[test]
    fn the_floor_message_names_both_sizes() {
        assert_eq!(below_floor(FLOOR_WIDTH, FLOOR_HEIGHT), None);
        assert_eq!(below_floor(200, 60), None);

        let message = below_floor(80, 25).expect("below the floor");
        assert!(message.contains("80x25"), "{message}");
        assert!(message.contains("100x30"), "{message}");
        assert!(message.contains("--format text"), "{message}");
    }

    #[test]
    fn either_dimension_alone_is_enough_to_be_below_the_floor() {
        assert!(below_floor(FLOOR_WIDTH - 1, FLOOR_HEIGHT).is_some());
        assert!(below_floor(FLOOR_WIDTH, FLOOR_HEIGHT - 1).is_some());
    }
}
