//! What the dashboard is about: one render, as its own stream describes it.
//!
//! # Nothing here decides how far along a render is
//!
//! Every number below either arrives on [`RenderProgress`] or is arithmetic on
//! numbers that did. The rule the three surfaces share is that the render says
//! how far along it is and a surface says how that looks, and the moment a
//! surface starts counting for itself there are two answers to one question.
//!
//! Stage durations are the one thing measured here rather than reported, and
//! they are measured rather than derived because they are wall time between
//! events the stream already sends. That is presentation: it says how long
//! something took, not how much is left.

use std::time::{Duration, Instant};

use solarxy_render::RenderProgress;

/// How many throughput readings the sparkline keeps.
///
/// About the width of a panel at the sizes this runs at, so the series is the
/// picture rather than something sampled down to fit it.
const THROUGHPUT_HISTORY: usize = 96;

/// The least time between two throughput readings.
///
/// The drive loop does not yield while a tile's readback is pending, so the
/// sink is called far faster than anything changes. Without a floor the series
/// would be a hundred readings of the same instant, and dividing by that
/// interval gives a rate with no meaning.
const THROUGHPUT_INTERVAL: Duration = Duration::from_millis(250);

/// A named step of a render, in the order they happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Loading,
    Cooking,
    BuildingHierarchy,
    Drawing,
    Writing,
    Done,
    Failed,
}

impl Stage {
    /// The word the timings panel puts in its left column.
    pub fn name(self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::Cooking => "cooking",
            Self::BuildingHierarchy => "hierarchy",
            Self::Drawing => "drawing",
            Self::Writing => "writing",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    fn of(progress: &RenderProgress) -> Self {
        match progress {
            RenderProgress::Loading => Self::Loading,
            RenderProgress::Cooking { .. } => Self::Cooking,
            RenderProgress::BuildingHierarchy { .. } => Self::BuildingHierarchy,
            RenderProgress::Sampling { .. } => Self::Drawing,
            RenderProgress::Writing { .. } => Self::Writing,
            RenderProgress::Done { .. } => Self::Done,
            RenderProgress::Failed { .. } => Self::Failed,
        }
    }
}

/// The picture so far, as cells can be asked to draw it.
///
/// Reduced to one dimension here rather than at draw time, because the terminal
/// can carry only one and doing it once a tile is cheaper than once a frame.
/// See [`super::picture`] for what the reduction is and why it is grey.
pub struct Picture {
    pub width: u32,
    pub height: u32,
    /// Luminance, one byte a pixel, already reduced from whatever the render
    /// handed over. Kept rather than the four-channel original because the
    /// terminal can only carry one dimension of it and reducing once per tile
    /// is cheaper than once per frame.
    pub luma: Vec<u8>,
}

/// What the reader asked for, which does not change while it runs.
pub struct Request {
    pub input: String,
    pub output: String,
    /// What the reader asked for, or nothing when the scene decides.
    ///
    /// Every one of these is optional for the same reason: the render node is
    /// authoritative and a flag is an override, so before the cook the command
    /// line genuinely does not know what is about to be rendered. The size
    /// arrives with the first tile, which is the first moment anything does.
    pub engine: Option<String>,
    pub size: Option<(u32, u32)>,
    pub samples: Option<u32>,
    pub bounces: Option<u32>,
    pub seed: Option<u32>,
}

/// One render, as the dashboard reads it.
pub struct RenderView {
    pub request: Request,
    pub stage: Stage,
    /// Every stage that has finished, and how long it took.
    pub timings: Vec<(Stage, Duration)>,
    /// How long the stage now running has been running.
    ///
    /// Kept beside the finished ones rather than computed at draw time,
    /// because a panel is handed what it reads and asking it to find out what
    /// time it is would be the one place a readout consulted something other
    /// than its subject.
    pub stage_elapsed: Duration,
    /// The cook's own pass counter, while cooking.
    pub cook: Option<(u32, u32)>,
    pub triangles: Option<u64>,
    /// The plan: how many tiles, and the shape they are laid out in.
    pub tiles: u32,
    pub columns: u32,
    pub rows: u32,
    /// Which tile is being drawn and how far into it.
    pub tile: u32,
    pub sample: u32,
    pub tile_samples: u32,
    pub elapsed: Duration,
    /// Samples a second, most recent last.
    pub throughput: Vec<u64>,
    pub picture: Option<Picture>,
    /// Set when the reader asked to stop, so the surface can say so before the
    /// render notices.
    pub cancelling: bool,

    /// When the stage now running began.
    stage_began: Instant,
    /// The last throughput reading: when, and how many samples had been drawn.
    last_rate: Option<(Instant, u64)>,
}

impl RenderView {
    pub fn new(request: Request) -> Self {
        Self {
            request,
            stage: Stage::Loading,
            timings: Vec::new(),
            stage_elapsed: Duration::ZERO,
            cook: None,
            triangles: None,
            tiles: 0,
            columns: 0,
            rows: 0,
            tile: 0,
            sample: 0,
            tile_samples: 0,
            elapsed: Duration::ZERO,
            throughput: Vec::new(),
            picture: None,
            cancelling: false,
            stage_began: Instant::now(),
            last_rate: None,
        }
    }

    /// Fold one event in.
    pub fn observe(&mut self, progress: &RenderProgress, now: Instant) {
        let stage = Stage::of(progress);
        if stage != self.stage {
            // The stage that just ended is recorded once. A render never
            // returns to a stage it has left, so there is nothing to merge.
            self.timings
                .push((self.stage, now.saturating_duration_since(self.stage_began)));
            self.stage = stage;
            self.stage_began = now;
        }
        self.stage_elapsed = now.saturating_duration_since(self.stage_began);

        match *progress {
            RenderProgress::Cooking { pass, passes } => self.cook = Some((pass, passes)),
            RenderProgress::BuildingHierarchy { triangles } => self.triangles = Some(triangles),
            RenderProgress::Sampling {
                tile,
                tiles,
                columns,
                rows,
                sample,
                samples,
                elapsed_ms,
            } => {
                self.tile = tile;
                self.tiles = tiles;
                self.columns = columns;
                self.rows = rows;
                self.sample = sample;
                self.tile_samples = samples;
                self.elapsed = Duration::from_millis(elapsed_ms);
                self.take_rate(now);
            }
            RenderProgress::Done { elapsed_ms } => {
                self.elapsed = Duration::from_millis(elapsed_ms);
            }
            // Nothing to fold in: the stage change above is the whole of
            // what these three say.
            RenderProgress::Loading
            | RenderProgress::Writing { .. }
            | RenderProgress::Failed { .. } => {}
        }
    }

    /// How many samples of the whole picture have been drawn.
    ///
    /// Tiles converge one at a time, so the tiles behind this one are whole
    /// and this one is however far into itself it says.
    pub fn samples_drawn(&self) -> u64 {
        u64::from(self.tile) * u64::from(self.tile_samples) + u64::from(self.sample)
    }

    /// How many there will be in total, or zero before drawing starts.
    pub fn samples_total(&self) -> u64 {
        u64::from(self.tiles) * u64::from(self.tile_samples)
    }

    /// The fraction of the whole render that is drawn, from zero to one.
    pub fn fraction(&self) -> f64 {
        let total = self.samples_total();
        if total == 0 {
            return 0.0;
        }
        self.samples_drawn() as f64 / total as f64
    }

    /// How much longer, from the rate so far, or nothing while there is not
    /// enough to say.
    ///
    /// Whole-run average rather than recent rate: tiles are the same size and
    /// the same cost, so the average is the better predictor and does not
    /// lurch when one tile happens to be sky.
    pub fn remaining(&self) -> Option<Duration> {
        let done = self.fraction();
        if done <= 0.0 || done >= 1.0 || self.elapsed.is_zero() {
            return None;
        }
        let total = self.elapsed.as_secs_f64() / done;
        Duration::try_from_secs_f64(total - self.elapsed.as_secs_f64()).ok()
    }

    /// The current tile's own progress, for the cell being drawn.
    pub fn tile_fraction(&self) -> f64 {
        if self.tile_samples == 0 {
            return 0.0;
        }
        f64::from(self.sample) / f64::from(self.tile_samples)
    }

    /// A reading, if enough time has passed for one to mean anything.
    fn take_rate(&mut self, now: Instant) {
        let drawn = self.samples_drawn();
        let Some((then, before)) = self.last_rate else {
            self.last_rate = Some((now, drawn));
            return;
        };
        let span = now.saturating_duration_since(then);
        if span < THROUGHPUT_INTERVAL {
            return;
        }
        let rate = (drawn.saturating_sub(before) as f64 / span.as_secs_f64()).round();
        self.throughput.push(rate.max(0.0) as u64);
        if self.throughput.len() > THROUGHPUT_HISTORY {
            self.throughput.remove(0);
        }
        self.last_rate = Some((now, drawn));
    }
}

/// Seconds, at a width a reader can compare down a column.
pub fn seconds(span: Duration) -> String {
    format!("{:.1}s", span.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sampling(tile: u32, sample: u32, elapsed_ms: u64) -> RenderProgress {
        RenderProgress::Sampling {
            tile,
            tiles: 4,
            columns: 2,
            rows: 2,
            sample,
            samples: 64,
            elapsed_ms,
        }
    }

    fn view() -> RenderView {
        RenderView::new(Request {
            input: "scene.slxy".into(),
            output: "out.png".into(),
            engine: Some("path traced".into()),
            size: Some((320, 240)),
            samples: Some(64),
            bounces: None,
            seed: None,
        })
    }

    /// Progress spans the whole picture, not the tile being drawn. A reader
    /// watching a four-tile render should not see the bar reach the end four
    /// times.
    #[test]
    fn the_bar_measures_the_picture_and_not_the_tile() {
        let mut view = view();
        let now = Instant::now();
        view.observe(&sampling(0, 32, 100), now);
        assert!(
            (view.fraction() - 0.125).abs() < 1e-9,
            "{}",
            view.fraction()
        );
        view.observe(&sampling(3, 64, 800), now);
        assert!((view.fraction() - 1.0).abs() < 1e-9, "{}", view.fraction());
    }

    /// A stage is timed once, when it ends, and the render's own stages are
    /// the only ones that appear.
    #[test]
    fn each_stage_is_recorded_once_as_it_ends() {
        let mut view = view();
        let start = Instant::now();
        view.observe(&RenderProgress::Loading, start);
        view.observe(
            &RenderProgress::Cooking {
                pass: 1,
                passes: 12,
            },
            start + Duration::from_millis(40),
        );
        view.observe(
            &RenderProgress::Cooking {
                pass: 2,
                passes: 12,
            },
            start + Duration::from_millis(90),
        );
        view.observe(&sampling(0, 0, 100), start + Duration::from_millis(120));
        view.observe(
            &RenderProgress::Writing {
                output: "out.png".into(),
            },
            start + Duration::from_millis(500),
        );

        let names: Vec<&str> = view.timings.iter().map(|(s, _)| s.name()).collect();
        assert_eq!(names, ["loading", "cooking", "drawing"]);
        assert_eq!(view.stage, Stage::Writing);
        // Cooking spanned the two passes rather than being restarted by the
        // second one.
        assert!(
            view.timings[1].1 >= Duration::from_millis(80),
            "{view:?}",
            view = view.timings[1].1.as_millis()
        );
    }

    /// The estimate is a rate, so it needs a rate: nothing before drawing has
    /// started and nothing once it has finished.
    #[test]
    fn an_estimate_appears_only_while_there_is_something_to_estimate() {
        let mut view = view();
        let now = Instant::now();
        assert!(view.remaining().is_none(), "estimated before drawing");
        view.observe(&sampling(1, 0, 1000), now);
        let left = view.remaining().expect("an estimate while drawing");
        // A quarter drawn in one second means three more.
        assert!(
            (left.as_secs_f64() - 3.0).abs() < 0.05,
            "{}s",
            left.as_secs_f64()
        );
        view.observe(&sampling(3, 64, 4000), now);
        assert!(view.remaining().is_none(), "estimated after finishing");
    }

    /// A reading needs a span to divide by. The drive loop calls the sink far
    /// faster than anything changes, and dividing by that interval would put
    /// a meaningless number on the screen.
    #[test]
    fn throughput_ignores_readings_too_close_together() {
        let mut view = view();
        let start = Instant::now();
        for i in 0..50u64 {
            view.observe(&sampling(0, i as u32, i), start + Duration::from_millis(i));
        }
        assert!(
            view.throughput.is_empty(),
            "a rate was taken over fifty milliseconds"
        );
        view.observe(&sampling(0, 50, 400), start + Duration::from_millis(400));
        assert_eq!(view.throughput.len(), 1);
    }
}
