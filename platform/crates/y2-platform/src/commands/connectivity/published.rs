//! `/run/y2/network.json`: the Wi-Fi observation the service daemon publishes every pass, with its
//! stamp (boot, monotonic time, schema), the supplicant events and the count of failed supplicant
//! queries. Read back by the service daemon itself (the count), the connectivity supervisor (F53),
//! `status wifi` (the fresh signal reading), the update check and download (Online) and Reborn
//! (stale after 15 s, an Online record after 45 s).
// SPDX-License-Identifier: GPL-2.0-only
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use y2p_core::{
    json::Object,
    lenient::{self, BootId},
    py, Root,
};

/// The record as written: the observation's fields, then the stamp (sorted when written, so the field
/// order here is free; a stamp field replaces an observation field of the same name).
#[derive(Serialize)]
pub struct Record<'a> {
    #[serde(flatten)]
    pub observation: &'a Object,
    pub status_query_failures: QueryFailures,
    pub schema: u8,
    pub boot_id: Option<&'a str>,
    pub monotonic_s: f64,
    pub supplicant_events: Value,
}

/// How many supplicant queries failed this boot while the radio was present. A record of an older
/// build may hold a bool there, which is kept as it is until the next failure counts it (owner
/// decision 2026-10-30: keep that tolerance).
#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(untagged)]
pub enum QueryFailures {
    Flag(bool),
    Count(i64),
}

impl Default for QueryFailures {
    fn default() -> Self {
        QueryFailures::Count(0)
    }
}

impl QueryFailures {
    /// One failure more (a bool counts as 0 or 1 first).
    pub fn counted(self) -> QueryFailures {
        QueryFailures::Count(match self {
            QueryFailures::Flag(flag) => i64::from(flag) + 1,
            QueryFailures::Count(count) => count.saturating_add(1),
        })
    }

    /// A previous record's value: a bool as it is, an integer from 0 up; anything else none.
    fn lenient<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Self>, D::Error> {
        let value = Value::deserialize(deserializer)?;
        Ok(match value {
            Value::Bool(flag) => Some(QueryFailures::Flag(flag)),
            _ => py::exact_int(Some(&value))
                .filter(|&count| count >= 0)
                .map(QueryFailures::Count),
        })
    }
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
    #[serde(deserialize_with = "lenient::truthy")]
    pub radio_present: bool,
    #[serde(deserialize_with = "QueryFailures::lenient")]
    pub status_query_failures: Option<QueryFailures>,
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
    use super::{Published, QueryFailures};
    use serde_json::json;

    fn failures(value: serde_json::Value) -> Option<QueryFailures> {
        Published::of(
            json!({ "status_query_failures": value })
                .as_object()
                .unwrap()
                .clone(),
        )
        .status_query_failures
    }

    #[test]
    fn query_failures_keep_an_old_bool_and_refuse_odd_values() {
        assert_eq!(failures(json!(true)), Some(QueryFailures::Flag(true)));
        assert_eq!(failures(json!(4)), Some(QueryFailures::Count(4)));
        assert_eq!(failures(json!(-1)), None);
        assert_eq!(failures(json!(2.0)), None);
        assert_eq!(failures(json!("3")), None);
        assert_eq!(QueryFailures::Flag(true).counted(), QueryFailures::Count(2));
        assert_eq!(
            QueryFailures::Flag(false).counted(),
            QueryFailures::Count(1)
        );
        assert_eq!(
            serde_json::to_string(&QueryFailures::Flag(true)).unwrap(),
            "true"
        );
        assert_eq!(
            serde_json::to_string(&QueryFailures::Count(3)).unwrap(),
            "3"
        );
    }
}
