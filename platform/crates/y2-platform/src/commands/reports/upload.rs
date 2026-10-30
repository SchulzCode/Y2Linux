//! The report queue and its upload (report.py): `queue` within the daily and per-boot limits,
//! `upload_blocked`, `upload` (the NetworkTransfer workload) with its retries, `upload_now` and
//! `report send`. The request itself is `curl`'s (upload/curl.rs).
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    build::{self, Bytes},
    clock, json_dict, or_dict,
    pyjson::{self, py_str, Py},
    pyops::{self, OrRefuse},
    state::{self, Reports, QUEUE},
    tarball, POWER_UNREADABLE, QUEUED_UNREADABLE, STATE_UNREADABLE,
};
use crate::reply::{mkdir_parents, Failure};
use std::{
    io::Write,
    os::unix::fs::OpenOptionsExt,
    time::{Duration, Instant as Monotonic},
};
use y2p_core::{command, Root};

const QUEUE_MAX: usize = 3;
const DAILY_MAX: i64 = 5;
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
) -> Result<Py, Failure> {
    let not_queued = |reason: &str| {
        Py::dict()
            .with("queued", Py::Bool(false))
            .with("reason", Py::str(reason))
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
    value.keep_day(&day)?;
    if trigger != "manual" {
        let (count, limit) =
            pyops::ordered(&value.count(&day), &Py::int(DAILY_MAX)).or_refuse(STATE_UNREADABLE)?;
        if count >= limit {
            return Ok(not_queued("daily_limit"));
        }
        // `all(t in value['boot_triggers'].get(boot, []) for t in triggers)`.
        let mut all = true;
        for t in triggers {
            if !pyops::contains(&value.reported(&boot)?, t).or_refuse(STATE_UNREADABLE)? {
                all = false;
                break;
            }
        }
        if all {
            return Ok(not_queued("already_reported_this_boot"));
        }
    }
    let install = value.field("install_id");
    let (data, index) = build::build(root, triggers, detail, topic, install, now, progress)?;
    let directory = root.path(QUEUE);
    mkdir_parents(&directory, 0o700)?;
    let report_id = index
        .get("report_id")
        .and_then(Py::as_str)
        .unwrap_or("")
        .to_owned();
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
    // `daily.get(day, 0) + 1`: an int stays an int, a float a float (a refusal here leaves the
    // report queued and reports.json as it was).
    let count = pyops::add_int(&value.count(&day), 1).or_refuse(STATE_UNREADABLE)?;
    value.set_count(&day, count);
    let reported = pyops::add_list(
        &value.reported(&boot)?,
        triggers.iter().cloned().map(Py::Str).collect(),
    )
    .or_refuse(STATE_UNREADABLE)?;
    // Only this boot's are kept.
    value.set("boot_triggers", Py::dict().with(&boot, reported));
    value.set(
        "last",
        state::Last {
            report_id: Py::Str(report_id.clone()),
            trigger,
            state: "queued",
            saved: None,
            created_utc: index.get("created_utc").cloned().unwrap_or(Py::None),
            bytes: data.len(),
        }
        .into(),
    );
    value.save(root)?;
    Ok(Py::dict()
        .with("queued", Py::Bool(true))
        .with("report_id", Py::Str(report_id))
        .with("bytes", Py::from(data.len()))
        .with("name", Py::Str(name)))
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
    let boot: Py = root.boot_id().into();
    let network = or_dict(json_dict(root, "/run/y2/network.json"));
    if !network
        .get("state")
        .is_some_and(|s| s.py_eq(&Py::str("Online")))
        || !network.get("boot_id").unwrap_or(&Py::None).py_eq(&boot)
    {
        return Ok(Some("offline"));
    }
    if !crate::commands::time::tls_ready(root) {
        return Ok(Some("clock_not_ready"));
    }
    let update = or_dict(json_dict(root, "/data/updates/state.json"));
    let quiet = match update.get("state") {
        None | Some(Py::None) => true,
        Some(s) => ["Idle", "Acknowledged", "RolledBack", "Failed"]
            .iter()
            .any(|w| s.py_eq(&Py::str(*w))),
    };
    if !quiet
        || root
            .path("/data/system/platform/maintenance-pending")
            .exists()
    {
        return Ok(Some("update_in_progress"));
    }
    let power = or_dict(json_dict(root, "/run/y2/power.json"));
    let same_boot = match power.get("boot_id") {
        None | Some(Py::None) => true,
        Some(b) => b.py_eq(&boot),
    };
    let battery = if same_boot {
        power
            .get("battery")
            .filter(|b| b.truthy())
            .cloned()
            .unwrap_or(Py::dict())
    } else {
        Py::dict()
    };
    if !battery.is_dict() {
        return Err(Failure::value(POWER_UNREADABLE));
    }
    let charging = matches!(battery.get("charging"), Some(Py::Bool(true)))
        || battery
            .get("status")
            .is_some_and(|s| s.py_eq(&Py::str("Charging")) || s.py_eq(&Py::str("Full")));
    let percent = battery.get("percent").and_then(Py::as_int);
    if !charging && percent.is_none_or(|p| p < MINIMUM_PERCENT) {
        return Ok(Some("battery_low"));
    }
    Ok(None)
}

/// `_queued_index(path)`: report.json of a queued report (the first member: only it is
/// decompressed).
fn queued_index(data: &[u8]) -> Option<Py> {
    let members = tarball::members(data, true, 1, |m| m.name == "report.json").ok()?;
    let (member, body) = members.into_iter().next()?;
    if member.name != "report.json" {
        return None;
    }
    // Any JSON value: one that is no dict is an AttributeError at its first `.get`.
    pyjson::loads_bytes(&body?)
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
) -> Result<Py, Failure> {
    let _lease = crate::commands::workload::Lease::take(root, "NetworkTransfer")?;
    let now = wall(root);
    if let Some(blocked) = upload_blocked(root)? {
        return Ok(Py::dict()
            .with("uploaded", Py::List(Vec::new()))
            .with("reason", Py::str(blocked)));
    }
    let directory = root.path(QUEUE);
    mkdir_parents(&directory, 0o700)?;
    let lock_path = directory.join(".upload.lock");
    let Some(_lock) =
        y2p_core::lock::try_exclusive(&lock_path).map_err(|e| super::os(e, &lock_path))?
    else {
        return Ok(Py::dict()
            .with("uploaded", Py::List(Vec::new()))
            .with("reason", Py::str("busy")));
    };
    // The key once more, under the lock (upload_blocked read it first): a key gone meanwhile is the
    // same answer as no key.
    let Some(key) = state::upload_key(root) else {
        return Ok(Py::dict()
            .with("uploaded", Py::List(Vec::new()))
            .with("reason", Py::str("no_upload_key")));
    };
    upload_queue(root, now, &key, progress, first)
}

/// What one queued report's run came to: an item of the answer's `uploaded`.
enum Uploaded {
    /// Stored: the receiver's own report ID (any JSON it answered, `null` without one).
    Sent { report_id: Py },
    /// Gone from the queue without being stored.
    Dropped { reason: String },
    /// Still queued until `next`.
    Retry { reason: String, next: f64 },
}

impl Uploaded {
    fn item(self, name: String) -> Py {
        let item = Py::dict().with("name", Py::Str(name));
        match self {
            Uploaded::Sent { report_id } => item
                .with("outcome", Py::str("sent"))
                .with("report_id", report_id),
            Uploaded::Dropped { reason } => item
                .with("outcome", Py::str("dropped"))
                .with("reason", Py::Str(reason)),
            Uploaded::Retry { reason, next } => item
                .with("outcome", Py::str("retry"))
                .with("reason", Py::Str(reason))
                .with("next", Py::Float(next)),
        }
    }
}

/// A queued report's retry record (`uploads[name]`).
struct Retry {
    /// Tries so far: a count, of the type it was read as (an int stays an int).
    attempts: Py,
    /// When it may go again (wall clock).
    next: f64,
    /// Why the last try failed (`HTTP_503`, an exception class).
    error: String,
}

impl From<Retry> for Py {
    fn from(retry: Retry) -> Py {
        Py::dict()
            .with("attempts", retry.attempts)
            .with("next", Py::Float(retry.next))
            .with("error", Py::Str(retry.error))
    }
}

/// The last report shown on the owner's screen, if it is this one: its new state and fields
/// (`reason`, `next_attempt` of an earlier try dropped). `last = value.get('last') or {}`: a truthy
/// one that is no object is refused.
fn settle(
    value: &mut Reports,
    index: &Py,
    outcome: &str,
    fields: Vec<(&str, Py)>,
) -> Result<(), Failure> {
    let last = or_dict(value.field("last"));
    let shown = pyops::get(&last, "report_id").or_refuse(STATE_UNREADABLE)?;
    if !shown
        .unwrap_or(&Py::None)
        .py_eq(index.get("report_id").unwrap_or(&Py::None))
    {
        return Ok(());
    }
    let Py::Dict(items) = last else {
        return Ok(()); // an object (pyops::get)
    };
    let mut kept = Py::Dict(
        items
            .into_iter()
            .filter(|(k, _)| k != "reason" && k != "next_attempt")
            .collect(),
    );
    kept.set("state", Py::str(outcome));
    for (k, v) in fields {
        kept.set(k, v);
    }
    value.set("last", kept);
    Ok(())
}

fn upload_queue(
    root: &Root,
    now: f64,
    key: &str,
    progress: Option<&build::Progress>,
    first: Option<&str>,
) -> Result<Py, Failure> {
    let mut value = Reports::load(root);
    let mut queued = state::queued(root);
    queued.sort_by_key(|p| {
        let name = state::name(p);
        (Some(name.as_str()) != first, name)
    });
    let names: Vec<String> = queued.iter().map(|p| state::name(p)).collect();
    let mut connected = false;
    // The retries of reports still queued (an entry that is no object is forgotten).
    let mut retries: Vec<(String, Py)> = match value.uploads() {
        Py::Dict(items) => items
            .iter()
            .filter(|(name, entry)| entry.is_dict() && names.contains(name))
            .cloned()
            .collect(),
        _ => Vec::new(),
    };
    let mut results = Vec::new();
    let entry_of = |retries: &[(String, Py)], name: &str| {
        retries
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, e)| e.clone())
            .unwrap_or(Py::dict())
    };
    let drop_retry =
        |retries: &mut Vec<(String, Py)>, name: &str| retries.retain(|(n, _)| n != name);
    for path in &queued {
        let name = state::name(path);
        let entry = entry_of(&retries, &name);
        // `entry.get('next', 0) > now`.
        let next = entry.get("next").cloned().unwrap_or(Py::int(0));
        let (next, now_) = pyops::ordered(&next, &Py::Float(now)).or_refuse(STATE_UNREADABLE)?;
        if next > now_ && !connected {
            continue;
        }
        let shown = if first.is_none() || first == Some(name.as_str()) {
            progress
        } else {
            None
        };
        let read = std::fs::read(path).ok();
        let index = read.as_deref().and_then(queued_index);
        if index.as_ref().is_some_and(|i| !i.is_dict()) {
            return Err(Failure::value(QUEUED_UNREADABLE));
        }
        let (Some(index), Some(data)) = (index, read) else {
            crate::reply::unlink_missing_ok(path)?;
            let reason = "unreadable".into();
            results.push(Uploaded::Dropped { reason }.item(name));
            continue;
        };
        let made = index
            .get("created_utc")
            .and_then(Py::as_str)
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
            drop_retry(&mut retries, &name);
            settle(
                &mut value,
                &index,
                "dropped",
                vec![("reason", Py::str("expired"))],
            )?;
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
                drop_retry(&mut retries, &name);
                let sent = clock::isoformat(clock::Instant::from_float(now));
                settle(
                    &mut value,
                    &index,
                    "sent",
                    vec![("sent_utc", Py::Str(sent))],
                )?;
                let report_id = answer
                    .ok()
                    .and_then(|a| a.get("report_id").cloned())
                    .unwrap_or(Py::None);
                results.push(Uploaded::Sent { report_id }.item(name));
            }
            (Some(code), answer) if REFUSED.contains(&code) => {
                crate::reply::unlink_missing_ok(path)?;
                drop_retry(&mut retries, &name);
                let error = answer
                    .ok()
                    .and_then(|a| a.get("error").cloned())
                    .map_or_else(String::new, |e| py_str(&e));
                let reason = format!("refused_{code}_{error}");
                settle(
                    &mut value,
                    &index,
                    "dropped",
                    vec![("reason", Py::Str(reason.clone()))],
                )?;
                results.push(Uploaded::Dropped { reason }.item(name));
            }
            (status, answer) => {
                // `entry.get('attempts', 0) + 1` (after the request: a refusal leaves the
                // report queued and reports.json as it was).
                let attempts = pyops::add_int(entry.get("attempts").unwrap_or(&Py::int(0)), 1)
                    .or_refuse(STATE_UNREADABLE)?;
                let tries = attempts.number().unwrap_or(0.0);
                let next = if status == Some(429) {
                    clock::next_midnight(now)
                } else {
                    let i = (tries as usize).clamp(1, RETRY_S.len()) - 1;
                    now + RETRY_S[i]
                };
                let reason = match (status, answer) {
                    (Some(code), _) => format!("HTTP_{code}"),
                    (None, Err(class)) => class,
                    (None, Ok(_)) => "OSError".into(),
                };
                drop_retry(&mut retries, &name);
                let retry = Retry {
                    attempts,
                    next,
                    error: reason.clone(),
                };
                retries.push((name.clone(), retry.into()));
                settle(
                    &mut value,
                    &index,
                    "failed",
                    vec![
                        ("reason", Py::Str(reason.clone())),
                        ("next_attempt", Py::Float(next)),
                    ],
                )?;
                results.push(Uploaded::Retry { reason, next }.item(name));
                if status != Some(429) {
                    break; // no answer or a server error: the rest would fare no better
                }
            }
        }
    }
    value.set("uploads", Py::Dict(retries));
    value.save(root)?;
    Ok(Py::dict().with("uploaded", Py::List(results)))
}

/// `upload` for callers whose own work must not fail with it (a
/// queued report stays queued): a sleep in progress, a refusal or an I/O error becomes the reason
/// (an OSError by its class).
pub fn upload_now(root: &Root, progress: Option<&build::Progress>, first: Option<&str>) -> Py {
    upload(root, progress, first).unwrap_or_else(|error| {
        let reason = match &error {
            Failure::Os { class, .. } | Failure::Crash { class, .. } => (*class).to_owned(),
            Failure::Value(text) => text.clone(),
        };
        Py::dict()
            .with("uploaded", Py::List(Vec::new()))
            .with("reason", Py::Str(reason))
    })
}

/// `report send`: queue a manual report, then send it at once when it can be; otherwise it
/// waits in the queue.
pub fn send(root: &Root, topic: Option<&str>) -> Result<Py, Failure> {
    let progress = build::Progress::new(root, None);
    let result = (|| {
        let mut result = queue(root, &["manual".to_owned()], "", topic, &progress)?;
        if matches!(result.get("queued"), Some(Py::Bool(true))) {
            let name = result.get("name").and_then(Py::as_str).map(str::to_owned);
            let upload = upload_now(root, Some(&progress), name.as_deref());
            result.set("upload", upload);
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
        assert_eq!(value.get("report_id"), Some(&Py::str("r")));
        assert_eq!(value.get("bytes").and_then(Py::as_int), Some(100_000));
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
            (status, value.unwrap().get("error").cloned()),
            (Some(400), Some(Py::str("bad_trigger")))
        );
    }
}

#[cfg(test)]
#[path = "upload_tests.rs"]
mod upload_tests;
