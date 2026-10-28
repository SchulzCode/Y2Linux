//! report.py's queue and its upload: `queue`, `upload_blocked`, `upload` (the NetworkTransfer
//! workload), `upload_now` and cli.py's `report send`.
//!
//! The receiver (tools/reports/worker) is reached through the `curl` binary instead of Python's
//! urllib, with the same request: POST, no redirects followed (a 3xx is an answer, retried as
//! `HTTP_3xx`), no proxy, no configuration file, 30 s to connect and no 30 s without progress, the
//! same headers (urllib's own Accept-Encoding and Connection too, curl's Accept, Expect and chunked
//! encoding removed) and the report as the body, streamed from stdin in 16 KiB pieces so the owner's
//! screen follows the upload. Below a fixture root the call is a fixture command like any other.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    boot_id, build, clock, json_dict, or_dict,
    pyjson::{self, py_str, Py},
    state::{self, QUEUE},
    tarball, workload,
};
use crate::reply::{mkdir_parents, Failure};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    time::{Duration, Instant as Monotonic},
};
use y2p_core::{command, Root};

const QUEUE_MAX: usize = 3;
const DAILY_MAX: f64 = 5.0;
pub const ENDPOINT: &str = "https://reports.schulzcode.dev/v1/report";
const RETRY_S: [f64; 5] = [300.0, 900.0, 3600.0, 3.0 * 3600.0, 6.0 * 3600.0];
const GIVE_UP_S: f64 = 3.0 * 86400.0;
/// Never below this on battery.
const MINIMUM_PERCENT: i64 = 15;
/// The receiver will never take this report.
const REFUSED: [i64; 6] = [400, 403, 404, 405, 411, 413];
/// Where the request's headers wait for curl (`-H @file`) while the upload lock is held.
const HEADERS: &str = ".upload-headers";

fn wall() -> f64 {
    clock::Instant::now().timestamp()
}

/// `queue(ctx, trigger, detail, topic, now, progress, triggers)`: build and queue one report if the
/// user opted in and the limits allow it.
pub fn queue(
    root: &Root,
    triggers: &[String],
    detail: &str,
    topic: Option<&str>,
    progress: &build::Progress,
) -> Result<Py, Failure> {
    let mut value = state::state(root);
    if !matches!(value.get("enabled"), Some(Py::Bool(true))) {
        return Ok(Py::dict()
            .with("queued", Py::Bool(false))
            .with("reason", Py::str("not_enabled")));
    }
    let trigger = &triggers[0];
    let now = clock::Instant::now();
    let day = clock::date(now);
    let boot = boot_id(root).unwrap_or_default();
    let daily = match value.get("daily") {
        Some(Py::Dict(items)) => items.iter().filter(|(k, _)| *k == day).cloned().collect(),
        _ => Vec::new(),
    };
    value.set("daily", Py::Dict(daily));
    let today = value
        .get("daily")
        .and_then(|d| d.get(&day))
        .and_then(Py::number)
        .unwrap_or(0.0);
    let reported: Vec<String> = match value.get("boot_triggers").and_then(|b| b.get(&boot)) {
        Some(Py::List(items)) => items
            .iter()
            .filter_map(|t| t.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    };
    if trigger != "manual" {
        if today >= DAILY_MAX {
            return Ok(Py::dict()
                .with("queued", Py::Bool(false))
                .with("reason", Py::str("daily_limit")));
        }
        if triggers.iter().all(|t| reported.contains(t)) {
            return Ok(Py::dict()
                .with("queued", Py::Bool(false))
                .with("reason", Py::str("already_reported_this_boot")));
        }
    }
    let install = value.get("install_id").cloned().unwrap_or(Py::None);
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
    // `daily.get(day, 0) + 1`: an int stays an int, a float a float.
    let count = match value.get("daily").and_then(|d| d.get(&day)) {
        Some(Py::Float(f)) => Py::Float(f + 1.0),
        Some(other) => Py::int(other.number().unwrap_or(0.0) as i64 + 1),
        None => Py::int(1),
    };
    value.get_mut("daily").expect("set above").set(&day, count);
    let mut boot_triggers: Vec<Py> = match value.get("boot_triggers").and_then(|b| b.get(&boot)) {
        Some(Py::List(items)) => items.clone(),
        _ => Vec::new(),
    };
    boot_triggers.extend(triggers.iter().cloned().map(Py::Str));
    value.set(
        "boot_triggers",
        Py::dict().with(&boot, Py::List(boot_triggers)),
    );
    value.set(
        "last",
        Py::dict()
            .with("report_id", Py::Str(report_id.clone()))
            .with("trigger", Py::Str(trigger.clone()))
            .with("state", Py::str("queued"))
            .with(
                "created_utc",
                index.get("created_utc").cloned().unwrap_or(Py::None),
            )
            .with("bytes", Py::from(data.len())),
    );
    state::save(root, &value)?;
    Ok(Py::dict()
        .with("queued", Py::Bool(true))
        .with("report_id", Py::Str(report_id))
        .with("bytes", Py::from(data.len()))
        .with("name", Py::Str(name)))
}

/// `upload_blocked(ctx, now)`: why queued reports must wait now, or None. Uploads run in the
/// background: only on Wi-Fi, with a trustworthy clock (TLS), never during an update and never below
/// MINIMUM_PERCENT on battery.
fn upload_blocked(root: &Root) -> Result<Option<&'static str>, Failure> {
    if state::upload_key(root).is_none() {
        return Ok(Some("no_upload_key"));
    }
    if !matches!(state::state(root).get("enabled"), Some(Py::Bool(true))) {
        return Ok(Some("not_enabled"));
    }
    let boot: Py = boot_id(root).into();
    let network = or_dict(json_dict(root, "/run/y2/network.json"));
    if !network
        .get("state")
        .is_some_and(|s| s.py_eq(&Py::str("Online")))
        || !network.get("boot_id").unwrap_or(&Py::None).py_eq(&boot)
    {
        return Ok(Some("offline"));
    }
    let clock = crate::commands::time::status(root)?;
    let ready = serde_json::from_str::<serde_json::Value>(&clock.stdout)
        .ok()
        .and_then(|v| v.get("tls_ready").and_then(serde_json::Value::as_bool))
        .unwrap_or(false);
    if !ready {
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
        return Err(Failure::value("battery_not_a_dict"));
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
    match pyjson::loads_bytes(&body?)? {
        value @ Py::Dict(_) => Some(value),
        _ => None, // .get() on a list: unreadable as far as the queue is concerned
    }
}

/// curl's answer as urllib's: (HTTP status, answer) or (None, the exception class Python names).
fn answer(outcome: &command::Outcome) -> (Option<i64>, Result<Py, String>) {
    let output = outcome.output.clone().unwrap_or_default();
    let (body, last) = match output.rsplit_once('\n') {
        Some((body, last)) => (body, last),
        None => ("", output.as_str()),
    };
    let mut fields = last.splitn(3, ' ');
    let (code, exit, message) = (fields.next(), fields.next(), fields.next().unwrap_or(""));
    let parsed = match (code, exit) {
        (Some(c), Some(e)) if c.len() == 3 && c.bytes().all(|b| b.is_ascii_digit()) => e
            .parse::<i64>()
            .ok()
            .map(|e| (c.parse::<i64>().expect("digits"), e)),
        _ => None,
    };
    let Some((status, exit)) = parsed else {
        return (
            None,
            Err(match outcome.reason.as_deref() {
                Some("command_unavailable") => "FileNotFoundError",
                Some("command_timeout") => "TimeoutError",
                _ => "OSError",
            }
            .into()),
        );
    };
    if status != 0 {
        let limit = body.len().min(4096);
        let mut cut = limit;
        while !body.is_char_boundary(cut) {
            cut -= 1;
        }
        let text = &body[..cut];
        let value = match pyjson::loads(if text.is_empty() { "{}" } else { text }) {
            Some(v @ Py::Dict(_)) => v,
            _ => Py::dict(),
        };
        return (Some(status), Ok(value));
    }
    let lower = message.to_lowercase();
    let class = match exit {
        6 => "gaierror",
        7 if lower.contains("refused") => "ConnectionRefusedError",
        7 if lower.contains("timed out") => "TimeoutError",
        28 => "TimeoutError",
        35 => "SSLError",
        60 => "SSLCertVerificationError",
        52 => "RemoteDisconnected",
        55 | 56 if lower.contains("reset") => "ConnectionResetError",
        55 if lower.contains("broken pipe") => "BrokenPipeError",
        _ => "OSError",
    };
    (None, Err(class.into()))
}

/// The curl command line: everything but the headers (a file) and the body (stdin) is fixed.
fn argv(headers: &str) -> Vec<String> {
    [
        "curl",
        "-q",
        "--silent",
        "--show-error",
        "--proto",
        "=https",
        "--noproxy",
        "*",
        "--connect-timeout",
        "30",
        "--speed-limit",
        "1",
        "--speed-time",
        "30",
        "--request",
        "POST",
        "--header",
        &format!("@{headers}"),
        "--upload-file",
        "-",
        "--write-out",
        "\\n%{http_code} %{exitcode} %{errormsg}",
        ENDPOINT,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// The request's headers (`_post`), one per line as curl reads them.
pub fn headers(data: &[u8], index: &Py, key: &str) -> String {
    let release = or_dict(index.get("release").cloned().unwrap_or(Py::None));
    let version = release
        .get("release_version")
        .filter(|v| v.truthy())
        .map(py_str);
    let text = |name: &str| {
        index
            .get(name)
            .filter(|v| v.truthy())
            .map(py_str)
            .unwrap_or_default()
    };
    let schema = index.get("schema").map_or_else(|| "1".into(), py_str);
    let digest: String = Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    [
        ("Accept-Encoding", "identity".to_owned()),
        ("Content-Type", "application/gzip".to_owned()),
        (
            "User-Agent",
            format!(
                "Y2Linux/{}",
                version.clone().unwrap_or_else(|| "unknown".into())
            ),
        ),
        ("X-Y2-Upload-Key", key.to_owned()),
        ("X-Y2-Schema", schema),
        ("X-Y2-Install-Id", text("install_id")),
        ("X-Y2-Report-Id", text("report_id")),
        ("X-Y2-Sha256", digest),
        ("X-Y2-Trigger", text("trigger")),
        ("X-Y2-Release", version.unwrap_or_default()),
        ("Content-Length", data.len().to_string()),
        ("Connection", "close".to_owned()),
        // curl's own: urllib sends neither.
        ("Accept", String::new()),
        ("Expect", String::new()),
        ("Transfer-Encoding", String::new()),
    ]
    .iter()
    .map(|(name, value)| {
        format!(
            "{name}:{}{value}\n",
            if value.is_empty() { "" } else { " " }
        )
    })
    .collect()
}

/// `_post(opener, data, index, key, progress)`: POST one report.
fn post(
    root: &Root,
    data: &[u8],
    index: &Py,
    key: &str,
    progress: Option<&build::Progress>,
) -> Result<(Option<i64>, Result<Py, String>), Failure> {
    let directory = root.path(QUEUE);
    let file = directory.join(HEADERS);
    let _ = std::fs::remove_file(&file);
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&file)
        .map_err(|e| super::os(e, &file))?;
    out.write_all(headers(data, index, key).as_bytes())
        .map_err(|e| super::os(e, &file))?;
    drop(out);
    let argv = argv(&file.to_string_lossy());
    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    let outcome = if root.is_real() {
        stream(&refs, data, progress)
    } else {
        command::run_with(root, &refs, 600.0, 65536, true)
    };
    let _ = std::fs::remove_file(&file);
    Ok(answer(&outcome))
}

/// curl on the real root, the body written to its stdin in 16 KiB pieces (`_Body`), its output
/// bounded, the whole tree killed at the deadline (`common.command`'s rules otherwise).
fn stream(argv: &[&str], data: &[u8], progress: Option<&build::Progress>) -> command::Outcome {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let failed = |reason: &str, output: Option<String>| command::Outcome {
        ok: false,
        reason: Some(reason.into()),
        output,
    };
    let mut process = Command::new(argv[0]);
    process
        .args(&argv[1..])
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // SAFETY: setsid() is async-signal-safe; it runs in the forked child before exec.
    unsafe {
        process.pre_exec(|| {
            if libc::setsid() < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let Ok(mut child) = process.spawn() else {
        return failed("command_unavailable", None);
    };
    let pid = child.id() as libc::pid_t;
    let mut stdout = child.stdout.take().expect("piped");
    let reader = std::thread::spawn(move || {
        let mut output = Vec::new();
        let _ = (&mut stdout).take(65_537).read_to_end(&mut output);
        output
    });
    let deadline = Monotonic::now() + Duration::from_secs(600);
    let mut stdin = child.stdin.take().expect("piped");
    let mut sent = 0;
    while sent < data.len() {
        let piece = &data[sent..(sent + 16384).min(data.len())];
        if stdin.write_all(piece).is_err() {
            break;
        }
        sent += piece.len();
        if let Some(progress) = progress {
            progress.update(
                "upload",
                sent as f64 / data.len().max(1) as f64,
                false,
                &[
                    ("bytes_done", Py::from(sent)),
                    ("bytes_total", Py::from(data.len())),
                ],
            );
        }
        if Monotonic::now() >= deadline {
            break;
        }
    }
    drop(stdin);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Monotonic::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            _ => break None,
        }
    };
    if status.is_none() {
        command::kill_tree(pid);
        let _ = child.wait();
        return failed("command_timeout", None);
    }
    let output = reader.join().unwrap_or_default();
    if output.len() > 65_536 {
        return failed("command_output_limit", None);
    }
    let text = y2p_core::root::python_strip(&String::from_utf8_lossy(&output)).to_owned();
    let ok = status.is_some_and(|s| s.success());
    command::Outcome {
        ok,
        reason: (!ok).then(|| "command_failed".into()),
        output: Some(text),
    }
}

/// `upload(ctx, progress, first)`: send every queued report that is due, oldest first.
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
    let _lease = workload::Lease::take(root, "NetworkTransfer")?;
    let now = wall();
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
    let key = state::upload_key(root).expect("checked by upload_blocked");
    upload_queue(root, now, &key, progress, first)
}

fn upload_queue(
    root: &Root,
    now: f64,
    key: &str,
    progress: Option<&build::Progress>,
    first: Option<&str>,
) -> Result<Py, Failure> {
    let mut value = state::state(root);
    let mut queued = state::queued(root);
    queued.sort_by_key(|p| {
        let name = state::name(p);
        (Some(name.as_str()) != first, name)
    });
    let names: Vec<String> = queued.iter().map(|p| state::name(p)).collect();
    let mut connected = false;
    let mut retries: Vec<(String, Py)> = match value.get("uploads") {
        Some(Py::Dict(items)) => items
            .iter()
            .filter(|(name, entry)| entry.is_dict() && names.contains(name))
            .cloned()
            .collect(),
        _ => Vec::new(),
    };
    let mut results = Vec::new();
    let settle = |value: &mut Py, index: &Py, outcome: &str, fields: Vec<(&str, Py)>| {
        let last = or_dict(value.get("last").cloned().unwrap_or(Py::None));
        if last
            .get("report_id")
            .unwrap_or(&Py::None)
            .py_eq(index.get("report_id").unwrap_or(&Py::None))
        {
            let mut kept = match last {
                Py::Dict(items) => Py::Dict(
                    items
                        .into_iter()
                        .filter(|(k, _)| k != "reason" && k != "next_attempt")
                        .collect(),
                ),
                other => other,
            };
            kept.set("state", Py::str(outcome));
            for (k, v) in fields {
                kept.set(k, v);
            }
            value.set("last", kept);
        }
    };
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
        let next = entry.get("next").and_then(Py::number).unwrap_or(0.0);
        if next > now && !connected {
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
            results.push(
                Py::dict()
                    .with("name", Py::Str(name))
                    .with("outcome", Py::str("dropped"))
                    .with("reason", Py::str("unreadable")),
            );
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
            );
            results.push(
                Py::dict()
                    .with("name", Py::Str(name))
                    .with("outcome", Py::str("dropped"))
                    .with("reason", Py::str("expired")),
            );
            continue;
        }
        if let Some(shown) = shown {
            shown.update(
                "upload",
                0.0,
                true,
                &[
                    ("bytes_done", Py::int(0)),
                    ("bytes_total", Py::from(data.len())),
                ],
            );
        }
        let (status, answer) = post(root, &data, &index, key, shown)?;
        match (status, answer) {
            (Some(code @ (200 | 201)), answer) => {
                connected = connected || Some(name.as_str()) == first;
                let _ = code;
                crate::reply::unlink_missing_ok(path)?;
                drop_retry(&mut retries, &name);
                let sent = clock::isoformat(clock::Instant::from_float(now));
                settle(
                    &mut value,
                    &index,
                    "sent",
                    vec![("sent_utc", Py::Str(sent))],
                );
                let id = answer
                    .ok()
                    .and_then(|a| a.get("report_id").cloned())
                    .unwrap_or(Py::None);
                results.push(
                    Py::dict()
                        .with("name", Py::Str(name))
                        .with("outcome", Py::str("sent"))
                        .with("report_id", id),
                );
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
                );
                results.push(
                    Py::dict()
                        .with("name", Py::Str(name))
                        .with("outcome", Py::str("dropped"))
                        .with("reason", Py::Str(reason)),
                );
            }
            (status, answer) => {
                let attempts = entry.get("attempts").and_then(Py::number).unwrap_or(0.0) + 1.0;
                let attempts_value = match entry.get("attempts") {
                    Some(Py::Float(_)) => Py::Float(attempts),
                    _ => Py::int(attempts as i64),
                };
                let wait = if status == Some(429) {
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
                drop_retry(&mut retries, &name);
                retries.push((
                    name.clone(),
                    Py::dict()
                        .with("attempts", attempts_value)
                        .with("next", Py::Float(wait))
                        .with("error", Py::Str(reason.clone())),
                ));
                settle(
                    &mut value,
                    &index,
                    "failed",
                    vec![
                        ("reason", Py::Str(reason.clone())),
                        ("next_attempt", Py::Float(wait)),
                    ],
                );
                results.push(
                    Py::dict()
                        .with("name", Py::Str(name))
                        .with("outcome", Py::str("retry"))
                        .with("reason", Py::Str(reason))
                        .with("next", Py::Float(wait)),
                );
                if status != Some(429) {
                    break; // no answer or a server error: the rest would fare no better
                }
            }
        }
    }
    value.set("uploads", Py::Dict(retries));
    state::save(root, &value)?;
    Ok(Py::dict().with("uploaded", Py::List(results)))
}

/// `upload_now(ctx, progress, first)`: upload() for callers whose own work must not fail with it (a
/// queued report stays queued): a sleep in progress or an I/O error becomes the reason.
pub fn upload_now(root: &Root, progress: Option<&build::Progress>, first: Option<&str>) -> Py {
    match upload(root, progress, first) {
        Ok(value) => value,
        Err(error) => Py::dict().with("uploaded", Py::List(Vec::new())).with(
            "reason",
            Py::Str(match &error {
                Failure::Os { class, .. } => (*class).to_owned(),
                other => other.message(),
            }),
        ),
    }
}

/// cli.py's `report send`: queue a manual report, then send it at once when it can be; otherwise it
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
