//! The automatic triggers of an error report: `crash_signature`, `check` and `run_checks`.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    build::{rel, Progress},
    json_dict, or_dict,
    pyops::{self, OrRefuse},
    pyre::{self, lazy, S},
    sha256_hex,
    state::Reports,
    HEALTH_UNREADABLE, STATE_UNREADABLE,
};
use crate::commands::reports::{filled, loads_text, object, text_of, Value, ValueExt};
use crate::reply::Failure;
use regex::Regex;
use std::sync::OnceLock;
use y2p_core::{command, root::python_strip, Root};

fn crash() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    lazy(
        &CELL,
        r"Kernel panic|Internal error: Oops|BUG: |Unable to handle kernel|kernel BUG at",
    )
}

fn dark() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    lazy(
        &CELL,
        r"DSI register reads still lag|flip_done timed out|vblank wait timed out",
    )
}

/// The internal eMMC only: a pulled SD card (mmcblk1) logs I/O errors too.
fn storage() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    lazy(
        &CELL,
        r"EXT4-fs error|I/O error, dev mmcblk0|Remounting filesystem read-only|blk_update_request: I/O error, dev mmcblk0",
    )
}

/// The newest retained kernel crash as (record time, kernel timestamp, text hash), or None.
///
/// F50: a ramoops panic record stays in pstore over many starts, while the console and pmsg records
/// change every start; a hash over all of them sent fcdf3a25's 10-25 panic again as 8e9181e5. The
/// record time (pstore's mtime of a dmesg dump; up to 6 are kept, the newest counts) and the crash
/// line's kernel timestamp name one crash; either matching the last one is enough, as RAM decay over a
/// cold start may flip a digit of one. The text hash only serves records with neither.
fn crash_signature(root: &Root) -> Option<[String; 3]> {
    static STAMP: OnceLock<Regex> = OnceLock::new();
    let stamp_re = lazy(&STAMP, &format!(r"\[[{S}]*(\d+\.\d+)\]"));
    let mut found: Vec<(i64, String, String)> = Vec::new();
    for path in root.glob("/sys/fs/pstore/*") {
        let text = root
            .read_limited(&rel(root, &path), 1 << 20)
            .unwrap_or_default();
        let Some(m) = crash().find(&text) else {
            continue;
        };
        let line_start = text[..m.start()].rfind('\n').map_or(0, |i| i + 1);
        let stamp = stamp_re
            .captures(&text[line_start..m.start()])
            .map(|c| c[1].to_owned())
            .unwrap_or_default();
        let mut when = 0;
        if path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("dmesg-"))
        {
            if let Ok(meta) = std::fs::metadata(&path) {
                use std::os::unix::fs::MetadataExt;
                when = meta.mtime();
            }
        }
        found.push((when, stamp, text));
    }
    // max() keeps the first of equal record times.
    let mut best: Option<&(i64, String, String)> = None;
    for entry in &found {
        if best.is_none_or(|b| entry.0 > b.0) {
            best = Some(entry);
        }
    }
    let (when, uptime, text) = best?;
    let digest = if *when > 0 || !uptime.is_empty() {
        String::new()
    } else {
        sha256_hex(text.as_bytes())[..16].to_owned()
    };
    Some([
        if *when > 0 {
            when.to_string()
        } else {
            String::new()
        },
        uptime.clone(),
        digest,
    ])
}

/// `_changed(value, name, current)`: True once per new value of a watched marker (a `seen` that is
/// no object is refused).
fn changed(value: &mut Reports, name: &str, current: Value) -> Result<bool, Failure> {
    if matches!(&current, Value::Null)
        || current.eq(&Value::from(""))
        || current.eq(&Value::from(0))
    {
        return Ok(false);
    }
    if value.seen()?.get(name).is_some_and(|v| v.eq(&current)) {
        return Ok(false);
    }
    value.mark_seen(name, current);
    Ok(true)
}

fn int(root: &Root, name: &str) -> Option<i64> {
    root.integer(name)
}

/// A trigger that fired, as (trigger, detail).
type Fired = Option<(String, String)>;

/// Every automatic trigger that fired since the last check, as (trigger, detail); what fired is
/// marked as seen in reports.json.
pub fn check(root: &Root) -> Result<Vec<(String, String)>, Failure> {
    let mut value = Reports::load(root);
    let value = &mut value;
    let boot = root.boot_id().unwrap_or_default();
    let mut fired: Vec<(String, String)> = Vec::new();
    fired.extend(kernel_crash(root, value)?);
    fired.extend(kernel_warning(root, value, &boot)?);
    fired.extend(unclean_restart(root, value, &boot)?);
    fired.extend(reborn_crash(root, value)?);
    fired.extend(reborn_failure(root, value, &boot)?);
    fired.extend(update_failure(root, value)?);
    fired.extend(sleep_failure(root, value, &boot)?);
    fired.extend(radio_failure(root, value, &boot)?);
    let kernel = command::run_with(root, &["dmesg"], 5.0, 2 * 1024 * 1024, false)
        .output
        .unwrap_or_default();
    fired.extend(display_failure(root, value, &boot, &kernel)?);
    fired.extend(storage_error(root, value, &boot, &kernel)?);
    value.save(root)?;
    Ok(fired)
}

/// A kernel crash recorded in pstore that was not reported yet (by its signature).
fn kernel_crash(root: &Root, value: &mut Reports) -> Result<Fired, Failure> {
    if let Some(crash) = crash_signature(root) {
        let seen_map = value.seen()?;
        let previous = seen_map
            .get("pstore_crash")
            .filter(|v| filled(v))
            .map(text_of)
            .unwrap_or_default();
        let seen: Vec<String> = (previous + "||")
            .split('|')
            .take(3)
            .map(str::to_owned)
            .collect();
        // Before 8.42 the key was the all-records hash: a crash on record then was already reported
        // by that release (no resend after the update).
        let known = seen_map.get("pstore_crash").is_none() && seen_map.get("pstore").is_some();
        value.mark_seen("pstore_crash", Value::String(crash.join("|")));
        if !known
            && !crash
                .iter()
                .zip(&seen)
                .any(|(part, last)| !part.is_empty() && part == last)
        {
            return Ok(Some(("kernel_crash".into(), "previous boot".into())));
        }
    }
    Ok(None)
}

/// A new kernel taint this boot.
fn kernel_warning(root: &Root, value: &mut Reports, boot: &str) -> Result<Fired, Failure> {
    let tainted = int(root, "/proc/sys/kernel/tainted");
    if let Some(tainted) = tainted.filter(|t| *t != 0) {
        if changed(value, "tainted", Value::String(format!("{boot}:{tainted}")))? {
            return Ok(Some(("kernel_warning".into(), format!("taint {tainted}"))));
        }
    }
    Ok(None)
}

/// The previous boot ended without an orderly shutdown (with what the PMIC and loader saw).
fn unclean_restart(root: &Root, value: &mut Reports, boot: &str) -> Result<Fired, Failure> {
    let evidence = json_dict(root, "/data/system/platform/previous-boot-evidence.json");
    if matches!(
        evidence.get("previous_orderly_shutdown"),
        Some(Value::Bool(false))
    ) {
        let previous = evidence
            .get("previous_boot_id")
            .filter(|v| filled(v))
            .map(text_of)
            .unwrap_or_else(|| boot.to_owned());
        if changed(value, "unclean", Value::String(previous))? {
            // F56/F57: the PMIC's long-press record tells a held Power from a cut, the loader's warm
            // start a watchdog reset (or kernel restart) from both.
            let pmic = evidence
                .get("pmic")
                .filter(|p| p.is_object())
                .cloned()
                .unwrap_or(object());
            let loader = evidence
                .get("loader")
                .filter(|p| p.is_object())
                .cloned()
                .unwrap_or(object());
            let field = |v: &Value, k: &str| text_of(v.get(k).unwrap_or(&Value::Null));
            let loader_text = match loader.get("reset") {
                Some(reset) if filled(reset) => {
                    format!("{}({})", text_of(reset), field(&loader, "boot_reason"))
                }
                _ => "not_observed".into(),
            };
            let pmic_text = pmic
                .get("long_press_reset_reason")
                .map_or_else(|| "not_observed".into(), text_of);
            return Ok(Some((
                "unclean_restart".into(),
                format!(
                    "{} {} last stage {} loader {loader_text} pmic {pmic_text}",
                    field(&evidence, "reset_cause"),
                    field(&evidence, "reset_cause_reason"),
                    field(&evidence, "previous_last_stage"),
                ),
            )));
        }
    }
    Ok(None)
}

/// A new Reborn panic of this release.
fn reborn_crash(root: &Root, value: &mut Reports) -> Result<Fired, Failure> {
    let panic_path = root.path("/data/reborn/logs/panic-last.json");
    if panic_path.is_file() {
        // One report per crash, not per rewrite: a crash at every start (playlists.rs:223, #49)
        // rewrote the file each restart, every boot.
        let record = json_dict(root, "/data/reborn/logs/panic-last.json");
        let parts: Vec<Value> = ["version", "location", "message"]
            .iter()
            .map(|k| record.get(k).cloned().unwrap_or(Value::Null))
            .collect();
        let signature = if parts.iter().any(filled) {
            let text = y2p_core::json::python_dumps_sorted(
                &Value::Array(parts),
                y2p_core::json::Separators::Spaced,
            )
            .unwrap_or_default();
            sha256_hex(text.as_bytes())[..16].to_owned()
        } else {
            use std::os::unix::fs::MetadataExt;
            std::fs::metadata(&panic_path)
                .map_or(0, |m| m.mtime())
                .to_string()
        };
        // A panic of an earlier release is not news on this one: a player that updated past 8.12
        // resent its old #49 panic once (19a41489, d48362a7). Its hash is still recorded and the
        // file still goes into any report.
        let version = record
            .get("version")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let release = or_dict(json_dict(root, "/etc/y2linux/release.json"));
        let running = if filled(&release) {
            release
        } else {
            or_dict(json_dict(root, "/etc/y2linux/versions.json"))
        };
        let running = running
            .get("reborn_version")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let earlier = match (&version, &running) {
            (Some(v), Some(r)) => !r.is_empty() && v != r && !v.starts_with(&format!("{r}-")),
            _ => false,
        };
        if changed(value, "reborn_panic", Value::String(signature))? && !earlier {
            // Reborn 8.12 catches a worker's panic, skips that one file and runs on.
            let isolated = matches!(record.get("isolated"), Some(Value::Bool(true)));
            return Ok(Some((
                "reborn_crash".into(),
                if isolated { "isolated panic" } else { "panic" }.into(),
            )));
        }
    }
    Ok(None)
}

/// Required Reborn subsystems that failed (Reborn's health answer); an answer that is no object
/// stops the check.
fn reborn_failure(root: &Root, value: &mut Reports, boot: &str) -> Result<Fired, Failure> {
    let health = command::run_with(
        root,
        &["rebornctl", "health", "--json"],
        5.0,
        262_144,
        false,
    );
    let output = health.output.unwrap_or_default();
    let subsystems = loads_text(if output.is_empty() { "{}" } else { &output });
    // An answer, or its subsystems, that is no object is refused: the check stops here, before
    // reports.json is saved.
    let subsystems = match subsystems {
        None => object(),
        Some(v @ Value::Object(_)) => v.get("subsystems").cloned().unwrap_or(object()),
        Some(_) => return Err(Failure::value(HEALTH_UNREADABLE)),
    };
    let mut failed: Vec<String> = match &subsystems {
        Value::Object(items) => items
            .iter()
            .filter(|(_, e)| {
                e.is_object()
                    && e.get("state").is_some_and(|s| s.eq(&Value::from("failed")))
                    && e.get("required").is_some_and(filled)
            })
            .map(|(k, _)| k.clone())
            .collect(),
        _ => return Err(Failure::value(HEALTH_UNREADABLE)),
    };
    failed.sort();
    if !failed.is_empty()
        && changed(
            value,
            "reborn_failed",
            Value::String(format!("{boot}:{}", failed.join(","))),
        )?
    {
        return Ok(Some(("reborn_failure".into(), failed.join(","))));
    }
    Ok(None)
}

/// An update that failed or was rolled back.
fn update_failure(root: &Root, value: &mut Reports) -> Result<Fired, Failure> {
    let update = json_dict(root, "/data/updates/state.json");
    let update_state = update.get("state").cloned().unwrap_or(Value::Null);
    if ["Failed", "RolledBack", "RescueRequired", "RollbackPending"]
        .iter()
        .any(|s| update_state.eq(&Value::from(*s)))
    {
        let marker = format!(
            "{}:{}",
            text_of(update.get("sequence").unwrap_or(&Value::Null)),
            text_of(&update_state)
        );
        if changed(value, "update", Value::String(marker))? {
            return Ok(Some(("update_failure".into(), text_of(&update_state))));
        }
    }
    Ok(None)
}

/// A real sleep failure (not a wake event).
fn sleep_failure(root: &Root, value: &mut Reports, boot: &str) -> Result<Fired, Failure> {
    // A sleep cancelled by a wake event (a button, the charger) is counted as a failure with -EBUSY:
    // routine on a player. Report a failed resume, tasks that would not freeze, or a driver that
    // refused with another error.
    let stats = "/sys/power/suspend_stats/";
    let fails = int(root, &format!("{stats}fail")).unwrap_or(0);
    let resumes: i64 = [
        "failed_resume",
        "failed_resume_early",
        "failed_resume_noirq",
    ]
    .iter()
    .map(|n| int(root, &format!("{stats}{n}")).unwrap_or(0))
    .sum();
    let frozen = int(root, &format!("{stats}failed_freeze")).unwrap_or(0);
    let code = python_strip(
        &root
            .read(&format!("{stats}last_failed_errno"))
            .unwrap_or_default(),
    )
    .to_owned();
    let real = resumes != 0 || frozen != 0 || (fails != 0 && code != "0" && code != "-16");
    if real
        && changed(
            value,
            "suspend_fail",
            Value::String(format!("{boot}:{fails}:{resumes}:{frozen}")),
        )?
    {
        let or_mark = |name: &str| {
            let text = root.read(&format!("{stats}{name}")).unwrap_or_default();
            let text = python_strip(&text);
            if text.is_empty() {
                "?".to_owned()
            } else {
                text.to_owned()
            }
        };
        return Ok(Some((
            "sleep_failure".into(),
            format!(
                "suspend_stats fail {fails} resume {resumes} freeze {frozen} errno {} step {} dev {}",
                if code.is_empty() { "?" } else { &code },
                or_mark("last_failed_step"),
                or_mark("last_failed_dev")
            ),
        )));
    }
    Ok(None)
}

/// The connectivity chip reported an error.
fn radio_failure(root: &Root, value: &mut Reports, boot: &str) -> Result<Fired, Failure> {
    // Only the radio itself: wifi-failure.json is rewritten on every Wi-Fi connection and names the
    // network's answers (wrong password, out of range).
    static ERROR: OnceLock<Regex> = OnceLock::new();
    let error_re = lazy(&ERROR, r"error=(-?\d+)");
    let status_line = root
        .read("/sys/devices/platform/18070000.connectivity/status")
        .unwrap_or_default();
    let error = pyre::finditer(error_re, &status_line, 0, pyre::word_start)
        .into_iter()
        .next()
        .map(|c| c[1].to_owned());
    if let Some(error) = error.filter(|e| e != "0") {
        if changed(
            value,
            "conn_error",
            Value::String(format!("{boot}:{error}")),
        )? {
            return Ok(Some((
                "radio_failure".into(),
                format!("connectivity error={error}"),
            )));
        }
    }
    Ok(None)
}

/// A display timeout in the kernel log that matters (not the boot-time DSI lag once Reborn drew).
fn display_failure(
    root: &Root,
    value: &mut Reports,
    boot: &str,
    kernel: &str,
) -> Result<Fired, Failure> {
    // The DSI read lag at boot (a restart through the recovery can leave the SMI gate closed) is
    // worked around by the driver: only a problem when Reborn's first frame never reached the screen.
    // Timeouts later always are.
    let ready = or_dict(json_dict(root, "/run/y2/application-ready.json"));
    let lit = ready
        .get("boot_id")
        .is_some_and(|b| b.eq(&Value::from(boot)))
        && matches!(ready.get("first_frame"), Some(Value::Bool(true)));
    let darks: Vec<&str> = dark()
        .find_iter(kernel)
        .map(|m| m.as_str())
        .filter(|m| !(lit && m.starts_with("DSI register")))
        .collect();
    if !darks.is_empty() && changed(value, "dark", Value::String(boot.to_owned()))? {
        return Ok(Some(("display_failure".into(), darks[0].to_owned())));
    }
    Ok(None)
}

/// A storage error in the kernel log or ext4 error counts.
fn storage_error(
    root: &Root,
    value: &mut Reports,
    boot: &str,
    kernel: &str,
) -> Result<Fired, Failure> {
    let found = storage().find(kernel).map(|m| m.as_str().to_owned());
    let ext4: i64 = root
        .glob("/sys/fs/ext4/*/errors_count")
        .iter()
        .map(|p| int(root, &rel(root, p)).unwrap_or(0))
        .sum();
    if (found.is_some() || ext4 != 0)
        && changed(
            value,
            "storage",
            Value::String(format!(
                "{boot}:{}:{ext4}",
                if found.is_some() { "True" } else { "False" }
            )),
        )?
    {
        return Ok(Some((
            "storage_error".into(),
            found.unwrap_or_else(|| format!("ext4 errors_count {ext4}")),
        )));
    }
    Ok(None)
}

/// `report check`: queue one report for the triggers fired in this check (when enabled).
///
/// They usually share a cause (a crash loop: unclean restart, Reborn crash and a rolled-back update
/// within seconds) and one report carries all of them; one report each sent near copies against the
/// daily limit.
pub fn run_checks(root: &Root) -> Result<Value, Failure> {
    if !Reports::load(root).enabled() {
        return Ok(Value::Array(Vec::new()));
    }
    let fired = check(root)?;
    if fired.is_empty() {
        return Ok(Value::Array(Vec::new()));
    }
    let boot = root.boot_id().unwrap_or_default();
    let reported = Reports::load(root).reported(&boot)?;
    let mut fresh: Vec<(String, String)> = Vec::new();
    for (t, d) in &fired {
        if !pyops::contains(&reported, t).or_refuse(STATE_UNREADABLE)? {
            fresh.push((t.clone(), d.clone()));
        }
    }
    let fired = if fresh.is_empty() { fired } else { fresh };
    let trigger = fired[0].0.clone();
    let detail = if fired.len() == 1 {
        fired[0].1.clone()
    } else {
        fired
            .iter()
            .map(|(t, d)| format!("{t}: {d}"))
            .collect::<Vec<_>>()
            .join("; ")
    };
    let triggers: Vec<String> = fired.iter().map(|(t, _)| t.clone()).collect();
    let mut result = super::upload::queue(root, &triggers, &detail, None, &Progress::quiet())?;
    result.set("trigger", Value::String(trigger));
    result.set(
        "triggers",
        Value::Array(triggers.into_iter().map(Value::String).collect()),
    );
    Ok(Value::Array(vec![result]))
}

#[cfg(test)]
#[path = "checks_tests.rs"]
mod checks_tests;
