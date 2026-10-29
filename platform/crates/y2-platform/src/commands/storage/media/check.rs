//! The read-only filesystem check: at admission, and in the background for a card admitted while
//! its check had not finished (F49); what each check found is remembered across boots (F12).
// SPDX-License-Identifier: GPL-2.0-only
use super::super::observe::MEDIA_MOUNT;
use super::super::records::ReadOnly;
use super::super::sys::{exists, get, lock_within, mkdir, object, to_object, wall_seconds, Object};
use super::window::{sd_state, PROTECT_OFF};
use super::{candidates, milliseconds, prefix, set, set_state, Card, SD};
use crate::reply::{atomic_json, Failure};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Instant;
use y2p_core::{
    command,
    mounts::mounts_at,
    py::{truthy, PyValue},
    storage_media::{fat_benign_only, fat_findings, fat_other_shapes},
    Root,
};

// F49 (d32f8f3c, 8.41): fsck.exfat -n took 59 s on a 1 TB card with 71 400
// songs and ran out of the old 60 s at a few more; the card then stayed
// read-only behind a Repair that found nothing. A FAT/exFAT card whose check
// does not finish within ADMISSION_CHECK_SECONDS is mounted read-only at once
// ('checking') and checked again in the background (verify) for CHECK_SECONDS.
pub(super) const ADMISSION_CHECK_SECONDS: f64 = 20.0;
pub(super) const EXT4_CHECK_SECONDS: f64 = 60.0; // no background check: a read-only ext4 mount replays its journal
pub(super) const CHECK_SECONDS: f64 = 1800.0;

// Why the last cards were refused or let in, across boots (F12: a card refused
// at every insertion left no reason in a report). No checker output, no names.
pub(super) const ADMISSIONS: &str = "/data/system/platform/sd-admissions.json";
pub(super) const ADMISSIONS_KEPT: usize = 10;
const READ_ONLY_FALLBACK: [&str; 2] = ["vfat", "exfat"];

pub(super) fn fallback_filesystem(filesystem: &str) -> bool {
    READ_ONLY_FALLBACK.contains(&filesystem)
}

/// What a filesystem check found (`filesystem_check` of the records). The checker's output is kept
/// only from a run that passed: a failed one can name files.
#[derive(Clone, Debug, Serialize)]
pub(super) struct Check {
    pub(super) ok: bool,
    pub(super) reason: Option<String>,
    output: Option<String>,
    elapsed_ms: u64,
    /// The classes of what it reported: `timeout`, the FAT line classes, or `other`.
    #[serde(skip_serializing_if = "Option::is_none")]
    findings: Option<Vec<String>>,
    /// What FAT's unclassified lines said, without names.
    #[serde(skip_serializing_if = "Option::is_none")]
    other: Option<Value>,
    /// Why a FAT check that failed passed anyway: `unclean_unmount` (#44) or `benign`.
    #[serde(skip_serializing_if = "Option::is_none")]
    finding: Option<&'static str>,
    /// Run by `verify` after the card was admitted read-only.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    background: bool,
}

impl Check {
    /// The checker ran out of time; that is not damage (F49).
    pub(super) fn timed_out(&self) -> bool {
        self.findings.as_ref().is_some_and(|f| f == &["timeout"])
    }

    pub(super) fn value(&self) -> Value {
        Value::Object(to_object(self))
    }
}

/// The admission check, never a repair. Benign findings pass with `finding`, and every run names
/// its classes in `findings`.
pub(super) fn read_only_check(
    root: &Root,
    card: &Card,
    force: bool,
    timeout: Option<f64>,
) -> Check {
    let filesystem = card.filesystem;
    let mut argv: Vec<&str> = match filesystem {
        "ext4" if force => vec!["e2fsck", "-f", "-n"],
        "ext4" => vec!["e2fsck", "-n"],
        "vfat" => vec!["fsck.fat", "-n"],
        _ => vec!["fsck.exfat", "-n"],
    };
    let timeout = timeout.unwrap_or(if fallback_filesystem(filesystem) {
        ADMISSION_CHECK_SECONDS
    } else {
        EXT4_CHECK_SECONDS
    });
    argv.push(&card.device);
    let started = Instant::now();
    let result = command::run_with(root, &argv, timeout, command::LIMIT, filesystem == "vfat");
    let mut check = Check {
        ok: result.ok,
        reason: result.reason.clone(),
        output: if result.ok {
            result.output.clone()
        } else {
            None
        },
        elapsed_ms: milliseconds(started),
        findings: None,
        other: None,
        finding: None,
        background: false,
    };
    let output = result.output.as_deref();
    if result.reason.as_deref() == Some("command_timeout") {
        check.findings = Some(vec!["timeout".into()]);
    } else if !result.ok
        && result.reason.as_deref() == Some("command_failed")
        && filesystem == "vfat"
    {
        let findings = fat_findings(output);
        if findings.iter().any(|f| f == "other") {
            check.other = Some(fat_other_shapes(output, 5));
        }
        let only_dirty = findings == ["dirty"];
        check.findings = Some(findings);
        if fat_benign_only(output) {
            // 'unclean_unmount' is the #44 name for the dirty bit alone.
            check.ok = true;
            check.finding = Some(if only_dirty {
                "unclean_unmount"
            } else {
                "benign"
            });
        }
    } else if !result.ok {
        check.findings = Some(vec!["other".into()]);
    }
    check
}

/// One entry of sd-admissions.json.
#[derive(Serialize)]
struct Admission<'a> {
    time: i64,
    card: String,
    filesystem: &'a str,
    ok: bool,
    reason: &'a Option<String>,
    finding: Option<&'static str>,
    findings: &'a [String],
    elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    other: Option<&'a Value>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    background: bool,
}

pub(super) fn remember_admission(root: &Root, card: &Card, check: &Check) {
    let old = object(root, ADMISSIONS);
    let mut entries: Vec<Value> = match old.get("admissions") {
        None => Vec::new(),
        Some(Value::Array(items)) => items.iter().filter(|e| e.is_object()).cloned().collect(),
        // A text or an object holds no entries; a number or a flag is no list at all: the record
        // is left as it is rather than replaced.
        Some(Value::String(_)) | Some(Value::Object(_)) => Vec::new(),
        Some(_) => return,
    };
    let entry = Admission {
        time: wall_seconds(),
        card: prefix(card.id()),
        filesystem: card.filesystem,
        ok: check.ok,
        reason: &check.reason,
        finding: check.finding,
        findings: check.findings.as_deref().unwrap_or_default(),
        elapsed_ms: check.elapsed_ms,
        other: check.other.as_ref().filter(|o| truthy(o)),
        background: check.background,
    };
    entries.push(Value::Object(to_object(&entry)));
    let path = root.path(ADMISSIONS);
    let Some(directory) = path.parent() else {
        return;
    };
    if mkdir(directory, 0o700).is_err() {
        return;
    }
    let kept = entries.split_off(entries.len().saturating_sub(ADMISSIONS_KEPT));
    let _ = atomic_json(&path, &json!({"schema": 1, "admissions": kept}), false);
}

// ---------------------------------------------------------------------------
// Background check (F49) of a card mounted read-only ('checking') because its
// check did not finish within the admission budget. The service starts it
// (`media verify`) while the claim says 'checking'. No lock is held while the
// checker reads; the result applies only to the same mount of the same card.

/// CLOCK_BOOTTIME − CLOCK_MONOTONIC: grows only while the system sleeps.
fn slept_ns(root: &Root) -> i128 {
    let clock = root.clock();
    i128::from(clock.boottime_ns()) - i128::from(clock.monotonic_ns())
}

/// The id of a record's card identity (`null` without one); an identity that is not an object is
/// refused.
pub(super) fn identity_id(identity: &Value) -> Result<Value, Failure> {
    match identity {
        Value::Object(identity) => Ok(get(identity, "id").clone()),
        other if truthy(other) => Err(Failure::value("card_identity_invalid")),
        _ => Ok(Value::Null),
    }
}

/// The one candidate is the card a record names by identity id and block instance.
fn same_card(cards: &[Card], id: &Value, instance: &Value) -> bool {
    match cards {
        [card] => json!(card.id()).py_eq(id) && json!(card.source_instance).py_eq(instance),
        _ => false,
    }
}

pub fn verify(root: &Root) -> Result<Object, Failure> {
    let mut state = Object::new();
    set(&mut state, "schema", 1);
    set(&mut state, "action", "verify");
    set_state(&mut state, "Starting", Value::Null);
    let (claim, mount) = sd_state(root);
    if mount.is_none() || claim.read_only() != Some(ReadOnly::Checking) {
        set_state(&mut state, "Unavailable", "nothing_to_check");
        return Ok(state);
    }
    let id = identity_id(claim.get("identity"))?;
    let instance = claim.get("source_instance");
    let available = candidates(root);
    if !same_card(&available, &id, instance) {
        set_state(&mut state, "Failed", "card_changed");
        return Ok(state);
    }
    let card = &available[0];
    let slept = slept_ns(root);
    let mut check = read_only_check(root, card, false, Some(CHECK_SECONDS));
    check.background = true;
    if !check.ok && slept_ns(root) - slept > 1_000_000_000 {
        // A check read across a system sleep is no verdict; the service runs it again.
        set_state(&mut state, "Failed", "slept_during_check");
        return Ok(state);
    }
    remember_admission(root, card, &check);
    let runtime = root.path("/run/y2");
    let Some(_lock) = lock_within(&runtime.join("media.lock"), 30.0)? else {
        set_state(&mut state, "Failed", "media_operation_in_progress");
        return Ok(state);
    };
    let (now, mount) = sd_state(root);
    if mount.is_none()
        || !now.get("mount_id").py_eq(claim.get("mount_id"))
        || now.read_only() != Some(ReadOnly::Checking)
        || !same_card(&candidates(root), &id, instance)
    {
        set_state(&mut state, "Failed", "card_changed");
        return Ok(state);
    }
    let mut now = now.into_object();
    if check.ok {
        let mut read_only = ReadOnly::Protected;
        if exists(&root.path(PROTECT_OFF)) {
            command::run(root, &["mount", "-o", "remount,rw", SD], 10.0);
            let current = mounts_at(root, SD);
            if current.first().is_some_and(|m| m.has_option("rw")) {
                read_only = ReadOnly::ReadWrite;
            }
        }
        set_state(&mut now, "Ready", Value::Null);
        set(&mut now, "read_only", read_only.value());
    } else {
        // Errors found, or still unfinished after CHECK_SECONDS: Repair is offered.
        let read_only = if check.timed_out() {
            ReadOnly::CheckTimedOut
        } else {
            ReadOnly::FilesystemErrors
        };
        set_state(&mut now, "Degraded", "read_only");
        set(&mut now, "read_only", read_only.value());
    }
    set(&mut now, "filesystem_check", check.value());
    atomic_json(&root.path(MEDIA_MOUNT), &now, false)?;
    atomic_json(&runtime.join("media.json"), &now, false)?;
    state.extend(now);
    set(&mut state, "action", "verify");
    Ok(state)
}
