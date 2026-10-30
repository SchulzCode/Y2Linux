//! Transfer tests: verified uploads below a fixture root with a verified /data (and Y2Media) as the
//! golden cases (tests/platform_cases/devices.py) mount them; `blkid` answers their UUIDs.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::testing::{ok, Fixture, Outcome};
use std::{fs, io::Write, os::unix::fs::MetadataExt, thread};

const DATA: &str = "22 1 179:2 / / ro - ext4 /dev/mmcblk0p2 ro\n\
                    30 22 259:1 / /data rw,noatime - ext4 /dev/mmcblk0p9 rw\n";
const MEDIA: &str = "31 22 259:2 / /media/internal rw,noatime - ext4 /dev/mmcblk0p8 rw\n";
const TRACK: &str = "/media/internal/music/Album/track.flac";
const BLKID: [&str; 6] = ["blkid", "-p", "-s", "UUID", "-o", "value"];

/// `blkid`'s answer for a partition: Y2DATA's UUID for p9, Y2Media's for p8.
fn uuid(device: &str) -> Option<&'static str> {
    match device {
        "/dev/mmcblk0p9" => Some("79324c69-6e75-4801-8000-000000000102"),
        "/dev/mmcblk0p8" => Some("79324c69-6e75-4801-8000-000000000103"),
        _ => None,
    }
}

/// A verified /data (device 259:1 on the eMMC controller, Y2DATA's UUID).
fn fixture() -> Fixture {
    let answers = ["/dev/mmcblk0p9", "/dev/mmcblk0p8"]
        .map(|device| {
            let argv: Vec<&str> = BLKID.iter().copied().chain([device]).collect();
            ok(&argv, uuid(device).unwrap_or_default())
        })
        .to_vec();
    prepared(Fixture::new().with_answers(answers))
}

fn prepared(fixture: Fixture) -> Fixture {
    fixture
        .put("/proc/sys/kernel/random/boot_id", "boot-a")
        .put("/proc/self/mountinfo", DATA)
        .mkdir("/data");
    device(&fixture, "259:1", "mmcblk0p9");
    fixture
}

fn device(fixture: &Fixture, number: &str, partition: &str) {
    let device = format!(
        "/sys/devices/platform/11230000.mmc/mmc_host/mmc0/mmc0:0001/block/mmcblk0/{partition}"
    );
    fixture
        .put(&format!("{device}/dev"), number)
        .mkdir("/sys/dev/block");
    std::os::unix::fs::symlink(
        fixture.path(&device),
        fixture.path(&format!("/sys/dev/block/{number}")),
    )
    .unwrap();
}

/// `use_internal_media`: Y2Media mounted and recorded Ready this boot, below `media` (the fixture's
/// own /media, or a directory on another filesystem the fixture's /media links to).
fn use_internal_media(fixture: &Fixture, media: Option<&Path>) {
    fixture.put("/proc/self/mountinfo", &format!("{DATA}{MEDIA}"));
    device(fixture, "259:2", "mmcblk0p8");
    fixture.put_json(
        "/run/y2/internal-media.json",
        &json!({"schema": 1, "state": "Ready", "boot_id": "boot-a", "mount_id": 31,
                "device_id": "259:2"}),
    );
    if let Some(media) = media {
        fs::create_dir_all(media.join("internal")).unwrap();
        std::os::unix::fs::symlink(media, fixture.path("/media")).unwrap();
    } else {
        fixture.mkdir("/media/internal");
    }
}

fn payload() -> Vec<u8> {
    b"owner music test\0".repeat(4000)
}

/// `begin` of the payload; the stage id and the upload's host path.
fn start(fixture: &Fixture, name: &str) -> (String, PathBuf) {
    let payload = payload();
    let stage = begin(
        &fixture.root,
        Some(name),
        Some(payload.len() as i64),
        &y2p_core::digest::sha256_hex(&payload),
    )
    .unwrap();
    (
        stage["id"].as_str().unwrap().to_owned(),
        fixture.path(stage["upload_path"].as_str().unwrap()),
    )
}

/// `call` again while the stage lock is busy: a child that another test thread forks holds a copy of
/// the previous call's lock descriptor until it execs (the platform makes these calls unthreaded).
fn locked(call: impl Fn() -> Result<Value, Failure>) -> Result<Value, Failure> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match call() {
            Err(Failure::Os {
                class: "BlockingIOError",
                ..
            }) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            other => return other,
        }
    }
}

fn message(result: Result<Value, Failure>) -> String {
    result.expect_err("an error").message()
}

fn class(result: Result<Value, Failure>) -> &'static str {
    match result {
        Err(Failure::Os { class, .. }) => class,
        other => panic!("not an OSError: {:?}", other.map_err(|e| e.message())),
    }
}

fn names(fixture: &Fixture, directory: &str) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(fixture.path(directory))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A directory on another filesystem than the fixture's (removed on drop), for a cross-filesystem
/// publish; `None` when this host has none.
struct Elsewhere(PathBuf);

impl Elsewhere {
    fn new(fixture: &Fixture) -> Option<Elsewhere> {
        let own = fs::metadata(fixture.base()).unwrap().dev();
        let target = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target");
        ["/var/tmp", "/dev/shm", target]
            .iter()
            .filter(|c| fs::metadata(c).is_ok_and(|m| m.dev() != own))
            .find_map(|c| {
                let name = fixture.base().file_name().unwrap().to_string_lossy();
                let path = Path::new(c).join(format!("{name}-elsewhere"));
                fs::create_dir(&path).ok().map(|()| Elsewhere(path))
            })
    }
}

impl Drop for Elsewhere {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn interrupted_upload_bad_hash_and_reserve_never_publish() {
    let fixture = fixture();
    let root = &fixture.root;
    let (id, path) = start(&fixture, "Album/track.flac");
    let payload = payload();
    fs::write(&path, &payload[..100]).unwrap();
    assert!(message(locked(|| commit(root, &id))).contains("incomplete"));
    assert!(!fixture.exists("/data/music"));
    fs::write(&path, vec![b'x'; payload.len()]).unwrap();
    assert!(message(locked(|| commit(root, &id))).contains("hash"));
    assert!(!fixture.exists("/data/music"));
    locked(|| discard(root, &id)).unwrap();
    // 97 MiB free cannot keep a 1 MiB upload, its seal and the 96 MiB reserve (begin's admission).
    let low = Volume {
        path: "/data",
        state: "Ready",
        generation: Some("boot:1:179:7".into()),
        uuid: None,
        space_state: "Normal",
        available_bytes: Some(97 * 1024 * 1024),
    };
    assert!(!storage::admitted(&low, 2 * 1024 * 1024));
    assert!(storage::admitted(&low, 1024 * 1024 / 2));
}

#[test]
fn verified_copy_does_not_change_with_old_upload_fd_and_never_overwrites() {
    let fixture = fixture();
    let root = &fixture.root;
    let payload = payload();
    let (id, path) = start(&fixture, "Album/track.flac");
    fs::write(&path, &payload).unwrap();
    let mut old = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    assert_eq!(locked(|| commit(root, &id)).unwrap()["state"], "Committed");
    old.write_all(b"changed").unwrap();
    old.flush().unwrap();
    let target = fixture.path("/data/music/Album/track.flac");
    assert_eq!(fs::read(&target).unwrap(), payload);
    assert_eq!(locked(|| commit(root, &id)).unwrap()["state"], "Committed");
    locked(|| discard(root, &id)).unwrap();
    let (id, path) = start(&fixture, "Album/track.flac");
    fs::write(&path, &payload).unwrap();
    assert_eq!(class(locked(|| commit(root, &id))), "FileExistsError");
    assert_eq!(fs::read(&target).unwrap(), payload);
}

#[test]
fn uploads_publish_on_y2media_by_copy_across_filesystems_and_never_replace() {
    let fixture = fixture();
    let Some(elsewhere) = Elsewhere::new(&fixture) else {
        eprintln!("skipped: no second filesystem on this host");
        return;
    };
    use_internal_media(&fixture, Some(&elsewhere.0));
    let root = &fixture.root;
    let payload = payload();
    let (id, path) = start(&fixture, "Album/track.flac");
    fs::write(&path, &payload).unwrap();
    let stage = fs::metadata(path.parent().unwrap()).unwrap().dev();
    assert_ne!(
        stage,
        fs::metadata(fixture.path("/media/internal")).unwrap().dev()
    );
    assert_eq!(locked(|| commit(root, &id)).unwrap()["state"], "Committed");
    assert_eq!(fs::read(fixture.path(TRACK)).unwrap(), payload);
    assert!(!fixture.exists("/data/music"));
    // The temporary name is gone.
    assert_eq!(
        names(&fixture, "/media/internal/music/Album"),
        ["track.flac"]
    );
    locked(|| discard(root, &id)).unwrap();
    let (id, path) = start(&fixture, "Album/track.flac");
    fs::write(&path, &payload).unwrap();
    assert_eq!(class(locked(|| commit(root, &id))), "FileExistsError");
    assert_eq!(fs::read(fixture.path(TRACK)).unwrap(), payload);
    assert_eq!(
        names(&fixture, "/media/internal/music/Album"),
        ["track.flac"]
    );
}

#[test]
fn same_filesystem_media_still_uses_an_atomic_link() {
    let fixture = fixture();
    use_internal_media(&fixture, None);
    // publish_copy's exclusive temporary name is taken: only the link can publish.
    fixture.mkdir("/media/internal/music/Album/.track.flac.y2part");
    let root = &fixture.root;
    let payload = payload();
    let (id, path) = start(&fixture, "Album/track.flac");
    fs::write(&path, &payload).unwrap();
    assert_eq!(locked(|| commit(root, &id)).unwrap()["state"], "Committed");
    assert_eq!(fs::read(fixture.path(TRACK)).unwrap(), payload);
}

/// Y2Media mounted again while a commit runs: the volume is read again after the target directories
/// are opened, and a new generation refuses before anything is published. Once the upload is sealed
/// the commit reads /data, then Y2Media, before it opens the target (each volume probed alone); the
/// runner remounts Y2Media (a new mount id) as it answers that Y2Media read's `blkid`, so only the read
/// after the target is open sees it.
#[test]
fn media_replacement_fails_closed_before_publishing() {
    let fixture = Fixture::new();
    let mountinfo = fixture.path("/proc/self/mountinfo");
    let staging = fixture.path("/data/transfer-staging");
    let sealed_reads = std::sync::atomic::AtomicUsize::new(0);
    let fixture = prepared(fixture.responding(move |argv, _| {
        let device = argv.last().copied().unwrap_or_default();
        let sealed = fs::read_dir(&staging)
            .into_iter()
            .flatten()
            .flatten()
            .any(|stage| stage.path().join("sealed.part").exists());
        if device == "/dev/mmcblk0p8"
            && sealed
            && sealed_reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0
        {
            fs::write(
                &mountinfo,
                format!("{DATA}{}", MEDIA.replace("31 22", "33 22")),
            )
            .unwrap();
        }
        Outcome {
            ok: uuid(device).is_some(),
            reason: None,
            output: uuid(device).map(str::to_owned),
        }
    }));
    use_internal_media(&fixture, None);
    let root = &fixture.root;
    let payload = payload();
    let (id, path) = start(&fixture, "Album/track.flac");
    fs::write(&path, &payload).unwrap();
    assert_eq!(
        message(locked(|| commit(root, &id))),
        "media_generation_changed"
    );
    assert!(fixture.exists("/media/internal/music/Album"));
    assert!(names(&fixture, "/media/internal/music/Album").is_empty());
}

/// The copy checks its reserve before it writes a byte and removes its temporary name either way.
/// The free space of the target filesystem cannot be lowered from here: the order is pinned.
#[test]
fn low_space_fails_closed_before_copying() {
    let source = include_str!("transfer.rs");
    let copy =
        &source[source.find("fn publish_copy(").unwrap()..source.find("fn inspect(").unwrap()];
    let reserve = copy
        .find("return Err(Failure::value(\"transfer_storage_reserve\"));")
        .unwrap();
    assert!(reserve < copy.find("write_some(").unwrap());
    assert!(copy.find("let result = (|| {").unwrap() < reserve);
    assert!(
        copy.rfind("unlinkat(target, &temporary, 0, true)?;")
            .unwrap()
            > copy.find("})();").unwrap()
    );
}

#[test]
fn mount_replacement_and_symlink_escape_fail_closed() {
    let fixture = fixture();
    let root = &fixture.root;
    let payload = payload();
    let (id, path) = start(&fixture, "Album/track.flac");
    fs::write(&path, &payload).unwrap();
    // /data mounted again (a new mount id): another generation.
    fixture.put(
        "/proc/self/mountinfo",
        &DATA.replace("30 22 259:1", "32 22 259:1"),
    );
    assert!(message(locked(|| commit(root, &id))).contains("generation_changed"));
    locked(|| discard(root, &id)).unwrap();
    let outside = fixture.path("/outside");
    fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, fixture.path("/data/music")).unwrap();
    let (id, path) = start(&fixture, "Album/track.flac");
    fs::write(&path, &payload).unwrap();
    assert!(matches!(
        locked(|| commit(root, &id)),
        Err(Failure::Os { .. })
    ));
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    for name in ["../state/db", "/root/key", "ok/../bad", "ok\nput secret"] {
        let result = begin(root, Some(name), Some(5), &"a".repeat(64));
        assert!(matches!(result, Err(Failure::Value(_))), "{name:?}");
    }
}

#[test]
fn a_tampered_manifest_refuses_before_publishing() {
    let fixture = fixture();
    let root = &fixture.root;
    let payload = payload();
    let (id, path) = start(&fixture, "Album/track.flac");
    fs::write(&path, &payload).unwrap();
    let manifest = path.with_file_name("manifest.json");
    let mut staged: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    staged["record"] = Value::Null;
    for tampered in [staged.to_string(), "[]".to_owned()] {
        fs::write(&manifest, &tampered).unwrap();
        assert_eq!(
            message(locked(|| commit(root, &id))),
            "invalid_transfer_manifest"
        );
        assert!(!fixture.exists("/data/music"));
        assert_eq!(fs::read_to_string(&manifest).unwrap(), tampered);
    }
}
