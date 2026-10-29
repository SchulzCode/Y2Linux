//! Helpers of the devices commands that other groups use too: the evidence record a measured or
//! user-visible operation carries, its UTC ISO 8601 timestamp, and SHA-256.
// SPDX-License-Identifier: GPL-2.0-only
use crate::{
    json::{Object, ObjectExt},
    Root,
};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

pub use sha2::{Digest, Sha256};

/// The wall clock as ISO 8601 UTC with microseconds: `2026-10-28T20:25:20.108044+00:00`, without the
/// fraction when the microseconds are zero (the records' established timestamp form).
pub fn utc_isoformat_now() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    utc_isoformat(now.as_secs() as i64, now.subsec_micros())
}

/// The ISO 8601 text of a UTC time given as seconds and microseconds since 1970.
pub fn utc_isoformat(seconds: i64, micros: u32) -> String {
    let (days, second) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let clock = format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        second / 3600,
        second / 60 % 60,
        second % 60
    );
    if micros == 0 {
        format!("{clock}+00:00")
    } else {
        format!("{clock}.{micros:06}+00:00")
    }
}

/// The evidence record of `workload` with its `parameters` (none: `{}`), keys in this order: when
/// and on which build it ran; `result` and `failure` are filled in when it ends.
pub fn record(root: &Root, workload: &str, parameters: Value) -> Object {
    let versions = root.json_object("/etc/y2linux/versions.json");
    let version = |key: &str| versions.value(key);
    let parameters = match parameters {
        Value::Null => json!({}),
        other => other,
    };
    Object::new()
        .with("wall_timestamp", utc_isoformat_now())
        .with("monotonic_ns", root.clock().monotonic_ns())
        .with("boot_id", json!(root.boot_id()))
        .with("y2linux_commit", version("build_git_commit"))
        .with("reborn_commit", version("reborn_source_commit"))
        .with("kernel", json!(root.read("/proc/sys/kernel/osrelease")))
        .with("rootfs_release", version("rootfs_version"))
        .with("build_id", json!(root.read("/etc/y2linux/build-id")))
        .with("workload", workload)
        .with("parameters", parameters)
        .with("units", json!({}))
        .with("result", Value::Null)
        .with("failure", Value::Null)
        .with("evidence_level", "IMPLEMENTED")
}

/// A finished digest as lowercase hex.
pub fn hex(digest: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(digest.len() * 2);
    for byte in digest {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 0xf)]));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_iso_8601_utc() {
        // The expected texts are Python's datetime isoformat of these instants (the earlier writer).
        assert_eq!(
            utc_isoformat(1_788_000_500, 250_000),
            "2026-08-29T10:48:20.250000+00:00"
        );
        assert_eq!(utc_isoformat(0, 0), "1970-01-01T00:00:00+00:00");
        assert_eq!(
            utc_isoformat(951_782_400, 1),
            "2000-02-29T00:00:00.000001+00:00"
        );
    }

    #[test]
    fn digests_are_lowercase_hex() {
        assert_eq!(
            hex(&Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
