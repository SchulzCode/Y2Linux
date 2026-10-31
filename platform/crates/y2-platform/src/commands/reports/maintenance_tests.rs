//! tests/test_platform_maintenance.py: the owner reset (plan, execute, resume, purge) and the private
//! state export on a fixture Y2DATA. Python patched `transfer.storage` Ready; here the volume is a
//! verified mount whose blkid answer names Y2DATA, and its generation follows the boot and the mount.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::testing::{ok, Fixture};
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;

/// The commands' answers as JSON.
fn plan(root: &Root, scope: Option<&str>) -> Result<Value, Failure> {
    super::plan(root, scope).map(|p| json!(p))
}

fn execute(
    root: &Root,
    confirm: Option<&str>,
    erase: bool,
    resume: bool,
) -> Result<Value, Failure> {
    super::execute(root, confirm, erase, resume).map(|r| json!(r))
}

fn purge(root: &Root, token: &str, confirm: Option<&str>) -> Result<Value, Failure> {
    super::purge(root, token, confirm).map(|p| json!(p))
}

const MOUNTS: &str = "22 1 179:2 / / ro - ext4 /dev/mmcblk0p2 ro\n\
    30 22 ../../devices/platform/11230000.mmc/mmcblk0p9 / /data rw,noatime - ext4 /dev/mmcblk0p9 rw\n";

/// setUp: Y2DATA with music, an SSH key, factory data and Reborn's saved session; the commands
/// answered as by a schema 2 Reborn.
fn fixture(session: &Value) -> Fixture {
    fixture_for(session, 2)
}

/// The same with a Reborn whose defaults are of session schema `schema`.
fn fixture_for(session: &Value, schema: i64) -> Fixture {
    let fixture = Fixture::new().with_answers(answers(schema));
    fixture.put("/data/.y2data-schema", "1");
    fixture.mkdir("/data/updates");
    fs::set_permissions(
        fixture.path("/data/updates"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    fixture.put("/proc/self/mountinfo", MOUNTS);
    fixture.put("/sys/dev/block/.present", "");
    fixture.put(
        "/sys/devices/platform/11230000.mmc/mmcblk0p9/dev",
        "179:9\n",
    );
    for name in [
        "/data/reborn/state",
        "/data/music",
        "/data/network",
        "/data/ssh/host-keys",
        "/data/factory",
    ] {
        fixture.mkdir(name);
    }
    fixture.put("/proc/sys/kernel/random/boot_id", "boot");
    fixture.put("/data/music/song.flac", "music");
    fixture.put("/data/ssh/host-keys/private", "test-only-secret");
    fixture.put("/data/factory/preserve", "factory-fixture");
    fixture.put_json(&format!("/data/{SETTINGS}"), session);
    fixture
}

fn session() -> Value {
    json!({"schema": 2, "model": {"settings": {"volume": 99}, "queue": [{"id": 3}], "position_ms": 42}})
}

/// The volume's blkid answer and Reborn's default settings (`ctx.runner` in Python).
fn answers(schema: i64) -> Vec<(Vec<String>, y2p_core::command::Outcome)> {
    vec![
        ok(
            &["blkid", "-p", "-s", "UUID", "-o", "value", "/dev/mmcblk0p9"],
            "79324c69-6e75-4801-8000-000000000102",
        ),
        ok(
            &["/usr/bin/reborn", "--default-settings"],
            &json!({"session_schema": schema, "settings": {"volume": 35}}).to_string(),
        ),
    ]
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap()
}

fn targets(plan: &Value) -> Vec<String> {
    let Some(Value::Array(items)) = plan.get("targets") else {
        panic!("no targets");
    };
    items.iter().map(|t| text(t, "path").to_owned()).collect()
}

#[test]
fn settings_reset_preserves_queue_database_music_and_bounded_purge() {
    let original = session();
    let fixture = fixture(&original);
    let root = &fixture.root;
    let plan = plan(root, Some("settings")).unwrap();
    let error = execute(root, Some("wrong"), false, false).unwrap_err();
    assert!(error.message().contains("confirmation"));
    let confirmation = text(&plan, "confirmation");
    execute(root, Some(confirmation), false, false).unwrap();
    let saved = fixture.json(&format!("/data/{SETTINGS}"));
    assert_eq!(saved["model"]["settings"], json!({"volume": 35}));
    assert_eq!(saved["model"]["queue"], original["model"]["queue"]);
    assert_eq!(saved["model"]["position_ms"], json!(42));
    assert_eq!(fixture.read("/data/music/song.flac"), "music");
    let purged = purge(root, text(&plan, "id"), Some(confirmation)).unwrap();
    assert_eq!(text(&purged, "state"), "Complete");
    assert!(!fixture.exists("/data/system/platform/maintenance-pending"));
}

#[test]
fn a_plan_whose_expiry_has_seventeen_digits_reads_back_as_signed() {
    // The confirmation is a hash over the plan as read back from /data. serde_json without
    // float_roundtrip read about 1 in 450 expiries one ULP off (this clock gives 1300.0000000109999)
    // and the reset refused as reset_plan_changed (2026-10-31: the gate failed 4 runs in 6).
    let clock = std::sync::Arc::new(y2p_core::clock::Manual::at(
        1_000_000_000_011,
        1_000_000_000_011,
    ));
    let fixture = fixture(&session()).with_clock(clock);
    let root = &fixture.root;
    let plan = plan(root, Some("settings")).unwrap();
    let result = execute(root, Some(text(&plan, "confirmation")), false, false).unwrap();
    assert_eq!(text(&result, "state"), "Complete");
}

#[test]
fn settings_reset_keeps_a_schema_3_queue_of_song_references() {
    // Reborn Beta 8.33: songs by reference, your queue, history; kept as read.
    let original = json!({"schema": 3, "model": {"settings": {"volume": 99},
        "queue": [{"ref": ["internal", "/data/music/song.flac", 3]}], "queue_queued": [1],
        "history": [{"source_id": "internal", "path": "/data/music/song.flac"}], "position_ms": 42}});
    let fixture = fixture_for(&original, 3);
    let root = &fixture.root;
    let plan = plan(root, Some("settings")).unwrap();
    execute(root, Some(text(&plan, "confirmation")), false, false).unwrap();
    let saved = fixture.json(&format!("/data/{SETTINGS}"));
    assert_eq!(saved["schema"], json!(3));
    assert_eq!(saved["model"]["settings"], json!({"volume": 35}));
    for key in ["queue", "queue_queued", "history", "position_ms"] {
        assert_eq!(saved["model"][key], original["model"][key], "{key}");
    }
}

#[test]
fn settings_reset_forgets_the_lastfm_sign_in_and_its_waiting_plays() {
    let original = session();
    let fixture = fixture(&original);
    let root = &fixture.root;
    for name in LASTFM {
        fixture.put(
            &format!("/data/{name}"),
            "{\"name\":\"luca\",\"key\":\"k\"}\n",
        );
    }
    let plan = plan(root, Some("settings")).unwrap();
    let mut listed = targets(&plan);
    listed.sort();
    let mut expected = vec![
        SETTINGS.to_owned(),
        LASTFM[0].to_owned(),
        LASTFM[1].to_owned(),
    ];
    expected.sort();
    assert_eq!(listed, expected);
    let confirmation = text(&plan, "confirmation");
    let result = execute(root, Some(confirmation), false, false).unwrap();
    assert_eq!(text(&result, "state"), "Complete");
    for name in LASTFM {
        assert!(!fixture.exists(&format!("/data/{name}")), "{name}");
    }
    // The session file keeps its queue and only gets default settings.
    let saved = fixture.json(&format!("/data/{SETTINGS}"));
    assert_eq!(saved["model"]["settings"], json!({"volume": 35}));
    assert_eq!(saved["model"]["queue"], original["model"]["queue"]);
    // Without an account there is nothing extra to plan.
    purge(root, text(&plan, "id"), Some(confirmation)).unwrap();
    assert_eq!(targets(&plan_settings(root)), vec![SETTINGS.to_owned()]);
}

fn plan_settings(root: &Root) -> Value {
    plan(root, Some("settings")).unwrap()
}

#[test]
fn process_source_and_mount_guards_and_full_reset_explicitness() {
    let fixture = fixture(&session());
    let root = &fixture.root;
    fixture.put("/proc/999/cmdline", "/usr/bin/reborn\0");
    assert_eq!(text(&plan_settings(root), "state"), "NeedsQuiescence");
    fs::remove_dir_all(fixture.path("/proc/999")).unwrap();
    let plan = plan(root, Some("full-user")).unwrap();
    let confirmation = text(&plan, "confirmation");
    let error = execute(root, Some(confirmation), false, false).unwrap_err();
    assert!(error.message().contains("erase_user_music"));
    // Another mount of Y2DATA than the one planned.
    fixture.put(
        "/proc/self/mountinfo",
        &MOUNTS.replace("\n30 22", "\n31 22"),
    );
    let error = execute(root, Some(confirmation), true, false).unwrap_err();
    assert!(error.message().contains("generation"));
    fixture.put("/proc/self/mountinfo", MOUNTS);
    let result = execute(root, Some(confirmation), true, false).unwrap();
    assert!(!fixture.exists("/data/music"));
    assert_eq!(fixture.read("/data/factory/preserve"), "factory-fixture");
    assert!(fixture.exists("/data/ssh/host-keys/private"));
    assert_eq!(text(&result, "state"), "Complete");
}

#[test]
fn interrupted_reset_resumes_after_new_boot_without_overwriting_new_files() {
    let fixture = fixture(&session());
    let root = &fixture.root;
    for suffix in ["", "-wal", "-shm"] {
        fixture.put(&format!("/data/reborn/library.db{suffix}"), "test");
    }
    let plan = plan(root, Some("library")).unwrap();
    let confirmation = text(&plan, "confirmation");
    // Power lost after the first target's durable rename, before the record named it: the pending
    // flag and the quarantine holding 000 are what the interrupted execute left.
    let quarantine = fixture.path(&format!("/data/reset-quarantine/{}", text(&plan, "id")));
    fs::create_dir_all(&quarantine).unwrap();
    for directory in [quarantine.parent().unwrap(), &quarantine] {
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).unwrap();
    }
    fs::rename(
        fixture.path("/data/reborn/library.db"),
        quarantine.join("000"),
    )
    .unwrap();
    fixture.put_mode(
        "/data/system/platform/maintenance-pending",
        "Owner maintenance pending; keep mutable consumers stopped\n",
        0o600,
    );
    let refused = super::plan(root, Some("settings")).unwrap_err();
    assert_eq!(
        refused.message(),
        "resume_existing_reset_before_planning_another"
    );
    // The next boot (a new Y2DATA generation).
    fixture.put("/proc/sys/kernel/random/boot_id", "next");
    let result = execute(root, Some(confirmation), false, true).unwrap();
    assert_eq!(text(&result, "state"), "Complete");
    assert!(!fixture.exists("/data/system/platform/maintenance-pending"));
    let Some(Value::Array(completed)) = result.get("completed") else {
        panic!("no completed list");
    };
    assert_eq!(completed.len(), 3);
}

#[test]
fn private_export_uses_online_sqlite_backup_and_excludes_bonds_keys() {
    use y2p_core::reports_sqlite::{Database, Value as Sql};
    let fixture = fixture(&session());
    let root = &fixture.root;
    let database = fixture.path("/data/reborn/library.db");
    // Open in WAL mode while the export runs.
    let db = Database::open(&database.to_string_lossy(), 1000).unwrap();
    for sql in [
        "PRAGMA journal_mode=WAL",
        "CREATE TABLE test(value)",
        "INSERT INTO test VALUES(42)",
    ] {
        db.query(sql).unwrap();
    }
    fixture.put("/data/network/wpa_supplicant.conf", "secret test fixture");
    let names = |result: &Value| -> (Vec<String>, tar::Archive<flate2::read::GzDecoder<fs::File>>) {
        let path = fixture.path(text(result, "path"));
        let open =
            || tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(&path).unwrap()));
        let mut names: Vec<String> = open()
            .entries()
            .unwrap()
            .map(|e| e.unwrap().path().unwrap().to_string_lossy().into_owned())
            .collect();
        names.sort();
        (names, open())
    };
    let result = export(root, true, false).unwrap();
    assert_eq!(
        result.get("contains_network_credentials"),
        Some(&Value::Bool(false))
    );
    assert_eq!(fixture.mode(text(&result, "path")), 0o600);
    let (listed, mut archive) = names(&result);
    assert_eq!(
        listed,
        ["library.db", "manifest.json", "reborn--state--session.json"]
    );
    let restored = fixture.base().join("restored.db");
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        if entry.path().unwrap().to_string_lossy() == "library.db" {
            entry.unpack(&restored).unwrap();
        }
    }
    let check = Database::open(&restored.to_string_lossy(), 1000).unwrap();
    assert_eq!(
        check.query("SELECT value FROM test").unwrap(),
        vec![vec![Sql::Integer(42)]]
    );
    let result = export(root, false, true).unwrap();
    assert_eq!(
        result.get("contains_network_credentials"),
        Some(&Value::Bool(true))
    );
    assert!(names(&result)
        .0
        .contains(&"network--wpa_supplicant.conf".to_owned()));
    drop(db);
}

#[test]
fn symlink_never_escapes_purge_and_source_reset_refuses_symlink() {
    let fixture = fixture(&session());
    let root = &fixture.root;
    let outside = fixture.base().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("keep"), "keep").unwrap();
    std::os::unix::fs::symlink(&outside, fixture.path("/data/music/link")).unwrap();
    let plan = plan(root, Some("full-user")).unwrap();
    let confirmation = text(&plan, "confirmation");
    execute(root, Some(confirmation), true, false).unwrap();
    while text(
        &purge(root, text(&plan, "id"), Some(confirmation)).unwrap(),
        "state",
    ) != "Complete"
    {}
    assert!(outside.join("keep").exists());
    std::os::unix::fs::symlink(&outside, fixture.path("/data/reborn")).unwrap();
    assert!(matches!(plan_full(root), Err(Failure::Value(_))));
}

fn plan_full(root: &Root) -> Result<Value, Failure> {
    plan(root, Some("full-user"))
}
