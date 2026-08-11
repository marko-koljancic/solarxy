//! The plain progress sink: one line on standard error, and the rules for when
//! to redraw it.
//!
//! The sink automation reads. A render farm sees this one and nothing else, so
//! it has two jobs and they pull against each other: tell a person watching a
//! terminal what is happening now, and leave a continuous-integration log that
//! is readable after the fact. A carriage return serves the first and ruins the
//! second, since a log file keeps every overwrite as a line of its own.
//!
//! So the stream is one thing and the presentation is two. On a terminal the
//! line is rewritten in place as often as the render reports. Anywhere else it
//! is written once per **step**, where a step is a stage or, while drawing, a
//! tile: a thousand sample counts collapse to one line per tile, which is what
//! someone reading the log afterwards wanted.
//!
//! Standard error, never standard output. That stream carries the image when
//! the output is a pipe, and the report when the result is asked for as data.

use std::io::Write;

use solarxy_render::RenderProgress;

/// What a step is, for deciding whether a non-terminal sink writes again.
///
/// Coarser than the event: every sample of one tile is the same step, because
/// the alternative is a log with a line per sample.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Step {
    Loading,
    Cooking,
    BuildingHierarchy,
    /// Carries the tile, so moving to the next one is a new step.
    Sampling(u32),
    Writing,
    Ended,
}

fn step_of(progress: &RenderProgress) -> Step {
    match progress {
        RenderProgress::Loading => Step::Loading,
        RenderProgress::Cooking { .. } => Step::Cooking,
        RenderProgress::BuildingHierarchy { .. } => Step::BuildingHierarchy,
        RenderProgress::Sampling { tile, .. } => Step::Sampling(*tile),
        RenderProgress::Writing { .. } => Step::Writing,
        RenderProgress::Done { .. } | RenderProgress::Failed { .. } => Step::Ended,
    }
}

/// What one event says, in a line.
///
/// `live` is whether the line will be rewritten. It changes exactly one thing,
/// the sample count, and for a good reason: a log writes its line when a step
/// **begins**, where the count is always zero, so printing "sample 0 of 64"
/// once and never again reads as a render that stalled. A terminal rewrites the
/// line as the count climbs, where it is the most useful thing on it.
fn line(progress: &RenderProgress, live: bool) -> String {
    match progress {
        RenderProgress::Loading => "loading".to_string(),
        RenderProgress::Cooking { pass, passes } => format!("cooking, pass {pass} of {passes}"),
        RenderProgress::BuildingHierarchy { triangles } => {
            format!("building the ray hierarchy over {triangles} triangles")
        }
        RenderProgress::Sampling {
            tile,
            tiles,
            sample,
            samples,
            elapsed_ms,
        } => {
            let seconds = *elapsed_ms as f64 / 1000.0;
            if live {
                format!(
                    "tile {} of {tiles}, sample {sample} of {samples}, {seconds:.1}s",
                    tile + 1
                )
            } else {
                format!(
                    "tile {} of {tiles}, {samples} samples, {seconds:.1}s",
                    tile + 1
                )
            }
        }
        RenderProgress::Writing { output } => format!("writing {output}"),
        RenderProgress::Done { elapsed_ms } => {
            format!("done in {:.1}s", *elapsed_ms as f64 / 1000.0)
        }
        RenderProgress::Failed { stage } => format!("failed while {stage}"),
    }
}

/// Writes progress as a line, to whatever it is given.
///
/// Generic over the writer so a test can hand it a buffer; the binary hands it
/// standard error.
pub struct PlainSink<W: Write> {
    out: W,
    /// Whether to rewrite one line in place. False for a pipe or a file.
    interactive: bool,
    last: Option<Step>,
    /// How wide the line being overwritten was, so a shorter one following a
    /// longer one does not leave the tail of the longer behind.
    painted: usize,
}

impl<W: Write> PlainSink<W> {
    pub fn new(out: W, interactive: bool) -> Self {
        Self {
            out,
            interactive,
            last: None,
            painted: 0,
        }
    }

    /// Reports one event. Failures to write are dropped: a sink that panicked
    /// because a pipe closed would take the render with it.
    pub fn report(&mut self, progress: &RenderProgress) {
        let step = step_of(progress);
        if !self.interactive {
            if self.last == Some(step) {
                return;
            }
            self.last = Some(step);
            let _ = writeln!(self.out, "{}", line(progress, false));
            let _ = self.out.flush();
            return;
        }
        self.last = Some(step);
        let text = line(progress, true);
        let pad = self.painted.saturating_sub(text.chars().count());
        self.painted = text.chars().count();
        let _ = write!(self.out, "\r{text}{:pad$}", "", pad = pad);
        // A run that has ended stops owning the line, so whatever writes next
        // starts on its own.
        if step == Step::Ended {
            let _ = writeln!(self.out);
            self.painted = 0;
        }
        let _ = self.out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sampling(tile: u32, sample: u32) -> RenderProgress {
        RenderProgress::Sampling {
            tile,
            tiles: 4,
            sample,
            samples: 64,
            elapsed_ms: 1500,
        }
    }

    fn drain(interactive: bool, events: &[RenderProgress]) -> String {
        let mut sink = PlainSink::new(Vec::new(), interactive);
        for e in events {
            sink.report(e);
        }
        String::from_utf8(sink.out).expect("utf-8")
    }

    /// The property a continuous-integration log depends on: a render that
    /// reports a thousand times does not write a thousand lines.
    #[test]
    fn a_log_gets_one_line_per_step_however_often_the_render_reports() {
        let mut events = vec![RenderProgress::Loading];
        for sample in 0..64 {
            events.push(sampling(0, sample));
        }
        for sample in 0..64 {
            events.push(sampling(1, sample));
        }
        events.push(RenderProgress::Done { elapsed_ms: 2000 });

        let written = drain(false, &events);
        let lines: Vec<&str> = written.lines().collect();
        assert_eq!(
            lines.len(),
            4,
            "expected loading, two tiles and a completion, got {lines:?}"
        );
        assert!(lines[1].contains("tile 1 of 4"));
        assert!(lines[2].contains("tile 2 of 4"));
        assert!(!written.contains('\r'), "a log carries no carriage returns");
    }

    /// And the property a person watching depends on: one line, rewritten.
    #[test]
    fn a_terminal_gets_one_line_rewritten_in_place() {
        let written = drain(
            true,
            &[
                RenderProgress::Loading,
                sampling(0, 1),
                sampling(0, 2),
                RenderProgress::Done { elapsed_ms: 2000 },
            ],
        );
        assert_eq!(
            written.matches('\r').count(),
            4,
            "every report should return to the start of the line"
        );
        // Exactly one line break, at the end, where the run stops owning the
        // line it has been painting.
        assert_eq!(written.matches('\n').count(), 1);
        assert!(written.ends_with('\n'));
    }

    /// A short line after a long one leaves nothing of the long one behind.
    #[test]
    fn a_shorter_line_erases_the_one_it_replaces() {
        let written = drain(
            true,
            &[
                RenderProgress::BuildingHierarchy { triangles: 123_456 },
                RenderProgress::Loading,
            ],
        );
        let long = line(
            &RenderProgress::BuildingHierarchy { triangles: 123_456 },
            true,
        );
        let short = line(&RenderProgress::Loading, true);
        let padding = long.chars().count() - short.chars().count();
        assert!(
            written.ends_with(&format!("\r{short}{}", " ".repeat(padding))),
            "the shorter line did not blank the tail of the longer: {written:?}"
        );
    }

    /// The failure closes the line rather than leaving the last progress on it.
    #[test]
    fn a_failure_ends_the_line_and_names_the_step() {
        let written = drain(
            true,
            &[sampling(0, 1), RenderProgress::Failed { stage: "drawing" }],
        );
        assert!(written.contains("failed while drawing"));
        assert!(written.ends_with('\n'));
    }
}
