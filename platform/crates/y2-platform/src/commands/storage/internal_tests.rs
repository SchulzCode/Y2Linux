//! Y2Media (eMMC p8) on a fixture (from tests/test_internal_media.py, test_filesystems.py and
//! test_exfat_readahead.py): exact-identity mount, owner-confirmed provisioning, verified migration,
//! the automatic setup. The player's root answers commands with a fake whose state follows them.
// SPDX-License-Identifier: GPL-2.0-only
use super::{provision::*, setup::*, *};
use crate::commands::observation::snapshot;
use crate::commands::storage::{observe::setup_status, sys::statvfs};
use crate::testing::{Fixture, Outcome};
use serde_json::json;
use std::{
    cell::Cell,
    fs::{self, FileTimes},
    os::unix::fs::MetadataExt,
    os::unix::fs::{symlink, PermissionsExt},
    process::Command,
    sync::{Arc, Mutex},
    time::{Duration, UNIX_EPOCH},
};

/// A digest under which every copy reads back wrong.
fn mismatching(path: &Path) -> Result<String, Failure> {
    let part = path.to_string_lossy().ends_with(".part");
    Ok((if part { "x" } else { "y" }).to_owned())
}

const STOCK_SERIAL: &str = "3F93-07E6";
const MOUNT_LINE: &str = "9 1 179:8 / /media/internal rw - ext4 /dev/mmcblk0p8 rw";
const OLD_LINE: &str = "12 1 179:8 / /run/y2/old-storage ro - vfat /dev/mmcblk0p8 ro";
const DISK: &str = "/sys/devices/platform/11230000.mmc/block/mmcblk0";

fn answer(ok: bool, reason: Option<&str>, output: Option<&str>) -> Outcome {
    Outcome {
        ok,
        reason: reason.map(Into::into),
        output: output.map(Into::into),
    }
}

fn done() -> Outcome {
    answer(true, None, Some(""))
}

fn command_failed(output: Option<&str>) -> Outcome {
    answer(false, Some("command_failed"), output)
}

/// The player's commands: blkid prints `fs`, mkfs.ext4 makes Y2Media's filesystem, a mount adds the
/// Y2Media or the old storage line, umount removes it.
struct Fake {
    mountinfo: PathBuf,
    phase: PathBuf,
    fs: Vec<(&'static str, String)>,
    fsck_ok: bool,
    fat_check: Outcome,
    mkfs_ok: bool,
    /// The start screen's phase whenever it changed, as the commands ran.
    phases: Vec<String>,
    everything: Option<Outcome>,
}

impl Fake {
    fn mount_line(&self, line: &str, present: bool) {
        let text = fs::read_to_string(&self.mountinfo).unwrap_or_default();
        let mut lines: Vec<&str> = text
            .trim()
            .lines()
            .filter(|l| !l.is_empty() && *l != line)
            .collect();
        if present {
            lines.push(line);
        }
        fs::write(&self.mountinfo, lines.join("\n")).unwrap();
    }

    fn answer(&mut self, argv: &[&str]) -> Outcome {
        if let Ok(phase) = fs::read_to_string(&self.phase) {
            if self.phases.last() != Some(&phase) {
                self.phases.push(phase);
            }
        }
        if let Some(everything) = &self.everything {
            return everything.clone();
        }
        let value = |flag: &str| argv[argv.iter().position(|a| *a == flag).unwrap() + 1].to_owned();
        match argv[0] {
            "blkid" => {
                let text: Vec<String> = self.fs.iter().map(|(k, v)| format!("{k}={v}")).collect();
                answer(true, None, Some(&text.join("\n")))
            }
            "e2fsck" if !self.fsck_ok => command_failed(None),
            "mkfs.ext4" if !self.mkfs_ok => command_failed(None),
            "mkfs.ext4" => {
                self.fs = vec![
                    ("TYPE", "ext4".into()),
                    ("LABEL", value("-L")),
                    ("UUID", value("-U")),
                ];
                done()
            }
            "fsck.fat" => self.fat_check.clone(),
            "mount" => {
                self.mount_line(
                    if argv[2] == "vfat" {
                        OLD_LINE
                    } else {
                        MOUNT_LINE
                    },
                    true,
                );
                done()
            }
            "umount" if argv[1] == OLD => {
                self.mount_line(OLD_LINE, false);
                done()
            }
            "umount" => {
                fs::write(&self.mountinfo, "").unwrap();
                done()
            }
            _ => done(),
        }
    }
}

struct Player {
    fixture: Fixture,
    fake: Arc<Mutex<Fake>>,
    /// The commands before this many were cleared.
    cleared: Cell<usize>,
}

fn stock() -> Vec<(&'static str, String)> {
    vec![("TYPE", "vfat".into()), ("UUID", STOCK_SERIAL.into())]
}

fn y2media() -> Vec<(&'static str, String)> {
    vec![
        ("TYPE", "ext4".into()),
        ("LABEL", LABEL.into()),
        ("UUID", UUID.into()),
    ]
}

impl Player {
    /// Fixture.setUp: the eMMC's p8 (stock storage) and p7, nothing mounted.
    fn new() -> Player {
        let fixture = Fixture::new();
        fixture.put("/proc/sys/kernel/random/boot_id", "boot-one");
        fixture.put("/proc/self/mountinfo", "");
        let fake = Arc::new(Mutex::new(Fake {
            mountinfo: fixture.path("/proc/self/mountinfo"),
            phase: fixture.path("/run/reborn-splash/phase"),
            fs: stock(),
            fsck_ok: true,
            fat_check: done(),
            mkfs_ok: true,
            phases: Vec::new(),
            everything: None,
        }));
        let answering = Arc::clone(&fake);
        let player = Player {
            fixture: fixture.responding(move |argv, _| answering.lock().unwrap().answer(argv)),
            fake,
            cleared: Cell::new(0),
        };
        player.add_partition("mmcblk0p8", "3742720", "11460608", "179:8");
        player.add_partition("mmcblk0p7", "2104320", "1638400", "179:7");
        player
    }

    /// Setup.setUp: on battery at 3.995 V with the stock profile, the start screen showing.
    fn on_battery() -> Player {
        let player = Player::new();
        let fixture = &player.fixture;
        fixture.put("/sys/class/power_supply/BAT0/status", "Discharging");
        fixture.put("/sys/class/power_supply/BAT0/voltage_now", "3995068");
        fixture.copy_from_repository(
            "buildroot/board/y2/production-overlay/etc/y2linux/battery-profile.json",
            "/etc/y2linux/battery-profile.json",
        );
        fixture
            .mkdir("/media/internal")
            .mkdir("/data")
            .mkdir("/run/reborn-splash");
        player
    }

    fn add_partition(&self, name: &str, start: &str, size: &str, dev: &str) {
        let part = format!("{DISK}/{name}");
        let number = &name[name.len() - 1..];
        for (key, value) in [
            ("partition", number),
            ("start", start),
            ("size", size),
            ("dev", dev),
        ] {
            self.fixture.put(&format!("{part}/{key}"), value);
        }
        for link in [
            format!("/sys/class/block/{name}"),
            format!("/sys/dev/block/{dev}"),
        ] {
            self.fixture
                .mkdir(Path::new(&link).parent().unwrap().to_str().unwrap());
            symlink(self.path(&part), self.path(&link)).unwrap();
        }
    }

    fn root(&self) -> &Root {
        &self.fixture.root
    }

    fn path(&self, name: &str) -> PathBuf {
        self.fixture.path(name)
    }

    fn set(&self, change: impl FnOnce(&mut Fake)) {
        change(&mut self.fake.lock().unwrap());
    }

    fn provisioned(&self) {
        self.set(|f| f.fs = y2media());
    }

    fn mount(&self) -> Value {
        Value::Object(mount(self.root()).unwrap())
    }

    fn unmount(&self) -> Value {
        Value::Object(unmount(self.root()).unwrap())
    }

    fn status(&self) -> Value {
        Value::Object(status(self.root()))
    }

    fn ready(&self) -> bool {
        ready(self.root())
    }

    fn provision(&self, confirm: Option<&str>) -> Value {
        Value::Object(provision(self.root(), &json!(confirm), true).unwrap())
    }

    fn set_up(&self, automatic: bool, confirm: Option<&str>) -> Value {
        Value::Object(setup(self.root(), automatic, &json!(confirm)).unwrap())
    }

    /// The argv of every command since the last `clear`.
    fn calls(&self) -> Vec<Vec<String>> {
        let calls = self.fixture.calls().split_off(self.cleared.get());
        calls.into_iter().map(|(argv, _)| argv).collect()
    }

    fn automatic(&self) -> Value {
        self.set_up(true, None)
    }

    fn names(&self) -> Vec<String> {
        self.calls().into_iter().map(|a| a[0].clone()).collect()
    }

    fn ran(&self, name: &str) -> bool {
        self.names().iter().any(|n| n == name)
    }

    fn index(&self, name: &str) -> usize {
        self.names().iter().position(|n| n == name).unwrap()
    }

    fn clear(&self) {
        self.cleared.set(self.fixture.calls().len());
    }

    fn first(&self, name: &str) -> Vec<String> {
        self.calls().into_iter().find(|a| a[0] == name).unwrap()
    }

    fn put_bytes(&self, name: &str, data: &[u8]) {
        let path = self.path(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, data).unwrap();
    }

    fn music(&self, files: &[(&str, &[u8])]) {
        for (name, data) in files {
            self.put_bytes(&format!("/data/music/{name}"), data);
        }
        self.fixture.mkdir("/media/internal");
    }

    /// Files on the stock storage (seen through its read-only mount).
    fn old(&self, files: &[(&str, &[u8])]) {
        for (name, data) in files {
            self.put_bytes(&format!("{OLD}/{name}"), data);
        }
    }

    fn mounted(&self) -> String {
        self.fixture.read("/proc/self/mountinfo")
    }

    fn parts(&self) -> bool {
        walk(&self.path(MUSIC)).iter().any(|w| {
            w.files
                .iter()
                .chain(&w.directories)
                .any(|n| n.ends_with(".part"))
        })
    }
}

fn pair(value: &Value, a: &str, b: &str) -> Value {
    json!([value[a], value[b]])
}

fn set_mtime(path: &Path, nanoseconds: u64) {
    let time = UNIX_EPOCH + Duration::from_nanos(nanoseconds);
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(FileTimes::new().set_modified(time))
        .unwrap();
}

fn mtime_ns(path: &Path) -> i128 {
    let meta = fs::metadata(path).unwrap();
    i128::from(meta.mtime()) * 1_000_000_000 + i128::from(meta.mtime_nsec())
}

/// A sparse file of `size` bytes (its blocks are never written).
fn sparse(path: &Path, size: u64) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    File::create(path).unwrap().set_len(size).unwrap();
}

// ---- identity -----------------------------------------------------------------------------------

#[test]
fn partition_needs_the_exact_geometry_on_the_internal_controller_only() {
    let player = Player::new();
    let name =
        |p: Option<PathBuf>| p.map(|p| p.file_name().unwrap().to_string_lossy().into_owned());
    assert_eq!(name(partition(player.root())).as_deref(), Some("mmcblk0p8"));
    let size = format!("{DISK}/mmcblk0p8/size");
    player.fixture.put(&size, "11460607");
    assert_eq!(partition(player.root()), None);
    player.fixture.put(&size, SIZE);
    // the same numbers on the removable-card controller are not Y2Media
    let sd = "/sys/devices/platform/11240000.mmc/block/mmcblk1/mmcblk1p9";
    for (key, value) in [("partition", "9"), ("start", START), ("size", SIZE)] {
        player.fixture.put(&format!("{sd}/{key}"), value);
    }
    symlink(player.path(sd), player.path("/sys/class/block/mmcblk1p9")).unwrap();
    assert_eq!(name(partition(player.root())).as_deref(), Some("mmcblk0p8"));
}

#[test]
fn mount_checks_then_mounts_only_the_exact_identity() {
    let player = Player::new();
    player.provisioned();
    let result = player.mount();
    assert_eq!(pair(&result, "state", "reason"), json!(["Ready", null]));
    assert!(player.index("e2fsck") < player.index("mount"));
    let mount = player.first("mount");
    assert_eq!(mount[..4], ["mount", "-t", "ext4", "-o"]);
    assert_eq!(mount[4], "rw,nosuid,nodev,noexec,noatime,noinit_itable");
    assert_eq!(mount[5..], ["/dev/mmcblk0p8", "/media/internal"]);
    assert_eq!(player.first("e2fsck")[..2], ["e2fsck", "-p"]);
    assert!(player.ready());
    // idempotent: an existing exact mount is recorded, not remounted
    player.clear();
    assert_eq!(player.mount()["state"], "Ready");
    assert!(!player.ran("mount"));
}

#[test]
fn every_card_format_mounts_noatime() {
    // Y2Media's half of test_exfat_readahead's pin (the SD card's is in media_tests).
    let player = Player::new();
    player.provisioned();
    player.mount();
    assert_eq!(
        player.first("mount")[4],
        "rw,nosuid,nodev,noexec,noatime,noinit_itable"
    );
}

#[test]
fn nothing_else_is_ever_mounted() {
    let player = Player::new();
    for fs in [
        stock(),
        vec![
            ("TYPE", "ext4".into()),
            ("LABEL", "Y2DATA".into()),
            ("UUID", UUID.into()),
        ],
        vec![
            ("TYPE", "ext4".into()),
            ("LABEL", LABEL.into()),
            ("UUID", "79324c69-6e75-4801-8000-000000000102".into()),
        ],
        vec![
            ("TYPE", "exfat".into()),
            ("LABEL", LABEL.into()),
            ("UUID", UUID.into()),
        ],
        vec![],
    ] {
        player.set(|f| f.fs = fs.clone());
        player.clear();
        let result = player.mount();
        assert_eq!(
            pair(&result, "state", "reason"),
            json!(["Unavailable", "filesystem_identity_not_y2media"]),
            "{fs:?}"
        );
        assert!(!player.ran("mount") && !player.ran("e2fsck"));
    }
    assert!(!player.ready());
}

#[test]
fn failed_check_or_foreign_mount_is_not_ready() {
    let player = Player::new();
    player.provisioned();
    player.set(|f| f.fsck_ok = false);
    assert_eq!(player.mount()["reason"], "filesystem_check_failed");
    assert!(!player.ran("mount"));
    player.set(|f| f.fsck_ok = true);
    player.fixture.put(
        "/proc/self/mountinfo",
        "9 1 179:7 / /media/internal rw - ext4 /dev/mmcblk0p7 rw",
    );
    assert_eq!(
        pair(&player.mount(), "state", "reason"),
        json!(["Failed", "foreign_mount_at_path"])
    );
    assert!(!player.ready());
}

#[test]
fn ready_follows_the_live_mount_and_this_boot() {
    let player = Player::new();
    player.provisioned();
    player.mount();
    assert!(player.ready());
    // remounted: new id
    player.fixture.put(
        "/proc/self/mountinfo",
        "10 1 179:8 / /media/internal rw - ext4 /dev/mmcblk0p8 rw",
    );
    assert!(!player.ready());
    player.fixture.put("/proc/self/mountinfo", MOUNT_LINE);
    assert!(player.ready());
    player
        .fixture
        .put("/proc/sys/kernel/random/boot_id", "boot-two");
    assert!(!player.ready());
    player
        .fixture
        .put("/proc/sys/kernel/random/boot_id", "boot-one");
    player.fixture.put("/proc/self/mountinfo", "");
    assert!(!player.ready());
}

#[test]
fn storage_reports_the_fourth_volume_and_rejects_the_wrong_uuid() {
    let player = Player::new();
    player.provisioned();
    player.fixture.put("/proc/self/mountinfo", MOUNT_LINE);
    player.fixture.mkdir("/media/internal");
    let volumes = snapshot::storage(player.root()).unwrap()["volumes"].clone();
    assert_eq!(volumes.as_array().unwrap().len(), 4);
    assert_eq!(volumes[3]["path"], "/media/internal");
    player.set(|f| f.everything = Some(answer(true, None, Some("something-else"))));
    let volumes = snapshot::storage(player.root()).unwrap()["volumes"].clone();
    assert_eq!(volumes[3]["reason"], "filesystem_identity_invalid");
}

// ---- provisioning -------------------------------------------------------------------------------

#[test]
fn provision_needs_the_stock_serial_and_replaces_nothing_else() {
    let player = Player::new();
    for (fs, confirm) in [
        (stock(), None),
        (stock(), Some("AAAA-BBBB")),
        (
            vec![
                ("TYPE", "ext4".into()),
                ("LABEL", "Y2DATA".into()),
                ("UUID", "x".into()),
            ],
            Some("x"),
        ),
        (
            vec![("TYPE", "ntfs".into()), ("UUID", "x".into())],
            Some("x"),
        ),
    ] {
        player.set(|f| f.fs = fs.clone());
        player.clear();
        let result = player.provision(confirm);
        assert_eq!(
            pair(&result, "state", "reason"),
            json!(["Failed", "confirmation_does_not_match_stock_filesystem"]),
            "{fs:?} {confirm:?}"
        );
        assert!(!player.ran("mkfs.ext4"));
    }
}

#[test]
fn a_mounted_foreign_filesystem_is_never_replaced() {
    let player = Player::new();
    player.fixture.put(
        "/proc/self/mountinfo",
        "9 1 179:8 / /mnt/other rw - vfat /dev/mmcblk0p8 rw",
    );
    let result = player.provision(Some(STOCK_SERIAL));
    assert_eq!(
        pair(&result, "state", "reason"),
        json!(["Failed", "partition_is_mounted"])
    );
    assert!(!player.ran("mkfs.ext4"));
}

#[test]
fn provision_formats_the_confirmed_stock_fat_mounts_and_creates_the_music_folder() {
    let player = Player::new();
    player.fixture.mkdir("/media/internal");
    let result = Value::Object(provision(player.root(), &json!(STOCK_SERIAL), false).unwrap());
    assert_eq!(pair(&result, "state", "reason"), json!(["Ready", null]));
    let mkfs = player.first("mkfs.ext4");
    let after = |flag: &str| mkfs[mkfs.iter().position(|a| a == flag).unwrap() + 1].clone();
    assert_eq!(mkfs.last().unwrap(), "/dev/mmcblk0p8");
    assert_eq!(after("-L"), "Y2MEDIA");
    assert_eq!(after("-U"), UUID);
    assert_eq!(after("-m"), "0");
    assert!(player.index("mkfs.ext4") < player.index("mount"));
    assert!(player.path("/media/internal/music").is_dir());
    assert_eq!(
        player.names().iter().filter(|n| *n == "mkfs.ext4").count(),
        1
    );
}

#[test]
fn a_failed_mkfs_leaves_the_state_failed_and_mounts_nothing() {
    let player = Player::new();
    player.set(|f| f.mkfs_ok = false);
    let result = player.provision(Some(STOCK_SERIAL));
    assert_eq!(result["state"], "Failed");
    assert!(result["reason"]
        .as_str()
        .unwrap()
        .starts_with("mkfs_failed"));
    assert!(!player.ran("mount"));
}

#[test]
fn provision_moves_the_library_with_hashes_and_removes_only_verified_originals() {
    let player = Player::new();
    let files: [(&str, Vec<u8>); 3] = [
        ("a.flac", vec![b'A'; 3000]),
        ("album/b.mp3", vec![b'B'; 70000]),
        ("album/disc2/c.ogg", vec![b'C'; 11]),
    ];
    let listed: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (*n, d.as_slice())).collect();
    player.music(&listed);
    set_mtime(
        &player.path("/data/music/a.flac"),
        1_700_000_000_000_000_000,
    );
    let result = player.provision(Some(STOCK_SERIAL));
    assert_eq!(result["state"], "Ready");
    assert_eq!(
        result["migration"],
        json!({"files": 3, "bytes": 73011, "copied": 3, "removed": 3})
    );
    for (name, data) in &files {
        assert_eq!(
            &fs::read(player.path(&format!("/media/internal/music/{name}"))).unwrap(),
            data
        );
        assert!(!player.fixture.exists(&format!("/data/music/{name}")));
    }
    assert_eq!(
        mtime_ns(&player.path("/media/internal/music/a.flac")),
        1_700_000_000_000_000_000
    );
    assert!(player.path("/data/music").is_dir()); // the folder itself stays
    assert!(!player.parts());
}

#[test]
fn any_migration_problem_keeps_every_original() {
    let player = Player::new();
    player.music(&[("a.flac", &[b'A'; 100]), ("b.flac", &[b'B'; 100])]);
    let confirm = json!(STOCK_SERIAL);
    let result = Value::Object(provision_with(player.root(), &confirm, true, mismatching).unwrap());
    assert_eq!(result["state"], "Degraded");
    assert!(result["reason"]
        .as_str()
        .unwrap()
        .contains("migration_hash_mismatch"));
    for name in ["a.flac", "b.flac"] {
        assert!(player.fixture.exists(&format!("/data/music/{name}")));
    }
    assert!(!player.parts());
}

#[test]
fn symlinks_special_files_and_oversized_libraries_are_refused_untouched() {
    let player = Player::new();
    player.music(&[("a.flac", &[b'A'; 100])]);
    player.fixture.mkdir("/data/elsewhere");
    symlink(
        player.path("/data/elsewhere"),
        player.path("/data/music/link"),
    )
    .unwrap();
    let result = player.provision(Some(STOCK_SERIAL));
    assert_eq!(result["state"], "Degraded");
    assert!(result["reason"]
        .as_str()
        .unwrap()
        .contains("migration_refuses_symlink"));
    assert!(player.fixture.exists("/data/music/a.flac"));
    fs::remove_file(player.path("/data/music/link")).unwrap();
    player.provisioned();
    // A library larger than the free space: a sparse file.
    let free = statvfs(&player.path(PATH)).unwrap().available();
    sparse(&player.path("/data/music/huge.flac"), free * 2 + (1 << 30));
    let result = player.provision(None);
    assert!(result["reason"]
        .as_str()
        .unwrap()
        .contains("migration_does_not_fit"));
    assert!(player.fixture.exists("/data/music/a.flac"));
}

#[test]
fn rerun_after_success_is_idempotent_and_resumes_a_partial_copy() {
    let player = Player::new();
    player.music(&[("a.flac", &[b'A'; 100])]);
    assert_eq!(player.provision(Some(STOCK_SERIAL))["state"], "Ready");
    player.clear();
    // already Y2Media: no confirmation, no mkfs
    assert_eq!(player.provision(None)["state"], "Ready");
    assert!(!player.ran("mkfs.ext4"));
    // a previously copied but not yet removed file is recognised by hash, not duplicated
    player.fixture.put("/data/music/b.flac", &"B".repeat(50));
    player
        .fixture
        .put("/media/internal/music/b.flac", &"B".repeat(50));
    let result = player.provision(None);
    assert_eq!(result["migration"]["copied"], 0);
    assert_eq!(result["migration"]["removed"], 1);
    // but a different file at the same name is never overwritten
    player.fixture.put("/data/music/c.flac", "one");
    player.fixture.put("/media/internal/music/c.flac", "two!");
    let result = player.provision(None);
    assert!(result["reason"]
        .as_str()
        .unwrap()
        .contains("migration_target_differs"));
    assert!(player.fixture.exists("/data/music/c.flac"));
}

#[test]
fn unmount_and_status() {
    let player = Player::new();
    player.provisioned();
    player.mount();
    assert_eq!(player.status()["ready"], true);
    assert_eq!(player.unmount()["reason"], "unmounted");
    let status = player.status();
    assert_eq!(status["ready"], false);
    assert_eq!(status["identity_valid"], true);
    assert_eq!(player.unmount()["reason"], "not_mounted");
}

// ---- the automatic setup (Setup) ----------------------------------------------------------------

#[test]
fn stock_leftovers_only_set_up_at_once_with_the_pinned_layout() {
    let player = Player::on_battery();
    player.old(&[
        (
            "Android/data/com.android.providers.media/albumthumbs/1",
            &[b'x'; 10],
        ),
        ("DCIM/.thumbnails/t.jpg", b"t"),
        (".android_secure/x", b""),
        ("Music/.nomedia", b""),
    ]);
    player.fixture.mkdir(&format!("{OLD}/Ringtones"));
    let result = player.automatic();
    assert_eq!(pair(&result, "state", "kept"), json!(["Done", 0]));
    let mkfs = player.first("mkfs.ext4");
    assert_eq!(mkfs, mkfs_argv("/dev/mmcblk0p8"));
    let features = &mkfs[mkfs.iter().position(|a| a == "-O").unwrap() + 1];
    assert_eq!(
        features.split(',').collect::<Vec<_>>(),
        [
            "none",
            "has_journal",
            "ext_attr",
            "resize_inode",
            "dir_index",
            "orphan_file",
            "filetype",
            "extent",
            "64bit",
            "flex_bg",
            "metadata_csum_seed",
            "sparse_super",
            "large_file",
            "huge_file",
            "dir_nlink",
            "extra_isize",
            "metadata_csum"
        ]
    );
    assert!(player.ready());
    assert!(!player.fixture.exists(SETUP));
    assert!(!player.fixture.exists(&format!("{MUSIC}/{KEPT}")));
    // The start screen showed step 0 first (before the old storage was read) and 100 last.
    let phases = player.fake.lock().unwrap().phases.clone();
    assert_eq!(phases.first().map(String::as_str), Some("media_setup\n"));
    assert_eq!(
        player.fixture.read("/run/reborn-splash/phase").trim(),
        "media_setup_100"
    );
    // Next start: nothing to do.
    player.clear();
    assert_eq!(player.automatic()["state"], "NotNeeded");
    assert!(!player.ran("mkfs.ext4"));
}

/// `im.MKFS + [device]`.
fn mkfs_argv(device: &str) -> Vec<String> {
    MKFS[..21]
        .iter()
        .chain([&device])
        .map(|a| a.to_string())
        .collect()
}

#[test]
fn a_few_files_are_kept_in_old_player_files_with_hashes_and_times() {
    let player = Player::on_battery();
    player.old(&[
        ("Themes/neon/skin.json", b"{\"a\":1}"),
        ("Music/Song.mp3", &[b'M'; 5000]),
        (
            "Android/data/com.android.providers.media/albumthumbs/9",
            b"cache",
        ),
    ]);
    set_mtime(
        &player.path(&format!("{OLD}/Music/Song.mp3")),
        1_600_000_000_000_000_000,
    );
    player.music(&[("legacy.flac", &[b'L'; 10])]);
    let result = player.automatic();
    assert_eq!(pair(&result, "state", "kept"), json!(["Done", 2]));
    let kept = player.path(&format!("{MUSIC}/{KEPT}"));
    assert_eq!(
        fs::read(kept.join("Themes/neon/skin.json")).unwrap(),
        b"{\"a\":1}"
    );
    assert_eq!(fs::read(kept.join("Music/Song.mp3")).unwrap(), [b'M'; 5000]);
    assert_eq!(
        mtime_ns(&kept.join("Music/Song.mp3")),
        1_600_000_000_000_000_000
    );
    assert!(!kept.join("Android").exists());
    assert_eq!(
        fs::read(player.path("/media/internal/music/legacy.flac")).unwrap(),
        [b'L'; 10]
    );
    assert!(!player.fixture.exists(KEPT_STAGING));
    assert!(!player.fixture.exists(SETUP));
    assert!(player.index("fsck.fat") < player.index("mkfs.ext4"));
    assert_eq!(
        Value::Object(setup_status(player.root())),
        json!({"state": "Done", "reason": null, "kept": 2})
    );
}

#[test]
fn a_large_library_waits_for_the_listener_and_nothing_is_written() {
    let player = Player::on_battery();
    // More than KEEP_LIMIT: sparse files.
    sparse(&player.path(&format!("{OLD}/Music/a.flac")), 200_000_000);
    sparse(&player.path(&format!("{OLD}/Music/b.flac")), 300_000_000);
    let result = player.automatic();
    assert_eq!(
        json!([
            result["state"],
            result["reason"],
            result["files"],
            result["bytes"],
            result["serial"]
        ]),
        json!([
            "Waiting",
            "old_storage_has_files",
            2,
            500_000_000,
            STOCK_SERIAL
        ])
    );
    assert!(!player.ran("mkfs.ext4"));
    assert!(!player.fixture.exists(SETUP));
    assert!(player.mounted().contains(OLD_LINE)); // Old Storage stays readable
    assert!(player.first("mount")[4].starts_with("ro,"));
    // Set Up Now: only with the old storage's serial; then it is erased, nothing kept.
    assert_eq!(
        player.set_up(false, Some("AAAA-BBBB"))["reason"],
        "confirmation_does_not_match_stock_filesystem"
    );
    assert!(!player.ran("mkfs.ext4"));
    let result = player.set_up(false, Some(STOCK_SERIAL));
    assert_eq!(pair(&result, "state", "kept"), json!(["Done", 0]));
    assert!(!player.mounted().contains(OLD_LINE));
}

#[test]
fn y2data_must_keep_its_reserve() {
    // A fixture's /data is a real filesystem, so the decision is checked at its edge: a few files
    // are kept only with the reserve left over.
    let kept = [("Music/a.flac".to_string(), 2000)];
    assert_eq!(decide(&kept, DATA_RESERVE + 1999), 3);
    assert_eq!(decide(&kept, DATA_RESERVE + 2000), 2);
}

#[test]
fn an_unreadable_old_storage_is_never_set_up_automatically() {
    let player = Player::on_battery();
    player.set(|f| f.fat_check = command_failed(Some("/Music/x.mp3\n  File size is 9 bytes")));
    let result = player.automatic();
    assert_eq!(
        pair(&result, "state", "reason"),
        json!(["Waiting", "old_storage_check_failed"])
    );
    assert!(!player.ran("mkfs.ext4"));
    // A card pulled once (dirty bit only) is fine (#44 rules).
    player.set(|f| {
        f.fat_check = command_failed(Some(
            "Dirty bit is set. Fs was not properly unmounted and some data may be corrupt.\n \
             Automatically removing dirty bit.\nLeaving filesystem unchanged.",
        ))
    });
    assert_eq!(player.automatic()["state"], "Done");
}

#[test]
fn it_waits_for_a_confirmed_update_charge_or_enough_battery() {
    let player = Player::on_battery();
    let fixture = &player.fixture;
    let battery = "/sys/class/power_supply/BAT0";
    fixture.put_json(
        "/data/updates/state.json",
        &json!({"schema": 2, "state": "PendingHealth"}),
    );
    assert_eq!(player.automatic()["reason"], "update_in_progress");
    fixture.remove("/data/updates/state.json");
    fixture.put(&format!("{battery}/voltage_now"), "3600000");
    assert_eq!(player.automatic()["reason"], "battery_low");
    // Report 20261019-153610: power-on OCV 3.621 V is 24.8 % (shown 21 %): still too low.
    fixture.put(&format!("{battery}/voltage_ocv"), "3621000");
    assert_eq!(player.automatic()["reason"], "battery_low");
    fixture.remove(&format!("{battery}/voltage_now"));
    assert_eq!(player.automatic()["reason"], "battery_low");
    fixture.remove(&format!("{battery}/voltage_ocv"));
    assert_eq!(player.automatic()["reason"], "battery_unknown");
    fixture.put(&format!("{battery}/voltage_now"), "3995068");
    fixture.remove("/etc/y2linux/battery-profile.json");
    assert_eq!(player.automatic()["reason"], "battery_unknown");
    assert!(!["mkfs.ext4", "fsck.fat", "mount"]
        .iter()
        .any(|n| player.ran(n)));
    fixture.put(&format!("{battery}/status"), "Charging");
    assert_eq!(player.automatic()["state"], "Done");
}

#[test]
fn battery_is_judged_by_the_power_on_ocv_not_the_boot_load() {
    // 25 % at rest (3.622 V) sags below 3.6 V under the boot load; the shown percentage
    // comes from the power-on OCV, and so does the gate.
    let player = Player::on_battery();
    player
        .fixture
        .put("/sys/class/power_supply/BAT0/voltage_now", "3560000");
    player
        .fixture
        .put("/sys/class/power_supply/BAT0/voltage_ocv", "3630000");
    assert_eq!(player.automatic()["state"], "Done");
}

#[test]
fn anything_but_the_stock_fat_is_left_alone() {
    let player = Player::on_battery();
    for fs in [
        vec![
            ("TYPE", "ext4".into()),
            ("LABEL", "Y2DATA".into()),
            ("UUID", "x".into()),
        ],
        vec![],
    ] {
        player.set(|f| f.fs = fs.clone());
        player.clear();
        assert_eq!(player.automatic()["reason"], "not_stock_storage", "{fs:?}");
        assert!(!player.ran("mkfs.ext4"));
    }
}

impl Player {
    /// A setup record at `phase` (power lost there), files kept on Y2DATA meanwhile, p8 as `fs`.
    fn resume_from(
        &self,
        phase: &str,
        fs: Vec<(&'static str, String)>,
        staged: &[(&str, &[u8])],
    ) -> Value {
        let bytes: usize = staged.iter().map(|(_, d)| d.len()).sum();
        let record = json!({"schema": 1, "device": "/dev/mmcblk0p8", "start": START, "size": SIZE,
                            "serial": STOCK_SERIAL, "phase": phase, "files": staged.len(), "bytes": bytes});
        save(self.root(), record.as_object().unwrap()).unwrap();
        for (name, data) in staged {
            self.put_bytes(&format!("{KEPT_STAGING}/{name}"), data);
        }
        self.set(|f| f.fs = fs);
        self.automatic()
    }
}

#[test]
fn power_lost_while_formatting_formats_again_only_with_the_record() {
    let player = Player::on_battery();
    // p8 unreadable now
    let result = player.resume_from("formatting", vec![], &[("Music/a.mp3", &[b'A'; 10])]);
    assert_eq!(pair(&result, "state", "kept"), json!(["Done", 1]));
    assert_eq!(
        fs::read(player.path(&format!("{MUSIC}/{KEPT}/Music/a.mp3"))).unwrap(),
        [b'A'; 10]
    );
    // Without a record an unreadable p8 is only reported.
    player.clear();
    player.set(|f| f.fs = vec![]);
    assert_eq!(player.automatic()["reason"], "not_stock_storage");
    assert!(!player.ran("mkfs.ext4"));
}

#[test]
fn power_lost_after_formatting_or_while_moving_resumes_without_formatting() {
    let player = Player::on_battery();
    let result = player.resume_from("formatting", y2media(), &[("a.mp3", b"A")]);
    assert_eq!(result["state"], "Done");
    assert!(!player.ran("mkfs.ext4"));
    player.fixture.put("/proc/self/mountinfo", "");
    let result = player.resume_from("migrating", y2media(), &[("b.mp3", b"B")]);
    assert_eq!(result["state"], "Done");
    assert!(!player.ran("mkfs.ext4"));
    assert!(player.fixture.exists(&format!("{MUSIC}/{KEPT}/b.mp3")));
}

#[test]
fn power_lost_while_copying_redoes_the_copy_from_the_unchanged_old_storage() {
    let player = Player::on_battery();
    player.old(&[("Music/a.mp3", &[b'A'; 100])]);
    player.put_bytes(&format!("{KEPT_STAGING}/Music/a.mp3.part"), b"AA"); // interrupted copy
    let result = player.resume_from("staging", stock(), &[]);
    assert_eq!(result["state"], "Done");
    assert_eq!(
        fs::read(player.path(&format!("{MUSIC}/{KEPT}/Music/a.mp3"))).unwrap(),
        [b'A'; 100]
    );
    assert!(!player.parts());
}

#[test]
fn a_record_for_another_partition_or_a_changed_old_storage_stops() {
    let player = Player::on_battery();
    let record = json!({"schema": 1, "device": "/dev/mmcblk0p9", "start": START, "size": SIZE,
                        "serial": STOCK_SERIAL, "phase": "formatting"});
    save(player.root(), record.as_object().unwrap()).unwrap();
    assert_eq!(player.automatic()["reason"], "setup_record_mismatch");
    let changed = vec![("TYPE", "vfat".into()), ("UUID", "AAAA-BBBB".into())];
    assert_eq!(
        player.resume_from("formatting", changed, &[])["reason"],
        "old_storage_changed"
    );
    fs::write(player.path(SETUP), "{broken").unwrap();
    assert_eq!(player.automatic()["reason"], "setup_record_unreadable");
    assert!(!player.ran("mkfs.ext4"));
}

#[test]
fn leftover_rules() {
    for path in [
        "Android/data/com.android.providers.media/albumthumbs/1",
        "DCIM/.thumbnails/a.jpg",
        ".android_secure/x",
        "Music/.nomedia",
        "System Volume Information/IndexerVolumeGuid",
        "Music/._Song.mp3",
        ".DS_Store",
        "Pictures/Thumbs.db",
    ] {
        assert!(leftover(path), "{path}");
    }
    for path in [
        "Music/Song.mp3",
        "LOST.DIR/0001",
        "Themes/x/skin.json",
        "Android/data/com.app/files/a",
        "Android/data/com.android.providers.media/other",
    ] {
        assert!(!leftover(path), "{path}");
    }
    assert_eq!(
        [
            decide(&[], 0),
            decide(&[("a".into(), 10)], 1_000_000_000),
            decide(&[("a".into(), 1_000_000_000)], 10_000_000_000)
        ],
        [1, 2, 3]
    );
}

// ---- the real mkfs (test_filesystems.py) --------------------------------------------------------

fn tool(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|d| {
            d.join(name)
                .metadata()
                .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
        })
    })
}

#[test]
fn y2media_layout_is_pinned_to_the_first_players() {
    // STORAGE-SETUP: every player's Y2Media equals the owner's (made 2026-10-10), whatever
    // e2fsprogs defaults are: features, inodes, blocks, reserve, label and UUID.
    if !tool("mkfs.ext4") || !tool("dumpe2fs") {
        eprintln!("skipped: native ext4 tools required");
        return;
    }
    let directory = Fixture::new();
    let image = directory.path("/p8.img");
    sparse(&image, SIZE.parse::<u64>().unwrap() * 512); // p8's exact size
    let image = image.to_str().unwrap();
    let run = |argv: &[&str]| {
        Command::new(argv[0])
            .args(&argv[1..])
            .env("LC_ALL", "C.UTF-8")
            .output()
            .unwrap()
    };
    let made = run(&mkfs(image));
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    let header = String::from_utf8(run(&["dumpe2fs", "-h", image]).stdout).unwrap();
    let field = |name: &str| {
        header
            .lines()
            .filter_map(|l| l.split_once(':'))
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.trim().to_owned())
            .unwrap()
    };
    assert_eq!(
        field("Filesystem features")
            .split_whitespace()
            .collect::<Vec<_>>(),
        [
            "has_journal",
            "ext_attr",
            "resize_inode",
            "dir_index",
            "orphan_file",
            "filetype",
            "extent",
            "64bit",
            "flex_bg",
            "metadata_csum_seed",
            "sparse_super",
            "large_file",
            "huge_file",
            "dir_nlink",
            "extra_isize",
            "metadata_csum"
        ]
    );
    assert_eq!(
        [
            field("Inode count"),
            field("Block count"),
            field("Reserved block count"),
            field("Block size"),
            field("Inode size")
        ],
        ["358336", "1432576", "0", "4096", "256"]
    );
    assert_eq!(
        [field("Filesystem volume name"), field("Filesystem UUID")],
        [LABEL, UUID]
    );
}
