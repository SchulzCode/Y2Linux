//! One report's request to the receiver (tools/reports/worker) through the `curl` binary, as
//! Python's urllib made it: POST, no redirects followed (a 3xx is an answer, retried as `HTTP_3xx`), no
//! proxy, no configuration file, 30 s to connect and no 30 s without progress, the same headers as
//! urllib sends them (its own Accept-Encoding and Connection too; curl's Accept, Expect and chunked
//! encoding removed) and the report as the body, streamed from stdin in 16 KiB pieces so the owner's
//! screen follows the upload. Below a fixture root the call is a fixture command like any other (its
//! argv carries the headers, so the golden cases compare them).
// SPDX-License-Identifier: GPL-2.0-only
use super::{build, command, Bytes, Duration, Monotonic, ReportIndex, Root, ENDPOINT};
use std::io::Read;
use std::io::Write;

/// curl's answer as urllib's: (HTTP status, answer) or (None, the exception class Python names).
pub(super) fn answer(outcome: &command::Outcome) -> (Option<i64>, Result<ReceiverAnswer, String>) {
    let output = outcome.output.clone().unwrap_or_default();
    let (body, last) = match output.rsplit_once('\n') {
        Some((body, last)) => (body, last),
        None => ("", output.as_str()),
    };
    let mut fields = last.splitn(3, ' ');
    let (code, exit, message) = (fields.next(), fields.next(), fields.next().unwrap_or(""));
    // `%{http_code}` is three digits (000 without an answer), `%{exitcode}` curl's exit code.
    let parsed = match (code, exit) {
        (Some(c), Some(e)) if c.len() == 3 && c.bytes().all(|b| b.is_ascii_digit()) => {
            c.parse::<i64>().ok().zip(e.parse::<i64>().ok())
        }
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
        return (
            Some(status),
            Ok(serde_json::from_str(text).unwrap_or_default()),
        );
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

/// The receiver's answer (JSON; an answer that does not read has none of these).
#[derive(serde::Deserialize, Default, Debug, PartialEq)]
#[serde(default)]
pub(super) struct ReceiverAnswer {
    /// Its ID of a stored report.
    #[serde(deserialize_with = "y2p_core::lenient::text")]
    pub report_id: Option<String>,
    /// Why it refused one.
    #[serde(deserialize_with = "y2p_core::lenient::text")]
    pub error: Option<String>,
    /// The bytes it received.
    #[serde(deserialize_with = "y2p_core::lenient::or_none")]
    pub bytes: Option<i64>,
}

/// The curl command line: the request's headers as `--header` arguments (urllib's names, title-cased as
/// it sends them; an empty value as curl's `Name;`), the body on stdin.
pub(super) fn argv(headers: &[(&str, String)]) -> Vec<String> {
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

/// The request's headers, in urllib's order (the receiver reads them by name).
pub fn headers(data: &[u8], index: &ReportIndex, key: &str) -> Vec<(&'static str, String)> {
    let version = index.release_version().map(str::to_owned);
    let text = |value: &Option<String>| value.clone().unwrap_or_default();
    let schema = index.schema.to_string();
    let digest = super::super::sha256_hex(data);
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
        ("X-Y2-Install-Id", text(&index.install_id)),
        ("X-Y2-Report-Id", text(&index.report_id)),
        ("X-Y2-Sha256", digest),
        ("X-Y2-Trigger", text(&index.trigger)),
        ("X-Y2-Release", version.unwrap_or_default()),
        ("Content-Length", data.len().to_string()),
    ]
}

/// `_post(opener, data, index, key, progress)`: POST one report.
pub(super) fn post(
    root: &Root,
    data: &[u8],
    index: &ReportIndex,
    key: &str,
    progress: Option<&build::Progress>,
) -> (Option<i64>, Result<ReceiverAnswer, String>) {
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
pub(super) fn stream(
    argv: &[&str],
    data: &[u8],
    progress: Option<&build::Progress>,
) -> command::Outcome {
    use std::process::{Command, Stdio};
    let failed = |reason: &str, output: Option<String>| command::Outcome {
        ok: false,
        reason: Some(reason.into()),
        output,
    };
    let Some((program, arguments)) = argv.split_first() else {
        return failed("command_unavailable", None);
    };
    let mut process = Command::new(program);
    process
        .args(arguments)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    y2p_core::command::new_session(&mut process);
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
                Bytes::of(sent, data.len()),
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
