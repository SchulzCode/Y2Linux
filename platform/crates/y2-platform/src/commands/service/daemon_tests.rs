//! service.py's decisions that the Python tests checked with patched children (tests/
//! test_platform_update.py, test_radio_firmware.py, test_cpu_fix02.py, test_cpu_fix03.py): when the
//! service starts its background jobs, keeps a running one, stops a hung one, and how the media
//! inventory and the dark cadence decide. Below a fixture root no child starts (process.rs); a running
//! child is a real `sleep` started on the host, and which children a tick starts is checked at the
//! command line (tests/service.rs).
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::testing::{repository, Fixture};
use std::path::Path;

/// A child that keeps running (the patched Popen whose poll() answers None).
fn running() -> Child {
    process::spawn(&Root::new("/"), &["/bin/sleep", "30"], Start::Service).unwrap()
}

fn alive(pid: i32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

/// Same boot as `boot-a`, Python's `ctx.read(boot_id)`.
fn fixture() -> Fixture {
    let fixture = Fixture::new();
    fixture.put("/proc/sys/kernel/random/boot_id", "boot-a\n");
    fixture
}

#[test]
fn media_reconcile_runs_in_the_background_and_stops_a_hung_check() {
    let fixture = fixture();
    // Not needed (the lifecycle names this inventory): nothing to start.
    fixture.put_json(
        "/run/y2/media-lifecycle.json",
        &json!({"schema": 1, "inventory": inventory(&fixture.root), "retry": false}),
    );
    assert!(!media_reconcile_needed(&fixture.root));
    let idle = media_reconcile(&fixture.root, Run::default(), 100.0);
    assert!(idle.process.is_none() && idle.retry_at == 0.0);
    fixture.remove("/run/y2/media-lifecycle.json");
    assert!(media_reconcile_needed(&fixture.root));
    // Started at 100 (the start itself: tests/service.rs).
    let child = running();
    let pid = child.pid();
    let run = Run {
        process: Some((child, 100.0)),
        retry_at: 0.0,
    };
    // A long card check is left alone (it used to be killed after 7 s) ...
    let run = media_reconcile(&fixture.root, run, 100.0 + 60.0);
    assert_eq!(
        run.process.as_ref().map(|(c, s)| (c.pid(), *s)),
        Some((pid, 100.0))
    );
    // ... a hung one is stopped with everything it started, then backs off.
    let stopped = media_reconcile(&fixture.root, run, 100.0 + RECONCILE_LIMIT_S);
    assert!(stopped.process.is_none());
    assert_eq!(
        stopped.retry_at,
        100.0 + RECONCILE_LIMIT_S + RECONCILE_BACKOFF_S
    );
    assert!(!alive(pid));
    let retry_at = stopped.retry_at;
    let waiting = media_reconcile(&fixture.root, stopped, 100.0 + RECONCILE_LIMIT_S + 1.0);
    assert!(waiting.process.is_none());
    assert_eq!(waiting.retry_at, retry_at);
    // At retry_at it starts again (refused below a fixture root: no back-off left).
    let again = media_reconcile(&fixture.root, waiting, retry_at);
    assert!(again.process.is_none() && again.retry_at == 0.0);
}

#[test]
fn media_verify_runs_one_background_check_while_a_card_is_checking() {
    let fixture = fixture();
    assert!(!media_verify_needed(&fixture.root));
    let idle = media_verify(&fixture.root, Run::default(), 100.0);
    assert!(idle.process.is_none() && idle.retry_at == 0.0);
    // A long check is never stopped by the loop (media.CHECK_SECONDS bounds it).
    let child = running();
    let pid = child.pid();
    let run = Run {
        process: Some((child, 100.0)),
        retry_at: 0.0,
    };
    let mut run = media_verify(&fixture.root, run, 100.0 + 3600.0);
    assert_eq!(run.process.as_ref().map(|(c, _)| c.pid()), Some(pid));
    assert!(alive(pid));
    // Ended without a verdict: again only after VERIFY_RETRY_S.
    let (child, _) = run.process.as_mut().unwrap();
    child.kill();
    assert!(child.wait(2.0));
    let ended = media_verify(&fixture.root, run, 4000.0);
    assert!(ended.process.is_none());
    assert_eq!(ended.retry_at, 4000.0 + VERIFY_RETRY_S);
    let waiting = media_verify(&fixture.root, ended, 4001.0);
    assert_eq!(waiting.retry_at, 4000.0 + VERIFY_RETRY_S);
    // A card mounted while its check ran: needed for this boot only.
    fixture.put_json(
        "/run/y2/media-mount.json",
        &json!({"read_only": "checking", "boot_id": "boot-a"}),
    );
    assert!(media_verify_needed(&fixture.root));
    // At retry_at the check starts again (refused below a fixture root: the next try waits again).
    let again = media_verify(&fixture.root, waiting, 4000.0 + VERIFY_RETRY_S);
    assert_eq!(again.retry_at, 4000.0 + 2.0 * VERIFY_RETRY_S);
    fixture.put_json(
        "/run/y2/media-mount.json",
        &json!({"read_only": "checking", "boot_id": "boot-z"}),
    );
    assert!(!media_verify_needed(&fixture.root));
    fixture.put_json(
        "/run/y2/media-mount.json",
        &json!({"read_only": "protected", "boot_id": "boot-a"}),
    );
    assert!(!media_verify_needed(&fixture.root));
}

#[test]
fn service_spawns_the_check_only_when_due() {
    // Which states start the check: tests/service.rs (the started argv).
    let fixture = fixture();
    let network = |state: &str| Object::new().with("state", state);
    assert!(daily_update_check(&fixture.root, &network("Scanning"), None).is_none());
    // A running check is kept, whatever the network does.
    let child = running();
    let pid = child.pid();
    let kept = daily_update_check(&fixture.root, &network("Online"), Some(child));
    assert_eq!(kept.as_ref().map(Child::pid), Some(pid));
    let kept = daily_update_check(&fixture.root, &network("Scanning"), kept);
    let mut kept = kept.expect("still running");
    kept.kill();
    assert!(kept.wait(2.0));
    // An ended one is not kept.
    assert!(daily_update_check(&fixture.root, &network("Scanning"), Some(kept)).is_none());
}

/// Every reviewed radio firmware file with its size (present() looks at sizes only).
fn firmware_present(fixture: &Fixture) {
    for (name, size, _) in radiofw::FILES.iter().chain(radiofw::DEFAULTS.iter()) {
        fixture.put(
            &format!("/data/firmware/mt6582/{name}"),
            &"x".repeat(*size as usize),
        );
    }
}

#[test]
fn removed_firmware_is_noticed_at_the_next_maintenance_pass() {
    // service.radio_firmware notices firmware removed while running.
    let fixture = fixture();
    assert!(!radiofw::present(&fixture.root));
    let (child, ready) = radio_firmware(&fixture.root, None, true, false);
    assert!(child.is_none() && ready); // between passes: no work
    let (child, ready) = radio_firmware(&fixture.root, None, true, true);
    assert!(child.is_none() && !ready); // maintenance: re-checked (the scan: tests/service.rs)
    firmware_present(&fixture);
    assert!(radiofw::present(&fixture.root));
    let (child, ready) = radio_firmware(&fixture.root, None, false, false);
    assert!(child.is_none() && ready);
    // Firmware in place and a copy waiting: the maintenance pass scans to delete it.
    fixture.put("/media/sd/Firmware/system.img", "x");
    assert!(radiofw::leftover_waiting(&fixture.root));
    let (child, ready) = radio_firmware(&fixture.root, None, true, false);
    assert!(child.is_none() && ready);
    assert!(!radio_firmware(&fixture.root, None, true, true).1);
    // A running scan is kept until it ends.
    let child = running();
    let pid = child.pid();
    let (child, ready) = radio_firmware(&fixture.root, Some(child), true, true);
    assert!(!ready);
    let mut child = child.expect("still running");
    assert_eq!(child.pid(), pid);
    child.kill();
    assert!(child.wait(2.0));
}

/// tests/test_cpu_fix02.py ScreenOffBackground.card: the SD card's class link below its controller.
fn card(fixture: &Fixture, present: bool) {
    let target = "/sys/devices/platform/11240000.mmc/mmc_host/mmc1/mmc1:0001/block/mmcblk1";
    fixture.mkdir(target);
    fixture.mkdir("/sys/class/block");
    let link = fixture.path("/sys/class/block/mmcblk1");
    if present && !link.is_symlink() {
        std::os::unix::fs::symlink(fixture.path(target), &link).unwrap();
    }
    if !present && link.is_symlink() {
        std::fs::remove_file(&link).unwrap();
    }
}

fn lifecycle(fixture: &Fixture, value: Value) {
    fixture.put_json("/run/y2/media-lifecycle.json", &value);
}

#[test]
fn media_reconcile_spawns_only_when_its_own_noop_is_false() {
    let fixture = fixture();
    assert!(media_reconcile_needed(&fixture.root)); // first run
    card(&fixture, true);
    let current = inventory(&fixture.root);
    assert_eq!(current.as_array().unwrap().len(), 1);
    lifecycle(
        &fixture,
        json!({"schema": 1, "inventory": current, "retry": false}),
    );
    assert!(!media_reconcile_needed(&fixture.root));
    lifecycle(
        &fixture,
        json!({"schema": 1, "inventory": current, "retry": true}),
    );
    assert!(media_reconcile_needed(&fixture.root)); // bounded retry kept
    lifecycle(
        &fixture,
        json!({"schema": 1, "inventory": current, "retry": false}),
    );
    card(&fixture, false);
    assert!(media_reconcile_needed(&fixture.root)); // removal
    lifecycle(
        &fixture,
        json!({"schema": 1, "inventory": [], "retry": false}),
    );
    assert!(!media_reconcile_needed(&fixture.root));
    card(&fixture, true);
    assert!(media_reconcile_needed(&fixture.root)); // insertion
}

#[test]
fn dark_cadence_keeps_network_fresh_and_health_deadline_safe() {
    let fixture = fixture();
    assert!(!display_dark(&fixture.root)); // unknown is not dark
    fixture.put("/sys/class/backlight/y2/brightness", "7\n");
    fixture.put("/sys/class/backlight/y2/bl_power", "0\n");
    assert!(!display_dark(&fixture.root));
    // Reborn ScreenSleep: FB_BLANK_POWERDOWN, brightness kept.
    fixture.put("/sys/class/backlight/y2/bl_power", "4\n");
    assert!(display_dark(&fixture.root));
    fixture.put("/sys/class/backlight/y2/bl_power", "0\n");
    fixture.put("/sys/class/backlight/y2/brightness", "0\n");
    assert!(display_dark(&fixture.root));
    let reborn = repository().join("../Y2Reborn/crates/reborn-platform/src/power.rs");
    if let Ok(text) = std::fs::read_to_string(reborn) {
        assert!(text.contains("bl_power"));
    }
    assert_eq!(cadence(false, true), (3.0, 30.0));
    assert_eq!(cadence(true, false), (10.0, 30.0)); // pending health keeps 30 s
    assert_eq!(cadence(true, true), (10.0, 300.0));
    const { assert!(DARK_INTERVAL_S + 3.0 < 15.0) }; // Reborn stale limit
    let answer = |ok: bool, output: &str| command::Outcome {
        ok,
        reason: None,
        output: Some(output.into()),
    };
    assert!(!health_settled(&answer(false, r#"{"state":"Idle"}"#)));
    assert!(!health_settled(&answer(
        true,
        r#"{"state":"PendingHealth"}"#
    )));
    assert!(!health_settled(&answer(true, "garbage")));
    assert!(health_settled(&answer(true, r#"{"state":"Idle"}"#)));
}

#[test]
fn service_loop_uses_gated_reconcile_and_cadence() {
    let source = include_str!("daemon.rs");
    let loop_ = &source[source.find("fn serve(&mut self").unwrap()..];
    // B5: reconcile runs in its own process, still started only when needed.
    assert!(loop_.contains("reconcile = media_reconcile(root, reconcile, before);"));
    let helper = &source[source.find("fn media_reconcile(").unwrap()
        ..source.find("fn media_verify_needed(").unwrap()];
    assert!(helper.contains("!media_reconcile_needed(root)"));
    assert!(loop_.contains("next_maintenance = before + maintenance;"));
    assert!(loop_.contains("interval - (clock::monotonic() - before)"));
}

/// tests/test_cpu_fix03.py SdInventoryIdentity.card: a card and its partition, links made anew.
fn sd_card(fixture: &Fixture, diskseq: &str, cid: &str) {
    let disk = "/sys/devices/platform/11240000.mmc/mmc_host/mmc1/mmc1:0001/block/mmcblk1";
    let part = format!("{disk}/mmcblk1p1");
    fixture.put(&format!("{disk}/dev"), "179:24\n");
    fixture.put(&format!("{disk}/diskseq"), &format!("{diskseq}\n"));
    fixture.put(&format!("{disk}/device/cid"), &format!("{cid}\n"));
    fixture.put(&format!("{part}/dev"), "179:25\n");
    fixture.put(&format!("{part}/partition"), "1\n");
    fixture.mkdir("/sys/class/block");
    for (name, target) in [("mmcblk1", disk.to_string()), ("mmcblk1p1", part)] {
        let link = fixture.path(&format!("/sys/class/block/{name}"));
        if link.is_symlink() {
            std::fs::remove_file(&link).unwrap();
        }
        std::os::unix::fs::symlink(fixture.path(&target), &link).unwrap();
    }
}

#[test]
fn kernfs_reclaim_is_not_a_media_change() {
    let fixture = fixture();
    let cid = "0353445331364780";
    sd_card(&fixture, "17", cid);
    let before = inventory(&fixture.root);
    assert_eq!(
        before,
        json!([
            ["mmcblk1", "179:24", "17", cid],
            ["mmcblk1p1", "179:25", "17", cid]
        ])
    );
    // drop_caches: kernfs recreates the class links (new inodes and ctimes).
    sd_card(&fixture, "17", cid);
    assert_eq!(inventory(&fixture.root), before);
    lifecycle(
        &fixture,
        json!({"schema": 1, "inventory": before, "retry": false}),
    );
    assert!(!media_reconcile_needed(&fixture.root));
    // Re-enumeration of the identical card allocates a new diskseq.
    sd_card(&fixture, "18", cid);
    assert!(media_reconcile_needed(&fixture.root));
}

#[test]
fn source_no_longer_uses_inode_identity() {
    // The inventory (storage/media.rs) never compares kernfs inodes or ctimes.
    let media = include_str!("../storage/media.rs");
    let start = media.find("pub(crate) fn inventory(").unwrap();
    let inventory = &media[start..start + media[start..].find("\n}\n").unwrap()];
    for needle in ["st_ino", "st_ctime", ".ino()", ".ctime()"] {
        assert!(!inventory.contains(needle), "{needle}");
    }
    for needle in ["st_ino", "st_ctime"] {
        assert!(!media.contains(needle), "{needle}");
    }
}
