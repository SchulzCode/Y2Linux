//! report.py's automatic triggers: `crash_signature`, `check` and `run_checks`.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    boot_id,
    build::{rel, Progress},
    json_dict, or_dict,
    pyjson::{self, py_str, Py},
    pyops,
    pyre::{self, lazy, S},
    state,
};
use crate::reply::Failure;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::sync::OnceLock;
use y2p_core::{command, Root};

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

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `crash_signature(ctx)`: the newest retained kernel crash as (record time, kernel timestamp, text
/// hash), or None.
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
        hex(&Sha256::digest(text.as_bytes()))[..16].to_owned()
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

/// `_changed(value, name, current)`: True once per new value of a watched marker (`value['seen']`
/// that is no dict: AttributeError at its `.get`).
fn changed(value: &mut Py, name: &str, current: Py) -> Result<bool, Failure> {
    if matches!(&current, Py::None) || current.py_eq(&Py::str("")) || current.py_eq(&Py::int(0)) {
        return Ok(false);
    }
    let seen = value.get_mut("seen").expect("state has seen");
    if pyops::get(seen, name)?.is_some_and(|v| v.py_eq(&current)) {
        return Ok(false);
    }
    seen.set(name, current);
    Ok(true)
}

fn int(root: &Root, name: &str) -> Option<i64> {
    root.integer(name)
}

/// `check(ctx)`: every automatic trigger that fired since the last check, as (trigger, detail).
pub fn check(root: &Root) -> Result<Vec<(String, String)>, Failure> {
    let mut value = state::state(root);
    let mut fired: Vec<(String, String)> = Vec::new();
    let boot = boot_id(root).unwrap_or_default();
    if let Some(crash) = crash_signature(root) {
        let previous = pyops::get(value.get("seen").expect("state has seen"), "pstore_crash")?
            .filter(|v| v.truthy())
            .map(py_str)
            .unwrap_or_default();
        let seen: Vec<String> = (previous + "||")
            .split('|')
            .take(3)
            .map(str::to_owned)
            .collect();
        // Before 8.42 the key was the all-records hash: a crash on record then was already reported
        // by that release (no resend after the update).
        let seen_map = value.get("seen").expect("seen");
        let known = seen_map.get("pstore_crash").is_none() && seen_map.get("pstore").is_some();
        let joined = crash.join("|");
        value
            .get_mut("seen")
            .expect("seen")
            .set("pstore_crash", Py::Str(joined));
        if !known
            && !crash
                .iter()
                .zip(&seen)
                .any(|(part, last)| !part.is_empty() && part == last)
        {
            fired.push(("kernel_crash".into(), "previous boot".into()));
        }
    }
    let tainted = int(root, "/proc/sys/kernel/tainted");
    if let Some(tainted) = tainted.filter(|t| *t != 0) {
        if changed(&mut value, "tainted", Py::Str(format!("{boot}:{tainted}")))? {
            fired.push(("kernel_warning".into(), format!("taint {tainted}")));
        }
    }
    let evidence = json_dict(root, "/data/system/platform/previous-boot-evidence.json");
    if matches!(
        evidence.get("previous_orderly_shutdown"),
        Some(Py::Bool(false))
    ) {
        let previous = evidence
            .get("previous_boot_id")
            .filter(|v| v.truthy())
            .map(py_str)
            .unwrap_or_else(|| boot.clone());
        if changed(&mut value, "unclean", Py::Str(previous))? {
            // F56/F57: the PMIC's long-press record tells a held Power from a cut, the loader's warm
            // start a watchdog reset (or kernel restart) from both.
            let pmic = evidence
                .get("pmic")
                .filter(|p| p.is_dict())
                .cloned()
                .unwrap_or(Py::dict());
            let loader = evidence
                .get("loader")
                .filter(|p| p.is_dict())
                .cloned()
                .unwrap_or(Py::dict());
            let field = |v: &Py, k: &str| py_str(v.get(k).unwrap_or(&Py::None));
            let loader_text = match loader.get("reset") {
                Some(reset) if reset.truthy() => {
                    format!("{}({})", py_str(reset), field(&loader, "boot_reason"))
                }
                _ => "not_observed".into(),
            };
            let pmic_text = pmic
                .get("long_press_reset_reason")
                .map_or_else(|| "not_observed".into(), py_str);
            fired.push((
                "unclean_restart".into(),
                format!(
                    "{} {} last stage {} loader {loader_text} pmic {pmic_text}",
                    field(&evidence, "reset_cause"),
                    field(&evidence, "reset_cause_reason"),
                    field(&evidence, "previous_last_stage"),
                ),
            ));
        }
    }
    let panic_path = root.path("/data/reborn/logs/panic-last.json");
    if panic_path.is_file() {
        // One report per crash, not per rewrite: a crash at every start (playlists.rs:223, #49)
        // rewrote the file each restart, every boot.
        let record = json_dict(root, "/data/reborn/logs/panic-last.json");
        let parts: Vec<Py> = ["version", "location", "message"]
            .iter()
            .map(|k| record.get(k).cloned().unwrap_or(Py::None))
            .collect();
        let signature = if parts.iter().any(Py::truthy) {
            let text = pyjson::dumps(&Py::List(parts), pyjson::Style::Spaced);
            hex(&Sha256::digest(text.as_bytes()))[..16].to_owned()
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
            .and_then(Py::as_str)
            .map(str::to_owned);
        let release = or_dict(json_dict(root, "/etc/y2linux/release.json"));
        let running = if release.truthy() {
            release
        } else {
            or_dict(json_dict(root, "/etc/y2linux/versions.json"))
        };
        let running = running
            .get("reborn_version")
            .and_then(Py::as_str)
            .map(str::to_owned);
        let earlier = match (&version, &running) {
            (Some(v), Some(r)) => !r.is_empty() && v != r && !v.starts_with(&format!("{r}-")),
            _ => false,
        };
        if changed(&mut value, "reborn_panic", Py::Str(signature))? && !earlier {
            // Reborn 8.12 catches a worker's panic, skips that one file and runs on.
            let isolated = matches!(record.get("isolated"), Some(Py::Bool(true)));
            fired.push((
                "reborn_crash".into(),
                if isolated { "isolated panic" } else { "panic" }.into(),
            ));
        }
    }
    let health = command::run_with(
        root,
        &["rebornctl", "health", "--json"],
        5.0,
        262_144,
        false,
    );
    let output = health.output.unwrap_or_default();
    let subsystems = pyjson::loads(if output.is_empty() { "{}" } else { &output });
    // `json.loads(...).get('subsystems', {})` on a non-dict answer raises AttributeError (a
    // traceback); Python's run then stops here.
    let subsystems = match subsystems {
        None => Py::dict(),
        Some(v @ Py::Dict(_)) => v.get("subsystems").cloned().unwrap_or(Py::dict()),
        Some(other) => return Err(pyops::no_attribute(&other, "get")), // a traceback
    };
    let mut failed: Vec<String> = match &subsystems {
        Py::Dict(items) => items
            .iter()
            .filter(|(_, e)| {
                e.is_dict()
                    && e.get("state").is_some_and(|s| s.py_eq(&Py::str("failed")))
                    && e.get("required").is_some_and(Py::truthy)
            })
            .map(|(k, _)| k.clone())
            .collect(),
        other => return Err(pyops::no_attribute(other, "items")),
    };
    failed.sort();
    if !failed.is_empty()
        && changed(
            &mut value,
            "reborn_failed",
            Py::Str(format!("{boot}:{}", failed.join(","))),
        )?
    {
        fired.push(("reborn_failure".into(), failed.join(",")));
    }
    let update = json_dict(root, "/data/updates/state.json");
    let update_state = update.get("state").cloned().unwrap_or(Py::None);
    if ["Failed", "RolledBack", "RescueRequired", "RollbackPending"]
        .iter()
        .any(|s| update_state.py_eq(&Py::str(*s)))
    {
        let marker = format!(
            "{}:{}",
            py_str(update.get("sequence").unwrap_or(&Py::None)),
            py_str(&update_state)
        );
        if changed(&mut value, "update", Py::Str(marker))? {
            fired.push(("update_failure".into(), py_str(&update_state)));
        }
    }
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
    let code = pyre::strip(
        &root
            .read(&format!("{stats}last_failed_errno"))
            .unwrap_or_default(),
    )
    .to_owned();
    let real = resumes != 0 || frozen != 0 || (fails != 0 && code != "0" && code != "-16");
    if real
        && changed(
            &mut value,
            "suspend_fail",
            Py::Str(format!("{boot}:{fails}:{resumes}:{frozen}")),
        )?
    {
        let or_mark = |name: &str| {
            let text = root.read(&format!("{stats}{name}")).unwrap_or_default();
            let text = pyre::strip(&text);
            if text.is_empty() {
                "?".to_owned()
            } else {
                text.to_owned()
            }
        };
        fired.push((
            "sleep_failure".into(),
            format!(
                "suspend_stats fail {fails} resume {resumes} freeze {frozen} errno {} step {} dev {}",
                if code.is_empty() { "?" } else { &code },
                or_mark("last_failed_step"),
                or_mark("last_failed_dev")
            ),
        ));
    }
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
        if changed(&mut value, "conn_error", Py::Str(format!("{boot}:{error}")))? {
            fired.push((
                "radio_failure".into(),
                format!("connectivity error={error}"),
            ));
        }
    }
    let kernel = command::run_with(root, &["dmesg"], 5.0, 2 * 1024 * 1024, false)
        .output
        .unwrap_or_default();
    // The DSI read lag at boot (a restart through the recovery can leave the SMI gate closed) is
    // worked around by the driver: only a problem when Reborn's first frame never reached the screen.
    // Timeouts later always are.
    let ready = or_dict(json_dict(root, "/run/y2/application-ready.json"));
    let lit = ready
        .get("boot_id")
        .is_some_and(|b| b.py_eq(&Py::str(boot.clone())))
        && matches!(ready.get("first_frame"), Some(Py::Bool(true)));
    let darks: Vec<&str> = dark()
        .find_iter(&kernel)
        .map(|m| m.as_str())
        .filter(|m| !(lit && m.starts_with("DSI register")))
        .collect();
    if !darks.is_empty() && changed(&mut value, "dark", Py::Str(boot.clone()))? {
        fired.push(("display_failure".into(), darks[0].to_owned()));
    }
    let found = storage().find(&kernel).map(|m| m.as_str().to_owned());
    let ext4: i64 = root
        .glob("/sys/fs/ext4/*/errors_count")
        .iter()
        .map(|p| int(root, &rel(root, p)).unwrap_or(0))
        .sum();
    if (found.is_some() || ext4 != 0)
        && changed(
            &mut value,
            "storage",
            Py::Str(format!(
                "{boot}:{}:{ext4}",
                if found.is_some() { "True" } else { "False" }
            )),
        )?
    {
        fired.push((
            "storage_error".into(),
            found.unwrap_or_else(|| format!("ext4 errors_count {ext4}")),
        ));
    }
    state::save(root, &value)?;
    Ok(fired)
}

/// `run_checks(ctx)`: queue one report for the triggers fired in this check (when enabled).
///
/// They usually share a cause (a crash loop: unclean restart, Reborn crash and a rolled-back update
/// within seconds) and one report carries all of them; one report each sent near copies against the
/// daily limit.
pub fn run_checks(root: &Root) -> Result<Py, Failure> {
    if !matches!(state::state(root).get("enabled"), Some(Py::Bool(true))) {
        return Ok(Py::List(Vec::new()));
    }
    let fired = check(root)?;
    if fired.is_empty() {
        return Ok(Py::List(Vec::new()));
    }
    let boot = boot_id(root).unwrap_or_default();
    // `state(ctx)['boot_triggers'].get(boot, [])`, then `t not in reported`.
    let triggers = state::state(root)
        .get("boot_triggers")
        .cloned()
        .unwrap_or(Py::None);
    let reported = pyops::get(&triggers, &boot)?
        .cloned()
        .unwrap_or(Py::List(Vec::new()));
    let mut fresh: Vec<(String, String)> = Vec::new();
    for (t, d) in &fired {
        if !pyops::contains(&reported, t)? {
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
    result.set("trigger", Py::Str(trigger));
    result.set(
        "triggers",
        Py::List(triggers.into_iter().map(Py::Str).collect()),
    );
    Ok(Py::List(vec![result]))
}

#[cfg(test)]
#[path = "checks_tests.rs"]
mod checks_tests;
