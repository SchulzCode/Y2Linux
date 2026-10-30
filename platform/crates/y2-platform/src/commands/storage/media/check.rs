//! The read-only filesystem check: at admission, and in the background for a card admitted while
//! its check had not finished (F49); what each check found is remembered across boots (F12).
// SPDX-License-Identifier: GPL-2.0-only
use super::super::observe::MEDIA_MOUNT;
pub(super) use super::super::records::Check;
use super::super::records::{MediaRecord, ReadOnly};
use super::super::sys::{exists, lock_within, mkdir, wall_seconds};
use super::window::{sd_state, PROTECT_OFF};
use super::{candidates, milliseconds, prefix, Card, SD};
use crate::commands::storage::records::OnPlayer;
use crate::reply::{atomic_json, Failure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Instant;
use y2p_core::{
    command,
    lenient::Shaped,
    mounts::mounts_at,
    py::truthy,
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
            check.finding = Some(
                if only_dirty {
                    "unclean_unmount"
                } else {
                    "benign"
                }
                .into(),
            );
        }
    } else if !result.ok {
        check.findings = Some(vec!["other".into()]);
    }
    check
}

/// One entry of sd-admissions.json (/data: entries earlier versions wrote are kept as they are).
#[derive(Serialize)]
struct Admission<'a> {
    time: i64,
    card: String,
    filesystem: &'a str,
    ok: bool,
    reason: &'a Option<String>,
    finding: &'a Option<String>,
    findings: &'a [String],
    elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    other: Option<&'a Value>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    background: bool,
}

/// sd-admissions.json: its entries (objects; anything else in the list is left out).
#[derive(Deserialize)]
struct Admissions {
    #[serde(default = "no_entries", deserialize_with = "entries")]
    admissions: Option<Vec<Value>>,
}

/// A record without the list starts one.
fn no_entries() -> Option<Vec<Value>> {
    Some(Vec::new())
}

/// A list's objects; a text or an object holds no entries; a number, a flag or `null` is no list at
/// all.
fn entries<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<Value>>, D::Error> {
    Ok(match Value::deserialize(deserializer)? {
        Value::Array(items) => Some(items.into_iter().filter(Value::is_object).collect()),
        Value::String(_) | Value::Object(_) => Some(Vec::new()),
        _ => None,
    })
}

pub(super) fn remember_admission(root: &Root, card: &Card, check: &Check) {
    let old: Option<Admissions> =
        serde_json::from_value(Value::Object(root.json_object(ADMISSIONS))).ok();
    // A record whose list is a number or a flag is left as it is rather than replaced.
    let Some(mut entries) = old.map_or(Some(Vec::new()), |old| old.admissions) else {
        return;
    };
    let entry = Admission {
        time: wall_seconds(),
        card: prefix(card.id()),
        filesystem: card.filesystem,
        ok: check.ok,
        reason: &check.reason,
        finding: &check.finding,
        findings: check.findings.as_deref().unwrap_or_default(),
        elapsed_ms: check.elapsed_ms,
        other: check.other.as_ref().filter(|o| truthy(o)),
        background: check.background,
    };
    let Ok(entry) = serde_json::to_value(&entry) else {
        return;
    };
    entries.push(entry);
    let path = root.path(ADMISSIONS);
    let Some(directory) = path.parent() else {
        return;
    };
    if mkdir(directory, 0o700).is_err() {
        return;
    }
    let kept = entries.split_off(entries.len().saturating_sub(ADMISSIONS_KEPT));
    let _ = atomic_json(
        &path,
        &serde_json::json!({"schema": 1, "admissions": kept}),
        false,
    );
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

/// The one candidate is the card a record names by identity id and block instance.
fn same_card(cards: &[Card], id: Option<&str>, instance: Option<&str>) -> bool {
    match cards {
        [card] => Some(card.id()) == id && Some(card.source_instance.as_str()) == instance,
        _ => false,
    }
}

pub fn verify(root: &Root) -> Result<MediaRecord, Failure> {
    let mut state = MediaRecord::begin(root, "verify");
    let (claim, mount) = sd_state(root);
    if mount.is_none() || claim.read_only() != Some(ReadOnly::Checking) {
        state.set_state("Unavailable", Some("nothing_to_check"));
        return Ok(state);
    }
    let id = claim.identity_id()?;
    let instance = claim.source_instance.as_deref();
    let available = candidates(root);
    if !same_card(&available, id, instance) {
        state.set_state("Failed", Some("card_changed"));
        return Ok(state);
    }
    let card = &available[0];
    let slept = slept_ns(root);
    let mut check = read_only_check(root, card, false, Some(CHECK_SECONDS));
    check.background = true;
    if !check.ok && slept_ns(root) - slept > 1_000_000_000 {
        // A check read across a system sleep is no verdict; the service runs it again.
        state.set_state("Failed", Some("slept_during_check"));
        return Ok(state);
    }
    remember_admission(root, card, &check);
    let runtime = root.path("/run/y2");
    let Some(_lock) = lock_within(&runtime.join("media.lock"), 30.0)? else {
        state.set_state("Failed", Some("media_operation_in_progress"));
        return Ok(state);
    };
    let (mut now, mount) = sd_state(root);
    if mount.is_none()
        || now.mount_id != claim.mount_id
        || now.read_only() != Some(ReadOnly::Checking)
        || !same_card(&candidates(root), id, instance)
    {
        state.set_state("Failed", Some("card_changed"));
        return Ok(state);
    }
    if check.ok {
        let mut read_only = ReadOnly::Protected;
        if exists(&root.path(PROTECT_OFF)) {
            command::run(root, &["mount", "-o", "remount,rw", SD], 10.0);
            let current = mounts_at(root, SD);
            if current.first().is_some_and(|m| m.has_option("rw")) {
                read_only = ReadOnly::ReadWrite;
            }
        }
        now.set_state("Ready", None);
        now.set_read_only(read_only);
    } else {
        // Errors found, or still unfinished after CHECK_SECONDS: Repair is offered.
        let read_only = if check.timed_out() {
            ReadOnly::CheckTimedOut
        } else {
            ReadOnly::FilesystemErrors
        };
        now.set_state("Degraded", Some("read_only"));
        now.set_read_only(read_only);
    }
    now.filesystem_check = Some(Shaped::Is(check));
    atomic_json(&root.path(MEDIA_MOUNT), &now, false)?;
    atomic_json(&runtime.join("media.json"), &now, false)?;
    state.absorb(now);
    state.action = Some("verify".into());
    Ok(state)
}
