//! `/run/y2/network.json`: the Wi-Fi observation the service daemon publishes every pass, with its
//! stamp (boot, monotonic time, schema), the supplicant events and the count of failed supplicant
//! queries. Read back by the service daemon itself (the count), the connectivity supervisor (F53),
//! `status wifi` (the fresh signal reading), the update check and download (Online) and Reborn
//! (stale after 15 s, an Online record after 45 s).
// SPDX-License-Identifier: GPL-2.0-only
use serde::{Deserialize, Serialize};
use serde_json::Value;
use y2p_core::{
    json::Object,
    lenient::{self, BootId},
    Root,
};

/// The record as written: the observation's fields, then the stamp (sorted when written, so the field
/// order here is free; a stamp field replaces an observation field of the same name).
#[derive(Serialize)]
pub struct Record<'a> {
    #[serde(flatten)]
    pub observation: &'a Object,
    /// How many supplicant queries failed this boot while the radio was present.
    pub status_query_failures: u64,
    pub schema: u8,
    pub boot_id: Option<&'a str>,
    pub monotonic_s: f64,
    pub supplicant_events: Value,
}

/// The fields readers of a published record use, read leniently (an odd field reads as absent).
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Published {
    pub boot_id: BootId,
    #[serde(deserialize_with = "lenient::number")]
    pub monotonic_s: Option<f64>,
    #[serde(deserialize_with = "lenient::text")]
    pub state: Option<String>,
    #[serde(deserialize_with = "lenient::text")]
    pub association: Option<String>,
    #[serde(deserialize_with = "lenient::is_true")]
    pub radio_present: bool,
    #[serde(deserialize_with = "lenient::count")]
    pub status_query_failures: Option<u64>,
}

impl Published {
    /// The record at `/run/y2/network.json` (all absent when it is missing or no object).
    pub fn read(root: &Root) -> Published {
        Published::of(root.json_object("/run/y2/network.json"))
    }

    /// The fields of a parsed record.
    pub fn of(record: Object) -> Published {
        lenient::read(record)
    }

    /// Whether the record was written this boot.
    pub fn is_this_boot(&self, boot: Option<&str>) -> bool {
        self.boot_id.is(boot)
    }

    /// Whether the record was renewed less than `seconds` ago (`0 <= now - monotonic_s < seconds`).
    pub fn fresh(&self, root: &Root, seconds: f64) -> bool {
        self.monotonic_s
            .is_some_and(|t| (0.0..seconds).contains(&(root.clock().monotonic() - t)))
    }

    /// Whether the record says Online.
    pub fn online(&self) -> bool {
        self.state.as_deref() == Some("Online")
    }
}

#[cfg(test)]
mod tests {
    use super::Published;
    use serde_json::json;

    fn failures(value: serde_json::Value) -> Option<u64> {
        Published::of(
            json!({ "status_query_failures": value })
                .as_object()
                .unwrap()
                .clone(),
        )
        .status_query_failures
    }

    #[test]
    fn query_failures_are_a_count_and_anything_else_none() {
        assert_eq!(failures(json!(4)), Some(4));
        assert_eq!(failures(json!(true)), None);
        assert_eq!(failures(json!(-1)), None);
        assert_eq!(failures(json!(2.0)), None);
        assert_eq!(failures(json!("3")), None);
    }
}
