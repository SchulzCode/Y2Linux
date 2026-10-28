//! The kernel clocks the records use: CLOCK_MONOTONIC (Python `time.monotonic`, deadlines and
//! `monotonic_ns`) and CLOCK_BOOTTIME (which also counts suspended time: battery state, the sleep retry
//! gate, "did the player sleep" as boottime minus monotonic).
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

#[cfg(test)]
mod tests {
    #[test]
    fn boottime_never_runs_behind_monotonic() {
        let monotonic = super::monotonic_ns();
        assert!(super::boottime_ns() >= monotonic);
        assert!(super::monotonic_ns() >= monotonic);
    }
}
