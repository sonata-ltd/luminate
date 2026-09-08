//! Frame timing for the examples: how long each frame took, and which ones
//! were slow.
//!
//! Three things keep the measurement honest.
//!
//! `iced::window::frames()` asks for a redraw every frame, so a bench that is
//! always on turns a demo into a busy loop and erases the difference between
//! an idle application and a working one. This one is off unless [`ENV`] says
//! otherwise, the way [`Autoplay`](crate::Autoplay) is.
//!
//! A slow frame is measured against the frame period the application is
//! actually running at, kept as a moving average, rather than against a
//! constant: 16.7 ms is a stall at 120 Hz and an ordinary frame at 60 Hz.
//!
//! The first frames are dropped. A window pays for its surface, its pipelines
//! and its first glyphs once, and that says nothing about the frames after it.
//!
//! A stall prints a line as it happens, which is what a bench is for; the
//! distribution behind those lines is [`Bench::report`], printed on drop and
//! available at any time.

use std::fmt;

use iced::Subscription;
use iced::time::Instant;

/// The environment variable that enables the bench when set to `1`.
pub const ENV: &str = "LUMINATE_BENCH";

/// A frame this many times slower than the baseline is a stall.
const STALL_FACTOR: f64 = 2.0;

/// How fast the baseline follows the frame period. Low enough that a burst of
/// slow frames cannot quietly raise the bar it is judged against.
const BASELINE_RATE: f64 = 0.05;

/// Frames dropped before anything is recorded; the last of them seeds the
/// baseline.
const WARMUP: usize = 5;

/// Stalls listed in a report; the rest are counted only.
const MAX_STALLS: usize = 20;

/// One frame's cost.
#[derive(Debug, Clone, Copy)]
struct Sample {
    /// Which frame it was, counted from the first tick.
    frame: usize,
    /// How long it took, in milliseconds.
    ms: f64,
}

/// Times the gap between frames.
///
/// Read [`ENV`] once with [`Bench::from_env`] in the application's
/// constructor, hand [`Bench::subscription`] to `subscription()` and every
/// timestamp to [`Bench::tick`]. Disabled, it subscribes to nothing and the
/// application redraws exactly as it would without it.
#[derive(Debug, Default)]
pub struct Bench {
    enabled: bool,
    last: Option<Instant>,
    frames: usize,
    baseline: f64,
    samples: Vec<Sample>,
    stalls: Vec<Sample>,
}

impl Bench {
    /// A bench that is on when `enabled` is `true`.
    #[must_use]
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            last: None,
            frames: 0,
            baseline: 0.0,
            samples: Vec::new(),
            stalls: Vec::new(),
        }
    }

    /// Reads [`ENV`] once.
    #[must_use]
    pub fn from_env() -> Self {
        Self::new(std::env::var(ENV).is_ok_and(|value| value == "1"))
    }

    /// Whether the bench is on.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Frames seen so far, warm-up included.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// The frame period the application is running at, in milliseconds.
    ///
    /// Zero until the warm-up is over.
    #[must_use]
    pub fn baseline_ms(&self) -> f64 {
        self.baseline
    }

    /// Frame timestamps while enabled; nothing otherwise.
    ///
    /// Map the `Instant` to the example's own message and hand it to
    /// [`Bench::tick`].
    pub fn subscription(&self) -> Subscription<Instant> {
        if self.enabled {
            iced::window::frames()
        } else {
            Subscription::none()
        }
    }

    /// Records one frame, and prints a line if it was a stall.
    ///
    /// A stall leaves the baseline alone: the bar is the speed the application
    /// runs at, not the speed it has slowed down to.
    pub fn tick(&mut self, now: Instant) {
        let previous = self.last.replace(now);
        self.frames += 1;

        let Some(previous) = previous else {
            return;
        };

        let ms = (now - previous).as_secs_f64() * 1000.0;

        if self.frames <= WARMUP {
            self.baseline = ms;
            return;
        }

        let sample = Sample {
            frame: self.frames,
            ms,
        };

        self.samples.push(sample);

        if self.baseline > 0.0 && ms > self.baseline * STALL_FACTOR {
            self.stalls.push(sample);

            eprintln!(
                "bench: frame {:<6} {:>8.1}ms  ({:.1}x the {:.1}ms baseline)",
                sample.frame,
                ms,
                ms / self.baseline,
                self.baseline
            );
        } else {
            self.baseline += BASELINE_RATE * (ms - self.baseline);
        }
    }

    /// What the frames looked like. Printable.
    #[must_use]
    pub fn report(&self) -> Report {
        Report::new(&self.samples, &self.stalls)
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        if self.enabled && !self.samples.is_empty() {
            eprintln!("{}", self.report());
        }
    }
}

/// The distribution of frame times, and the frames that stood out.
///
/// Two runs are compared by their median and p95; a one-off cost — a page
/// shown for the first time, a font rasterised for the first time — shows up
/// as a single stall against an unchanged median.
#[derive(Debug, Clone)]
pub struct Report {
    frames: usize,
    median: f64,
    p95: f64,
    max: f64,
    stalls: Vec<Sample>,
}

impl Report {
    /// Summarises the samples, warm-up already dropped.
    fn new(samples: &[Sample], stalls: &[Sample]) -> Self {
        let mut sorted: Vec<f64> = samples.iter().map(|sample| sample.ms).collect();
        sorted.sort_by(f64::total_cmp);

        Self {
            frames: samples.len(),
            median: percentile(&sorted, 0.50),
            p95: percentile(&sorted, 0.95),
            max: sorted.last().copied().unwrap_or(0.0),
            stalls: stalls.to_vec(),
        }
    }

    /// Frames the report is built from.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// The median frame, in milliseconds: the application's real frame period.
    #[must_use]
    pub fn median_ms(&self) -> f64 {
        self.median
    }

    /// The 95th percentile frame, in milliseconds.
    #[must_use]
    pub fn p95_ms(&self) -> f64 {
        self.p95
    }

    /// The slowest frame, in milliseconds.
    #[must_use]
    pub fn max_ms(&self) -> f64 {
        self.max
    }

    /// How many frames were stalls. Which ones they were is in the [`Display`]
    /// output.
    ///
    /// [`Display`]: fmt::Display
    #[must_use]
    pub fn stall_count(&self) -> usize {
        self.stalls.len()
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.frames == 0 {
            return write!(f, "bench: no frames recorded");
        }

        write!(
            f,
            "bench: {} frames  median {:.1}ms  p95 {:.1}ms  max {:.1}ms",
            self.frames, self.median, self.p95, self.max
        )?;

        if self.stalls.is_empty() {
            return write!(f, "\nbench: no stalls");
        }

        write!(
            f,
            "\nbench: {} stall{}",
            self.stalls.len(),
            if self.stalls.len() == 1 { "" } else { "s" }
        )?;

        for stall in self.stalls.iter().take(MAX_STALLS) {
            write!(f, "\nbench:   frame {:<6} {:>8.1}ms", stall.frame, stall.ms)?;
        }

        if let Some(rest) = self.stalls.len().checked_sub(MAX_STALLS).filter(|n| *n > 0) {
            write!(f, "\nbench:   … {rest} more")?;
        }

        Ok(())
    }
}

/// The nearest-rank percentile of an ascending slice; `0.0` when it is empty.
fn percentile(sorted: &[f64], quantile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }

    let rank = (quantile * sorted.len() as f64).ceil().max(1.0) as usize;

    sorted[rank.min(sorted.len()) - 1]
}
