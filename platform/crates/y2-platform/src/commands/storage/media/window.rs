//! Write windows on a protected card (`media write`) and the reconcile tick that ends them.
// SPDX-License-Identifier: GPL-2.0-only
use super::super::records::{Claim, ReadOnly};
use super::super::sys::{get_eq, lock_within, object, to_object, try_lock, Mount, Object};
use super::{owned_mount, runtime, SD};
use crate::reply::{atomic_json, Failure};
use serde::Serialize;
use serde_json::{json, Value};
use y2p_core::{
    command,
    mounts::mounts_at,
    py::{number, truthy},
    Root,
};

// ---------------------------------------------------------------------------
// Protected card: a healthy card is mounted read-only and becomes writable only
// while something writes to it — the computer over USB, a playlist change, a
// report saved to the card, the firmware import removing its system.img. Each
// writer holds a named window with an end time; the reconcile tick (3 s) turns
// the card read-only again when none is left. A remount to read-only flushes the
// card and clears its dirty bit, so a card pulled out during playback is clean.

pub(super) const PROTECT_OFF: &str = "/data/system/platform/sd-always-writable"; // owner switch: the old rw mount
pub(super) const SD_WRITE: &str = "/run/y2/sd-write.json";
const WRITE_HOLD_MAX: i64 = 600;
const WRITE_HOLDERS: [&str; 4] = ["usb", "reborn", "report", "radiofw"];

/// sd-write.json: the open write windows of one mount, holder → monotonic end time.
#[derive(Serialize)]
struct Windows<'a> {
    schema: u8,
    boot_id: Option<String>,
    mount_id: i64,
    holders: &'a Object,
}

/// (claim, mount) of our current SD mount, else (claim, None).
pub(super) fn sd_state(root: &Root) -> (Claim, Option<Mount>) {
    let claim = Claim::load(root);
    let mut old = mounts_at(root, SD);
    if old.len() == 1 && owned_mount(root, &old[0]) && claim.holds(root, old[0].mount_id) {
        return (claim, old.pop());
    }
    (claim, None)
}

/// The live windows of `mount` (a holder whose end time has passed is left out).
fn writers(root: &Root, mount: &Mount) -> Result<Object, Failure> {
    let value = object(root, SD_WRITE);
    if !get_eq(&value, "boot_id", &Value::from(root.boot_id()))
        || !get_eq(&value, "mount_id", &json!(mount.mount_id))
    {
        return Ok(Object::new());
    }
    let holders = match value.get("holders") {
        Some(Value::Object(holders)) => holders.clone(),
        Some(other) if truthy(other) => return Err(Failure::value("sd_write_holders_invalid")),
        _ => Object::new(),
    };
    let now = root.clock().monotonic();
    Ok(holders
        .into_iter()
        .filter(|(k, v)| {
            WRITE_HOLDERS.contains(&k.as_str()) && number(Some(v)).is_some_and(|v| v > now)
        })
        .collect())
}

/// Remount to match the holders; records them. Returns writable now.
fn apply_window(root: &Root, mount: &Mount, holders: &Object) -> Result<bool, Failure> {
    let windows = Windows {
        schema: 1,
        boot_id: root.boot_id(),
        mount_id: mount.mount_id,
        holders,
    };
    atomic_json(&root.path(SD_WRITE), &windows, false)?;
    let mut writable = mount.has_option("rw");
    if !holders.is_empty() != writable {
        let mode = if holders.is_empty() { "ro" } else { "rw" };
        command::run(root, &["mount", "-o", &format!("remount,{mode}"), SD], 10.0);
        let now = mounts_at(root, SD);
        writable = now.first().is_some_and(|m| m.has_option("rw"));
    }
    Ok(writable)
}

/// What `write` prints (with `holders` once a window changed).
#[derive(Serialize)]
struct WindowAnswer {
    state: &'static str,
    reason: Option<&'static str>,
    writable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    holders: Option<Vec<String>>,
}

/// An answer before any window changed.
fn answer(state: &'static str, reason: Option<&'static str>, writable: bool) -> Object {
    to_object(&WindowAnswer {
        state,
        reason,
        writable,
        holders: None,
    })
}

/// Open (or end) the named holder's window on a protected card.
pub fn write_window(
    root: &Root,
    holder: &str,
    seconds: i64,
    release: bool,
) -> Result<Object, Failure> {
    if !WRITE_HOLDERS.contains(&holder) {
        return Err(Failure::value("unknown_write_holder"));
    }
    let runtime = runtime(root)?;
    // A check or a repair holds the card; a short admission passes.
    let Some(_lock) = lock_within(&runtime.join("media.lock"), 5.0)? else {
        return Ok(answer("Failed", Some("media_operation_in_progress"), false));
    };
    let (claim, mount) = sd_state(root);
    let Some(mount) = mount else {
        return Ok(answer("Unavailable", Some("not_mounted"), false));
    };
    match claim.read_only() {
        // Writable once its background check passes (USB asks again every 10 s).
        Some(ReadOnly::Checking) => return Ok(answer("Failed", Some("card_being_checked"), false)),
        Some(ReadOnly::ReadWrite) => return Ok(answer("Ready", None, mount.has_option("rw"))),
        Some(ReadOnly::Protected) => {}
        // Errors found or a check unfinished: Repair first, never a window.
        _ => return Ok(answer("Degraded", Some("card_has_errors"), false)),
    }
    let mut holders = writers(root, &mount)?;
    if release {
        holders.shift_remove(holder);
    } else {
        let hold = seconds.clamp(1, WRITE_HOLD_MAX);
        holders.insert(holder.into(), json!(root.clock().monotonic() + hold as f64));
    }
    let writable = apply_window(root, &mount, &holders)?;
    let ok = writable == !holders.is_empty() || release;
    let mut names: Vec<String> = holders.keys().cloned().collect();
    names.sort();
    Ok(to_object(&WindowAnswer {
        state: if ok { "Ready" } else { "Failed" },
        reason: (!ok).then_some("remount_failed"),
        writable,
        holders: Some(names),
    }))
}

/// Whether `protect` has work: a protected card still mounted read-write with no live window. The
/// service asks before it starts `media reconcile` (file reads only, no subprocess).
pub fn window_expired(root: &Root) -> bool {
    // The service asks every tick: cheapest first. With no card, or a card read-only between
    // writes (the steady state), the answer comes from the mountinfo alone.
    let current = mounts_at(root, SD);
    let [mount] = &current[..] else {
        return false;
    };
    if !mount.has_option("rw") {
        return false;
    }
    let claim = Claim::load(root);
    claim.read_only() == Some(ReadOnly::Protected)
        && claim.holds(root, mount.mount_id)
        && owned_mount(root, mount)
        // A holders record that is not an object fails reconcile too: never a restart per tick.
        && writers(root, mount).is_ok_and(|holders| holders.is_empty())
}

/// Reconcile tick: a protected card with no live window goes read-only.
pub(super) fn protect(root: &Root) -> Result<(), Failure> {
    match sd_state(root) {
        (claim, Some(m))
            if claim.read_only() == Some(ReadOnly::Protected) && m.has_option("rw") => {}
        _ => return Ok(()),
    }
    // A writer or an operation holds the card; the next tick tries again.
    if let Some(_lock) = try_lock(&root.path("/run/y2/media.lock"))? {
        if let (claim, Some(mount)) = sd_state(root) {
            if claim.read_only() == Some(ReadOnly::Protected) {
                let holders = writers(root, &mount)?;
                apply_window(root, &mount, &holders)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::storage::observe::MEDIA_MOUNT;
    use crate::testing::Fixture;

    const NOW: f64 = 1000.0;

    /// A protected card mounted by this boot (mount 51), `rw` or `ro`, with these holders'
    /// deadlines, at monotonic time NOW.
    fn protected(options: &str, holders: Value) -> Fixture {
        let clock = std::sync::Arc::new(y2p_core::clock::Manual::at(
            1_000_000_000_000,
            1_000_000_000_000,
        ));
        let fixture = Fixture::new().with_clock(clock);
        fixture
            .put("/proc/sys/kernel/random/boot_id", "boot-a\n")
            .put(
                "/proc/self/mountinfo",
                &format!(
                    "51 22 179:97 / /media/sd {options},nosuid,nodev,noexec,noatime - vfat \
                     /dev/mmcblk1p1 {options}\n"
                ),
            )
            .put_json(
                MEDIA_MOUNT,
                &json!({"boot_id": "boot-a", "mount_id": 51, "device_id": "179:97",
                        "device": "/dev/mmcblk1p1", "read_only": "protected"}),
            )
            .put_json(
                SD_WRITE,
                &json!({"schema": 1, "boot_id": "boot-a", "mount_id": 51, "holders": holders}),
            );
        fixture
    }

    #[test]
    fn an_expired_write_window_asks_for_a_reconcile() {
        let now = NOW;
        assert!(window_expired(
            &protected("rw", json!({"reborn": now - 1.0})).root
        ));
        assert!(window_expired(&protected("rw", json!({})).root));
        // A live window, or a card already read-only: nothing to do.
        assert!(!window_expired(
            &protected("rw", json!({"usb": now + 60.0})).root
        ));
        assert!(!window_expired(
            &protected("ro", json!({"reborn": now - 1.0})).root
        ));
        // A record the reconcile fails on never restarts it every tick.
        assert!(!window_expired(&protected("rw", json!(["reborn"])).root));
    }
}
