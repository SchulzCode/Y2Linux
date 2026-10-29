//! The kernel clocks the records use: CLOCK_MONOTONIC (Python `time.monotonic`, deadlines and
//! `monotonic_ns`) and CLOCK_BOOTTIME (which also counts suspended time: battery state, the sleep retry
//! gate, "did the player sleep" as boottime minus monotonic), and the wall clock for dates.
// SPDX-License-Identifier: GPL-2.0-only

fn read(clock: libc::clockid_t) -> u64 {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `now` is a valid, writable timespec and both clock ids exist on Linux.
    let result = unsafe { libc::clock_gettime(clock, &mut now) };
    assert_eq!(result, 0, "clock_gettime");
    now.tv_sec as u64 * 1_000_000_000 + now.tv_nsec as u64
}

/// `time.monotonic_ns()`.
pub fn monotonic_ns() -> u64 {
    read(libc::CLOCK_MONOTONIC)
}

/// `time.clock_gettime_ns(time.CLOCK_BOOTTIME)`.
pub fn boottime_ns() -> u64 {
    read(libc::CLOCK_BOOTTIME)
}

/// `time.monotonic()`.
pub fn monotonic() -> f64 {
    monotonic_ns() as f64 / 1e9
}

/// The wall clock in nanoseconds since the epoch (0 before it), as `SystemTime` reads it.
pub fn wall_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
}

/// The clocks a root reads (`root.clock()`): the kernel's, or a test's.
pub trait Clock: Send + Sync {
    fn monotonic_ns(&self) -> u64;
    fn boottime_ns(&self) -> u64;

    /// The wall clock (CLOCK_REALTIME): what a record's dates and a retry's time are made of.
    fn wall_ns(&self) -> u64 {
        wall_ns()
    }

    /// `time.monotonic()`.
    fn monotonic(&self) -> f64 {
        self.monotonic_ns() as f64 / 1e9
    }

    /// Pause a polling loop for `duration` (the kernel's clock sleeps; a test's clock only moves
    /// on, so a bounded wait on it ends without real time passing).
    fn sleep(&self, duration: std::time::Duration) {
        std::thread::sleep(duration);
    }
}

/// CLOCK_MONOTONIC and CLOCK_BOOTTIME.
pub struct Kernel;

impl Clock for Kernel {
    fn monotonic_ns(&self) -> u64 {
        monotonic_ns()
    }

    fn boottime_ns(&self) -> u64 {
        boottime_ns()
    }
}

/// A test's clock: it stands still until the test advances it; `suspend` moves only boottime and the
/// wall clock (time spent asleep). Its wall clock starts at the epoch unless given (`with_wall`).
#[derive(Debug, Default)]
pub struct Manual {
    monotonic_ns: std::sync::atomic::AtomicU64,
    boottime_ns: std::sync::atomic::AtomicU64,
    wall_ns: std::sync::atomic::AtomicU64,
}

impl Manual {
    pub fn at(monotonic_ns: u64, boottime_ns: u64) -> Self {
        Self {
            monotonic_ns: monotonic_ns.into(),
            boottime_ns: boottime_ns.into(),
            wall_ns: 0.into(),
        }
    }

    /// This clock with its wall clock at `wall_ns` since the epoch.
    pub fn with_wall(self, wall_ns: u64) -> Self {
        self.wall_ns
            .store(wall_ns, std::sync::atomic::Ordering::SeqCst);
        self
    }

    /// Every clock runs `ns` on.
    pub fn advance(&self, ns: u64) {
        use std::sync::atomic::Ordering::SeqCst;
        self.monotonic_ns.fetch_add(ns, SeqCst);
        self.boottime_ns.fetch_add(ns, SeqCst);
        self.wall_ns.fetch_add(ns, SeqCst);
    }

    /// `ns` asleep: boottime and the wall clock run on, monotonic does not.
    pub fn suspend(&self, ns: u64) {
        use std::sync::atomic::Ordering::SeqCst;
        self.boottime_ns.fetch_add(ns, SeqCst);
        self.wall_ns.fetch_add(ns, SeqCst);
    }
}

impl Clock for Manual {
    fn monotonic_ns(&self) -> u64 {
        self.monotonic_ns.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn boottime_ns(&self) -> u64 {
        self.boottime_ns.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn wall_ns(&self) -> u64 {
        self.wall_ns.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// A sleep on a test's clock: every clock runs on by `duration`, at once.
    fn sleep(&self, duration: std::time::Duration) {
        self.advance(u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX));
    }
}

/// The wall clock in seconds since the epoch (`time.time()`); 0 before 1970.
pub fn wall() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::Clock;

    #[test]
    fn a_manual_clock_moves_only_when_told() {
        let clock = super::Manual::at(5_000_000_000, 7_000_000_000).with_wall(9_000_000_000);
        clock.advance(1_000_000_000);
        clock.suspend(3_000_000_000);
        assert_eq!(clock.monotonic(), 6.0);
        assert_eq!(clock.boottime_ns(), 11_000_000_000);
        assert_eq!(clock.wall_ns(), 13_000_000_000);
        assert!(super::Kernel.wall_ns() > 1_700_000_000_000_000_000);
        clock.sleep(std::time::Duration::from_millis(250));
        assert_eq!(clock.monotonic(), 6.25);
        assert_eq!(clock.wall_ns(), 13_250_000_000);
        let monotonic = super::Kernel.monotonic_ns();
        assert!(super::Kernel.boottime_ns() >= monotonic);
    }

    #[test]
    fn boottime_never_runs_behind_monotonic() {
        let monotonic = super::monotonic_ns();
        assert!(super::boottime_ns() >= monotonic);
        assert!(super::monotonic_ns() >= monotonic);
    }
}
