//! The scene clock: the document's sense of time, and the transport that
//! drives it.
//!
//! **Foundation only** (decision M-11). There is a clock, transport control,
//! and a tick that dirties what depends on time. There are no event nodes,
//! no keyframe channels and no actor graph; those wait until this has a real
//! consumer to be judged against, and its single consumer today is
//! time-driven expressions.
//!
//! **Fixed step, not wall clock.** A tick advances exactly one frame rather
//! than `dt * fps` seconds. The consequence is the reason: `$T` is then
//! exactly `frame / fps`, so cooking frame 90 gives bit-identical geometry
//! every time, on any machine, however long the cook took. A heavy scene
//! plays slowly instead of skipping, which for a modeling tool is the honest
//! trade: you are watching the geometry, not hitting a broadcast deadline.
//!
//! **What persists and what does not.** `fps`, `frame_range` and `loop_mode`
//! are document state and travel in `.slxy`; `playing` and `frame` are
//! session state and do not. That split is why a saved scene always reloads
//! stopped at its range start, which is what keeps golden captures, CLI
//! cooks and `.slxy` reload reproducible.

use serde::{Deserialize, Serialize};

use crate::expr::SceneTime;

/// What happens when playback reaches the end of the range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LoopMode {
    /// Stop on the last frame.
    Once,
    /// Jump back to the start.
    #[default]
    Loop,
    /// Reverse direction at each end.
    PingPong,
}

impl LoopMode {
    /// The wire/storage key, matching the enum param convention.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            LoopMode::Once => "once",
            LoopMode::Loop => "loop",
            LoopMode::PingPong => "pingPong",
        }
    }

    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "once" => Some(LoopMode::Once),
            "loop" => Some(LoopMode::Loop),
            "pingPong" => Some(LoopMode::PingPong),
            _ => None,
        }
    }
}

/// The lowest frame rate that is still a frame rate, and the highest worth
/// offering. Both are clamps rather than opinions: outside them the clock
/// stops meaning anything (`fps = 0` divides by zero in `$T`).
pub const MIN_FPS: f64 = 1.0;
pub const MAX_FPS: f64 = 240.0;

/// The default range and rate. 1 to 240 at 24fps is ten seconds of film,
/// which is the convention every DCC opens with.
pub const DEFAULT_START: i64 = 1;
pub const DEFAULT_END: i64 = 240;
pub const DEFAULT_FPS: f64 = 24.0;

/// The scene clock.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneClock {
    /// Session state: never saved, always false on load.
    pub playing: bool,
    /// Session state: never saved, reset to the range start on load.
    pub frame: i64,
    /// Document state.
    pub fps: f64,
    /// Document state, inclusive at both ends.
    pub frame_range: (i64, i64),
    /// Document state.
    pub loop_mode: LoopMode,
    /// Document state: whether a *player* starts playing on load. The
    /// editor stores and saves it but never acts on it.
    pub autoplay: bool,
    /// Ping-pong direction, +1 or -1. Session state, derived from play.
    direction: i64,
}

impl Default for SceneClock {
    fn default() -> Self {
        Self {
            playing: false,
            frame: DEFAULT_START,
            fps: DEFAULT_FPS,
            frame_range: (DEFAULT_START, DEFAULT_END),
            loop_mode: LoopMode::default(),
            autoplay: false,
            direction: 1,
        }
    }
}

impl SceneClock {
    /// The clock as an expression sees it.
    ///
    /// `$T` is derived from the frame rather than tracked separately, which
    /// is what makes a frame reproducible: there is only one number, and
    /// seconds is a view of it.
    #[must_use]
    pub fn scene_time(&self) -> SceneTime {
        let fps = self.effective_fps();
        SceneTime {
            seconds: self.frame as f64 / fps,
            frame: self.frame as f64,
            fps,
        }
    }

    /// The rate, guaranteed usable as a divisor.
    #[must_use]
    pub fn effective_fps(&self) -> f64 {
        if self.fps.is_finite() && self.fps >= MIN_FPS {
            self.fps.min(MAX_FPS)
        } else {
            DEFAULT_FPS
        }
    }

    /// The range, guaranteed non-empty and ordered.
    #[must_use]
    pub fn effective_range(&self) -> (i64, i64) {
        let (a, b) = self.frame_range;
        if a <= b { (a, b) } else { (b, a) }
    }

    /// Advances one frame under the loop mode. Returns true when the frame
    /// moved, so a caller can skip work on a clock that has stopped.
    ///
    /// Under [`LoopMode::Once`] reaching the end also clears `playing`: the
    /// transport should show stopped rather than pretend to run against a
    /// frame that no longer advances.
    pub fn advance(&mut self) -> bool {
        let (start, end) = self.effective_range();
        if start == end {
            // A one-frame range has nowhere to go. Once still finishes.
            if self.loop_mode == LoopMode::Once {
                self.playing = false;
            }
            return false;
        }
        let before = self.frame;
        match self.loop_mode {
            LoopMode::Once => {
                if self.frame >= end {
                    self.playing = false;
                } else {
                    self.frame += 1;
                }
            }
            LoopMode::Loop => {
                self.frame = if self.frame >= end {
                    start
                } else {
                    self.frame + 1
                };
            }
            LoopMode::PingPong => {
                if self.direction >= 0 {
                    if self.frame >= end {
                        self.direction = -1;
                        self.frame = end - 1;
                    } else {
                        self.frame += 1;
                    }
                } else if self.frame <= start {
                    self.direction = 1;
                    self.frame = start + 1;
                } else {
                    self.frame -= 1;
                }
            }
        }
        self.frame != before
    }

    /// Starts playback. A stopped-at-the-end `Once` clock rewinds first, so
    /// pressing play again replays instead of doing nothing.
    pub fn play(&mut self) {
        let (start, end) = self.effective_range();
        if self.loop_mode == LoopMode::Once && self.frame >= end {
            self.frame = start;
            self.direction = 1;
        }
        self.playing = true;
    }

    pub fn pause(&mut self) {
        self.playing = false;
    }

    /// Stops and rewinds to the range start.
    pub fn stop(&mut self) {
        self.playing = false;
        self.direction = 1;
        self.frame = self.effective_range().0;
    }

    /// Jumps to a frame, clamped into the range.
    pub fn set_frame(&mut self, frame: i64) {
        let (start, end) = self.effective_range();
        self.frame = frame.clamp(start, end);
    }

    /// Steps by `delta` frames, clamped (a step is a nudge, not a wrap:
    /// stepping past the end of a looping range should land on the end, not
    /// silently restart).
    pub fn step(&mut self, delta: i64) {
        self.set_frame(self.frame.saturating_add(delta));
    }

    /// Sets the range and pulls the current frame back inside it.
    pub fn set_range(&mut self, start: i64, end: i64) {
        self.frame_range = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        self.set_frame(self.frame);
    }

    pub fn set_fps(&mut self, fps: f64) {
        self.fps = if fps.is_finite() {
            fps.clamp(MIN_FPS, MAX_FPS)
        } else {
            DEFAULT_FPS
        };
    }

    pub fn set_loop_mode(&mut self, mode: LoopMode) {
        self.loop_mode = mode;
        self.direction = 1;
    }

    /// The document half, for saving. Session state is deliberately absent.
    #[must_use]
    pub fn settings(&self) -> RuntimeSettings {
        let (start, end) = self.effective_range();
        RuntimeSettings {
            fps: self.effective_fps(),
            frame_start: start,
            frame_end: end,
            loop_mode: self.loop_mode,
            autoplay: self.autoplay,
        }
    }

    /// Restores the document half. `playing` stays false and the frame goes
    /// to the range start, which is what makes a reloaded scene reproducible.
    pub fn apply_settings(&mut self, s: &RuntimeSettings) {
        self.set_fps(s.fps);
        self.set_range(s.frame_start, s.frame_end);
        self.loop_mode = s.loop_mode;
        self.autoplay = s.autoplay;
        self.playing = false;
        self.direction = 1;
        self.frame = self.effective_range().0;
    }
}

/// The persisted half of the clock: what `.slxy` carries.
///
/// Separate from [`SceneClock`] on purpose. The clock holds session state
/// too, and a save format that could accidentally serialize `playing` would
/// make "the scene I saved" depend on whether I happened to be playing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSettings {
    pub fps: f64,
    pub frame_start: i64,
    pub frame_end: i64,
    pub loop_mode: LoopMode,
    /// Whether a *player* starts playing on load. The editor ignores it:
    /// opening a scene that immediately starts animating is a surprise in an
    /// authoring tool and the point of publishing in a viewer.
    pub autoplay: bool,
}

impl Default for RuntimeSettings {
    fn default() -> Self {
        Self {
            fps: DEFAULT_FPS,
            frame_start: DEFAULT_START,
            frame_end: DEFAULT_END,
            loop_mode: LoopMode::default(),
            autoplay: false,
        }
    }
}

#[cfg(test)]
mod tests {
    // Exact comparison is the assertion: fps values are clamped to literals
    // the test supplies, so "came back as exactly 30" is the claim, not
    // "came back near 30".
    #![allow(clippy::float_cmp)]

    use super::*;

    fn clock(start: i64, end: i64, mode: LoopMode) -> SceneClock {
        let mut c = SceneClock {
            loop_mode: mode,
            ..SceneClock::default()
        };
        c.set_range(start, end);
        c.frame = start;
        c
    }

    #[test]
    fn time_is_derived_from_the_frame_so_a_frame_is_reproducible() {
        let mut c = SceneClock::default();
        c.set_fps(24.0);
        c.set_frame(48);
        let t = c.scene_time();
        assert!((t.seconds - 2.0).abs() < 1e-12, "48 frames at 24fps is 2s");
        assert!((t.frame - 48.0).abs() < 1e-12);
        // Same frame, same seconds, no matter how we got here.
        c.set_frame(1);
        c.set_frame(48);
        assert!((c.scene_time().seconds - 2.0).abs() < 1e-12);
    }

    #[test]
    fn loop_wraps_to_the_start() {
        let mut c = clock(1, 3, LoopMode::Loop);
        c.play();
        for expected in [2, 3, 1, 2] {
            c.advance();
            assert_eq!(c.frame, expected);
        }
        assert!(c.playing, "looping never stops on its own");
    }

    #[test]
    fn once_stops_at_the_end() {
        let mut c = clock(1, 3, LoopMode::Once);
        c.play();
        c.advance();
        c.advance();
        assert_eq!(c.frame, 3);
        assert!(c.playing);
        assert!(!c.advance(), "no further movement");
        assert!(!c.playing, "and the transport shows stopped");
    }

    #[test]
    fn playing_again_after_once_finished_rewinds_instead_of_doing_nothing() {
        let mut c = clock(1, 3, LoopMode::Once);
        c.frame = 3;
        c.play();
        assert_eq!(c.frame, 1);
        assert!(c.playing);
    }

    #[test]
    fn ping_pong_reverses_at_both_ends() {
        let mut c = clock(1, 3, LoopMode::PingPong);
        c.play();
        let mut seen = vec![c.frame];
        for _ in 0..6 {
            c.advance();
            seen.push(c.frame);
        }
        assert_eq!(seen, vec![1, 2, 3, 2, 1, 2, 3]);
    }

    #[test]
    fn stop_rewinds_to_the_range_start() {
        let mut c = clock(10, 20, LoopMode::Loop);
        c.set_frame(17);
        c.play();
        c.stop();
        assert!(!c.playing);
        assert_eq!(c.frame, 10);
    }

    #[test]
    fn a_frame_outside_the_range_is_clamped_not_wrapped() {
        let mut c = clock(10, 20, LoopMode::Loop);
        c.set_frame(999);
        assert_eq!(c.frame, 20);
        c.set_frame(-999);
        assert_eq!(c.frame, 10);
    }

    #[test]
    fn narrowing_the_range_pulls_the_current_frame_inside_it() {
        let mut c = clock(1, 100, LoopMode::Loop);
        c.set_frame(90);
        c.set_range(1, 10);
        assert_eq!(c.frame, 10);
    }

    #[test]
    fn a_reversed_range_is_ordered_rather_than_refused() {
        let mut c = SceneClock::default();
        c.set_range(50, 10);
        assert_eq!(c.effective_range(), (10, 50));
    }

    #[test]
    fn a_one_frame_range_does_not_spin() {
        let mut c = clock(7, 7, LoopMode::Loop);
        c.play();
        assert!(!c.advance());
        assert_eq!(c.frame, 7);
    }

    #[test]
    fn fps_is_clamped_to_something_divisible() {
        let mut c = SceneClock::default();
        c.set_fps(0.0);
        assert!(c.effective_fps() >= MIN_FPS);
        c.set_fps(f64::NAN);
        assert_eq!(c.effective_fps(), DEFAULT_FPS);
        c.set_fps(10_000.0);
        assert_eq!(c.effective_fps(), MAX_FPS);
    }

    #[test]
    fn settings_round_trip_without_carrying_session_state() {
        let mut c = clock(5, 60, LoopMode::PingPong);
        c.set_fps(30.0);
        c.set_frame(42);
        c.play();

        let saved = c.settings();
        let mut restored = SceneClock::default();
        restored.apply_settings(&saved);

        assert_eq!(restored.fps, 30.0);
        assert_eq!(restored.effective_range(), (5, 60));
        assert_eq!(restored.loop_mode, LoopMode::PingPong);
        // The reproducibility contract: a reloaded scene is stopped at the
        // start regardless of what the author was doing when they saved.
        assert!(!restored.playing);
        assert_eq!(restored.frame, 5);
    }

    #[test]
    fn loop_mode_keys_round_trip() {
        for mode in [LoopMode::Once, LoopMode::Loop, LoopMode::PingPong] {
            assert_eq!(LoopMode::from_key(mode.key()), Some(mode));
        }
        assert_eq!(LoopMode::from_key("nonsense"), None);
    }
}
