//! Autosave: a rotating set of scene files in the platform data directory,
//! and the offer to recover one on the next launch.
//!
//! **Plain files replace the browser's origin-private ring.** The browser
//! rotates archives through origin-private storage because it has no
//! filesystem; this shell writes files. What is copied is the behaviour, not
//! the mechanism: the same cadence, the same ring of three, the same whole
//! archive per write so recovery is one load, and the same offer on the next
//! launch.
//!
//! **The cadence is the browser's rule, line for line.** An edit arms a
//! write at the debounce, and a write is forced at the ceiling under
//! continuous editing, so a pause is covered by the first and someone who
//! never pauses by the second (`web/src/engine/session.ts`, `autosaveDelayMs`).
//! [`autosave_delay_ms`] is that function and a drift test reads the
//! browser's constants.
//!
//! **A write never blocks the interface.** The archive is assembled on the
//! main thread, which is the same trade the browser makes, and the disk
//! write runs on its own thread with the result polled per frame. One
//! failure toasts once, and the next success clears the toast's memory.
//!
//! **The ring exists only after a crash.** An explicit save, a clean quit
//! and a chosen discard all clear it, so the offer on launch appears only
//! when work was lost without the user choosing to lose it. A restored
//! document is dirty from the start, whatever its origin, because nothing on
//! disk holds it once the ring is cleared.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Instant, SystemTime};

use super::State;
use super::document::write_atomically;
use crate::gui::{RecoveryChoice, ToastSeverity};

/// How many autosaves are kept.
pub(super) const RING: usize = 3;
/// A write is forced this long after the previous one under continuous
/// editing.
pub(super) const FORCE_MS: u64 = 15_000;
/// The debounce never drops below this, whatever the preference says.
pub(super) const MIN_DEBOUNCE_MS: u64 = 500;

/// A `saved_revision` no engine revision equals, marking a document that
/// nothing on disk holds: the one a recovery restored.
pub(super) const NEVER_SAVED: u64 = u64::MAX;

/// How long after an edit at `now_ms` the next autosave is due, or `None`
/// when autosave is off. The browser's `autosaveDelayMs`.
pub(super) fn autosave_delay_ms(
    enabled: bool,
    debounce_secs: f32,
    now_ms: u64,
    last_save_ms: u64,
) -> Option<u64> {
    if !enabled {
        return None;
    }
    let debounce = ((debounce_secs.max(0.0) * 1000.0).round() as u64).max(MIN_DEBOUNCE_MS);
    let force_in = FORCE_MS.saturating_sub(now_ms.saturating_sub(last_save_ms));
    Some(debounce.min(force_in))
}

/// Whether a write is due at `now_ms`, given the last edit and the last
/// write.
///
/// The browser arms one timer at each edit and re-arms it at the next, so
/// the write lands at the last edit plus the delay computed there; this
/// asks the same question without a timer.
pub(super) fn autosave_due(
    enabled: bool,
    debounce_secs: f32,
    now_ms: u64,
    last_edit_ms: Option<u64>,
    last_save_ms: u64,
) -> bool {
    let Some(edit) = last_edit_ms else {
        return false;
    };
    match autosave_delay_ms(enabled, debounce_secs, edit, last_save_ms) {
        Some(delay) => now_ms >= edit.saturating_add(delay),
        None => false,
    }
}

/// What a launch found in the ring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Recovered {
    /// The autosaved archive.
    pub path: PathBuf,
    /// The document's own path when it had one, or empty.
    pub origin: String,
    pub when: SystemTime,
}

/// The ring on disk: `autosave-N.slxy` beside `autosave-N.path`, and a
/// cursor naming the newest slot.
#[derive(Debug, Clone)]
pub(super) struct AutosaveRing {
    dir: PathBuf,
}

impl AutosaveRing {
    pub(super) fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// The ring under the platform data directory, or `None` where the
    /// platform reports none, in which case autosave is silently off.
    pub(super) fn default_location() -> Option<Self> {
        solarxy_core::preferences::data_dir().map(|d| Self::new(d.join("autosave")))
    }

    fn slot(&self, i: usize) -> PathBuf {
        self.dir.join(format!("autosave-{i}.slxy"))
    }

    fn origin(&self, i: usize) -> PathBuf {
        self.dir.join(format!("autosave-{i}.path"))
    }

    fn cursor(&self) -> PathBuf {
        self.dir.join("cursor")
    }

    fn read_cursor(&self) -> Option<usize> {
        std::fs::read_to_string(self.cursor())
            .ok()?
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|i| *i < RING)
    }

    /// Write one archive into the next slot and point the cursor at it.
    pub(super) fn write(&self, bytes: &[u8], origin: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let next = self.read_cursor().map_or(0, |i| (i + 1) % RING);
        write_atomically(&self.slot(next), bytes)?;
        std::fs::write(self.origin(next), origin)?;
        std::fs::write(self.cursor(), next.to_string())
    }

    /// The newest archive, by the cursor when it names one that exists and
    /// by modification time otherwise, which is what survives a cursor the
    /// crash left half-written.
    pub(super) fn newest(&self) -> Option<Recovered> {
        let found = |i: usize| -> Option<Recovered> {
            let path = self.slot(i);
            let when = std::fs::metadata(&path).ok()?.modified().ok()?;
            let origin = std::fs::read_to_string(self.origin(i)).unwrap_or_default();
            Some(Recovered {
                path,
                origin: origin.trim().to_string(),
                when,
            })
        };
        if let Some(hit) = self.read_cursor().and_then(found) {
            return Some(hit);
        }
        (0..RING).filter_map(found).max_by_key(|r| r.when)
    }

    /// Remove every slot, origin and the cursor. A slot that is already gone
    /// is not an error.
    pub(super) fn clear(&self) -> std::io::Result<()> {
        let tolerate = |r: std::io::Result<()>| match r {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        };
        for i in 0..RING {
            tolerate(std::fs::remove_file(self.slot(i)))?;
            tolerate(std::fs::remove_file(self.origin(i)))?;
        }
        tolerate(std::fs::remove_file(self.cursor()))
    }
}

/// The shell's autosave bookkeeping.
pub(crate) struct AutosaveState {
    ring: Option<AutosaveRing>,
    epoch: Instant,
    /// When the newest unsaved edit arrived, in ms since `epoch`; `None`
    /// when everything since the last write is written.
    last_edit_ms: Option<u64>,
    /// When the last write started.
    last_write_ms: u64,
    /// The engine revision last seen, so an edit is noticed once.
    seen_revision: u64,
    /// The write in flight, if any.
    writer: Option<mpsc::Receiver<Result<(), String>>>,
    /// Whether the last write failed, so one failure toasts once.
    failed: bool,
    /// What the launch found and the prompt is asking about.
    recovery: Option<Recovered>,
}

impl AutosaveState {
    pub(super) fn new() -> Self {
        Self {
            ring: AutosaveRing::default_location(),
            epoch: Instant::now(),
            last_edit_ms: None,
            last_write_ms: 0,
            seen_revision: 0,
            writer: None,
            failed: false,
            recovery: None,
        }
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// A document was installed: nothing is pending against it yet.
    pub(super) fn reset(&mut self, revision: u64) {
        self.seen_revision = revision;
        self.last_edit_ms = None;
    }
}

impl State {
    /// One frame of the autosave: notice an edit, start a write when one is
    /// due, and collect the result of the write before it.
    pub(super) fn poll_autosave(&mut self) {
        if let Some(rx) = &self.autosave.writer {
            match rx.try_recv() {
                Ok(Ok(())) => {
                    self.autosave.writer = None;
                    self.autosave.failed = false;
                }
                Ok(Err(e)) => {
                    self.autosave.writer = None;
                    if !self.autosave.failed {
                        self.autosave.failed = true;
                        self.gui
                            .set_toast(&format!("Autosave failed: {e}"), ToastSeverity::Warning);
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => self.autosave.writer = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }

        let Some(revision) = self.engine.as_ref().map(|e| e.revision()) else {
            return;
        };
        let now = self.autosave.now_ms();
        if revision != self.autosave.seen_revision {
            self.autosave.seen_revision = revision;
            // A change that leaves the document at its saved state is not an
            // edit worth writing, which is what makes an explicit save quiet
            // the ring rather than restart it.
            self.autosave.last_edit_ms = self.is_dirty().then_some(now);
        }
        if self.autosave.writer.is_some() || self.autosave.ring.is_none() {
            return;
        }
        let prefs = self.preferences.autosave;
        if !autosave_due(
            prefs.enabled,
            prefs.debounce_secs,
            now,
            self.autosave.last_edit_ms,
            self.autosave.last_write_ms,
        ) {
            return;
        }

        let sidecar = self.scene_sidecar();
        let Some(engine) = &self.engine else {
            return;
        };
        let bytes = match engine.save_slxy(&sidecar) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.autosave.last_edit_ms = None;
                if !self.autosave.failed {
                    self.autosave.failed = true;
                    self.gui
                        .set_toast(&format!("Autosave failed: {e}"), ToastSeverity::Warning);
                }
                return;
            }
        };
        let origin = self
            .engine_scene
            .as_ref()
            .map_or_else(String::new, |s| s.path.clone());
        let Some(ring) = self.autosave.ring.clone() else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(ring.write(&bytes, &origin).map_err(|e| e.to_string()));
        });
        self.autosave.writer = Some(rx);
        self.autosave.last_write_ms = now;
        self.autosave.last_edit_ms = None;
    }

    /// Drop the ring: what is on disk holds the document, or the user chose
    /// to lose the changes.
    pub(super) fn clear_autosaves(&mut self) {
        self.autosave.last_edit_ms = None;
        if let Some(ring) = &self.autosave.ring
            && let Err(e) = ring.clear()
        {
            tracing::warn!("Could not clear the autosave ring: {e}");
        }
    }

    /// On the way out. A clean document has nothing to recover; a dirty one
    /// is leaving through the guard's Discard, which cleared the ring
    /// itself, or through a failure that should keep it.
    pub fn flush_autosave_on_exit(&mut self) {
        if !self.is_dirty() {
            self.clear_autosaves();
        }
    }

    /// Offer the newest autosave, once, when the ring holds one.
    pub(super) fn check_recovery_on_launch(&mut self) {
        let Some(found) = self.autosave.ring.as_ref().and_then(AutosaveRing::newest) else {
            return;
        };
        let name = Path::new(&found.origin)
            .file_name()
            .and_then(|n| n.to_str())
            .map_or_else(|| "an untitled scene".to_string(), str::to_string);
        let when = describe_time(found.when);
        self.gui.open_recovery_prompt(&name, &when);
        self.autosave.recovery = Some(found);
    }

    /// The prompt's answer.
    pub(super) fn resolve_recovery(&mut self, choice: RecoveryChoice) {
        let Some(found) = self.autosave.recovery.take() else {
            return;
        };
        match choice {
            RecoveryChoice::Discard => self.clear_autosaves(),
            RecoveryChoice::Restore => self.restore_recovery(&found),
        }
    }

    /// Open the autosaved archive as the document, at its origin path when
    /// it had one, and dirty either way.
    fn restore_recovery(&mut self, found: &Recovered) {
        let bytes = match std::fs::read(&found.path) {
            Ok(b) => b,
            Err(e) => {
                self.gui.set_toast(
                    &format!("Could not read the autosave: {e}"),
                    ToastSeverity::Error,
                );
                return;
            }
        };
        let filename = Path::new(&found.origin)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Untitled")
            .to_string();
        if !self.adopt_scene_bytes(&bytes, &filename, &found.origin) {
            return;
        }
        self.saved_revision = NEVER_SAVED;
        self.refresh_title();
        self.gui
            .set_toast("Recovered unsaved work", ToastSeverity::Success);
    }
}

/// A moment, in the local clock, as the recovery prompt says it.
fn describe_time(when: SystemTime) -> String {
    let stamp = time::OffsetDateTime::from(when);
    let local = time::UtcOffset::current_local_offset().map_or(stamp, |o| stamp.to_offset(o));
    local
        .format(&time::macros::format_description!(
            "[year]-[month]-[day] [hour]:[minute]"
        ))
        .unwrap_or_else(|_| "an earlier session".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("solarxy-autosave-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// The delay is the browser's: off means never, the debounce has a
    /// floor, and the ceiling wins when it is nearer than the debounce.
    #[test]
    fn the_delay_is_the_browsers_rule() {
        assert_eq!(autosave_delay_ms(false, 2.0, 5_000, 0), None);
        assert_eq!(
            autosave_delay_ms(true, 2.0, 1_000, 0),
            Some(2_000),
            "the debounce"
        );
        assert_eq!(
            autosave_delay_ms(true, 0.1, 1_000, 0),
            Some(500),
            "the floor"
        );
        assert_eq!(
            autosave_delay_ms(true, 2.0, 14_500, 0),
            Some(500),
            "the ceiling is nearer"
        );
        assert_eq!(
            autosave_delay_ms(true, 2.0, 40_000, 0),
            Some(0),
            "long past the ceiling"
        );
        assert_eq!(autosave_delay_ms(true, 2.0, 40_000, 39_000), Some(2_000));
    }

    /// A write is due at the debounce after a pause and at the ceiling under
    /// continuous editing, and never with nothing edited or autosave off.
    #[test]
    fn an_autosave_is_due_at_the_debounce_and_at_the_ceiling() {
        assert!(!autosave_due(true, 2.0, 10_000, None, 0), "nothing edited");
        assert!(
            !autosave_due(false, 2.0, 10_000, Some(1_000), 0),
            "autosave off"
        );
        // A pause: the edit at 1 s is written at 3 s.
        assert!(!autosave_due(true, 2.0, 2_999, Some(1_000), 0));
        assert!(autosave_due(true, 2.0, 3_000, Some(1_000), 0));
        // Continuous editing: an edit at 14 s is written at the 15 s ceiling,
        // not two seconds later.
        assert!(!autosave_due(true, 2.0, 14_999, Some(14_000), 0));
        assert!(autosave_due(true, 2.0, 15_000, Some(14_000), 0));
        // The ceiling counts from the last write, not from launch.
        assert!(!autosave_due(true, 2.0, 30_999, Some(30_000), 20_000));
        assert!(autosave_due(true, 2.0, 32_000, Some(30_000), 20_000));
    }

    /// The desktop's constants are the browser's. Read from the TypeScript
    /// rather than restated, so the two cannot drift apart silently.
    #[test]
    fn the_cadence_constants_are_the_browsers() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let session = std::fs::read_to_string(root.join("web/src/engine/session.ts"))
            .expect("the browser's session module is beside this crate");
        let start = session
            .find("export function autosaveDelayMs")
            .expect("the browser's delay rule is named autosaveDelayMs");
        let body: String = session[start..].chars().take(400).collect();
        assert!(
            body.contains(&format!("Math.max({MIN_DEBOUNCE_MS},")),
            "the browser's debounce floor is not {MIN_DEBOUNCE_MS}:\n{body}"
        );
        assert!(
            body.contains(&format!("{FORCE_MS} -")),
            "the browser's ceiling is not {FORCE_MS}:\n{body}"
        );
        let prefs = std::fs::read_to_string(root.join("web/src/store/prefs.ts"))
            .expect("the browser's prefs module");
        let default = solarxy_core::preferences::AutosavePrefs::default();
        assert!(
            prefs.contains(&format!(
                "autosave: {{ enabled: {}, debounceSec: {} }}",
                default.enabled, default.debounce_secs
            )),
            "the browser's default autosave preference differs from this shell's"
        );
    }

    /// Three slots, the oldest replaced, the cursor and the origin naming
    /// the newest, and a clear that leaves nothing.
    #[test]
    fn the_ring_keeps_three_and_replaces_the_oldest() {
        let dir = scratch("ring");
        let ring = AutosaveRing::new(dir.clone());
        assert_eq!(ring.newest(), None, "an empty ring offers nothing");

        for (i, bytes) in [b"a", b"b", b"c", b"d"].iter().enumerate() {
            ring.write(*bytes, &format!("/scenes/{i}.slxy"))
                .expect("write");
        }
        let slots: Vec<PathBuf> = (0..RING)
            .map(|i| ring.slot(i))
            .filter(|p| p.exists())
            .collect();
        assert_eq!(slots.len(), RING);
        assert_eq!(
            ring.read_cursor(),
            Some(0),
            "the fourth write wrapped to slot 0"
        );

        let newest = ring.newest().expect("something to recover");
        assert_eq!(std::fs::read(&newest.path).expect("read"), b"d");
        assert_eq!(newest.origin, "/scenes/3.slxy");
        assert_eq!(
            std::fs::read(ring.slot(1)).expect("read"),
            b"b",
            "the oldest survivor"
        );

        ring.clear().expect("clear");
        assert_eq!(ring.newest(), None);
        assert!(!ring.cursor().exists());
        assert!((0..RING).all(|i| !ring.slot(i).exists() && !ring.origin(i).exists()));
        ring.clear()
            .expect("clearing an empty ring is not an error");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// With the cursor gone or wrong, the newest slot is found by its time,
    /// which is what a crash mid-write leaves behind.
    #[test]
    fn without_a_cursor_the_newest_slot_is_found_by_time() {
        let dir = scratch("cursorless");
        let ring = AutosaveRing::new(dir.clone());
        ring.write(b"old", "").expect("write");
        ring.write(b"new", "/scenes/x.slxy").expect("write");
        let base = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        for (i, offset) in [(0, 10), (1, 20)] {
            std::fs::File::options()
                .write(true)
                .open(ring.slot(i))
                .expect("open")
                .set_modified(base + std::time::Duration::from_secs(offset))
                .expect("set modified");
        }
        std::fs::write(ring.cursor(), "9").expect("a cursor naming no slot");
        let newest = ring.newest().expect("found by time");
        assert_eq!(std::fs::read(&newest.path).expect("read"), b"new");
        assert_eq!(newest.origin, "/scenes/x.slxy");

        std::fs::remove_file(ring.cursor()).expect("remove");
        assert_eq!(ring.newest().map(|r| r.path), Some(ring.slot(1)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
