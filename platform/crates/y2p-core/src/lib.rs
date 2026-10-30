//! Primitives every subcommand shares, each matching the Python platform's `common.py` it was ported
//! from byte for byte (platform/CONTRACT.md, "Rules every subcommand
//! shares"): device paths below a root (`ctx.path`), bounded text reads (`ctx.read`), `json.dumps`
//! output, `atomic_json` records, flock and the kernel clocks.
// SPDX-License-Identifier: GPL-2.0-only
pub mod clock;
pub mod command;
pub mod devices_support;
pub mod digest;
pub mod json;
pub mod lenient;
pub mod lock;
pub mod mounts;
pub mod observation_support;
pub mod os_text;
pub mod paths;
pub mod power_charger;
pub mod power_identity;
pub mod power_journal;
pub mod power_text;
pub mod py;
pub mod record;
#[cfg(any(test, feature = "testing"))]
pub mod recorded;
pub mod reports_sqlite;
pub mod root;
pub mod storage_media;
pub mod suspend_text;
pub mod sys;
pub mod text;
pub mod volume;

pub use root::Root;

/// Widen a C field whose type is narrower on 32-bit ARM (time_t, tm_gmtoff, nlink_t, …) than on the
/// host, without a cast that is useless on one of the two targets.
pub fn wide<T: Into<i64>>(value: T) -> i64 {
    value.into()
}

#[doc(hidden)]
pub use regex as __regex;

/// A constant regular expression, compiled on first use: `&'static Regex`.
#[macro_export]
macro_rules! regex {
    ($pattern:expr) => {{
        static PATTERN: std::sync::LazyLock<$crate::__regex::Regex> =
            std::sync::LazyLock::new(|| $crate::__regex::Regex::new($pattern).expect("pattern"));
        &*PATTERN
    }};
}
