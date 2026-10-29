//! The SD lifecycle on a fixture card (from tests/test_platform_storage_tools.py, test_media_change.py
//! and test_exfat_readahead.py): the card's root answers commands with a fake whose state follows
//! them (a `mount` writes its mountinfo line, blkid describes the card) and reads a manual clock.
// SPDX-License-Identifier: GPL-2.0-only
use super::{change::*, check::*, repair::*, *};
use crate::commands::storage::{observe::MEDIA_STATE, sys::statvfs};
use crate::commands::{observation::snapshot, reports::media::can_write};
use crate::testing::{Fixture, Outcome};
use std::{
    cell::Cell,
    fs::{File, FileTimes},
    io::{self, Read},
    os::unix::fs::symlink,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard,
    },
    thread,
    time::UNIX_EPOCH,
    time::{Duration, Instant},
};
use y2p_core::storage_media::{fat_benign_only, fat_findings, fat_other_shapes};
use y2p_core::{clock::Manual, command::Runner};

// ---- the fixture card --------------------------------------------------------------------------

const DISK: &str = "/sys/devices/platform/11240000.mmc/block/mmcblk1";
const CARD: &str = "/sys/devices/platform/11240000.mmc/block/mmcblk1/mmcblk1p1";
const CID: &str = "card-cid-one";
const SECTORS: u64 = 131_072; // a 64 MiB test card

fn answer(ok: bool, reason: Option<&str>, output: Option<&str>) -> Outcome {
    Outcome {
        ok,
        reason: reason.map(Into::into),
        output: output.map(Into::into),
    }
}

fn done(output: &str) -> Outcome {
    answer(true, None, Some(output))
}

fn command_failed(output: Option<&str>) -> Outcome {
    answer(false, Some("command_failed"), output)
}

type Special = Box<dyn FnMut(&[&str], f64) -> Option<Outcome> + Send>;

/// card_fixture's runner (MediaChange's with `changes`): blkid describes the card, `mount` writes its
/// line (rw when the options say so), umount fails; `special` answers first when it has an answer.
struct Fake {
    mountinfo: PathBuf,
    filesystem: &'static str,
    /// MediaChange: umount succeeds and clears the mount, every mount is rw.
    changes: bool,
    special: Option<Special>,
}

impl Fake {
    fn answer(&mut self, argv: &[&str], timeout: f64) -> Outcome {
        if let Some(special) = &mut self.special {
            if let Some(outcome) = special(argv, timeout) {
                return outcome;
            }
        }
        match argv[0] {
            "blkid" if argv.contains(&"UUID") => done("card-uuid-one"),
            "blkid" => done(&format!("TYPE={}\nUUID=card-uuid-one", self.filesystem)),
            "mount" => {
                let options = argv[argv.iter().position(|a| *a == "-o").unwrap() + 1];
                let rw = self.changes || options.split(',').any(|o| o == "rw");
                let access = if rw { "rw" } else { "ro" };
                let line = format!(
                    "51 1 179:9 / /media/sd {access} - {} /dev/mmcblk1p1 {access}",
                    self.filesystem
                );
                fs::write(&self.mountinfo, line).unwrap();
                done("")
            }
            "umount" if self.changes => {
                fs::write(&self.mountinfo, "").unwrap();
                done("")
            }
            "umount" => command_failed(None),
            "mkfs.ext4" => {
                self.filesystem = "ext4";
                done("")
            }
            _ => done(""),
        }
    }
}

/// The card's runner: the fake answers, and every request is recorded with whether a failed run
/// keeps its output.
struct Answering {
    fake: Arc<Mutex<Fake>>,
    requests: Mutex<Vec<(Vec<String>, f64, bool)>>,
}

impl Runner for Answering {
    fn run(&self, argv: &[&str], timeout: f64, _limit: usize, failure_output: bool) -> Outcome {
        let request = argv.iter().map(|a| a.to_string()).collect();
        self.requests
            .lock()
            .unwrap()
            .push((request, timeout, failure_output));
        self.fake.lock().unwrap().answer(argv, timeout)
    }

    fn calls(&self) -> Vec<(Vec<String>, f64)> {
        let requests = self.requests.lock().unwrap();
        requests.iter().map(|(a, t, _)| (a.clone(), *t)).collect()
    }
}

struct Card {
    fixture: Fixture,
    fake: Arc<Mutex<Fake>>,
    runner: Arc<Answering>,
    clock: Arc<Manual>,
    /// The commands before this many were cleared.
    cleared: Cell<usize>,
    /// Partition-table re-reads format asked for, and whether they fail (EBUSY).
    rereads: Cell<usize>,
    reread_fails: Cell<bool>,
}

impl Card {
    /// StorageTools.setUp + card_fixture: one card partition on the SD controller, not mounted.
    fn new(filesystem: &'static str) -> Card {
        let fixture = Fixture::new();
        fixture.mkdir("/data");
        fixture.put("/proc/sys/kernel/random/boot_id", "boot-one");
        fixture.put(&format!("{CARD}/partition"), "1");
        fixture.put(&format!("{CARD}/dev"), "179:9");
        fixture.put(&format!("{DISK}/diskseq"), "17");
        fixture.put(&format!("{DISK}/device/cid"), CID);
        for link in ["/sys/class/block/mmcblk1p1", "/sys/dev/block/179:9"] {
            fixture.mkdir(Path::new(link).parent().unwrap().to_str().unwrap());
            symlink(fixture.path(CARD), fixture.path(link)).unwrap();
        }
        fixture.put("/proc/self/mountinfo", "");
        fixture.mkdir("/media/sd");
        let fake = Arc::new(Mutex::new(Fake {
            mountinfo: fixture.path("/proc/self/mountinfo"),
            filesystem,
            changes: false,
            special: None,
        }));
        let clock = Arc::new(Manual::at(1_000_000_000_000, 1_000_000_000_000));
        let runner = Arc::new(Answering {
            fake: Arc::clone(&fake),
            requests: Mutex::default(),
        });
        let mut fixture = fixture.with_clock(clock.clone());
        fixture.root = fixture.root.clone().with_runner(runner.clone());
        Card {
            fixture,
            fake,
            runner,
            clock,
            cleared: Cell::new(0),
            rereads: Cell::new(0),
            reread_fails: Cell::new(false),
        }
    }

    /// fat_card_checked_as: a FAT card whose `fsck.fat` fails with this output.
    fn fat_checked_as(output: impl Into<String>) -> Card {
        let card = Card::new("vfat");
        let output = output.into();
        card.special(move |argv, _| (argv[0] == "fsck.fat").then(|| command_failed(Some(&output))));
        card
    }

    /// MediaChange.setUp: the whole 64 MiB card is a file full of old contents.
    fn change() -> Card {
        let card = Card::new("exfat");
        card.fake().changes = true;
        card.fixture
            .put(&format!("{DISK}/size"), &SECTORS.to_string());
        card.fixture.put(&format!("{DISK}/dev"), "179:8");
        symlink(card.path(DISK), card.path("/sys/class/block/mmcblk1")).unwrap();
        card.fixture.mkdir("/dev");
        fs::write(
            card.path("/dev/mmcblk1"),
            vec![0xa5; SECTORS as usize * 512],
        )
        .unwrap();
        fs::write(card.path("/dev/mmcblk1p1"), b"").unwrap(); // the node the kernel creates on re-read
        card
    }

    fn root(&self) -> &Root {
        &self.fixture.root
    }

    fn path(&self, name: &str) -> PathBuf {
        self.fixture.path(name)
    }

    fn fake(&self) -> MutexGuard<'_, Fake> {
        self.fake.lock().unwrap()
    }

    fn special(&self, special: impl FnMut(&[&str], f64) -> Option<Outcome> + Send + 'static) {
        self.fake().special = Some(Box::new(special));
    }

    fn operation(&self, action: &str) -> Value {
        Value::Object(operation(self.root(), action).unwrap())
    }

    /// The partition-table re-read format asks for, counted.
    fn reread(&self, _card: &File) -> io::Result<()> {
        self.rereads.set(self.rereads.get() + 1);
        if self.reread_fails.get() {
            return Err(io::Error::from_raw_os_error(libc::EBUSY));
        }
        Ok(())
    }

    fn change_card(&self, action: &str, cid: Option<&str>) -> Value {
        let reread = |card: &File| self.reread(card);
        Value::Object(change_card_with(self.root(), action, cid, &reread).unwrap())
    }

    fn verify(&self) -> Value {
        Value::Object(verify(self.root()).unwrap())
    }

    fn window(&self, holder: &str, seconds: i64, release: bool) -> Value {
        Value::Object(write_window(self.root(), holder, seconds, release).unwrap())
    }

    fn storage(&self) -> Value {
        snapshot::storage(self.root()).unwrap()
    }

    /// Whether each run of `program` keeps its output when it fails.
    fn kept_output(&self, program: &str) -> Vec<bool> {
        let requests = self.runner.requests.lock().unwrap();
        let runs = requests.iter().filter(|(a, _, _)| a[0] == program);
        runs.map(|(_, _, kept)| *kept).collect()
    }

    /// The commands since the last `clear` (argv, timeout).
    fn calls(&self) -> Vec<(Vec<String>, f64)> {
        self.fixture.calls().split_off(self.cleared.get())
    }

    /// `storage(ctx)['volumes'][2]`.
    fn sd(&self) -> Value {
        self.storage()["volumes"][2].clone()
    }

    fn argvs(&self) -> Vec<Vec<String>> {
        self.calls().into_iter().map(|(a, _)| a).collect()
    }

    fn names(&self) -> Vec<String> {
        self.argvs().into_iter().map(|a| a[0].clone()).collect()
    }

    fn called(&self, argv: &[&str]) -> bool {
        self.argvs().iter().any(|a| a == argv)
    }

    fn mount_options(&self) -> Vec<String> {
        self.argvs()
            .iter()
            .filter(|a| a[0] == "mount")
            .map(|a| a[a.iter().position(|o| o == "-o").unwrap() + 1].clone())
            .collect()
    }

    /// Never a repairing checker run during admission.
    fn repaired(&self) -> bool {
        self.argvs().iter().any(|a| {
            a[0].starts_with("fsck.") && a.iter().any(|o| ["-a", "-w", "-r", "-y"].contains(&&**o))
        })
    }

    fn clear(&self) {
        self.cleared.set(self.fixture.calls().len());
    }

    fn unmounted(&self) {
        self.fixture.put("/proc/self/mountinfo", "");
    }

    fn index(&self, name: &str) -> usize {
        self.names().iter().position(|n| n == name).unwrap()
    }
}

fn pair(value: &Value, a: &str, b: &str) -> Value {
    json!([value[a], value[b]])
}

// fsck.fat 4.2 -n (LC_ALL=C) on host images with each change (dosfstools as on the player).
const PULLED_FAT: &str = "fsck.fat 4.2 (2021-01-31)\n\
There are differences between boot sector and its backup.\n\
This is mostly harmless. Differences: (offset:original/backup)\n  \
65:01/00\n  \
Not automatically fixing this.\n\
Dirty bit is set. Fs was not properly unmounted and some data may be corrupt.\n \
Automatically removing dirty bit.\n\
Leaving filesystem unchanged.\n\
/dev/mmcblk1p1: 13383 files, 1397005/1907917 clusters";
const STALE_FREE: &str = "fsck.fat 4.2 (2021-01-31)\n\
Free cluster summary wrong (1234 vs. really 76393)\n  \
Auto-correcting.\n\n\
Leaving filesystem unchanged.\n\
/dev/mmcblk1p1: 10 files, 250/76643 clusters\n";
const LABEL: &str =
    "Volume label 'MUSIC' stored in root directory and label 'Y2' stored in boot sector \
and different.\n  Auto-copying volume label from root directory to boot sector.\n";
const LOST: &str = "Reclaimed 4 unused clusters (16384 bytes).\n";
// What an interrupted write leaves that Linux does see (a chain shorter than the size).
const CHAIN_SHORTER: &str = "/Music/Song 0.mp3\n  File size is 300000 bytes, cluster chain length \
is 16384 bytes.\n  Truncating file to 16384 bytes.\n";

// ---- identity -----------------------------------------------------------------------------------

#[test]
fn missing_diskseq_cannot_validate_a_null_mount_claim() {
    let card = Card::new("exfat");
    card.operation("mount");
    fs::remove_file(card.path(&format!("{DISK}/diskseq"))).unwrap();
    assert_eq!(card.sd()["reason"], "block_instance_changed");
}

#[test]
fn exfat_mount_claim_failed_eject_and_card_swap() {
    let card = Card::new("exfat");
    assert_eq!(card.operation("mount")["state"], "Ready");
    assert_eq!(card.sd()["state"], "Ready");
    assert_eq!(card.operation("unmount")["state"], "Failed");
    assert_eq!(card.fixture.json(MEDIA_MOUNT)["mount_id"], 51);
    assert_eq!(
        card.operation("mount")["reason"],
        "mount_requires_unmounted_path"
    );
    assert_eq!(card.names().iter().filter(|n| *n == "mount").count(), 1);
    card.special(|_, _| Some(done("new-card-uuid")));
    assert_eq!(card.sd()["reason"], "mount_claim_changed_or_missing");
}

#[test]
fn uuidless_fat_identity_needs_geometry_and_detects_replacement() {
    let card = Card::new("exfat");
    card.fixture.put("/sys/class/block/mmcblk1p1/start", "2048");
    card.fixture
        .put("/sys/class/block/mmcblk1p1/size", "1000000");
    card.special(|argv, _| {
        (argv[0] == "blkid").then(|| {
            done(if argv.contains(&"UUID") {
                ""
            } else {
                "TYPE=exfat"
            })
        })
    });
    assert_eq!(card.operation("mount")["state"], "Ready");
    assert_eq!(card.sd()["state"], "Ready");
    card.fixture
        .put("/sys/class/block/mmcblk1p1/size", "999999");
    assert_eq!(card.sd()["reason"], "media_identity_changed_or_missing");
}

#[test]
fn block_identity_survives_metadata_changes_and_rejects_reenumeration() {
    let card = Card::new("exfat");
    let root = card.root();
    let entry = fs::canonicalize(card.path("/sys/dev/block/179:9")).unwrap();
    let expected = device_instance(root, "179:9");
    assert_eq!(expected.as_deref(), Some("block:179:9:diskseq:17"));
    assert_eq!(snapshot::device_instance(root, "179:9"), expected);
    let old = UNIX_EPOCH + Duration::from_secs(1);
    File::open(&entry)
        .unwrap()
        .set_times(FileTimes::new().set_accessed(old).set_modified(old))
        .unwrap();
    assert_eq!(device_instance(root, "179:9"), expected);
    let diskseq = entry.parent().unwrap().join("diskseq");
    fs::write(&diskseq, "18").unwrap();
    assert_ne!(device_instance(root, "179:9"), expected);
    fs::remove_file(&diskseq).unwrap();
    assert_eq!(device_instance(root, "179:9"), None);
    assert_eq!(snapshot::device_instance(root, "179:9"), None);
    fs::write(&diskseq, "0").unwrap();
    assert_eq!(device_instance(root, "179:9"), None);
    assert_eq!(snapshot::device_instance(root, "179:9"), None);
}

// ---- admission ----------------------------------------------------------------------------------

#[test]
fn fat_mount_enables_utf8_without_changing_private_mount_masks() {
    let card = Card::new("vfat");
    assert_eq!(card.operation("mount")["state"], "Ready");
    // A healthy card is protected: read-only until something writes (media.write_window).
    assert_eq!(
        card.mount_options(),
        ["ro,nosuid,nodev,noexec,noatime,fmask=0177,dmask=0077,utf8=1"]
    );
}

#[test]
fn every_card_format_mounts_noatime() {
    // Read-only between writes since 8.27 (protected card), noatime either way.
    for (filesystem, masks) in [
        ("exfat", ",fmask=0177,dmask=0077"),
        ("vfat", ",fmask=0177,dmask=0077,utf8=1"),
        ("ext4", ""),
    ] {
        for writable in [false, true] {
            let card = Card::new(filesystem);
            if writable {
                card.fixture.put(PROTECT_OFF, "");
            }
            assert_eq!(card.operation("mount")["state"], "Ready", "{filesystem}");
            let access = if writable { "rw" } else { "ro" };
            assert_eq!(
                card.mount_options(),
                [format!("{access},nosuid,nodev,noexec,noatime{masks}")]
            );
        }
    }
}

#[test]
fn fat_card_pulled_while_mounted_is_mounted_again() {
    // Issue #44: fsck.fat -n exits 1 on the dirty bit alone.
    let card = Card::fat_checked_as(PULLED_FAT);
    let result = card.operation("mount");
    assert_eq!(result["state"], "Ready");
    assert_eq!(result["filesystem_check"]["finding"], "unclean_unmount");
    assert!(card.called(&["fsck.fat", "-n", "/dev/mmcblk1p1"]));
    assert!(card.names().iter().any(|n| n == "mount"));
    // Admission still never repairs: no -a/-w/-r run.
    assert!(!card.repaired());
    assert_eq!(card.sd()["state"], "Ready");
    // The checker's findings come from its output on failure: a FAT check keeps it.
    assert_eq!(card.kept_output("fsck.fat"), [true]);
}

#[test]
fn fat_card_with_real_findings_plays_read_only_and_the_status_says_why() {
    // 29ed4ccb (8.25): every card the user had was refused ("Needs repair"),
    // so nothing played. Reading never changes a card: it is mounted read-only.
    let damaged = PULLED_FAT.replace(
        "Dirty bit",
        "/Music/Song.flac\n  File size is 4194304 bytes, cluster chain length is 1048576 bytes.\n  \
         Truncating file to 1048576 bytes.\nDirty bit",
    );
    let card = Card::fat_checked_as(damaged);
    let result = card.operation("mount");
    assert_eq!(
        json!([result["state"], result["reason"], result["read_only"]]),
        json!(["Degraded", "read_only", "filesystem_errors"])
    );
    assert!(card.mount_options()[0].starts_with("ro,"));
    assert!(!card.repaired());
    // Checker output can name files: it is not kept.
    assert!(result["filesystem_check"]["output"].is_null());
    assert!(!card.fixture.read(MEDIA_STATE).contains("Song.flac"));
    let sd = card.sd();
    assert_eq!(
        json!([
            sd["state"],
            sd["reason"],
            sd["read_only_reason"],
            sd["filesystem"]
        ]),
        json!(["Degraded", "read_only", "filesystem_errors", "vfat"])
    );
}

#[test]
fn fat_dirty_bit_with_other_boot_differences_is_mounted() {
    // A backup boot sector that differs alone already passes (fsck.fat exits 0 and
    // never fixes it); with the dirty bit it used to keep the card out. fsck.fat
    // wraps a long offset list onto a line starting with ", ".
    let card = Card::fat_checked_as(PULLED_FAT.replace(
        "65:01/00",
        "3:4d/6d, 4:53/6b, 5:57/66, 6:49/73, 7:4e/2e, 8:34/66, 9:2e/61, 10:31/74\n  , 65:01/00",
    ));
    let result = card.operation("mount");
    assert_eq!(result["state"], "Ready");
    assert_eq!(
        result["filesystem_check"]["findings"],
        json!(["backup_differs", "dirty"])
    );
    assert!(card.names().iter().any(|n| n == "mount"));
}

#[test]
fn fat_findings_windows_mounts_over_are_admitted() {
    // Reddit/F12: "needs repair" on a card the PC read; reformatting fixed it.
    let dirty = PULLED_FAT;
    let cases: [(&str, String, &[&str]); 7] = [
        ("stale free count", STALE_FREE.into(), &["fsinfo_free"]),
        (
            "dirty + stale free count",
            dirty.replace(
                "Leaving",
                "Free cluster summary wrong (1 vs. really 2)\n  Auto-correcting.\nLeaving",
            ),
            &["dirty", "fsinfo_free"],
        ),
        (
            "label mismatch",
            STALE_FREE.replace(
                "Free cluster summary wrong (1234 vs. really 76393)\n  Auto-correcting.\n",
                LABEL,
            ),
            &["label_mismatch"],
        ),
        (
            "lost clusters + dirty",
            dirty.replace("Dirty bit", &format!("{LOST}Dirty bit")),
            &["dirty", "lost_clusters"],
        ),
        (
            "one lost cluster",
            dirty.replace(
                "Dirty bit",
                "Reclaimed 1 unused cluster (4096 bytes).\nDirty bit",
            ),
            &["dirty", "lost_clusters"],
        ),
        // Labels written by other systems' tools (dosfstools check.c).
        (
            "invalid label",
            dirty.replace(
                "Dirty bit",
                "Volume label 'my*card' stored in root directory is not valid.\n  \
                 Auto-removing label.\nDirty bit",
            ),
            &["dirty", "label_mismatch"],
        ),
        (
            "lowercase label",
            dirty.replace(
                "Dirty bit",
                "Volume label 'Music' stored in root directory contains lowercase characters.\n  \
                 Auto-changing lowercase characters to uppercase\nDirty bit",
            ),
            &["dirty", "label_mismatch"],
        ),
    ];
    for (name, output, findings) in &cases {
        assert_eq!(fat_findings(Some(output)), *findings, "{name}");
    }
    let card = Card::fat_checked_as(STALE_FREE);
    let result = card.operation("mount");
    assert_eq!(
        json!([result["state"], result["filesystem_check"]["finding"]]),
        json!(["Ready", "benign"])
    );
    assert!(!card.repaired());
}

#[test]
fn fat_benign_lines_out_of_place_or_with_real_findings_stay_out() {
    let damaged = STALE_FREE.replace(
        "Leaving",
        "/Music/Song.flac\n  Contains a free cluster (77). Assuming EOF.\nLeaving",
    );
    assert!(!fat_benign_only(Some(&damaged)));
    assert!(fat_findings(Some(&damaged)).contains(&"other".to_string()));
    // "Auto-correcting." answers only the free count, not another finding.
    assert!(!fat_benign_only(Some(&format!(
        "{LOST}  Auto-correcting.\n"
    ))));
    assert!(!fat_benign_only(Some(
        "fsck.fat 4.2 (2021-01-31)\nLeaving filesystem unchanged.\n"
    )));
    let card = Card::fat_checked_as(damaged);
    let result = card.operation("mount");
    assert_eq!(
        pair(&result, "state", "read_only"),
        json!(["Degraded", "filesystem_errors"])
    );
    assert!(result["filesystem_check"]["findings"]
        .as_array()
        .unwrap()
        .contains(&json!("other")));
    assert!(card.mount_options().iter().any(|o| o.starts_with("ro,")));
}

#[test]
fn admissions_are_remembered_across_boots_without_names() {
    // F12 (c75aae95): a card refused at every insertion; only the last card's
    // verdict was kept, in /run. Bounded, no checker output, no label or file names.
    let damaged = STALE_FREE.replace(
        "Leaving",
        "/Music/Secret Song.flac\n  Bad cluster chain.\nLeaving",
    );
    let card = Card::fat_checked_as(damaged);
    for _ in 0..ADMISSIONS_KEPT + 3 {
        card.operation("mount");
        card.unmounted(); // taken out again (read-only mounts since 8.27)
    }
    let text = card.fixture.read(ADMISSIONS);
    let entries = serde_json::from_str::<Value>(&text).unwrap()["admissions"].clone();
    let entries = entries.as_array().unwrap();
    assert_eq!(entries.len(), ADMISSIONS_KEPT);
    let last = entries.last().unwrap();
    assert_eq!(
        json!([last["ok"], last["findings"], last["filesystem"]]),
        json!([false, ["fsinfo_free", "other"], "vfat"])
    );
    assert!(last["elapsed_ms"].is_u64());
    assert!(!text.contains("Secret"));
    // 29ed4ccb (8.25): what the unclassified line said, without the name.
    assert_eq!(
        last["other"],
        json!({"shapes": ["Bad cluster chain."], "path_lines": 1})
    );
    assert_eq!(
        fat_other_shapes(
            Some(
                "/A/B.mp3\n  Contains a free cluster (77). Assuming EOF.\n\
                 Duplicate directory entry 'SECRET  MP3'\n"
            ),
            5
        ),
        json!({"shapes": ["Contains a free cluster (#). Assuming EOF.", "Duplicate directory entry '…'"],
               "path_lines": 1})
    );
    assert!(!text.contains("Music"));
}

/// A FAT card whose check outlasts admission; `background` answers the check `verify` runs (F49,
/// d32f8f3c: 59 s on 71 400 songs).
fn slow_card(background: impl Fn() -> Outcome + Send + 'static) -> Card {
    let card = Card::new("vfat");
    card.special(move |argv, timeout| {
        (argv[0] == "fsck.fat").then(|| {
            if timeout < CHECK_SECONDS {
                answer(false, Some("command_timeout"), None)
            } else {
                background()
            }
        })
    });
    card
}

impl Card {
    fn checks(&self) -> Vec<(Vec<String>, f64)> {
        self.calls()
            .into_iter()
            .filter(|(a, _)| a[0] == "fsck.fat")
            .collect()
    }
}

#[test]
fn a_slow_check_plays_at_once_and_finishes_in_the_background() {
    let card = slow_card(|| done(""));
    let result = card.operation("mount");
    let check = |timeout: f64| {
        (
            vec!["fsck.fat".to_string(), "-n".into(), "/dev/mmcblk1p1".into()],
            timeout,
        )
    };
    assert_eq!(card.checks(), [check(ADMISSION_CHECK_SECONDS)]);
    assert_eq!(
        json!([
            result["state"],
            result["reason"],
            result["read_only"],
            result["filesystem_check"]["findings"]
        ]),
        json!(["Ready", null, "checking", ["timeout"]])
    );
    assert!(card.mount_options()[0].starts_with("ro,"));
    // Not damage: Ready, its space shown, no Repair, no window until the check passes.
    let sd = card.sd();
    assert_eq!(
        json!([sd["state"], sd["writable"], sd["space_state"]]),
        json!(["Ready", false, "Normal"])
    );
    assert!(sd.get("read_only_reason").is_none());
    assert!(marked_for_repair(card.root()).is_none());
    assert_eq!(
        card.window("usb", 30, false)["reason"],
        "card_being_checked"
    );
    assert!(!card
        .mount_options()
        .iter()
        .any(|o| o.contains("remount,rw")));
    let verified = card.verify();
    assert_eq!(card.checks().last().unwrap(), &check(CHECK_SECONDS));
    assert_eq!(
        pair(&verified, "state", "read_only"),
        json!(["Ready", "protected"])
    );
    assert_eq!(card.fixture.json(MEDIA_MOUNT)["read_only"], "protected");
    assert_eq!(card.window("usb", 30, false)["holders"], json!(["usb"]));
    let entries = card.fixture.json(ADMISSIONS)["admissions"].clone();
    let seen: Vec<Value> = entries
        .as_array()
        .unwrap()
        .iter()
        .map(|e| json!([e["ok"], e["background"]]))
        .collect();
    assert_eq!(seen, [json!([false, null]), json!([true, true])]);
    // Settled: nothing left to check.
    assert_eq!(card.verify()["reason"], "nothing_to_check");
}

#[test]
fn a_background_check_with_findings_or_no_end_offers_repair() {
    for (read_only, background) in [
        ("filesystem_errors", command_failed(Some(CHAIN_SHORTER))),
        (
            "check_timed_out",
            answer(false, Some("command_timeout"), None),
        ),
    ] {
        let card = slow_card(move || background.clone());
        assert_eq!(card.operation("mount")["read_only"], "checking");
        let result = card.verify();
        assert_eq!(
            json!([result["state"], result["reason"], result["read_only"]]),
            json!(["Degraded", "read_only", read_only])
        );
        let sd = card.sd();
        assert_eq!(
            pair(&sd, "state", "read_only_reason"),
            json!(["Degraded", read_only])
        );
        assert!(marked_for_repair(card.root()).is_some());
        assert!(!card.fixture.read(MEDIA_STATE).contains("Song"));
    }
}

#[test]
fn a_background_check_applies_only_to_the_card_it_read() {
    let card = slow_card(|| command_failed(Some("I/O error")));
    card.operation("mount");
    // The player slept while the checker read: no verdict, it runs again.
    let clock = Arc::clone(&card.clock);
    card.special(move |argv, timeout| {
        (argv[0] == "fsck.fat" && timeout == CHECK_SECONDS).then(|| {
            clock.suspend(5_000_000_000);
            command_failed(Some("I/O error"))
        })
    });
    assert_eq!(card.verify()["reason"], "slept_during_check");
    assert_eq!(card.fixture.json(MEDIA_MOUNT)["read_only"], "checking");
    // Reinserted (new disk sequence) before or while it was checked.
    card.fixture.put(&format!("{DISK}/diskseq"), "18");
    assert_eq!(card.verify()["reason"], "card_changed");
    assert_eq!(card.fixture.json(MEDIA_MOUNT)["read_only"], "checking");
}

#[test]
fn ext4_keeps_its_admission_check() {
    let card = Card::new("ext4");
    let [ext4] = &candidates(card.root())[..] else {
        panic!("one ext4 card");
    };
    card.clear();
    read_only_check(card.root(), ext4, false, None);
    assert_eq!(card.calls()[0].1, EXT4_CHECK_SECONDS);
}

#[test]
fn fsck_is_read_only_and_never_checks_mounted_media() {
    let card = Card::new("exfat");
    assert_eq!(card.operation("check")["state"], "Ready");
    assert!(card.called(&["fsck.exfat", "-n", "/dev/mmcblk1p1"]));
    // Only FAT's output names finding classes; nothing else keeps a failed run's output.
    assert_eq!(card.kept_output("fsck.exfat"), [false]);
    card.operation("mount");
    assert_eq!(card.operation("check")["reason"], "already_mounted");
    assert_eq!(
        card.names().iter().filter(|n| *n == "fsck.exfat").count(),
        2
    );
}

#[test]
fn exfat_card_that_fails_its_check_plays_read_only() {
    let card = Card::new("exfat");
    for reason in ["command_failed", "timeout"] {
        card.clear();
        card.special(move |argv, _| {
            (argv[0] == "fsck.exfat").then(|| answer(false, Some(reason), None))
        });
        let result = card.operation("mount");
        let check = card
            .calls()
            .into_iter()
            .find(|(a, _)| a[0] == "fsck.exfat")
            .unwrap()
            .1;
        assert_eq!(check, ADMISSION_CHECK_SECONDS);
        assert_eq!(
            json!([result["state"], result["reason"], result["read_only"]]),
            json!(["Degraded", "read_only", "filesystem_errors"])
        );
        assert_eq!(
            card.storage()["media"]["last_operation"]["filesystem_check"]["reason"],
            reason
        );
        assert!(card.mount_options()[0].starts_with("ro,"));
        card.unmounted();
        card.fake().special = None;
    }
}

#[test]
fn card_replacement_during_check_prevents_mount() {
    let card = Card::new("exfat");
    let cid = card.path(&format!("{DISK}/device/cid"));
    card.special(move |argv, _| {
        if argv[0] == "fsck.exfat" {
            fs::write(&cid, "replacement-cid").unwrap();
        }
        None
    });
    assert_eq!(
        card.operation("mount")["reason"],
        "source_changed_during_check"
    );
    assert!(!card.names().iter().any(|n| n == "mount"));
}

#[test]
fn same_card_reenumeration_during_check_prevents_mount() {
    let card = Card::new("exfat");
    let diskseq = card.path(&format!("{DISK}/diskseq"));
    card.special(move |argv, _| {
        if argv[0] == "fsck.exfat" {
            fs::write(&diskseq, "18").unwrap();
        }
        None
    });
    assert_eq!(
        card.operation("mount")["reason"],
        "source_changed_during_check"
    );
    assert!(!card.names().iter().any(|n| n == "mount"));
}

#[test]
fn removed_card_can_be_unmounted_only_with_its_retained_claim() {
    let card = Card::new("exfat");
    card.operation("mount");
    fs::remove_file(card.path("/sys/dev/block/179:9")).unwrap();
    let mountinfo = card.path("/proc/self/mountinfo");
    card.special(move |argv, _| {
        (argv[0] == "umount").then(|| {
            fs::write(&mountinfo, "").unwrap();
            done("")
        })
    });
    assert_eq!(card.operation("unmount")["reason"], "ejected");
    card.fixture.put(
        "/proc/self/mountinfo",
        "52 1 179:10 / /media/sd rw - ext4 /dev/unknown rw",
    );
    assert_eq!(card.operation("unmount")["reason"], "mounted_source_not_sd");
}

#[test]
fn automatic_mount_runs_once_per_insertion_and_failed_eject_is_bounded() {
    let card = Card::new("exfat");
    let reconcile = || Value::Object(reconcile(card.root()).unwrap());
    assert_eq!(reconcile()["state"], "Ready");
    assert_eq!(reconcile()["attempts"], 1);
    fs::remove_file(card.path("/sys/class/block/mmcblk1p1")).unwrap();
    let mut value = Value::Null;
    for _ in 0..8 {
        value = reconcile();
    }
    assert_eq!(value["attempts"], 5);
    assert_eq!(value["retry"], false);
    assert_eq!(value["reason"], "source_change_unmount_failed");
}

// ---- the protected card -------------------------------------------------------------------------

impl Card {
    fn protected() -> Card {
        let card = Card::fat_checked_as(PULLED_FAT);
        assert_eq!(card.operation("mount")["read_only"], "protected");
        card
    }
}

#[test]
fn a_healthy_card_is_read_only_between_writes() {
    let card = Card::protected();
    let sd = card.sd();
    // Healthy: Ready with its real space, only not writable this moment.
    assert_eq!(
        json!([sd["state"], sd["writable"], sd["space_state"]]),
        json!(["Ready", false, "Normal"])
    );
    let answer = card.window("reborn", 60, false);
    assert_eq!(
        pair(&answer, "writable", "holders"),
        json!([true, ["reborn"]])
    );
    assert!(card.called(&["mount", "-o", "remount,rw", "/media/sd"]));
    assert_eq!(card.sd()["writable"], true);
    // A second writer keeps it open after the first ends.
    card.window("report", 60, false);
    assert_eq!(card.window("reborn", 60, true)["writable"], true);
    assert_eq!(card.window("report", 60, true)["writable"], false);
    assert_eq!(
        card.argvs().last().unwrap(),
        &["mount", "-o", "remount,ro", "/media/sd"]
    );
    // A forgotten window ends at the reconcile tick after its time.
    card.window("usb", 30, false);
    protect(card.root()).unwrap();
    assert_eq!(card.sd()["writable"], true);
    card.clock.advance(31_000_000_000);
    protect(card.root()).unwrap();
    assert_eq!(card.sd()["writable"], false);
    assert!(write_window(card.root(), "anyone", 60, false).is_err());
}

#[test]
fn damaged_card_gets_no_window() {
    let card =
        Card::fat_checked_as(PULLED_FAT.replace("Dirty bit", &format!("{CHAIN_SHORTER}Dirty bit")));
    assert_eq!(card.operation("mount")["read_only"], "filesystem_errors");
    assert_eq!(
        card.window("reborn", 60, false)["reason"],
        "card_has_errors"
    );
    assert!(!card
        .mount_options()
        .iter()
        .any(|o| o.contains("remount,rw")));
    assert!(!can_write(card.root()));
}

#[test]
fn the_owner_switch_keeps_the_old_writable_mount() {
    let card = Card::fat_checked_as(PULLED_FAT);
    card.fixture.put(PROTECT_OFF, "");
    let result = card.operation("mount");
    assert_eq!(pair(&result, "state", "read_only"), json!(["Ready", null]));
    assert_eq!(card.sd()["writable"], true);
}

// ---- repair -------------------------------------------------------------------------------------

/// A FAT card mounted after its check (damaged or only pulled), the battery at `percent`, a FAT32
/// boot sector (32 reserved sectors, 2 FATs of 100 sectors) on its node; `fsck.fat -a -w` repairs it.
fn repair_fixture(damaged: bool, percent: i64, charging: bool) -> Card {
    let findings = if damaged { CHAIN_SHORTER } else { "" };
    let output = PULLED_FAT.replace("Dirty bit", &format!("{findings}Dirty bit"));
    let card = Card::new("vfat");
    let (mountinfo, repaired) = (
        card.path("/proc/self/mountinfo"),
        Arc::new(AtomicBool::new(false)),
    );
    card.special(move |argv, _| match argv {
        ["umount", ..] => {
            fs::write(&mountinfo, "").unwrap();
            Some(answer(true, None, None))
        }
        ["fsck.fat", "-a", "-w", ..] => {
            repaired.store(true, Ordering::SeqCst);
            let output = format!("{CHAIN_SHORTER}Reclaimed 70 unused clusters (286720 bytes).\n");
            Some(command_failed(Some(&output)))
        }
        ["fsck.fat", "-n", ..] if repaired.load(Ordering::SeqCst) => Some(done("")),
        ["fsck.fat", ..] => Some(command_failed(Some(&output))),
        _ => None,
    });
    card.operation("mount");
    let status = if charging { "Charging" } else { "Discharging" };
    card.fixture.put_json(
        "/run/y2/power.json",
        &json!({"boot_id": "boot-one", "battery": {"percent": percent, "charging": charging, "status": status}}),
    );
    let mut boot = vec![0u8; 512];
    boot[11..13].copy_from_slice(&512u16.to_le_bytes());
    boot[13] = 8;
    boot[14..16].copy_from_slice(&32u16.to_le_bytes());
    boot[16] = 2;
    boot[21] = 0xF8;
    boot[36..40].copy_from_slice(&100u32.to_le_bytes());
    boot.extend(vec![0u8; 300 * 512]);
    card.fixture.mkdir("/dev");
    fs::write(card.path("/dev/mmcblk1p1"), boot).unwrap();
    card
}

impl Card {
    fn repair(&self) -> Value {
        Value::Object(repair(self.root()).unwrap())
    }

    fn fixed(&self) -> bool {
        self.argvs()
            .iter()
            .any(|a| a[0] == "fsck.fat" && a[1] == "-a")
    }
}

#[test]
fn repair_runs_only_when_asked_backs_up_and_admits_again() {
    let card = repair_fixture(true, 80, false);
    let result = card.repair();
    assert!(card.called(&["fsck.fat", "-a", "-w", "/dev/mmcblk1p1"]));
    assert_eq!(
        json!([result["state"], result["action"], result["read_only"]]),
        json!(["Ready", "repair", "protected"])
    );
    let changed = &result["repair"]["changed"];
    assert_eq!(
        pair(changed, "files_shortened", "clusters_freed"),
        json!([1, 70])
    );
    let backup = &result["repair"]["backup"];
    assert_eq!(
        pair(backup, "saved", "bytes"),
        json!([true, (32 + 200) * 512])
    );
    let file = card.path(backup["file"].as_str().unwrap());
    assert_eq!(fs::metadata(file).unwrap().len(), (32 + 200) * 512);
    let record = card.fixture.read(REPAIR_LAST);
    assert!(!record.contains("Song"));
    assert_eq!(
        serde_json::from_str::<Value>(&record).unwrap()["findings_before"],
        json!(["dirty", "other"])
    );
}

#[test]
fn repair_refuses_a_healthy_card() {
    let card = repair_fixture(false, 80, false);
    assert_eq!(card.repair()["reason"], "card_not_marked_for_repair");
    assert!(!card.fixed());
}

#[test]
fn repair_needs_a_charger_or_30_percent() {
    let card = repair_fixture(true, 20, false);
    assert_eq!(card.repair()["reason"], "repair_needs_charger_or_battery");
    assert!(!card.fixed());
    card.fixture.put_json(
        "/run/y2/power.json",
        &json!({"boot_id": "boot-one", "battery": {"percent": 20, "charging": true, "status": "Charging"}}),
    );
    assert_eq!(card.repair()["state"], "Ready");
}

// ---- format and grow (test_media_change.py) -----------------------------------------------------

#[test]
fn mbr_is_one_linux_partition_to_the_end_and_caps_at_32_bits() {
    let mbr = mbr_for(SECTORS);
    assert_eq!(mbr.len(), 512);
    assert_eq!(&mbr[510..], &[0x55, 0xaa]);
    let u32_at = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as u64;
    assert_eq!(
        (mbr[446], mbr[450], u32_at(&mbr, 454), u32_at(&mbr, 458)),
        (0, 0x83, PARTITION_START, SECTORS - PARTITION_START)
    );
    assert_eq!(&mbr[462..510], &[0u8; 48]);
    let big = mbr_for(1 << 33);
    assert_eq!(
        (u32_at(&big, 454), u32_at(&big, 458)),
        (PARTITION_START, 0xFFFF_FFFF)
    );
}

impl Card {
    fn device(&self) -> Vec<u8> {
        fs::read(self.path("/dev/mmcblk1")).unwrap()
    }
}

#[test]
fn format_needs_the_exact_card_cid_and_touches_nothing_otherwise() {
    let card = Card::change();
    let before = card.device();
    for wrong in [None, Some(""), Some("other-card")] {
        let result = card.change_card("format", wrong);
        assert_eq!(
            pair(&result, "state", "reason"),
            json!(["Failed", "cid_confirmation_mismatch"])
        );
    }
    assert_eq!(card.device(), before);
    assert!(card.calls().is_empty());
    assert_eq!(card.rereads.get(), 0);
}

#[test]
fn format_repartitions_wipes_stale_metadata_and_mounts_the_new_filesystem() {
    let card = Card::change();
    let result = card.change_card("format", Some(CID));
    assert_eq!(pair(&result, "state", "reason"), json!(["Ready", null]));
    let data = card.device();
    let start = PARTITION_START as usize * 512;
    assert_eq!(&data[..512], &mbr_for(SECTORS)[..]);
    // Everything between the MBR and the partition start, and the GPT backup area, is zero.
    assert!(data[512..start].iter().all(|b| *b == 0));
    let tail = WIPE_TAIL_SECTORS as usize * 512;
    assert!(data[data.len() - tail..].iter().all(|b| *b == 0));
    // The partition interior is not written by the platform: mkfs owns it.
    assert!(data[start..start + 512].iter().all(|b| *b == 0xa5));
    assert_eq!(card.rereads.get(), 1);
    let mkfs = card
        .argvs()
        .into_iter()
        .find(|a| a[0] == "mkfs.ext4")
        .unwrap();
    assert_eq!(mkfs.last().unwrap(), "/dev/mmcblk1p1");
    assert!(mkfs.iter().any(|a| a == "Y2SD"));
    // No root-reserved blocks on a media card; a small card keeps the default journal.
    assert_eq!(mkfs[mkfs.iter().position(|a| a == "-m").unwrap() + 1], "0");
    assert!(!mkfs.iter().any(|a| a == "-J"));
    assert!(card.index("mkfs.ext4") < card.index("mount"));
    assert_eq!(result["action"], "format");
}

#[test]
fn large_cards_get_a_bounded_journal() {
    let card = Card::change();
    let big: u64 = 64 * 1024 * 1024;
    card.fixture.put(&format!("{DISK}/size"), &big.to_string());
    // A sparse 32 GiB card: only its first MiB and last sectors are written.
    File::options()
        .write(true)
        .open(card.path("/dev/mmcblk1"))
        .unwrap()
        .set_len(big * 512)
        .unwrap();
    let mut state = Object::new();
    set(&mut state, "state", "Starting");
    format_steps(card.root(), &mut state, "/dev/mmcblk1", big, &|_| Ok(()));
    let mkfs = card
        .argvs()
        .into_iter()
        .find(|a| a[0] == "mkfs.ext4")
        .unwrap();
    assert_eq!(
        mkfs[mkfs.iter().position(|a| a == "-J").unwrap() + 1],
        "size=128"
    );
}

impl Card {
    /// The USB service's state: the SD card shown to a computer on a cable.
    fn shown_over_usb(&self) {
        self.fixture.put_json(
            "/run/y2/usb-transfer.json",
            &json!({"schema": "org.y2linux.usb-transfer/v1", "boot_id": "boot-one",
                    "state": "Ready", "cable": "attached", "views": [{"path": "/media/sd"}]}),
        );
    }
}

#[test]
fn mounted_card_is_released_from_mtp_even_with_a_cable_then_unmounted_before_any_write() {
    let card = Card::change();
    assert_eq!(card.operation("mount")["state"], "Ready");
    card.clear();
    card.shown_over_usb();
    let (marker, status, device) = (
        card.path("/run/y2/usb-release-sd.json"),
        card.path("/run/y2/usb-transfer.json"),
        card.path("/dev/mmcblk1"),
    );
    let (result, seen) = thread::scope(|scope| {
        // The USB service answers the release. With the cable attached only an explicit release
        // writes the request.
        let service = scope.spawn(|| {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !marker.exists() {
                assert!(Instant::now() < deadline, "no release request");
                thread::sleep(Duration::from_millis(5));
            }
            let asked: Value = serde_json::from_str(&fs::read_to_string(&marker).unwrap()).unwrap();
            // nothing may have been written yet
            let mut first = vec![0u8; 512];
            File::open(&device).unwrap().read_exact(&mut first).unwrap();
            assert_eq!(first, [0xa5; 512]);
            let released = json!({"schema": "org.y2linux.usb-transfer/v1", "boot_id": "boot-one",
                                  "state": "Ready", "cable": "attached", "views": []});
            let temporary = status.with_extension("tmp");
            fs::write(&temporary, released.to_string()).unwrap();
            fs::rename(&temporary, &status).unwrap();
            asked["mount_id"].clone()
        });
        let result = card.change_card("format", Some(CID));
        (result, service.join().unwrap())
    });
    assert_eq!(result["state"], "Ready");
    assert_eq!(seen, 51);
    assert!(card.index("umount") < card.index("mkfs.ext4"));
}

#[test]
fn a_failed_release_or_unmount_aborts_before_any_write() {
    let card = Card::change();
    card.operation("mount");
    card.clear();
    let before = card.device();
    // The USB service never lets the card go: the release times out.
    card.shown_over_usb();
    let result = card.change_card("format", Some(CID));
    assert_eq!(
        pair(&result, "state", "reason"),
        json!(["Failed", "usb_release_failed"])
    );
    assert_eq!(card.device(), before);
    assert!(!card.names().iter().any(|n| n == "mkfs.ext4"));
    fs::remove_file(card.path("/run/y2/usb-transfer.json")).unwrap();
    card.special(|argv, _| (argv[0] == "umount").then(|| command_failed(None)));
    let result = card.change_card("format", Some(CID));
    assert_eq!(result["state"], "Failed");
    assert_eq!(card.device(), before);
}

#[test]
fn only_the_removable_card_controller_can_ever_be_formatted() {
    let card = Card::change();
    // The internal eMMC disk is not a candidate, and without a card nothing is touched.
    let internal = "/sys/devices/platform/11230000.mmc/block/mmcblk0";
    card.fixture
        .put(&format!("{internal}/device/cid"), "internal-cid");
    card.fixture.put(&format!("{internal}/size"), "15203328");
    symlink(card.path(internal), card.path("/sys/class/block/mmcblk0")).unwrap();
    fs::remove_file(card.path("/sys/class/block/mmcblk1")).unwrap();
    fs::write(card.path("/dev/mmcblk0"), [0x11; 4096]).unwrap();
    for action in ["format", "grow"] {
        let result = card.change_card(action, Some("internal-cid"));
        assert_eq!(
            pair(&result, "state", "reason"),
            json!(["Unavailable", "no_unique_card"])
        );
    }
    assert_eq!(fs::read(card.path("/dev/mmcblk0")).unwrap(), [0x11; 4096]);
    assert!(card.calls().is_empty());
}

#[test]
fn a_too_small_card_and_unknown_actions_are_refused() {
    let card = Card::change();
    card.fixture.put(&format!("{DISK}/size"), "100");
    assert_eq!(
        card.change_card("format", Some(CID))["reason"],
        "card_too_small"
    );
    let unknown = change_card(card.root(), "wipe", Some(CID));
    assert!(matches!(unknown, Err(Failure::Value(text)) if text == "unknown_media_change"));
}

#[test]
fn partition_reread_failure_stops_before_mkfs() {
    let card = Card::change();
    card.reread_fails.set(true);
    let result = card.change_card("format", Some(CID));
    assert_eq!(
        pair(&result, "state", "reason"),
        json!(["Failed", "partition_reread_failed"])
    );
    assert!(!card.names().iter().any(|n| n == "mkfs.ext4"));
}

#[test]
fn grow_checks_then_resizes_only_one_ext4_card() {
    let card = Card::change();
    card.fake().filesystem = "ext4";
    assert_eq!(card.change_card("grow", None)["state"], "Ready");
    assert!(card.index("e2fsck") < card.index("resize2fs"));
    assert!(card.index("resize2fs") < card.index("mount"));
    assert!(!card.names().iter().any(|n| n == "mkfs.ext4"));
    // grow never writes the disk itself
    assert!(card.device().iter().all(|b| *b == 0xa5));
}

#[test]
fn grow_refuses_a_foreign_or_unchecked_filesystem() {
    let card = Card::change();
    let result = card.change_card("grow", None); // exfat card
    assert_eq!(
        pair(&result, "state", "reason"),
        json!(["Failed", "grow_requires_one_ext4_card"])
    );
    assert!(!card.names().iter().any(|n| n == "resize2fs"));
    card.fake().filesystem = "ext4";
    card.clear();
    card.special(|argv, _| (argv[0] == "e2fsck").then(|| command_failed(None)));
    let result = card.change_card("grow", None);
    assert_eq!(
        pair(&result, "state", "reason"),
        json!(["Failed", "filesystem_check_failed"])
    );
    assert!(!card.names().iter().any(|n| n == "resize2fs"));
}

#[test]
fn a_recycled_mount_id_cannot_leave_the_new_card_hidden() {
    // The kernel reuses mount ids: after reformat/eject the release marker must not match again.
    let card = Card::change();
    card.fixture
        .put_json("/run/y2/usb-release-sd.json", &json!({"mount_id": 51}));
    assert_eq!(card.operation("mount")["state"], "Ready");
    assert!(!card.fixture.exists("/run/y2/usb-release-sd.json"));
}

#[test]
fn storage_reports_an_undersized_filesystem_without_changing_it() {
    let card = Card::change();
    card.operation("mount");
    // The fixture's /media/sd is a real directory: size the partition against its filesystem.
    let vfs = statvfs(&card.path("/media/sd")).unwrap();

    let total = vfs.total();
    let sectors = total / 512 * 2;
    card.fixture
        .put("/sys/class/block/mmcblk1p1/size", &sectors.to_string());
    let volume = card.sd();
    assert_eq!(volume["partition_bytes"], sectors * 512);
    assert_eq!(volume["capacity_state"], "Undersized");
    card.fixture.put(
        "/sys/class/block/mmcblk1p1/size",
        &(total / 512).to_string(),
    );
    assert_eq!(card.sd()["capacity_state"], "Full");
}
