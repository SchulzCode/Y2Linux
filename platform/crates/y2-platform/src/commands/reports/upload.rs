//! report.py's queue and its upload: `queue`, `upload_blocked`, `upload` (the NetworkTransfer
//! workload), `upload_now` and cli.py's `report send`.
//!
//! The receiver (tools/reports/worker) is reached through the `curl` binary instead of Python's
//! urllib, with the same request: POST, no redirects followed (a 3xx is an answer, retried as
//! `HTTP_3xx`), no proxy, no configuration file, 30 s to connect and no 30 s without progress, the
//! same headers as urllib sends them (its own Accept-Encoding and Connection too; curl's Accept,
//! Expect and chunked encoding removed) and the report as the body, streamed from stdin in 16 KiB
//! pieces so the owner's screen follows the upload. Below a fixture root the call is a fixture command
//! like any other (its argv carries the headers, so the differential tests compare them).
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    build, clock, json_dict, or_dict,
    pyjson::{self, py_str, Py},
    pyops::{self, OrRefuse},
    state::{self, QUEUE},
    tarball, workload, POWER_UNREADABLE, QUEUED_UNREADABLE, STATE_UNREADABLE,
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
const DAILY_MAX: i64 = 5;
pub const ENDPOINT: &str = "https://reports.schulzcode.dev/v1/report";
const RETRY_S: [f64; 5] = [300.0, 900.0, 3600.0, 3.0 * 3600.0, 6.0 * 3600.0];
const GIVE_UP_S: f64 = 3.0 * 86400.0;
/// Never below this on battery.
const MINIMUM_PERCENT: i64 = 15;
/// The receiver will never take this report.
const REFUSED: [i64; 6] = [400, 403, 404, 405, 411, 413];

fn wall(root: &Root) -> f64 {
    clock::Instant::now(root).timestamp()
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
    let now = clock::Instant::now(root);
    let day = clock::date(now);
    let boot = root.boot_id().unwrap_or_default();
    // `{k: v for k, v in value['daily'].items() if k == day}`.
    let daily = match value.get("daily").cloned().unwrap_or(Py::None) {
        Py::Dict(items) => items.into_iter().filter(|(k, _)| *k == day).collect(),
        _ => return Err(Failure::value(STATE_UNREADABLE)),
    };
    value.set("daily", Py::Dict(daily));
    let boot_triggers = value.get("boot_triggers").cloned().unwrap_or(Py::None);
    let today = || {
        value
            .get("daily")
            .and_then(|d| d.get(&day))
            .cloned()
            .unwrap_or(Py::int(0))
    };
    // `value['boot_triggers'].get(boot, [])`.
    let reported = |triggers: &Py| -> Result<Py, Failure> {
        Ok(pyops::get(triggers, &boot)
            .or_refuse(STATE_UNREADABLE)?
            .cloned()
            .unwrap_or(Py::List(Vec::new())))
    };
    if trigger != "manual" {
        let (count, limit) =
            pyops::ordered(&today(), &Py::int(DAILY_MAX)).or_refuse(STATE_UNREADABLE)?;
        if count >= limit {
            return Ok(Py::dict()
                .with("queued", Py::Bool(false))
                .with("reason", Py::str("daily_limit")));
        }
        // `all(t in value['boot_triggers'].get(boot, []) for t in triggers)`.
        let mut all = true;
        for t in triggers {
            if !pyops::contains(&reported(&boot_triggers)?, t).or_refuse(STATE_UNREADABLE)? {
                all = false;
                break;
            }
        }
        if all {
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
    // `daily.get(day, 0) + 1`: an int stays an int, a float a float (a refusal here leaves the
    // report queued and reports.json as it was).
    let count = pyops::add_int(&today(), 1).or_refuse(STATE_UNREADABLE)?;
    if let Some(daily) = value.get_mut("daily") {
        daily.set(&day, count); // today's entry of the filtered dict above
    }
    let boot_triggers = pyops::add_list(
        &reported(&boot_triggers)?,
        triggers.iter().cloned().map(Py::Str).collect(),
    )
    .or_refuse(STATE_UNREADABLE)?;
    value.set("boot_triggers", Py::dict().with(&boot, boot_triggers));
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
    let boot: Py = root.boot_id().into();
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

/// The curl command line: the request's headers as `--header` arguments (urllib's names, title-cased as
/// it sends them; an empty value as curl's `Name;`), the body on stdin.
fn argv(headers: &[(&str, String)]) -> Vec<String> {
    let mut argv: Vec<String> = [
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
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for (name, value) in headers {
        argv.push("--header".into());
        argv.push(if value.is_empty() {
            format!("{name};")
        } else {
            format!("{name}: {value}")
        });
    }
    // http.client's own and urllib's Connection; curl's Accept, Expect and chunked encoding removed.
    for header in [
        "Accept-Encoding: identity",
        "Connection: close",
        "Accept:",
        "Expect:",
        "Transfer-Encoding:",
    ] {
        argv.push("--header".into());
        argv.push(header.into());
    }
    argv.extend(
        [
            "--upload-file",
            "-",
            "--write-out",
            "\\n%{http_code} %{exitcode} %{errormsg}",
            ENDPOINT,
        ]
        .iter()
        .map(|s| s.to_string()),
    );
    argv
}

/// `_post`'s headers, in its order.
pub fn headers(data: &[u8], index: &Py, key: &str) -> Vec<(&'static str, String)> {
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
    vec![
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
    ]
}

/// `_post(opener, data, index, key, progress)`: POST one report.
fn post(
    root: &Root,
    data: &[u8],
    index: &Py,
    key: &str,
    progress: Option<&build::Progress>,
) -> (Option<i64>, Result<Py, String>) {
    let argv = argv(&headers(data, index, key));
    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    let outcome = if root.is_real() {
        stream(&refs, data, progress)
    } else {
        command::run_with(root, &refs, 600.0, 65536, true)
    };
    answer(&outcome)
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
    // `last = value.get('last') or {}; last.get('report_id')`: a truthy non-dict is refused.
    let settle = |value: &mut Py,
                  index: &Py,
                  outcome: &str,
                  fields: Vec<(&str, Py)>|
     -> Result<(), Failure> {
        let last = or_dict(value.get("last").cloned().unwrap_or(Py::None));
        if pyops::get(&last, "report_id")
            .or_refuse(STATE_UNREADABLE)?
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
        Ok(())
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
            )?;
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
        let (status, answer) = post(root, &data, &index, key, shown);
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
                )?;
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
                )?;
                results.push(
                    Py::dict()
                        .with("name", Py::Str(name))
                        .with("outcome", Py::str("dropped"))
                        .with("reason", Py::Str(reason)),
                );
            }
            (status, answer) => {
                // `entry.get('attempts', 0) + 1` (after the request: a refusal leaves the
                // report queued and reports.json as it was).
                let attempts_value =
                    pyops::add_int(entry.get("attempts").unwrap_or(&Py::int(0)), 1)
                        .or_refuse(STATE_UNREADABLE)?;
                let attempts = attempts_value.number().unwrap_or(0.0);
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
                )?;
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
