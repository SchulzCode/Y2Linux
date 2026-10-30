//! The records an update reads besides its payloads: the native core's journal and answers
//! (`status`, `core`) and download.json as the Software Update page sees it.
// SPDX-License-Identifier: GPL-2.0-only
use super::check::LastCheck;
use super::staging::Download;
use super::{channel, command, fs, sorted, string, truthy, Map, Path, Raise, Result, Root, Value};
use serde::{Deserialize, Serialize};
pub(super) use y2p_core::json::python_error as json_error;
use y2p_core::lenient;
use y2p_core::py::PyValue;

/// Download.json, with a download or check whose process is gone shown as
/// interrupted. A user's player (8.17) said "Downloading Update… Keep Wi-Fi on." for a day: the record
/// a download writes first stays `Staging` when the download dies without reaching its own handler
/// (power loss, a kill), and that page offered neither Cancel nor Check for Updates; a restart kept
/// it (it is on /data).
pub(crate) fn download_record(root: &Root) -> Option<Download> {
    let record = Download::read(root)?;
    if matches!(record.state.as_str(), "Staging" | "Checking")
        && !lock_held(root, &root.path("/data/updates/.stage.lock"))
    {
        return Some(Download::failed("download_interrupted"));
    }
    Some(record)
}

/// `update status`: the core's journal (Idle on a ready Y2DATA without one, Unavailable without a
/// ready Y2DATA), with the download and check records and the channel's name.
#[derive(Serialize, Clone, Debug)]
pub struct Status {
    /// The core's journal (the platform's stand-in without one).
    #[serde(flatten)]
    pub journal: CoreJournal,
    /// What a readable journal is shown with.
    #[serde(flatten)]
    pub records: Option<Records>,
}

/// The records `update status` shows next to a readable journal.
#[derive(Serialize, Clone, Debug)]
pub struct Records {
    pub download: Option<Download>,
    pub last_check: Option<LastCheck>,
    pub channel: Option<String>,
    pub full_system: bool,
    pub physical_qualification: bool,
}

/// The journal's fields the platform reads (each lenient).
#[derive(Deserialize, Default)]
#[serde(default)]
struct Head {
    /// Compared as a number (1 or 2; `2.0` too): an update in progress must never read as none.
    schema: Value,
    #[serde(deserialize_with = "y2p_core::lenient::text")]
    state: Option<String>,
}

impl Status {
    /// The journal's failure text (none without one).
    pub fn failure(&self) -> Option<&str> {
        self.journal.failure()
    }
}

/// The native update core's journal (`/data/updates/state.json`, written by y2-update-core): the
/// fields the platform and Reborn read in their types (a field of another type is not shown); its
/// install records (backup, timings, outcome, …) as the core wrote them.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct CoreJournal {
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::integer"
    )]
    pub schema: Option<i64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    pub state: Option<String>,
    /// `null` without a failure.
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "failure")]
    failure: Option<Option<String>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::integer"
    )]
    pub sequence: Option<i64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::integer"
    )]
    pub previous_sequence: Option<i64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    pub release_version: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::or_none"
    )]
    pub rollback_allowed: Option<bool>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    pub y2linux_commit: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    pub manifest_sha256: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    pub kept_manifest_sha256: Option<String>,
    /// The core's install records, as it wrote them.
    #[serde(flatten)]
    pub records: Map<String, Value>,
}

fn failure<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<Option<String>>, D::Error> {
    lenient::text(deserializer).map(Some)
}

impl CoreJournal {
    /// The platform's stand-in for a journal (no update yet, an unreadable one, no Y2DATA).
    fn stand_in(state: &str, failure: Option<&str>) -> CoreJournal {
        CoreJournal {
            schema: Some(1),
            state: Some(state.into()),
            failure: Some(failure.map(str::to_owned)),
            ..CoreJournal::default()
        }
    }

    fn of(map: Map<String, Value>) -> CoreJournal {
        lenient::read(map)
    }

    /// The failure text (none without one).
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_ref().and_then(Option::as_deref)
    }
}

/// The journal as an object of schema 1 or 2 (the native core writes 2 since OTA-35, BOOTIMG and
/// root); none for a journal of another schema or shape.
fn journal(root: &Root) -> Option<Map<String, Value>> {
    let map = root.json_object("/data/updates/state.json");
    let head: Head = serde_json::from_value(Value::Object(map.clone())).ok()?;
    let schema = Some(&head.schema);
    (y2p_core::py::python_eq_int(schema, 1) || y2p_core::py::python_eq_int(schema, 2))
        .then_some(map)
}

pub fn status(root: &Root) -> Status {
    let path = root.path("/data/updates/state.json");
    let missing = root.json("/data/updates/state.json").is_none()
        && !path.exists()
        && path.symlink_metadata().is_err();
    let journal = if missing {
        let ready = root.read("/data/.y2data-schema").as_deref() == Some("1")
            && root.path("/data/updates").is_dir();
        if ready {
            CoreJournal::stand_in("Idle", None)
        } else {
            CoreJournal::stand_in("Unavailable", Some("data_state_unavailable"))
        }
    } else {
        match journal(root) {
            Some(map) => CoreJournal::of(map),
            None => CoreJournal::stand_in("Failed", Some("journal_unreadable")),
        }
    };
    let unreadable = !missing && journal.failure() == Some("journal_unreadable");
    Status {
        journal,
        records: (!unreadable).then(|| Records {
            download: download_record(root),
            last_check: LastCheck::load(root),
            channel: channel(root).name,
            full_system: true,
            physical_qualification: false,
        }),
    }
}

/// Whether any live process holds a flock on `path` (/proc/locks names the device and inode).
fn lock_held(root: &Root, path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| !y2p_core::lock::flock_pids(root, &meta).is_empty())
}

/// One action of the native core; its refusal becomes the error text.
pub fn core(
    root: &Root,
    action: &str,
    directory: Option<&str>,
    timeout: f64,
) -> Result<Map<String, Value>> {
    let mut argv = vec!["/usr/sbin/y2-update-core", action];
    argv.extend(directory);
    let result = command::run_with(root, &argv, timeout, 20000, true);
    if !result.ok {
        let refusal = serde_json::from_str::<Value>(
            result
                .output
                .as_deref()
                .filter(|o| !o.is_empty())
                .unwrap_or("{}"),
        )
        .ok()
        .and_then(|v| v.get("failure").cloned())
        .filter(truthy);
        return Err(Raise::Value(match refusal {
            Some(value) => sorted(value).py_str(),
            None => format!(
                "update_core_{}",
                result
                    .reason
                    .as_deref()
                    .filter(|r| !r.is_empty())
                    .unwrap_or("refused")
            ),
        }));
    }
    let output = result.output.unwrap_or_default();
    match serde_json::from_str::<Value>(&output) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(Raise::value("invalid_update_core_response")),
        Err(error) => Err(Raise::Value(json_error(&output, &error))),
    }
}

pub(super) fn installed_release(root: &Root) -> Value {
    Value::from(crate::commands::versions(root).and_then(|v| v.release_version))
}

/// The update journal's `state`, as the native core and this platform write it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JournalState {
    /// No update: none yet on a ready /data, or the last one ended.
    Idle,
    /// No journal and no ready /data.
    Unavailable,
    /// Staged and verified, waiting for `update apply`.
    Queued,
    /// The first boot of an installed update, until `health-ack`.
    PendingHealth,
    RollbackPending,
    Acknowledged,
    RolledBack,
    /// Also a journal of another schema, or no readable journal.
    Failed,
    /// A text no version writes.
    Other(String),
    /// A journal of a known schema without a text state.
    Absent,
}

impl JournalState {
    /// The state a journal's `state` text names (`None`: no text).
    pub fn named(text: Option<&str>) -> JournalState {
        match text {
            Some("Idle") => JournalState::Idle,
            Some("Unavailable") => JournalState::Unavailable,
            Some("Queued") => JournalState::Queued,
            Some("PendingHealth") => JournalState::PendingHealth,
            Some("RollbackPending") => JournalState::RollbackPending,
            Some("Acknowledged") => JournalState::Acknowledged,
            Some("RolledBack") => JournalState::RolledBack,
            Some("Failed") => JournalState::Failed,
            Some(other) => JournalState::Other(other.to_owned()),
            None => JournalState::Absent,
        }
    }

    /// No update runs: a new check, stage, maintenance or sleep may start (`TERMINAL`).
    pub fn terminal(&self) -> bool {
        matches!(
            self,
            JournalState::Idle
                | JournalState::Acknowledged
                | JournalState::RolledBack
                | JournalState::Failed
        )
    }

    /// An installed update is between its restart and its verdict.
    pub fn installing(&self) -> bool {
        matches!(
            self,
            JournalState::Queued | JournalState::PendingHealth | JournalState::RollbackPending
        )
    }
}

/// The update journal's state as other work gates on it (`Idle` when there is no journal yet on a
/// ready /data, `Unavailable` without one, `Failed` for a journal of another schema; the core writes
/// schema 2 since OTA-35, BOOTIMG and root); the same state `update status` shows, without the
/// download, lock and channel reads that status adds.
pub(crate) fn state(root: &Root) -> JournalState {
    let path = root.path("/data/updates/state.json");
    let value = root.json("/data/updates/state.json");
    if value.is_none() && !path.exists() && path.symlink_metadata().is_err() {
        let ready = root.read("/data/.y2data-schema").as_deref() == Some("1")
            && root.path("/data/updates").is_dir();
        return if ready {
            JournalState::Idle
        } else {
            JournalState::Unavailable
        };
    }
    match value.and_then(|_| journal(root)) {
        Some(map) => JournalState::named(string(map.get("state"))),
        None => JournalState::Failed,
    }
}
