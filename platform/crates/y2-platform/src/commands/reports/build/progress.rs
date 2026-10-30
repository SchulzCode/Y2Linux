//! The owner's screen during a report run: /run/y2/report-progress.json, rewritten at most five
//! times a second, in RAM.
// SPDX-License-Identifier: GPL-2.0-only
use super::{Cell, PathBuf, Root, PROGRESS};
pub use y2p_contract::reports::ReportPhase;
use y2p_contract::reports::ReportProgress;

/// The bytes an upload or a save has done of its whole (the progress record shows them).
#[derive(Clone, Copy, Default)]
pub struct Bytes {
    pub done: Option<usize>,
    pub total: Option<usize>,
}

impl Bytes {
    /// None: the collect, scrub and pack steps.
    pub const NONE: Bytes = Bytes {
        done: None,
        total: None,
    };

    pub fn of(done: usize, total: usize) -> Bytes {
        Bytes {
            done: Some(done),
            total: Some(total),
        }
    }

    /// Only the whole (a save is one write).
    pub fn total(total: usize) -> Bytes {
        Bytes {
            done: None,
            total: Some(total),
        }
    }
}

/// Live progress of one report run for the owner's screen: the step (collect, scrub, pack, then
/// upload or save), the whole run's percent and an estimate of the seconds left. At most five writes
/// a second, in RAM.
pub struct Progress {
    /// Where the record goes and whose clock times it; none for a run nobody watches.
    sink: Option<Sink>,
}

struct Sink {
    root: Root,
    path: PathBuf,
    destination: Option<String>,
    start: f64,
    written: Cell<f64>,
}

impl Progress {
    pub fn new(root: &Root, destination: Option<&str>) -> Progress {
        Progress {
            sink: Some(Sink {
                root: root.clone(),
                path: root.path(PROGRESS),
                destination: destination.map(str::to_owned),
                start: root.clock().monotonic(),
                written: Cell::new(0.0),
            }),
        }
    }

    /// A run nobody watches (the automatic checks): nothing is written.
    pub fn quiet() -> Progress {
        Progress { sink: None }
    }

    /// The step `phase` is `fraction` done; written at most every 0.2 s unless `force`d (a step's
    /// start and end).
    pub fn update(&self, phase: ReportPhase, fraction: f64, force: bool, bytes: Bytes) {
        let Some(sink) = &self.sink else {
            return;
        };
        let now = sink.root.clock().monotonic();
        if !force && now - sink.written.get() < 0.2 {
            return;
        }
        sink.written.set(now);
        let (low, high) = phase.span();
        let percent = low + (high - low) * fraction.clamp(0.0, 1.0);
        let elapsed = now - sink.start;
        // From 15 % on the run so far is a fair estimate of the rest.
        let seconds_left = (percent >= 15.0)
            .then(|| (elapsed * (100.0 - percent) / percent).round_ties_even() as i64);
        let record = ReportProgress {
            schema: 1,
            phase,
            // `round(percent, 1)`: the formatter rounds the exact binary value, as Python does.
            percent: format!("{percent:.1}").parse().unwrap_or(percent),
            seconds_left,
            destination: sink.destination.clone(),
            bytes_done: bytes.done.map(|n| n as u64),
            bytes_total: bytes.total.map(|n| n as u64),
        };
        // The screen goes without; the report does not.
        if let Some(parent) = sink.path.parent() {
            if std::fs::create_dir_all(parent).is_ok() || parent.is_dir() {
                let _ = y2p_core::record::atomic_json(&sink.path, &record, false, 0o600);
            }
        }
    }

    pub fn close(&self) {
        if let Some(sink) = &self.sink {
            let _ = std::fs::remove_file(&sink.path);
        }
    }
}
