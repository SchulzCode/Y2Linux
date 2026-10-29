//! tests/test_platform_update.py, the platform's part: `update.py` decisions (check, download,
//! cancel, space, health, battery, channel, automatic check) on a fixture root. The native core
//! (tools/update/core.c, its own Python tests) answers as a command of the fixture's root; `Fake`
//! stands in for the clock's readiness, the wall clock, the staging token, the free space and the
//! release server.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::testing::{ok, Fixture};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Mutex,
};
use y2p_core::command::Outcome;

/// `stage` keeps process-wide state (CANCELLED, the SIGTERM disposition): one at a time.
static STAGE: Mutex<()> = Mutex::new(());
const TOKEN: &str = "0123456789abcdef0123456789abcdef";
const CORE: &str = "/usr/sbin/y2-update-core";
const URL: &str = "https://github.com/o/r/releases/latest/download/manifest.json";

fn serial() -> std::sync::MutexGuard<'static, ()> {
    STAGE.lock().unwrap_or_else(|e| e.into_inner())
}

/// The answer for one release URL (`base` + file name).
type Remote = Box<dyn FnMut(&str) -> Result<Stream>>;

/// The test's clocks, release server and volume: the fixed token, no waits for Wi-Fi, and what a
/// test sets (the player's own for the rest).
#[derive(Default)]
struct Fake {
    time: Option<Map<String, Value>>,
    now: Cell<Option<f64>>,
    /// The bytes available (writable, no inode limit).
    space: Option<Box<dyn Fn() -> i128>>,
    remote: RefCell<Option<Remote>>,
}

impl Fake {
    /// `patch('y2_platform.timekeeping.status', return_value=…)`.
    fn clock(&mut self, tls: bool, entropy: bool) {
        let value = json!({"tls_ready": tls, "entropy_ready": entropy});
        self.time = value.as_object().cloned();
    }

    fn available(&mut self, available: impl Fn() -> i128 + 'static) {
        self.space = Some(Box::new(available));
    }

    fn remote(&mut self, answer: impl FnMut(&str) -> Result<Stream> + 'static) {
        *self.remote.get_mut() = Some(Box::new(answer));
    }
}

impl Io for Fake {
    fn time_status(&self, root: &Root) -> Map<String, Value> {
        self.time
            .clone()
            .unwrap_or_else(|| Player.time_status(root))
    }

    fn now(&self) -> f64 {
        self.now.get().unwrap_or_else(|| Player.now())
    }

    fn fetch(&self, url: &str, hosts: &[String], cancellable: bool) -> Result<Stream> {
        match self.remote.borrow_mut().as_mut() {
            Some(answer) => answer(url),
            None => Player.fetch(url, hosts, cancellable),
        }
    }

    fn space(&self, file: &File) -> Result<Space> {
        match &self.space {
            Some(available) => Ok(Space {
                readonly: false,
                available: available(),
                files: 0,
                favail: 0,
            }),
            None => Player.space(file),
        }
    }

    fn token(&self, _: &Root) -> Result<String> {
        Ok(format!(".incoming-{TOKEN}"))
    }

    fn network_wait(&self) -> f64 {
        0.0
    }

    fn retry_pause(&self) -> Duration {
        Duration::ZERO
    }
}

/// `f` on the fixture's root with these command answers: its result and the requests it made.
fn answering<T>(
    fixture: &Fixture,
    answers: Vec<(Vec<String>, Outcome)>,
    f: impl FnOnce(&Root) -> T,
) -> (T, Vec<(Vec<String>, f64)>) {
    let root = fixture.root.clone().with_answers(answers);
    let value = f(&root);
    (value, root.calls())
}

/// PlatformPolicyTests.setUp: the update directories, this boot and the shipped channel.
fn policy() -> Fixture {
    let fixture = Fixture::new();
    for name in ["/etc/y2linux", "/run/y2"] {
        fixture.mkdir(name);
    }
    fixture.mkdir("/data/updates");
    fs::set_permissions(
        fixture.path("/data/updates"),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    fixture.put("/proc/sys/kernel/random/boot_id", "boot-a\n");
    fixture.put("/data/.y2data-schema", "1");
    fixture.put_json(
        CHANNEL,
        &json!({"schema": 1, "name": "stable", "manifest_url": URL,
                "allowed_redirect_hosts": ["github.com", "release-assets.githubusercontent.com"]}),
    );
    fixture
}

/// A verified Y2DATA mount (`transfer.storage` patched Ready in Python): the device link resolves below
/// the internal controller, blkid names the Y2DATA filesystem (`BLKID`).
fn volume(fixture: &Fixture) {
    fixture.put(
        "/proc/self/mountinfo",
        "22 1 179:2 / / ro - ext4 /dev/mmcblk0p2 ro\n\
         30 22 ../../devices/platform/11230000.mmc/mmcblk0p9 / /data rw,noatime - ext4 /dev/mmcblk0p9 rw\n",
    );
    fixture.put("/sys/dev/block/.present", "");
    fixture.put(
        "/sys/devices/platform/11230000.mmc/mmcblk0p9/dev",
        "179:9\n",
    );
}

fn blkid() -> (Vec<String>, Outcome) {
    ok(
        &["blkid", "-p", "-s", "UUID", "-o", "value", "/dev/mmcblk0p9"],
        "79324c69-6e75-4801-8000-000000000102",
    )
}

fn incoming() -> String {
    format!("/data/updates/.incoming-{TOKEN}")
}

/// The core's answer to one action (its JSON on stdout, or its refusal).
fn core_answer(
    action: &str,
    directory: Option<&str>,
    ok: bool,
    output: &str,
) -> (Vec<String>, Outcome) {
    let mut argv = vec![CORE.to_string(), action.to_string()];
    argv.extend(directory.map(str::to_owned));
    let outcome = Outcome {
        ok,
        reason: (!ok).then(|| "command_failed".into()),
        output: Some(output.into()),
    };
    (argv, outcome)
}

/// A local package (as `build` writes one; the signature is the core's to check) and the manifest
/// the core's `check-manifest` reads from it, in the core's payload order.
struct Package {
    directory: PathBuf,
    manifest: String,
}

fn package(fixture: &Fixture, release: &str, raw_bytes: i64) -> Package {
    let directory = fixture.base().join("package");
    fs::create_dir_all(&directory).unwrap();
    let mut payloads = Vec::new();
    for (key, name, size) in [
        ("rootfs", "rootfs.ext4.gz", 3000usize),
        ("bootimg", "bootimg.img.gz", 1500),
        ("recovery", "recovery.img.gz", 1200),
    ] {
        let data: Vec<u8> = (0..size).map(|i| (i * 7 + key.len()) as u8).collect();
        fs::write(directory.join(name), &data).unwrap();
        let extra = if key == "rootfs" {
            format!(", \"raw_bytes\": {raw_bytes}")
        } else {
            String::new()
        };
        payloads.push(format!(
            "\"{key}\": {{\"bytes\": {}, \"sha256\": \"{}\"{extra}}}",
            data.len(),
            y2p_core::service_digest::sha256_hex(&data)
        ));
    }
    let manifest = format!(
        "{{\"release_version\": \"{release}\", \"sequence\": 1, \"notes\": \"\", \"payloads\": {{{}}}}}",
        payloads.join(", ")
    );
    fs::write(directory.join("manifest.json"), &manifest).unwrap();
    fs::write(directory.join("manifest.sig"), [b'x'; 64]).unwrap();
    Package {
        directory,
        manifest,
    }
}

impl Package {
    fn path(&self) -> String {
        self.directory.to_string_lossy().into_owned()
    }

    fn check(&self) -> (Vec<String>, Outcome) {
        core_answer("check-manifest", Some(&incoming()), true, &self.manifest)
    }

    fn bytes(&self) -> i64 {
        let manifest: Value = serde_json::from_str(&self.manifest).unwrap();
        manifest["payloads"]
            .as_object()
            .unwrap()
            .values()
            .map(|p| p["bytes"].as_i64().unwrap())
            .sum()
    }
}

/// `shutil.copy` of a package file to a device path.
fn copy(fixture: &Fixture, from: &Path, name: &str) {
    let to = fixture.path(name);
    fs::create_dir_all(to.parent().unwrap()).unwrap();
    fs::copy(from, to).unwrap();
}

fn queued() -> (Vec<String>, Outcome) {
    core_answer(
        "queue",
        None,
        true,
        r#"{"state": "Queued", "release_version": "test-v1"}"#,
    )
}

fn argvs(calls: &[(Vec<String>, f64)]) -> Vec<Vec<String>> {
    calls.iter().map(|(argv, _)| argv.clone()).collect()
}

fn asked(calls: &[(Vec<String>, f64)], action: &str) -> bool {
    calls
        .iter()
        .any(|(argv, _)| argv.len() > 1 && argv[0] == CORE && argv[1] == action)
}

fn sigterm_handler() -> libc::sighandler_t {
    // SAFETY: reading the current action into a zeroed struct.
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    unsafe { libc::sigaction(libc::SIGTERM, std::ptr::null(), &mut action) };
    action.sa_sigaction
}

#[test]
fn download_stage_authenticates_before_payload_and_never_queues_partial() {
    let _serial = serial();
    let io = Fake::default();
    let fixture = policy();
    volume(&fixture);
    let package = package(&fixture, "test-v1", 4096);
    let source = package.path();
    let answers = || vec![blkid(), package.check(), queued()];
    // A check reads only the signed manifest: the core's verdict, no journal, no payload.
    let (checked, calls) = answering(&fixture, answers(), |root| {
        stage(root, &io, Some(&source), None, true)
    });
    let checked = checked.unwrap();
    assert_eq!(checked["sequence"], json!(1));
    assert!(!fixture.exists("/data/updates/state.json"));
    assert!(!fixture.exists(&incoming()));
    assert!(!asked(&calls, "queue"));
    assert_eq!(
        fixture.json("/data/updates/download.json")["state"],
        json!("Checked")
    );
    // The manifest the core refuses (its signature) stops before any payload is read.
    let refused = core_answer(
        "check-manifest",
        Some(&incoming()),
        false,
        r#"{"failure": "signature"}"#,
    );
    let (result, calls) = answering(&fixture, vec![blkid(), refused], |root| {
        stage(root, &io, Some(&source), None, false)
    });
    assert_eq!(result.err().unwrap().text(), "signature");
    assert!(!asked(&calls, "queue"));
    assert!(!fixture.exists(&format!("{}/rootfs.ext4.gz", incoming())));
    assert!(!fixture.exists("/data/updates/pending"));
    // A payload cut short is never queued.
    for name in ["rootfs.ext4.gz", "bootimg.img.gz"] {
        let payload = package.directory.join(name);
        let original = fs::read(&payload).unwrap();
        fs::write(&payload, &original[..original.len() - 10]).unwrap();
        let (result, calls) = answering(&fixture, answers(), |root| {
            stage(root, &io, Some(&source), None, false)
        });
        assert_eq!(result.err().unwrap().text(), "incomplete_download");
        assert_eq!(fixture.json(PROGRESS)["phase"], json!("failed"));
        assert!(!fixture.exists("/data/updates/state.json"));
        assert!(!fixture.exists("/data/updates/pending"));
        assert!(!asked(&calls, "queue"));
        fs::write(&payload, original).unwrap();
    }
    // A stale check percentage is not left for Reborn (the queue writes its own, core.c).
    fixture.put(CHECK_PROGRESS, "40 check\n");
    let (result, calls) = answering(&fixture, answers(), |root| {
        stage(root, &io, Some(&source), None, false)
    });
    assert_eq!(result.unwrap()["state"], json!("Queued"));
    assert_eq!(
        argvs(&calls).last().unwrap(),
        &vec![CORE.to_string(), "queue".to_string()]
    );
    assert!(!fixture.exists(CHECK_PROGRESS));
    assert!(fixture.exists("/data/updates/pending/rootfs.ext4.gz"));
    assert_eq!(
        fixture.json("/data/updates/download.json")["state"],
        json!("Verified")
    );
    // OTA-48: live progress for Reborn in RAM.
    let live = fixture.json(PROGRESS);
    assert_eq!(live["phase"], json!("done"));
    assert_eq!(live["bytes_done"], live["bytes_total"]);
    assert_eq!(live["bytes_total"], json!(package.bytes()));
    assert!(live["bytes_total"].as_i64().unwrap() > 0);
    // The platform reads the core's own journal (schema 2) as the queued update.
    fixture.put_json(
        "/data/updates/state.json",
        &json!({"schema": 2, "state": "Queued"}),
    );
    assert_eq!(status(&fixture.root)["state"], json!("Queued"));
    // The installed release offered again: a check says up to date, not a failure.
    fixture.put_json(
        "/etc/y2linux/versions.json",
        &json!({"release_version": "test-v1"}),
    );
    fixture.put_json(
        "/data/updates/state.json",
        &json!({"schema": 2, "state": "Acknowledged"}),
    );
    let replay = core_answer(
        "check-manifest",
        Some(&incoming()),
        false,
        r#"{"failure": "downgrade/replay refused"}"#,
    );
    let (result, _) = answering(&fixture, vec![blkid(), replay], |root| {
        stage(root, &io, Some(&source), None, true)
    });
    assert_eq!(result.unwrap()["state"], json!("UpToDate"));
    let checked = fixture.json("/data/updates/download.json");
    assert_eq!(checked["up_to_date"], json!(true));
    assert_eq!(checked["release_version"], json!("test-v1"));
}

#[test]
fn download_stage_takes_only_an_https_manifest_url() {
    // The end of test_download_stage_authenticates_before_payload_and_never_queues_partial.
    for url in [
        "http://example.com/manifest.json",
        "https://u:p@example.com/manifest.json",
        "https://example.com/manifest.json?redirect=other",
        "https://example.com/other",
    ] {
        assert!(download::base_url(url).is_err(), "{url}");
    }
    assert_eq!(
        download::base_url("https://example.com/release/manifest.json").unwrap(),
        "https://example.com/release/"
    );
}

#[test]
fn cancel_stops_a_running_download_and_offers_the_release_again() {
    // Beta 7.1: Reborn's Cancel sends SIGTERM to `update stage`. The download stops, its partial files
    // go and the page shows the release as available, exactly as after the check.
    let _serial = serial();
    let mut io = Fake::default();
    let fixture = policy();
    volume(&fixture);
    let package = package(&fixture, "test-v1", 4096);
    let source = package.path();
    let before = sigterm_handler();
    let partial = fixture.path(&format!("{}/rootfs.ext4.gz", incoming()));
    let sent = Rc::new(Cell::new(false));
    let once = sent.clone();
    // Cancel pressed mid-download (the manifest is read before): while the payload is written.
    io.available(move || {
        if !once.get() && partial.exists() && sigterm_handler() == cancel as *const () as usize {
            once.set(true);
            // SAFETY: SIGTERM to this thread while stage's handler (an atomic store) is installed.
            unsafe { libc::raise(libc::SIGTERM) };
        }
        1 << 40
    });
    let answers = || vec![blkid(), package.check(), queued()];
    let (result, calls) = answering(&fixture, answers(), |root| {
        stage(root, &io, Some(&source), None, false)
    });
    assert!(sent.get());
    let error = result.err().unwrap();
    assert!(error.cancelled());
    assert_eq!(error.text(), "download_cancelled");
    assert!(!asked(&calls, "queue"));
    assert_eq!(sigterm_handler(), before);
    assert!(fixture.root.glob("/data/updates/.incoming-*").is_empty());
    assert!(!fixture.exists("/data/updates/pending"));
    assert!(!fixture.exists("/data/updates/state.json"));
    let offered = fixture.json("/data/updates/download.json");
    assert_eq!(
        (&offered["state"], &offered["sequence"]),
        (&json!("Checked"), &json!(1))
    );
    // Nothing was changed: the same download works afterwards.
    let (result, _) = answering(&fixture, answers(), |root| {
        stage(root, &io, Some(&source), None, false)
    });
    assert_eq!(result.unwrap()["state"], json!("Queued"));
}

#[test]
fn space_cleanup_stale_package_and_cancel_free_the_data_volume() {
    // OTA-46: disposable data goes before the download is refused; music, settings, live logs and the
    // rollback copies stay.
    let _serial = serial();
    let mut io = Fake::default();
    let fixture = policy();
    volume(&fixture);
    let raw = 8 * 1024 * 1024;
    let package = package(&fixture, "test-v1", raw);
    let source = package.path();
    let kept = [
        "/data/logs/system.log",
        "/data/logs/recovery-last.log",
        "/data/system/platform/power-journal.jsonl",
        "/data/settings.json",
    ];
    for name in kept {
        fixture.put(name, "keep");
    }
    let backup = "/data/updates/previous.ext4.gz";
    fixture.put(backup, &"b".repeat(5000));
    let disposable = || {
        fixture.put("/data/logs/deepsleep-old/rows", &"x".repeat(1000));
        fixture.put("/data/logs/system.log.0", "rotated");
        fixture.put(
            "/data/system/platform/power-journal.previous.jsonl",
            "rotated",
        );
        fixture.mkdir("/data/system/platform/fix02-abc");
    };
    disposable();
    // A package left by an earlier cancelled/failed update is replaced first.
    for entry in fs::read_dir(&package.directory).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        copy(
            &fixture,
            &entry.path(),
            &format!("/data/updates/pending/{name}"),
        );
    }
    let old = fixture.path("/data/logs/deepsleep-old");
    io.available(move || if old.exists() { 0 } else { 1 << 40 });
    let (result, _) = answering(&fixture, vec![blkid(), package.check(), queued()], |root| {
        stage(root, &io, Some(&source), None, false)
    });
    assert_eq!(result.unwrap()["state"], json!("Queued"));
    // Only as much as needed: the first entry was enough.
    assert!(!fixture.exists("/data/logs/deepsleep-old"));
    assert!(fixture.exists("/data/logs/system.log.0"));
    assert!(
        fixture.json("/data/updates/download.json")["cleaned_bytes"]
            .as_i64()
            .unwrap()
            >= 1000
    );
    // Cancel frees the package at once.
    let (result, _) = answering(
        &fixture,
        vec![core_answer("cancel", None, true, r#"{"state": "Failed"}"#)],
        cancel_update,
    );
    assert_eq!(result.unwrap()["state"], json!("Failed"));
    assert!(!fixture.exists("/data/updates/pending"));
    // Still too little space: everything disposable goes, then a clear refusal that names the total
    // the update needs (the rollback copy counts as free).
    disposable();
    // The kept package of the running release (OTA-50) goes only as the last step.
    for name in ["manifest.json", "manifest.sig", "rootfs.ext4.gz"] {
        copy(
            &fixture,
            &package.directory.join(name),
            &format!("/data/updates/current/{name}"),
        );
    }
    let need = package.bytes() as i128 + raw as i128 / 4 + RESERVE - 5000;
    // Room for the manifest, not for the payloads.
    io.available(|| RESERVE + (1 << 20));
    let (result, _) = answering(&fixture, vec![blkid(), package.check(), queued()], |root| {
        stage(root, &io, Some(&source), None, false)
    });
    assert!(matches!(result, Err(Raise::NoSpace(n)) if n == need));
    let failed = fixture.json("/data/updates/download.json");
    assert_eq!(
        (&failed["failure"], &failed["needed_bytes"]),
        (&json!("update_data_space_reserve"), &json!(need as i64))
    );
    for name in [
        "/data/logs/deepsleep-old",
        "/data/logs/system.log.0",
        "/data/system/platform/power-journal.previous.jsonl",
        "/data/system/platform/fix02-abc",
        "/data/updates/current",
    ] {
        assert!(!fixture.exists(name), "{name}");
    }
    for name in kept.iter().chain([&backup]) {
        assert!(fixture.exists(name), "{name}");
    }
    assert!(fixture.root.glob("/data/updates/.incoming-*").is_empty());
}

#[test]
fn health_uses_reborns_verdict_and_ignores_unreadable_songs() {
    // B3: one song that cannot be read must never roll an update back.
    let incomplete = json!({"healthy": true, "graphics": {"available": true}, "uptime_seconds": 40,
        "library": {"error": "scan incomplete; retained existing entries (1 traversal failures)"}});
    assert!(application_healthy(&incomplete));
    let with = |changes: Value| {
        let mut value = incomplete.clone();
        for (k, v) in changes.as_object().unwrap() {
            value[k] = v.clone();
        }
        value
    };
    assert!(!application_healthy(&with(
        json!({"healthy": false, "health_reasons": ["database failed"]})
    )));
    assert!(!application_healthy(&with(json!({"healthy": "yes"}))));
    let older =
        json!({"graphics": {"available": true}, "uptime_seconds": 40, "library": {"error": "x"}});
    assert!(application_healthy(&older));
    let mut early = older.clone();
    early["uptime_seconds"] = json!(3);
    assert!(!application_healthy(&early));
    assert!(!application_healthy(&json!({})));
    assert!(!application_healthy(&Value::Null));
}

#[test]
fn a_download_whose_process_is_gone_shows_as_interrupted() {
    // A user's 8.17 player said "Downloading Update… Keep Wi-Fi on." for a day: the record stayed
    // Staging after the download died, and that page had no way out.
    let _serial = serial();
    let mut io = Fake::default();
    let fixture = policy();
    fixture.put_json(
        "/data/updates/download.json",
        &json!({"schema": 1, "state": "Staging", "id": ".incoming-x"}),
    );
    fixture.put("/data/updates/.stage.lock", "");
    assert_eq!(
        status(&fixture.root)["download"],
        json!({"schema": 1, "state": "Failed", "failure": "download_interrupted"})
    );
    // A live download (its flock in /proc/locks) is still shown as running.
    let meta = fs::metadata(fixture.path("/data/updates/.stage.lock")).unwrap();
    fixture.put(
        "/proc/locks",
        &format!(
            "1: FLOCK  ADVISORY  WRITE 4242 {:02x}:{:02x}:{} 0 EOF\n",
            libc::major(meta.dev()),
            libc::minor(meta.dev()),
            meta.ino()
        ),
    );
    assert_eq!(status(&fixture.root)["download"]["state"], json!("Staging"));
    // A failed background check never writes the leftover back.
    fixture.put("/proc/locks", "");
    fixture.put_json("/run/y2/network.json", &json!({"state": "Online"}));
    io.clock(true, true);
    io.now.set(Some(1e9));
    // The check fails (here: no verified Y2DATA; Python patched stage to fail).
    let (result, _) = answering(&fixture, vec![], |root| automatic_check(root, &io));
    assert_eq!(result.unwrap()["result"], json!("failed"));
    assert_ne!(
        fixture.json("/data/updates/download.json")["state"],
        json!("Staging")
    );
}

#[test]
fn confirmed_health_restores_the_health_boot_kernel_settings() {
    // F23: init raises the console level and turns a CPU stall into a panic until health is confirmed.
    let fixture = policy();
    for (name, value) in [
        ("printk", "7"),
        ("panic", "10"),
        ("panic_on_rcu_stall", "1"),
    ] {
        fixture.put(&format!("/proc/sys/kernel/{name}"), &format!("{value}\n"));
    }
    // As init wrote it on the first test build: a raw tab inside the JSON string.
    fixture.put(
        "/run/y2/health-debug.json",
        "{\"printk\":\"3\t4\t1\t7\",\"panic\":\"0\",\"panic_on_rcu_stall\":\"0; reboot\"}\n",
    );
    restore_health_debug(&fixture.root);
    assert_eq!(fixture.read("/proc/sys/kernel/printk"), "3\t4\t1\t7\n");
    // Owner decision 2026-10-28: the running system keeps panic=10, not the command line's 0.
    assert_eq!(fixture.read("/proc/sys/kernel/panic"), "10\n");
    // Not a number list: untouched.
    assert_eq!(fixture.read("/proc/sys/kernel/panic_on_rcu_stall"), "1\n");
    assert!(!fixture.exists("/run/y2/health-debug.json"));
    restore_health_debug(&fixture.root); // nothing saved: nothing to do
}

#[test]
fn battery_gate_needs_charger_or_half_battery() {
    let fixture = policy();
    let power = |battery: Value| {
        fixture.put_json(
            "/run/y2/power.json",
            &json!({"boot_id": "boot-a", "battery": battery}),
        );
    };
    for (battery, allowed) in [
        (json!({"percent": 50, "charging": false}), true),
        (json!({"percent": 49}), false),
        (json!({"percent": 10, "charging": true}), true),
        (json!({"status": "Full"}), true),
        (json!({}), false),
        (json!({"percent": "80"}), false),
    ] {
        power(battery.clone());
        match battery_gate(&fixture.root) {
            Ok(()) => assert!(allowed, "{battery}"),
            Err(error) => {
                assert!(!allowed, "{battery}");
                assert!(error.text().contains("charger_or_half_battery"));
            }
        }
    }
    // A record from an earlier boot is not evidence for this one.
    fixture.put_json(
        "/run/y2/power.json",
        &json!({"boot_id": "old", "battery": {"percent": 90}}),
    );
    assert!(matches!(battery_gate(&fixture.root), Err(Raise::Value(_))));
}

#[test]
fn install_restarts_unless_the_owner_chose_power_off() {
    let fixture = policy();
    assert_eq!(restart_mode(&fixture.root), "reboot");
    fixture.put_json("/data/updates/restart.json", &json!({"mode": "poweroff"}));
    assert_eq!(restart_mode(&fixture.root), "poweroff");
    fixture.put_json("/data/updates/restart.json", &json!({"mode": "anything"}));
    assert_eq!(restart_mode(&fixture.root), "reboot");
}

#[test]
fn channel_override_and_redirect_allowlist() {
    let fixture = policy();
    assert_eq!(channel(&fixture.root).name, json!("stable"));
    fixture.put_json(
        "/data/updates/channel.json",
        &json!({"manifest_url": "https://example.org/x/manifest.json",
                "allowed_redirect_hosts": ["evil.example"]}),
    );
    let value = channel(&fixture.root);
    assert_eq!(
        value.manifest_url,
        Some(json!("https://example.org/x/manifest.json"))
    );
    // Only the shipped list.
    let hosts = value.allowed_redirect_hosts;
    assert_eq!(
        hosts,
        ["github.com", "release-assets.githubusercontent.com"]
    );
    for url in [
        "http://release-assets.githubusercontent.com/a",
        "https://evil.example/a",
        "https://u:p@github.com/a",
        "https://github.com.evil.example/a",
    ] {
        assert!(
            !download::redirect_allowed(&download::join(URL, url), &hosts),
            "{url}"
        );
    }
    let followed = download::join(URL, "https://release-assets.githubusercontent.com/a?sig=1");
    assert_eq!(
        followed,
        "https://release-assets.githubusercontent.com/a?sig=1"
    );
    assert!(download::redirect_allowed(&followed, &hosts));
}

#[test]
fn journal_schemas_and_unpublished_channel() {
    let _serial = serial();
    let mut io = Fake::default();
    let fixture = policy();
    fixture.put_json(
        "/data/updates/state.json",
        &json!({"schema": 2, "state": "PendingHealth"}),
    );
    assert_eq!(status(&fixture.root)["state"], json!("PendingHealth"));
    fixture.put_json(
        "/data/updates/state.json",
        &json!({"schema": 3, "state": "Idle"}),
    );
    assert_eq!(
        status(&fixture.root)["failure"],
        json!("journal_unreadable")
    );
    fixture.remove("/data/updates/state.json");
    fixture.put_json(
        "/etc/y2linux/versions.json",
        &json!({"release_version": "v35"}),
    );
    fixture.put_json("/run/y2/network.json", &json!({"state": "Online"}));
    volume(&fixture);
    io.clock(true, true);
    // Nothing published on the channel yet: the manifest answers 404.
    io.remote(|url| {
        assert_eq!(url, URL);
        Err(Raise::Net(Net::Http {
            code: 404,
            text: "HTTP Error 404: Not Found".into(),
        }))
    });
    let (result, _) = answering(&fixture, vec![blkid()], |root| {
        stage(root, &io, None, Some(URL), true)
    });
    assert_eq!(result.unwrap()["state"], json!("UpToDate"));
    let checked = fixture.json("/data/updates/download.json");
    assert_eq!(
        (
            &checked["state"],
            &checked["release_version"],
            &checked["published"]
        ),
        (&json!("Checked"), &json!("v35"), &json!(false))
    );
    // A download never treats 404 as success.
    let (result, _) = answering(&fixture, vec![blkid()], |root| {
        stage(root, &io, None, Some(URL), false)
    });
    assert!(matches!(
        result,
        Err(Raise::Net(Net::Http { code: 404, .. }))
    ));
}

#[test]
fn daily_check_is_rate_limited_and_never_shows_a_failed_update() {
    let _serial = serial();
    let mut io = Fake::default();
    let fixture = policy();
    volume(&fixture);
    fixture.put_json("/run/y2/network.json", &json!({"state": "Online"}));
    fixture.put_json(
        "/data/updates/download.json",
        &json!({"schema": 1, "state": "Checked", "release_version": "old"}),
    );
    io.clock(true, true);
    let package = package(&fixture, "new", 4096);
    // `stage` patched in Python; here the release channel answers (or fails) and the core checks.
    let staged = Rc::new(Cell::new(0));
    let fail = Rc::new(Cell::new(true));
    let (count, failing, directory) = (staged.clone(), fail.clone(), package.directory.clone());
    io.remote(move |url| {
        let name = url.rsplit('/').next().unwrap();
        if name == "manifest.json" {
            count.set(count.get() + 1);
        }
        if failing.get() {
            return Err(Raise::value("HTTP_status"));
        }
        Ok(Stream::File(File::open(directory.join(name)).unwrap()))
    });
    let check = |release: &str| {
        let manifest = package
            .manifest
            .replace("\"new\"", &format!("\"{release}\""));
        vec![
            blkid(),
            core_answer("check-manifest", Some(&incoming()), true, &manifest),
        ]
    };
    let at = |now: f64, release: &str| {
        io.now.set(Some(now));
        answering(&fixture, check(release), |root| automatic_check(root, &io))
            .0
            .unwrap()
    };
    let result = at(1000.0, "new");
    assert_eq!(result["result"], json!("failed"));
    assert_eq!(
        fixture.json("/data/updates/download.json")["release_version"],
        json!("old")
    );
    // A failed check is tried again (the service paces attempts hourly); it used to wait a whole day.
    fail.set(false);
    assert_eq!(at(1000.0 + 3600.0, "new")["release_version"], json!("new"));
    assert_eq!(staged.get(), 2);
    // Within a day of a real check: nothing due, and it says so.
    assert_eq!(at(1000.0 + 7200.0, "new")["reason"], json!("not_due"));
    assert_eq!(
        at(1000.0 + 3600.0 + 86401.0, "new")["release_version"],
        json!("new")
    );
    // A failed record written by ≤ 8.14 kept `time` (ef22a23d): it no longer holds off a day.
    fixture.put_json(
        "/data/updates/last-check.json",
        &json!({"schema": 1, "attempt": 5000, "time": 5000, "result": "failed",
                "failure": "[Errno 11]"}),
    );
    staged.set(0);
    assert_eq!(
        at(5000.0 + 3601.0, "newer")["release_version"],
        json!("newer")
    );
    assert_eq!(staged.get(), 1);
    // Offline: no network call, but the attempt is remembered.
    fixture.put_json("/run/y2/network.json", &json!({"state": "Associating"}));
    staged.set(0);
    assert_eq!(
        at(1000.0 + 3.0 * 86400.0, "new")["reason"],
        json!("offline")
    );
    assert_eq!(staged.get(), 0);
    assert_eq!(
        fixture.json("/data/updates/last-check.json")["attempt"],
        json!(1000.0 + 3.0 * 86400.0)
    );
}

#[test]
fn a_check_waits_for_wifi_retries_and_keeps_the_offered_release() {
    // A Check for Updates right after a wake (Wi-Fi still reconnecting) failed at once and replaced
    // 'Update Available' with 'Connect to Wi-Fi' (reports 61caa81e/297f1032).
    let _serial = serial();
    let mut io = Fake::default();
    let fixture = policy();
    volume(&fixture);
    fixture.put_json(
        "/etc/y2linux/versions.json",
        &json!({"release_version": "v35"}),
    );
    fixture.put_json(
        "/data/updates/download.json",
        &json!({"schema": 1, "state": "Checked", "release_version": "v36", "bytes": 1}),
    );
    fixture.put_json("/run/y2/network.json", &json!({"state": "Associating"}));
    io.clock(true, true);
    let opened = Rc::new(Cell::new(0u32));
    let count = opened.clone();
    io.remote(move |_| {
        count.set(count.get() + 1);
        Err(Raise::Net(Net::Url(
            "[Errno -3] Temporary failure in name resolution".into(),
        )))
    });
    let kept = || {
        let value = fixture.json("/data/updates/download.json");
        (value["state"].clone(), value["release_version"].clone())
    };
    // Never online: no request at all, the offered release stays.
    let (result, _) = answering(&fixture, vec![blkid()], |root| {
        stage(root, &io, None, Some(URL), true)
    });
    assert_eq!(result.err().unwrap().text(), "network_not_online");
    assert_eq!(opened.get(), 0);
    assert_eq!(kept(), (json!("Checked"), json!("v36")));
    // Online, but every lookup fails: three tries, then the error; still offered.
    fixture.put_json("/run/y2/network.json", &json!({"state": "Online"}));
    let (result, _) = answering(&fixture, vec![blkid()], |root| {
        stage(root, &io, None, Some(URL), true)
    });
    assert!(matches!(result, Err(Raise::Net(Net::Url(_)))));
    assert_eq!(opened.get(), MANIFEST_ATTEMPTS);
    assert_eq!(kept(), (json!("Checked"), json!("v36")));
    assert!(
        fixture.json("/data/updates/download.json")["last_check_failure"]
            .as_str()
            .unwrap()
            .contains("name resolution")
    );
    // A download that fails is still reported as failed.
    let (result, _) = answering(&fixture, vec![blkid()], |root| {
        stage(root, &io, None, Some(URL), false)
    });
    assert!(matches!(result, Err(Raise::Net(Net::Url(_)))));
    assert_eq!(
        fixture.json("/data/updates/download.json")["state"],
        json!("Failed")
    );
}
