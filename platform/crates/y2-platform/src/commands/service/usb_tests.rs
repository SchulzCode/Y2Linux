//! usb.py's product tests (tests/test_usb_product.py `USBProduct`, `MediaViews`) against `UsbService`
//! on a fixture root: descriptor readiness before the UDC bind, the bounded restart budget, the live
//! MTP view and the media that may be exported. The views are the real `media_views` of a fixture
//! Y2Media; where Python patched `media_views`, the cached views are set while the files they were
//! read from stay unchanged. A stand-in responder (a shell that sleeps) starts in place of umtprd.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::commands::service::process::testing::launching;
use crate::commands::storage::sys::release_sd;
use crate::testing::{ok, repository, scripted, Fixture};
use std::{
    cell::RefCell,
    os::unix::fs::{symlink as link, MetadataExt},
    rc::Rc,
};

const UUID: &str = "79324c69-6e75-4801-8000-000000000103";
const MOUNT: &str = "5 1 179:2 / /media/internal rw - ext4 /dev/mmcblk0p8 rw";
const DEVICE: &str =
    "/sys/devices/platform/11230000.mmc/mmc_host/mmc0/mmc0:0001/block/mmcblk0/mmcblk0p8";
const INTERNAL: &str = "/media/internal/music";

type Launch = Box<dyn FnMut(&[&str]) -> Option<Vec<String>>>;

/// Y2Media mounted at /media/internal: device 179:2 on the eMMC controller with Y2Media's geometry,
/// its music folder present (`blkid` answers its UUID, see `answers`).
fn media(fixture: &Fixture) {
    fixture
        .put(&format!("{DEVICE}/start"), "3742720")
        .put(&format!("{DEVICE}/size"), "11460608")
        .put(&format!("{DEVICE}/partition"), "8")
        .put(&format!("{DEVICE}/dev"), "179:2")
        .mkdir("/sys/dev/block")
        .put("/proc/self/mountinfo", MOUNT)
        .mkdir(INTERNAL);
    link(fixture.path(DEVICE), fixture.path("/sys/dev/block/179:2")).unwrap();
}

fn answers(uuid: &str) -> Vec<(Vec<String>, y2p_core::command::Outcome)> {
    vec![
        ok(
            &["blkid", "-p", "-s", "UUID", "-o", "value", "/dev/mmcblk0p8"],
            uuid,
        ),
        ok(&["mount", "-t", "functionfs", "mtp", "/dev/ffs-mtp"], ""),
    ]
}

/// The stand-in for `umtprd -conf …`; a responder IPC command (`-cmd:…`) is refused.
fn responder(argv: &[&str]) -> Option<Vec<String>> {
    (argv.get(1) == Some(&"-conf")).then(|| {
        ["/bin/sh", "-c", "exec sleep 30"]
            .map(String::from)
            .to_vec()
    })
}

/// Run `body` with the fixture's command answers and `launch` for what the service starts.
fn session<T>(launch: Launch, body: impl FnOnce() -> T) -> T {
    scripted(answers(UUID), || launching(launch, body)).0
}

struct Usb {
    fixture: Fixture,
    service: UsbService,
}

impl Usb {
    /// USBProduct.setUp: the UDC not attached, configfs and /run/y2 present, Internal Storage ready.
    fn new() -> Usb {
        let fixture = Fixture::new();
        fixture
            .put("/proc/sys/kernel/random/boot_id", "boot-one")
            .put("/sys/class/udc/musb/state", "not attached")
            .mkdir("/sys/kernel/config/usb_gadget")
            .mkdir("/run/y2");
        media(&fixture);
        let service = UsbService::new(&fixture.root);
        Usb { fixture, service }
    }

    /// A gadget attribute as the platform reads it (stripped).
    fn gadget(&self, relative: &str) -> String {
        self.fixture
            .root
            .read(&format!("{GADGET}/{relative}"))
            .unwrap()
    }

    fn tick(&mut self) -> Value {
        self.service.tick().unwrap()
    }

    fn descriptors(&self) {
        for i in 1..=3 {
            self.fixture.put(&format!("/dev/ffs-mtp/ep{i}"), "");
        }
        self.fixture
            .put(&format!("{GADGET}/functions/ffs.mtp/ready"), "1");
    }

    fn cable(&self, state: &str) {
        self.fixture.put("/sys/class/udc/musb/state", state);
    }

    /// One tick sets the gadget up, then the descriptors are written and a host configures it.
    fn ready(&mut self) -> Value {
        self.tick();
        self.descriptors();
        self.cable("configured");
        self.tick()
    }

    fn pid(&self) -> i32 {
        self.service.child.as_ref().map(Child::pid).unwrap()
    }

    /// The responder exits.
    fn responder_exits(&mut self) {
        let child = self.service.child.as_mut().unwrap();
        child.kill();
        assert!(child.wait(2.0));
    }

    fn internal_view(&self) -> Object {
        self.service.views.clone().unwrap()[0].clone()
    }
}

impl Drop for Usb {
    fn drop(&mut self) {
        let _ = self.service.stop();
    }
}

fn sd_view() -> Object {
    json!({"path": "/media/sd", "name": "SD Card", "access": "rw", "mount_id": 9,
           "device_id": "179:9", "uuid": "u"})
    .as_object()
    .cloned()
    .unwrap()
}

fn paths(record: &Value) -> Vec<&str> {
    record["views"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["path"].as_str().unwrap())
        .collect()
}

/// Whether `pid` is still a process (a stopped child is reaped).
fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    unsafe { libc::kill(pid, 0) == 0 }
}

// ---- USBProduct ----------------------------------------------------------------------------------

#[test]
fn no_udc_bind_before_all_functionfs_descriptors() {
    let mut usb = Usb::new();
    session(Box::new(responder), || {
        let state = usb.tick();
        assert_eq!(state["reason"], "waiting_for_descriptors");
        assert!(!usb.fixture.exists(&format!("{GADGET}/UDC")));
        assert_eq!(usb.ready()["state"], "Ready");
        assert_eq!(usb.gadget("UDC"), "musb");
        assert_eq!(usb.gadget("os_desc/use"), "1");
        assert!(is_link(&usb.fixture.path(&format!("{GADGET}/os_desc/c.1"))));
    });
    assert_eq!(selection("mtp").unwrap(), ["ffs.mtp", "ecm.usb0"]);
}

fn is_link(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
}

#[test]
fn no_block_export_or_triple_function_selection() {
    for invalid in [
        "msc",
        "mass_storage",
        "mtp+acm+ecm",
        "/dev/mmcblk0",
        "Y2DATA",
        "BOOTIMG",
    ] {
        assert_eq!(
            selection(invalid).err().unwrap().message(),
            "unsupported_usb_mode"
        );
    }
    assert_eq!(selection("developer").unwrap(), ["acm.usb0", "ecm.usb0"]);
    for path in ["/", "/data", "/dev/mmcblk0", "/data/reborn", "/data/ssh"] {
        let view = json!({"path": path, "name": "Internal Storage", "access": "rw"});
        assert!(config(&[view.as_object().cloned().unwrap()]).is_err(), "{path}");
    }
    let view = json!({"path": INTERNAL, "name": "Internal Storage", "access": "rw", "mount_id": 5});
    assert!(config(&[view.as_object().cloned().unwrap()])
        .unwrap()
        .contains("storage \"/media/internal/music\" \"Internal Storage\" \"rw\""));
}

#[test]
fn disconnect_refresh_keeps_linux_ownership_and_reconnect() {
    let mut usb = Usb::new();
    session(Box::new(responder), || {
        usb.ready();
        usb.cable("not attached");
        let disconnected = usb.tick();
        assert_eq!(disconnected["state"], "Disconnected");
        assert!(disconnected["refresh_generation"].as_u64().unwrap() > 0);
        assert_eq!(disconnected["filesystem_owner"], "linux");
        assert_eq!(disconnected["block_export"], false);
        usb.cable("configured");
        assert_eq!(usb.tick()["state"], "Ready");
    });
}

#[test]
fn only_durable_generation_not_activity_requests_library_refresh() {
    let mut usb = Usb::new();
    session(Box::new(responder), || {
        usb.ready();
        let pid = usb.pid();
        usb.fixture.put_json(
            "/run/y2/mtp-events.json",
            &json!({"pid": pid, "active": true, "generation": 0}),
        );
        let value = usb.tick();
        assert_eq!(value["state"], "Copying");
        assert_eq!(value["refresh_generation"], 0);
        usb.fixture.put_json(
            "/run/y2/mtp-events.json",
            &json!({"pid": pid, "active": false, "generation": 5}),
        );
        let first = usb.tick()["refresh_generation"].as_u64().unwrap();
        assert!(first > 0);
        assert_eq!(first, usb.tick()["refresh_generation"].as_u64().unwrap());
    });
}

#[test]
fn failed_daemon_and_failed_bind_have_bounded_retry() {
    let mut usb = Usb::new();
    session(Box::new(responder), || {
        usb.ready();
        for _ in 0..5 {
            if usb.service.child.is_some() {
                usb.responder_exits();
            }
            usb.tick();
            usb.tick();
        }
        assert!(usb.service.terminal);
        assert_eq!(usb.tick()["state"], "Failed");
    });
}

#[test]
fn failure_budget_decays_after_a_stable_responder() {
    let mut usb = Usb::new();
    session(Box::new(responder), || {
        usb.ready();
        usb.service.failures = 4;
        usb.service.started -= 120.0;
        usb.responder_exits();
        usb.tick();
        assert_eq!(usb.service.failures, 1);
        assert!(!usb.service.terminal);
    });
}

#[test]
fn bind_failure_never_reports_host_ownership() {
    let mut usb = Usb::new();
    session(Box::new(responder), || {
        usb.tick();
        usb.descriptors();
        // The bind fails: the UDC attribute cannot be written.
        usb.fixture.mkdir(&format!("{GADGET}/UDC"));
        let value = usb.tick();
        assert_eq!(value["state"], "Failed");
        assert_eq!(value["filesystem_owner"], "linux");
        assert!(!usb.service.bound);
        assert!(usb.service.child.is_none());
    });
}

#[test]
fn readiness_restart_unbinds_its_existing_gadget_before_setup() {
    let mut usb = Usb::new();
    session(Box::new(responder), || usb.ready());
    // The UDC as the restarted service's setup leaves it when it starts its responder (the last
    // step before the bind).
    let seen = Rc::new(RefCell::new(Vec::new()));
    let (record, udc) = (
        Rc::clone(&seen),
        usb.fixture.path(&format!("{GADGET}/UDC")),
    );
    let launch: Launch = Box::new(move |argv| {
        record
            .borrow_mut()
            .push(fs::read_to_string(&udc).unwrap());
        responder(argv)
    });
    let mut restarted = UsbService::new(&usb.fixture.root);
    session(launch, || {
        let value = restarted.tick().unwrap();
        assert_eq!(*seen.borrow(), ["\n"]);
        assert_eq!(usb.gadget("UDC"), "musb");
        assert_eq!(value["filesystem_owner"], "linux");
        assert_eq!(restarted.tick().unwrap()["filesystem_owner"], "linux");
        restarted.stop().unwrap();
    });
}

#[test]
fn gadget_requests_validated_bus_powered_500ma_allocation() {
    // The charger grants 450 mA to a *configured* 500 mA request only; a self-powered/0 mA
    // configuration left a PC port "Not charging".
    let mut usb = Usb::new();
    session(Box::new(responder), || usb.ready());
    assert_eq!(usb.gadget("configs/c.1/MaxPower"), "500");
    assert_eq!(usb.gadget("configs/c.1/bmAttributes"), "0x80");
}

#[test]
fn both_views_declared_in_fixed_slots_missing_sd_is_notmounted() {
    let internal = json!({"path": INTERNAL, "name": "Internal Storage", "access": "rw", "mount_id": 5});
    let internal = internal.as_object().cloned().unwrap();
    let text = config(std::slice::from_ref(&internal)).unwrap();
    assert!(text.contains("storage \"/media/internal/music\" \"Internal Storage\" \"rw\""));
    assert!(text.contains("storage \"/media/sd\" \"SD Card\" \"rw,removable,notmounted\""));
    let mut sd = sd_view();
    sd.insert("access".into(), json!("ro"));
    let text = config(&[internal, sd]).unwrap();
    assert!(text.contains("storage \"/media/sd\" \"SD Card\" \"ro,removable\""));
    assert!(text.find("Internal Storage") < text.find("SD Card"));
}

#[test]
fn sd_hotplug_changes_the_live_view_without_usb_reenumeration() {
    let mut usb = Usb::new();
    session(Box::new(responder), || usb.ready());
    let pid = usb.pid();
    let udc = usb.fixture.path(&format!("{GADGET}/UDC"));
    let bound = fs::metadata(&udc).unwrap();
    // The responder acknowledges each IPC command by publishing the roots it pins.
    let sent = Rc::new(RefCell::new(Vec::<String>::new()));
    let (record, roots) = (Rc::clone(&sent), usb.fixture.path("/run/y2/mtp-roots.json"));
    let launch: Launch = Box::new(move |argv| {
        let message = argv.get(1)?.strip_prefix("-cmd:")?.to_owned();
        let name = message.split_once(':').unwrap().1;
        let path = if name == "Internal Storage" {
            INTERNAL
        } else {
            "/media/sd"
        };
        let current: Value = serde_json::from_str(&fs::read_to_string(&roots).unwrap()).unwrap();
        let mut current: Vec<String> =
            serde_json::from_value(current["roots"].clone()).unwrap();
        current.retain(|p| p != path);
        if message.starts_with("mount") {
            current.push(path.into());
        }
        current.sort();
        fs::write(&roots, json!({"pid": pid, "roots": current}).to_string()).unwrap();
        record.borrow_mut().push(message);
        Some(vec!["true".into()])
    });
    usb.fixture.put_json(
        "/run/y2/mtp-roots.json",
        &json!({"pid": pid, "roots": [INTERNAL]}),
    );
    let internal = usb.internal_view();
    let (first, second) = session(launch, || {
        usb.service.cached_views = vec![internal.clone(), sd_view()];
        let first = usb.tick();
        assert_eq!(*sent.borrow(), ["mount:SD Card"]);
        assert_eq!(paths(&first), [INTERNAL, "/media/sd"]);
        usb.service.cached_views = vec![internal.clone()];
        (first, usb.tick())
    });
    assert!(first["refresh_generation"].as_u64().unwrap() > 0);
    assert_eq!(*sent.borrow(), ["mount:SD Card", "unmount:SD Card"]);
    assert_eq!(paths(&second), [INTERNAL]);
    // No UDC write and the same responder: the host saw no re-enumeration.
    let after = fs::metadata(&udc).unwrap();
    assert_eq!(
        (after.mtime(), after.mtime_nsec()),
        (bound.mtime(), bound.mtime_nsec())
    );
    assert_eq!(usb.pid(), pid);
    assert!(second["refresh_generation"].as_u64().unwrap() > 0);
}

#[test]
fn unacknowledged_unmount_falls_back_to_stopping_the_responder() {
    let mut usb = Usb::new();
    session(Box::new(responder), || usb.ready());
    let old = usb.pid();
    let sd = sd_view();
    usb.service.mounted.push(("/media/sd".into(), identity(&sd)));
    usb.service.views = Some(vec![usb.internal_view(), sd]);
    // The responder takes the command but never drops the root.
    let launch: Launch = Box::new(|argv| {
        argv.get(1)
            .is_some_and(|a| a.starts_with("-cmd:"))
            .then(|| vec!["true".into()])
    });
    session(launch, || usb.tick());
    assert!(usb.service.child.as_ref().map(Child::pid) != Some(old));
    assert!(!alive(old)); // the old responder released every pin
}

#[test]
fn media_identity_change_stops_old_responder_before_new_view() {
    let mut usb = Usb::new();
    session(Box::new(responder), || usb.ready());
    let old = usb.pid();
    let mut remounted = usb.internal_view();
    remounted.insert("mount_id".into(), json!(6));
    session(Box::new(responder), || {
        usb.service.cached_views = vec![remounted];
        usb.tick();
    });
    assert!(!alive(old));
    assert!(usb.service.child.as_ref().map(Child::pid) != Some(old));
}

#[test]
fn developer_change_waits_for_disconnect() {
    let mut usb = Usb::new();
    session(Box::new(responder), || {
        usb.ready();
        usb.fixture
            .put("/data/system/platform/usb-mode.json", "{\"mode\":\"developer\"}");
        assert_eq!(usb.tick()["mode"], "mtp");
        usb.cable("not attached");
        assert_eq!(usb.tick()["mode"], "developer");
    });
}

#[test]
fn sd_eject_refuses_connected_host_and_preserves_mount() {
    let usb = Usb::new();
    usb.fixture.put_json(
        "/run/y2/usb-transfer.json",
        &json!({"schema": SCHEMA, "boot_id": "boot-one", "cable": "configured",
                "views": [{"path": "/media/sd"}]}),
    );
    assert!(!release_sd(&usb.fixture.root, 42, false).unwrap());
    assert!(!usb.fixture.exists("/run/y2/usb-release-sd.json"));
}

/// The responder patches name the roots it pins; a stale path leaves a storage declared but never
/// mounted.
#[test]
fn responder_roots_are_exactly_the_platform_export_paths() {
    let patches: String = [
        "0001-y2-safe-media-transfers.patch",
        "0002-y2-mtp-pipeline-and-live-storage.patch",
    ]
    .iter()
    .map(|name| {
        fs::read_to_string(repository().join("buildroot/package/y2-mtp").join(name)).unwrap()
    })
    .collect();
    let added: Vec<&str> = patches
        .lines()
        .filter_map(|line| line.strip_prefix('+'))
        .collect();
    for (path, _) in NAMES {
        let pinned = format!("strcmp(root, \"{path}\")");
        assert!(added.iter().any(|line| line.contains(&pinned)), "{path}");
    }
    let first = NAMES.iter().find(|(p, _)| *p != "/media/sd").unwrap().0;
    let init = format!("y2_fs_init(has_internal ? \"{first}\" : NULL");
    assert!(added.iter().any(|line| line.contains(&init)));
    assert!(!added.concat().contains("/data/music"));
}

// ---- MediaViews ----------------------------------------------------------------------------------

/// MediaViews.setUp: Y2Media mounted with its geometry, an SD card folder but no card.
fn views_fixture() -> Fixture {
    let fixture = Fixture::new();
    fixture
        .put("/proc/sys/kernel/random/boot_id", "boot-one")
        .mkdir("/media/sd");
    media(&fixture);
    fixture
}

/// `media_views` with `blkid` answering `uuid` for Y2Media.
fn views_with(fixture: &Fixture, uuid: &str) -> Vec<Object> {
    scripted(answers(uuid), || media_views(&fixture.root)).0
}

fn views(fixture: &Fixture) -> Vec<Object> {
    views_with(fixture, UUID)
}

fn mountinfo(fixture: &Fixture) -> String {
    fixture.read("/proc/self/mountinfo")
}

#[test]
fn missing_sd_never_exposes_empty_mountpoint() {
    let fixture = views_fixture();
    let views = views(&fixture);
    let found: Vec<_> = views.iter().map(|v| text(v, "path").unwrap()).collect();
    assert_eq!(found, [INTERNAL]);
}

#[test]
fn dirty_or_failed_media_and_remount_failure_are_not_exposed() {
    let fixture = views_fixture();
    // A card mounted without a verified controller, claim or check is Failed.
    let text = mountinfo(&fixture) + "\n6 1 179:9 / /media/sd rw - exfat /dev/mmcblk1p1 rw";
    fixture.put("/proc/self/mountinfo", &text);
    assert_eq!(views(&fixture).len(), 1);
    fixture.put("/proc/self/mountinfo", "");
    assert!(views(&fixture).is_empty());
}

#[test]
fn readonly_superblock_is_never_advertised_writable() {
    let fixture = views_fixture();
    fixture.put(
        "/proc/self/mountinfo",
        "5 1 179:2 / /media/internal rw - ext4 /dev/mmcblk0p8 ro",
    );
    assert_eq!(text(&views(&fixture)[0], "access"), Some("ro"));
}

#[test]
fn wrong_internal_geometry_uuid_or_nested_mount_is_excluded() {
    let fixture = views_fixture();
    fixture.put("/sys/dev/block/179:2/start", "0");
    assert!(views(&fixture).is_empty());
    fixture.put("/sys/dev/block/179:2/start", "3742720");
    assert!(views_with(&fixture, "system-root").is_empty());
    // The old Y2DATA geometry/uuid is no longer an acceptable internal export.
    fixture
        .put("/sys/dev/block/179:2/start", "2104320")
        .put("/sys/dev/block/179:2/size", "1638400");
    assert!(views_with(&fixture, "79324c69-6e75-4801-8000-000000000102").is_empty());
    fixture
        .put("/sys/dev/block/179:2/start", "3742720")
        .put("/sys/dev/block/179:2/size", "11460608");
    assert_eq!(views(&fixture).len(), 1);
    let text = mountinfo(&fixture)
        + "\n8 5 179:1 / /media/internal/music rw - ext4 /dev/mmcblk0p1 rw";
    fixture.put("/proc/self/mountinfo", &text);
    assert!(views(&fixture).is_empty());
}

/// STORAGE-SETUP case 3: the stock storage can be copied off, never written.
#[test]
fn old_storage_is_read_only_and_only_while_the_setup_waits() {
    let fixture = views_fixture();
    fixture.put(
        "/proc/self/mountinfo",
        "5 1 179:2 / /run/y2/old-storage ro - vfat /dev/mmcblk0p8 ro",
    );
    fixture.mkdir("/run/y2/old-storage").mkdir("/sys/class/block");
    // The Y2Media partition entry (internal_media.partition): mmcblk0p8.
    link(
        fixture.path(DEVICE),
        fixture.path("/sys/class/block/mmcblk0p8"),
    )
    .unwrap();
    let setup = |state: &str| {
        fixture.put_json(
            "/run/y2/internal-media-setup.json",
            &json!({"boot_id": "boot-one", "state": state}),
        );
    };
    setup("Waiting");
    let found = views(&fixture);
    let found_views: Vec<_> = found
        .iter()
        .map(|v| {
            (
                text(v, "path").unwrap(),
                text(v, "name").unwrap(),
                text(v, "access").unwrap(),
            )
        })
        .collect();
    assert_eq!(found_views, [("/run/y2/old-storage", "Old Storage", "ro")]);
    assert!(config(&found)
        .unwrap()
        .contains("storage \"/run/y2/old-storage\" \"Old Storage\" \"ro\""));
    setup("Running"); // Set Up Now: it leaves the computer first
    assert!(views(&fixture).is_empty());
    assert!(config(&[])
        .unwrap()
        .contains("storage \"/run/y2/old-storage\" \"Old Storage\" \"rw,notmounted\""));
}

/// #39: the copy goes into Internal Storage's Firmware folder instead of Y2DATA.
#[test]
fn with_internal_storage_there_is_no_separate_firmware_drive() {
    let fixture = views_fixture();
    fixture.put(
        "/proc/self/mountinfo",
        &format!("{MOUNT}\n7 1 179:7 / /data rw - ext4 /dev/mmcblk0p7 rw"),
    );
    let paths = |fixture: &Fixture| -> Vec<String> {
        views(fixture)
            .iter()
            .map(|v| text(v, "path").unwrap().to_owned())
            .collect()
    };
    // Internal Storage not recorded Ready (internal_media.ready is false).
    assert!(paths(&fixture).contains(&"/data/firmware/inbox".to_owned()));
    fixture.put_json(
        "/run/y2/internal-media.json",
        &json!({"state": "Ready", "boot_id": "boot-one", "mount_id": 5, "device_id": "179:2"}),
    );
    let found = paths(&fixture);
    assert!(!found.contains(&"/data/firmware/inbox".to_owned()));
    assert!(found.contains(&INTERNAL.to_owned()));
    assert!(fixture.exists("/media/internal/music/Firmware/README.txt"));
}
