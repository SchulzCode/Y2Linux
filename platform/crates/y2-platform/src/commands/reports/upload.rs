//! The report queue and its upload (report.py): `queue` within the daily and per-boot limits,
//! `upload_blocked`, `upload` (the NetworkTransfer workload) with its retries, `upload_now` and
//! `report send`. The request itself is `curl`'s (upload/curl.rs).
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    build::{self, Bytes, ReportIndex},
    clock, json_dict,
    state::{self, Reports, Retry, QUEUE},
    tarball,
};
use crate::reply::{mkdir_parents, Failure};
use serde::Serialize;
use std::{
    io::Write,
    os::unix::fs::OpenOptionsExt,
    time::{Duration, Instant as Monotonic},
};
use y2p_core::lenient::BootId;
use y2p_core::{command, Root};

const QUEUE_MAX: usize = 3;
const DAILY_MAX: u64 = 5;
pub const ENDPOINT: &str = "https://reports.schulzcode.dev/v1/report";
const RETRY_S: [f64; 5] = [300.0, 900.0, 3600.0, 3.0 * 3600.0, 6.0 * 3600.0];
const GIVE_UP_S: f64 = 3.0 * 86400.0;
/// Never below this on battery.
const MINIMUM_PERCENT: i64 = 15;
/// The receiver will never take this report.
const REFUSED: [i64; 6] = [400, 403, 404, 405, 411, 413];

mod curl;
use curl::post;
#[cfg(test)]
use curl::{answer, argv, headers, stream};

fn wall(root: &Root) -> f64 {
    clock::Instant::now(root).timestamp()
}

/// Build and queue one report if the owner opted in and the limits allow it.
pub fn queue(
    root: &Root,
    triggers: &[String],
    detail: &str,
    topic: Option<&str>,
    progress: &build::Progress,
) -> Result<Queued, Failure> {
    let not_queued = |reason: &'static str| Queued {
        reason: Some(reason),
        ..Queued::default()
    };
    let mut value = Reports::load(root);
    if !value.enabled() {
        return Ok(not_queued("not_enabled"));
    }
    let Some(trigger) = triggers.first() else {
        return Err(Failure::value("unknown_trigger"));
    };
    let now = clock::Instant::now(root);
    let day = clock::date(now);
    let boot = root.boot_id().unwrap_or_default();
    value.keep_day(&day);
    if trigger != "manual" {
        if value.count(&day) >= DAILY_MAX {
            return Ok(not_queued("daily_limit"));
        }
        let reported = value.reported(&boot);
        if triggers.iter().all(|t| reported.contains(t)) {
            return Ok(not_queued("already_reported_this_boot"));
        }
    }
    let install = value.install_id.clone();
    let (data, index) = build::build(root, triggers, detail, topic, install, now, progress)?;
    let directory = root.path(QUEUE);
    mkdir_parents(&directory, 0o700)?;
    let report_id = index.report_id.clone().unwrap_or_default();
    let name = format!("{}-{report_id}.tar.gz", clock::queue_stamp(now));
    let temporary = directory.join(format!(".{name}"));
    let written = (|| -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o666)
            .custom_flags(libc::O_CLOEXEC)
            .open(&temporary)?;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        file.write_all(&data)?;
        file.sync_all()
    })();
    written.map_err(|e| super::os(e, &temporary))?;
    std::fs::rename(&temporary, directory.join(&name)).map_err(|e| super::os(e, &temporary))?;
    let mut all = state::queued(root);
    all.sort();
    let keep = all.len().saturating_sub(QUEUE_MAX);
    for old in &all[..keep] {
        crate::reply::unlink_missing_ok(old)?;
    }
    let count = value.count(&day) + 1;
    value.daily.insert(day.clone(), count);
    let mut reported = value.reported(&boot).to_vec();
    reported.extend(triggers.iter().cloned());
    // Only this boot's are kept.
    value.boot_triggers = [(boot.clone(), reported)].into();
    value.last = Some(state::Last {
        report_id: Some(report_id.clone()),
        trigger: trigger.clone(),
        state: "queued".into(),
        created_utc: index.created_utc.clone(),
        bytes: data.len() as u64,
        ..state::Last::default()
    });
    value.save(root)?;
    Ok(Queued {
        queued: true,
        report_id: Some(report_id),
        bytes: Some(data.len() as u64),
        name: Some(name),
        ..Queued::default()
    })
}

/// What queuing a report came to (`report check|send|work`'s items): queued with its name, ID and
/// size, or why not; `report check` adds the triggers, `report send` the upload that followed.
#[derive(Serialize, Debug, Default)]
pub struct Queued {
    pub queued: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub triggers: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upload: Option<UploadRun>,
}

/// What one upload run did: each queued report it handled, or why it did not run.
#[derive(Serialize, Debug, Default)]
pub struct UploadRun {
    pub uploaded: Vec<UploadItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl UploadRun {
    fn not_run(reason: &str) -> UploadRun {
        UploadRun {
            uploaded: Vec::new(),
            reason: Some(reason.into()),
        }
    }
}

/// One queued report's outcome in a run.
#[derive(Serialize, Debug)]
pub struct UploadItem {
    pub name: String,
    #[serde(flatten)]
    pub outcome: Uploaded,
}

/// Why queued reports must wait now, or None. Uploads run in the
/// background: only on Wi-Fi, with a trustworthy clock (TLS), never during an update and never below
/// MINIMUM_PERCENT on battery.
fn upload_blocked(root: &Root) -> Result<Option<&'static str>, Failure> {
    if state::upload_key(root).is_none() {
        return Ok(Some("no_upload_key"));
    }
    if !Reports::load(root).enabled() {
        return Ok(Some("not_enabled"));
    }
    let boot = root.boot_id();
    let network = crate::commands::connectivity::published::Published::read(root);
    if !network.online() || !network.is_this_boot(boot.as_deref()) {
        return Ok(Some("offline"));
    }
    if !crate::commands::time::tls_ready(root) {
        return Ok(Some("clock_not_ready"));
    }
    let update: super::checks::UpdateJournal =
        serde_json::from_value(json_dict(root, "/data/updates/state.json")).unwrap_or_default();
    let quiet = update
        .state
        .as_deref()
        .is_none_or(|state| ["Idle", "Acknowledged", "RolledBack", "Failed"].contains(&state));
    if !quiet
        || root
            .path("/data/system/platform/maintenance-pending")
            .exists()
    {
        return Ok(Some("update_in_progress"));
    }
    // The battery of this boot's power record (a record without a boot counts; a battery of
    // another shape reads as no reading).
    let power = crate::commands::power::published::PowerState::read(root);
    let same_boot = matches!(power.boot_id, BootId::Null) || power.is_this_boot(root);
    let battery = power.battery.reading();
    let charging = same_boot && battery.charging_or_full();
    let percent = battery.percent.filter(|_| same_boot);
    if !charging && percent.is_none_or(|p| p < MINIMUM_PERCENT) {
        return Ok(Some("battery_low"));
    }
    Ok(None)
}

/// `_queued_index(path)`: report.json of a queued report (the first member: only it is
/// decompressed).
fn queued_index(data: &[u8]) -> Option<ReportIndex> {
    let members = tarball::members(data, true, 1, |m| m.name == "report.json").ok()?;
    let (member, body) = members.into_iter().next()?;
    if member.name != "report.json" {
        return None;
    }
    serde_json::from_slice(&body?).ok()
}

/// `report upload`: send every queued report that is due, oldest first.
///
/// A stored report (201, or 200 for one the receiver already has) leaves the queue; one the receiver
/// refuses (REFUSED) is dropped; a full day (429) waits for the next UTC day; no answer or a server
/// error retries after RETRY_S and stops this run. A report still unsent GIVE_UP_S after it was made
/// is dropped. Only one upload runs at a time.
///
/// `first` (Send Report Now) is the owner's new report: it goes first and the progress follows it;
/// once it is stored, the others go too without waiting for their retry time (the connection has
/// just worked).
pub fn upload(
    root: &Root,
    progress: Option<&build::Progress>,
    first: Option<&str>,
) -> Result<UploadRun, Failure> {
    let _lease = crate::commands::workload::Lease::take(root, "NetworkTransfer")?;
    let now = wall(root);
    if let Some(blocked) = upload_blocked(root)? {
        return Ok(UploadRun::not_run(blocked));
    }
    let directory = root.path(QUEUE);
    mkdir_parents(&directory, 0o700)?;
    let lock_path = directory.join(".upload.lock");
    let Some(_lock) =
        y2p_core::lock::try_exclusive(&lock_path).map_err(|e| super::os(e, &lock_path))?
    else {
        return Ok(UploadRun::not_run("busy"));
    };
    // The key once more, under the lock (upload_blocked read it first): a key gone meanwhile is the
    // same answer as no key.
    let Some(key) = state::upload_key(root) else {
        return Ok(UploadRun::not_run("no_upload_key"));
    };
    upload_queue(root, now, &key, progress, first)
}

/// What one queued report's run came to (`outcome`).
#[derive(Serialize, Debug)]
#[serde(tag = "outcome", rename_all = "lowercase")]
pub enum Uploaded {
    /// Stored: the receiver's own report ID (null without one).
    Sent { report_id: Option<String> },
    /// Gone from the queue without being stored.
    Dropped { reason: String },
    /// Still queued until `next`.
    Retry { reason: String, next: f64 },
}

impl Uploaded {
    fn item(self, name: String) -> UploadItem {
        UploadItem {
            name,
            outcome: self,
        }
    }
}

/// The last report shown on the owner's screen, if it is this one: its new state and fields
/// (`reason`, `next_attempt` of an earlier try dropped).
fn settle(
    value: &mut Reports,
    index: &ReportIndex,
    outcome: &str,
    change: impl FnOnce(&mut state::Last),
) {
    let id = index.report_id.as_deref();
    if let Some(last) = value
        .last
        .as_mut()
        .filter(|last| last.report_id.as_deref() == id)
    {
        last.reason = None;
        last.next_attempt = None;
        last.state = outcome.into();
        change(last);
    }
}

fn upload_queue(
    root: &Root,
    now: f64,
    key: &str,
    progress: Option<&build::Progress>,
    first: Option<&str>,
) -> Result<UploadRun, Failure> {
    let mut value = Reports::load(root);
    let mut queued = state::queued(root);
    queued.sort_by_key(|p| {
        let name = state::name(p);
        (Some(name.as_str()) != first, name)
    });
    let names: Vec<String> = queued.iter().map(|p| state::name(p)).collect();
    let mut connected = false;
    // The retries of reports still queued.
    let mut retries = std::mem::take(&mut value.uploads);
    retries.retain(|name, _| names.contains(name));
    let mut results = Vec::new();
    for path in &queued {
        let name = state::name(path);
        let entry = retries.get(&name).cloned().unwrap_or_default();
        if entry.next > now && !connected {
            continue;
        }
        let shown = if first.is_none() || first == Some(name.as_str()) {
            progress
        } else {
            None
        };
        let read = std::fs::read(path).ok();
        let index = read.as_deref().and_then(queued_index);
        let (Some(index), Some(data)) = (index, read) else {
            crate::reply::unlink_missing_ok(path)?;
            let reason = "unreadable".into();
            results.push(Uploaded::Dropped { reason }.item(name));
            continue;
        };
        let made = index
            .created_utc
            .as_deref()
            .and_then(clock::fromisoformat)
            .or_else(|| {
                use std::os::unix::fs::MetadataExt;
                std::fs::metadata(path)
                    .ok()
                    .map(|m| m.mtime() as f64 + m.mtime_nsec() as f64 * 1e-9)
            })
            .unwrap_or(0.0);
        if now - made > GIVE_UP_S {
            crate::reply::unlink_missing_ok(path)?;
            retries.remove(&name);
            settle(&mut value, &index, "dropped", |last| {
                last.reason = Some("expired".into())
            });
            let reason = "expired".into();
            results.push(Uploaded::Dropped { reason }.item(name));
            continue;
        }
        if let Some(shown) = shown {
            shown.update("upload", 0.0, true, Bytes::of(0, data.len()));
        }
        let (status, answer) = post(root, &data, &index, key, shown);
        match (status, answer) {
            (Some(200 | 201), answer) => {
                connected = connected || Some(name.as_str()) == first;
                crate::reply::unlink_missing_ok(path)?;
                retries.remove(&name);
                let sent = clock::isoformat(clock::Instant::from_float(now));
                settle(&mut value, &index, "sent", |last| {
                    last.sent_utc = Some(sent)
                });
                let report_id = answer.ok().and_then(|a| a.report_id);
                results.push(Uploaded::Sent { report_id }.item(name));
            }
            (Some(code), answer) if REFUSED.contains(&code) => {
                crate::reply::unlink_missing_ok(path)?;
                retries.remove(&name);
                let error = answer.ok().and_then(|a| a.error).unwrap_or_default();
                let reason = format!("refused_{code}_{error}");
                settle(&mut value, &index, "dropped", |last| {
                    last.reason = Some(reason.clone())
                });
                results.push(Uploaded::Dropped { reason }.item(name));
            }
            (status, answer) => {
                let attempts = entry.attempts + 1;
                let next = if status == Some(429) {
                    clock::next_midnight(now)
                } else {
                    let i = (attempts as usize).clamp(1, RETRY_S.len()) - 1;
                    now + RETRY_S[i]
                };
                let reason = match (status, answer) {
                    (Some(code), _) => format!("HTTP_{code}"),
                    (None, Err(class)) => class,
                    (None, Ok(_)) => "OSError".into(),
                };
                retries.insert(
                    name.clone(),
                    Retry {
                        attempts,
                        next,
                        error: reason.clone(),
                    },
                );
                settle(&mut value, &index, "failed", |last| {
                    last.reason = Some(reason.clone());
                    last.next_attempt = Some(next);
                });
                results.push(Uploaded::Retry { reason, next }.item(name));
                if status != Some(429) {
                    break; // no answer or a server error: the rest would fare no better
                }
            }
        }
    }
    value.uploads = retries;
    value.save(root)?;
    Ok(UploadRun {
        uploaded: results,
        reason: None,
    })
}

/// `upload` for callers whose own work must not fail with it (a
/// queued report stays queued): a sleep in progress, a refusal or an I/O error becomes the reason
/// (an OSError by its class).
pub fn upload_now(
    root: &Root,
    progress: Option<&build::Progress>,
    first: Option<&str>,
) -> UploadRun {
    upload(root, progress, first).unwrap_or_else(|error| {
        UploadRun::not_run(match &error {
            Failure::Os { class, .. } => class,
            Failure::Value(text) => text,
        })
    })
}

/// `report send`: queue a manual report, then send it at once when it can be; otherwise it
/// waits in the queue.
pub fn send(root: &Root, topic: Option<&str>) -> Result<Queued, Failure> {
    let progress = build::Progress::new(root, None);
    let result = (|| {
        let mut result = queue(root, &["manual".to_owned()], "", topic, &progress)?;
        if result.queued {
            result.upload = Some(upload_now(root, Some(&progress), result.name.as_deref()));
        }
        Ok(result)
    })();
    progress.close();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real-root path: the body reaches the program's stdin whole, its output is read as curl's.
    #[test]
    fn uploads_stream_the_body_and_read_the_answer() {
        let script = "n=$(wc -c); printf '{\"report_id\": \"r\", \"bytes\": %s}\\n201 0 ' \"$n\"";
        let data = vec![7u8; 100_000];
        let outcome = stream(&["/bin/sh", "-c", script], &data, None);
        let (status, value) = answer(&outcome);
        assert_eq!(status, Some(201));
        let value = value.unwrap();
        assert_eq!(value.report_id.as_deref(), Some("r"));
        assert_eq!(value.bytes, Some(100_000));
        let failed = stream(
            &[
                "/bin/sh",
                "-c",
                "cat >/dev/null; printf '\\n000 6 Could not resolve host'; exit 6",
            ],
            b"x",
            None,
        );
        assert_eq!(answer(&failed), (None, Err("gaierror".into())));
        let missing = stream(&["/nonexistent/curl"], b"x", None);
        assert_eq!(answer(&missing), (None, Err("FileNotFoundError".into())));
        let refused = command::Outcome {
            ok: true,
            reason: None,
            output: Some("{\"error\": \"bad_trigger\"}\n400 0 ".into()),
        };
        let (status, value) = answer(&refused);
        assert_eq!(
            (status, value.unwrap().error),
            (Some(400), Some("bad_trigger".into()))
        );
    }
}

#[cfg(test)]
#[path = "upload_tests.rs"]
mod upload_tests;
