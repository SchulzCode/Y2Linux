//! tests/test_radio_firmware.py (RELEASE-49): radio firmware from the owner's own copy, never from an
//! image. Fixture files stand in for the MediaTek firmware (it is not in this repository), with the
//! stock sizes (`present()` looks at sizes only) and their own hashes in the reviewed tables. With the
//! owner's stock package present, the real system.img is read as well.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::testing::{ok, repository, Fixture};
use std::{process::Command, time::Duration};

/// Bytes that differ per file and per run of a test (`os.urandom` in Python).
fn noise(size: usize, seed: u64) -> Vec<u8> {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..size)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 24) as u8
        })
        .collect()
}

/// A stand-in for libcustom_nvram.so with the layout `defaults()` reads.
fn custom_library() -> Vec<u8> {
    let mut data = noise(CUSTOM.1 as usize, 7);
    data[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
    let mut wifi = noise(512, 8);
    wifi[..4].copy_from_slice(&[0x04, 0x01, 0, 0]);
    wifi[196..198].copy_from_slice(b"\x01\x00");
    data[0x5a8e..0x5c8e].copy_from_slice(&wifi);
    data
}

fn patch_bytes(size: usize, sequence: u8) -> Vec<u8> {
    let mut data = noise(size, u64::from(sequence) + 20);
    data[18..20].copy_from_slice(b"PS");
    data[24] = 0x20 | sequence;
    data
}

/// Android sparse encoding: raw chunks, zero runs as don't-care, one fill.
fn sparse(raw: &[u8]) -> Vec<u8> {
    const BLOCK: usize = 4096;
    let blocks: Vec<Vec<u8>> = raw
        .chunks(BLOCK)
        .map(|b| {
            let mut b = b.to_vec();
            b.resize(BLOCK, 0);
            b
        })
        .collect();
    let zero = |b: &Vec<u8>| b.iter().all(|&x| x == 0);
    let chunk = |kind: u16, blocks: usize, size: usize| {
        let mut out = Vec::new();
        out.extend(kind.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend((blocks as u32).to_le_bytes());
        out.extend((size as u32).to_le_bytes());
        out
    };
    let (mut chunks, mut i) = (Vec::new(), 0);
    while i < blocks.len() {
        let mut j = i;
        if zero(&blocks[i]) {
            while j < blocks.len() && zero(&blocks[j]) {
                j += 1;
            }
            chunks.push(chunk(0xCAC3, j - i, 12));
        } else {
            while j < blocks.len() && !zero(&blocks[j]) {
                j += 1;
            }
            let mut raw = chunk(0xCAC1, j - i, 12 + (j - i) * BLOCK);
            raw.extend(blocks[i..j].concat());
            chunks.push(raw);
        }
        i = j;
    }
    // One fill chunk past the data (as stock images end).
    let mut fill = chunk(0xCAC2, 2, 16);
    fill.extend([0xaa, 0xbb, 0xcc, 0xdd]);
    chunks.push(fill);
    let mut header = Vec::new();
    header.extend(0xED26_FF3Au32.to_le_bytes());
    for half in [1u16, 0, 28, 12] {
        header.extend(half.to_le_bytes());
    }
    for word in [BLOCK, blocks.len() + 2, chunks.len(), 0] {
        header.extend((word as u32).to_le_bytes());
    }
    header.extend(chunks.concat());
    header
}

/// A working mke2fs with -d (host e2fsprogs); the locked container has none.
fn mke2fs() -> Option<String> {
    ["mke2fs", "/build/buildroot/host/sbin/mke2fs"]
        .into_iter()
        .find(|candidate| {
            Command::new(candidate)
                .arg("-V")
                .output()
                .is_ok_and(|o| o.status.success())
        })
        .map(str::to_owned)
}

/// The fixture firmware and the reviewed tables swapped for theirs (setUp).
struct Firmware {
    fixture: Fixture,
    files: Files,
    custom: Vec<u8>,
    reviewed: Reviewed,
}

fn table(files: &Files) -> Vec<(String, u64, String)> {
    files
        .iter()
        .map(|(n, d)| (n.clone(), d.len() as u64, sha256_hex(d)))
        .collect()
}

impl Firmware {
    fn new() -> Self {
        let size = |name: &str| FILES.iter().find(|f| f.0 == name).unwrap().1 as usize;
        let mut cfg = b"coex_wmt_ant_mode=1\n".to_vec();
        cfg.resize(size("WMT_SOC.cfg"), b'\n');
        let files: Files = vec![
            (
                "WIFI_RAM_CODE_MT6582".into(),
                noise(size("WIFI_RAM_CODE_MT6582"), 1),
            ),
            ("WMT_SOC.cfg".into(), cfg),
            (
                "mt6572_82_patch_e1_0_hdr.bin".into(),
                patch_bytes(size("mt6572_82_patch_e1_0_hdr.bin"), 2),
            ),
            (
                "mt6572_82_patch_e1_1_hdr.bin".into(),
                patch_bytes(size("mt6572_82_patch_e1_1_hdr.bin"), 1),
            ),
            (
                "modem_1_2g_n.img".into(),
                noise(size("modem_1_2g_n.img"), 3),
            ),
        ];
        let custom = custom_library();
        let reviewed = Reviewed {
            files: table(&files),
            defaults: table(&defaults(&custom).unwrap()),
            custom: (CUSTOM.0.into(), custom.len() as u64, sha256_hex(&custom)),
        };
        Self {
            fixture: Fixture::new().with_answers(radios()),
            files,
            custom,
            reviewed,
        }
    }

    /// A raw ext4 image (ext3 with block maps for `extents == false`) of a stock system tree, or None
    /// without a host mke2fs (Python skips the test).
    fn image(&self, extents: bool) -> Option<PathBuf> {
        let mke2fs = mke2fs()?;
        let tree = self.fixture.base().join("tree");
        fs::create_dir_all(tree.join("etc/firmware")).unwrap();
        fs::create_dir_all(tree.join("lib")).unwrap();
        for (name, data) in &self.files {
            fs::write(tree.join("etc/firmware").join(name), data).unwrap();
        }
        fs::write(tree.join("lib/libcustom_nvram.so"), &self.custom).unwrap();
        let raw = self.fixture.base().join("raw.ext4");
        let options: &[&str] = if extents {
            &[]
        } else {
            &["-O", "^extent,^64bit", "-t", "ext3"]
        };
        let made = Command::new(mke2fs)
            .args(["-q", "-F", "-b", "4096"])
            .args(options)
            .arg("-d")
            .arg(&tree)
            .arg(&raw)
            .arg("16M")
            .output()
            .unwrap();
        assert!(made.status.success(), "{made:?}");
        fs::remove_dir_all(&tree).unwrap();
        Some(raw)
    }

    /// The sparse system.img as the firmware package has it.
    fn system_img(&self) -> Option<Vec<u8>> {
        let raw = self.image(true)?;
        let packed = sparse(&fs::read(&raw).unwrap());
        fs::remove_file(raw).unwrap();
        Some(packed)
    }

    /// The device root (Python's `ctx`): the responder exports the Firmware drive and the SD card.
    fn root(&self) -> &Root {
        for name in ["/run/y2", "/data", "/media/sd"] {
            self.fixture.mkdir(name);
        }
        self.fixture.put_json(
            "/run/y2/mtp-roots.json",
            &json!({"roots": ["/data/firmware/inbox", "/media/sd"]}),
        );
        &self.fixture.root
    }

    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.reviewed.all().map(|(n, _, _)| n.clone()).collect();
        names.sort();
        names
    }
}

/// The answers of `start()` (the radios restarted), recorded (`fixture.calls()`).
fn radios() -> Vec<(Vec<String>, command::Outcome)> {
    vec![
        ok(&["/etc/init.d/S41y2-connectivity", "stop"], ""),
        ok(&["/etc/init.d/S41y2-connectivity", "start"], ""),
    ]
}

fn argvs(calls: &[(Vec<String>, f64)]) -> Vec<Vec<String>> {
    calls.iter().map(|(argv, _)| argv.clone()).collect()
}

/// `os.utime(path, (old, old))` a minute ago: the copy is complete.
fn settle(path: &Path) {
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(60))
        .unwrap();
}

#[test]
fn sparse_and_raw_ext4_images_with_extents_and_block_maps() {
    let firmware = Firmware::new();
    for extents in [true, false] {
        let Some(raw) = firmware.image(extents) else {
            return; // mke2fs not available
        };
        let packed = firmware.fixture.base().join("system.img");
        fs::write(&packed, sparse(&fs::read(&raw).unwrap())).unwrap();
        for path in [&raw, &packed] {
            let got = from_system_image(&firmware.reviewed, path).unwrap();
            for (name, data) in &firmware.files {
                assert_eq!(get(&got, name), Some(data), "{name}");
            }
            let mut names: Vec<String> = got.iter().map(|(n, _)| n.clone()).collect();
            names.sort();
            assert_eq!(names, firmware.names());
        }
        fs::remove_file(raw).unwrap();
    }
}

#[test]
fn a_wrong_file_is_refused_by_name() {
    let mut firmware = Firmware::new();
    firmware.files[1].1 = vec![b'x'; 80];
    let Some(raw) = firmware.image(true) else {
        return; // mke2fs not available
    };
    let mut reviewed = firmware.reviewed.clone();
    for entry in &mut reviewed.files {
        if entry.0 == "WMT_SOC.cfg" {
            *entry = ("WMT_SOC.cfg".into(), 80, "f".repeat(64));
        }
    }
    assert_eq!(
        from_system_image(&reviewed, &raw).unwrap_err().text(),
        "firmware_mismatch:WMT_SOC.cfg"
    );
    let other = firmware.fixture.base().join("other");
    fs::write(&other, vec![0u8; 8192]).unwrap();
    assert_eq!(
        from_system_image(&firmware.reviewed, &other)
            .unwrap_err()
            .text(),
        "not_ext4"
    );
}

#[test]
fn owner_copy_is_taken_once_complete_then_deleted_and_radios_start() {
    let firmware = Firmware::new();
    let Some(image) = firmware.system_img() else {
        return; // mke2fs not available
    };
    let root = firmware.root();
    let reviewed = &firmware.reviewed;
    assert_eq!(state(root)["state"], json!("Missing"));
    assert_eq!(scan(root, reviewed).unwrap()["state"], json!("Missing"));
    let inbox = root.path(INBOX);
    assert!(fs::read_to_string(inbox.join("README.txt"))
        .unwrap()
        .contains("system.img"));
    let source = inbox.join("system.img");
    fs::write(&source, &image).unwrap();
    // Still arriving: the first pass only notes its size and time.
    assert_eq!(scan(root, reviewed).unwrap()["state"], json!("Missing"));
    settle(&source);
    // The time changed.
    assert_eq!(scan(root, reviewed).unwrap()["state"], json!("Missing"));
    assert_eq!(scan(root, reviewed).unwrap()["state"], json!("Ready"));
    assert!(!source.exists());
    assert_eq!(
        argvs(&firmware.fixture.calls()),
        vec![
            vec!["/etc/init.d/S41y2-connectivity", "stop"],
            vec!["/etc/init.d/S41y2-connectivity", "start"]
        ]
    );
    let target = root.path(TARGET);
    let mut names: Vec<String> = fs::read_dir(&target)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, firmware.names());
    assert_eq!(firmware.fixture.mode(TARGET), 0o700);
    assert!(installed(root, reviewed) && present(root));
    assert_eq!(state(root), json!({"state": "Ready"}));
}

#[test]
fn a_copy_arriving_after_the_install_is_deleted_too() {
    // 8.11 report: Firmware/system.img stayed after the firmware was in place.
    let firmware = Firmware::new();
    let Some(image) = firmware.system_img() else {
        return; // mke2fs not available
    };
    let root = firmware.root();
    let reviewed = &firmware.reviewed;
    let install = root.path("/data/firmware/inbox/system.img");
    fs::create_dir_all(install.parent().unwrap()).unwrap();
    fs::write(&install, &image).unwrap();
    settle(&install);
    scan(root, reviewed).unwrap();
    assert_eq!(scan(root, reviewed).unwrap()["state"], json!("Ready"));
    let sd = root.path("/media/sd/Firmware");
    fs::create_dir_all(&sd).unwrap();
    fs::write(sd.join("system.img"), &image).unwrap();
    settle(&sd.join("system.img"));
    // First sight: may still be arriving.
    assert_eq!(scan(root, reviewed).unwrap(), json!({"state": "Ready"}));
    assert!(sd.join("system.img").exists());
    assert_eq!(scan(root, reviewed).unwrap(), json!({"state": "Ready"}));
    assert!(!sd.join("system.img").exists());
    // Any other file of that name is the owner's and stays.
    fs::write(sd.join("system.img"), vec![0u8; 10000]).unwrap();
    settle(&sd.join("system.img"));
    assert!(leftover_waiting(root));
    for _ in 0..3 {
        scan(root, reviewed).unwrap();
    }
    assert!(sd.join("system.img").exists());
    // Judged once, not every pass.
    assert!(!leftover_waiting(root));
    // The radios not restarted again.
    assert_eq!(firmware.fixture.calls().len(), 2);
    assert_eq!(state(root), json!({"state": "Ready"}));
}

#[test]
fn a_rejected_copy_stays_failed_until_replaced() {
    let firmware = Firmware::new();
    let root = firmware.root();
    let reviewed = &firmware.reviewed;
    let copy = root.path("/media/sd/Firmware/system.img");
    fs::create_dir_all(copy.parent().unwrap()).unwrap();
    fs::write(&copy, vec![0u8; 10000]).unwrap();
    settle(&copy);
    scan(root, reviewed).unwrap();
    let failed = scan(root, reviewed).unwrap();
    assert_eq!(failed["state"], json!("Failed"));
    // Not decoded again.
    assert_eq!(scan(root, reviewed).unwrap()["state"], json!("Failed"));
    assert!(copy.exists());
    assert_eq!(state(root)["state"], json!("Failed"));
    fs::remove_file(&copy).unwrap();
    assert_eq!(scan(root, reviewed).unwrap()["state"], json!("Missing"));
}

#[test]
fn with_internal_storage_the_copy_goes_into_its_firmware_folder() {
    // #39: Internal Storage has room for any system.img; the folder goes away afterwards.
    let firmware = Firmware::new();
    let Some(image) = firmware.system_img() else {
        return; // mke2fs not available
    };
    let root = firmware.root();
    let reviewed = &firmware.reviewed;
    let fixture = &firmware.fixture;
    // No computer connected.
    fixture.put_json("/run/y2/mtp-roots.json", &json!({"roots": []}));
    fixture.mkdir("/media/internal/music");
    // `internal_media.ready` (patched True in Python): the recorded Y2Media mount is the live one.
    let device = "../../devices/platform/11230000.mmc/mmcblk0p8";
    fixture.put("/proc/sys/kernel/random/boot_id", "boot-a\n");
    fixture.put(
        "/proc/self/mountinfo",
        &format!("40 30 {device} / /media/internal rw - ext4 /dev/mmcblk0p8 rw\n"),
    );
    fixture.put_json(
        "/run/y2/internal-media.json",
        &json!({"state": "Ready", "boot_id": "boot-a", "mount_id": 40, "device_id": device}),
    );
    assert!(internal(root));
    assert_eq!(state(root)["location"], json!("internal"));
    scan(root, reviewed).unwrap();
    let folder = root.path(INTERNAL_INBOX);
    assert!(fs::read_to_string(folder.join("README.txt"))
        .unwrap()
        .contains("folder named Firmware on the SD card"));
    // No Y2DATA drive folder.
    assert!(!root.path(INBOX).exists());
    let listed = candidates(root);
    assert_eq!(
        listed[..3],
        [
            folder.join("system.img"),
            root.path("/data/firmware/inbox/system.img"),
            root.path("/media/sd/Firmware/system.img")
        ]
    );
    let source = folder.join("system.img");
    fs::write(&source, &image).unwrap();
    settle(&source);
    scan(root, reviewed).unwrap();
    assert_eq!(scan(root, reviewed).unwrap()["state"], json!("Ready"));
    // Note and folder removed.
    assert!(!folder.exists());
    assert!(installed(root, reviewed));
}

#[test]
fn a_copy_refused_for_space_says_so() {
    let firmware = Firmware::new();
    let root = firmware.root();
    assert!(state(root).get("reason").is_none());
    firmware.fixture.put_json(
        "/run/y2/mtp-events.json",
        &json!({"schema": 1, "active": false, "error": 28}),
    );
    let value = state(root);
    assert_eq!(
        (&value["state"], &value["reason"]),
        (&json!("Missing"), &json!("no_space"))
    );
}

#[test]
fn the_real_stock_system_image() {
    let stock = repository().join("../Y2Player/y2_v3.2.0_FM-20260813/system.img");
    if !stock.is_file() {
        return; // owner stock package not present
    }
    let reviewed = Reviewed::stock();
    let got = from_system_image(&reviewed, &stock).unwrap();
    let mut got = table(&got);
    got.sort();
    let mut all: Vec<_> = reviewed.all().cloned().collect();
    all.sort();
    assert_eq!(got, all);
}

#[test]
fn one_reviewed_list_everywhere() {
    // The inventory and tools/connectivity/provision.py (the host provisioning) name the same files.
    let inventory: Value = serde_json::from_str(
        &fs::read_to_string(
            repository().join("docs/knowledge/evidence/m5-firmware-inventory.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let mut listed: Vec<(String, u64, String)> = inventory["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| {
            (
                x["filename"].as_str().unwrap().to_owned(),
                x["bytes"].as_u64().unwrap(),
                x["sha256"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let mut files = Reviewed::stock().files;
    listed.sort();
    files.sort();
    assert_eq!(files, listed);
    let provision =
        fs::read_to_string(repository().join("tools/connectivity/provision.py")).unwrap();
    let defaults: Vec<(String, u64, String)> =
        regex::Regex::new(r"'([A-Z]+\.defaults)': \((\d+), '([0-9a-f]{64})'\)")
            .unwrap()
            .captures_iter(&provision)
            .map(|c| (c[1].to_owned(), c[2].parse().unwrap(), c[3].to_owned()))
            .collect();
    assert_eq!(defaults, Reviewed::stock().defaults);
    assert!(provision.contains(&format!("CUSTOM_SHA = '{}'", CUSTOM.2)));
    assert!(provision.contains(&format!("CUSTOM_SIZE = {}\n", CUSTOM.1)));
    // recovery.sh keeps exactly these files of an old root.
    let script = fs::read_to_string(repository().join("initramfs/production/recovery.sh")).unwrap();
    let block = script
        .split_once("RADIO_FIRMWARE='")
        .unwrap()
        .1
        .split_once('\'')
        .unwrap()
        .0;
    let mut kept: Vec<(String, String)> = block
        .lines()
        .map(|line| {
            let (sha, name) = line.split_once("  ").unwrap();
            (name.to_owned(), sha.to_owned())
        })
        .collect();
    let mut all: Vec<(String, String)> = Reviewed::stock()
        .all()
        .map(|(n, _, h)| (n.clone(), h.clone()))
        .collect();
    kept.sort();
    all.sort();
    assert_eq!(kept, all);
}
