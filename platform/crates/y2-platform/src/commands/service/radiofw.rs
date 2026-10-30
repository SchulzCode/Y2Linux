//! `y2-platform firmware`: radio firmware from the owner's own copy (RELEASE-49).
//!
//! Y2Linux images carry no MediaTek radio firmware: it may not be redistributed. Each player keeps the
//! reviewed files on Y2DATA (`/data/firmware/mt6582`, which `/lib/firmware/mediatek/mt6582` links
//! to). They come from the owner's copy: on a player that ran an older image, the recovery copies them
//! from the old root before it writes the new one (recovery.sh); otherwise the owner copies
//! `system.img` from the Innioasis Y2 firmware package into the `Firmware` folder over USB (on Internal
//! Storage, or the Firmware drive on Y2DATA while there is no Internal Storage); this module reads the
//! Android sparse ext4 image itself, checks the five files against the reviewed SHA-256 values,
//! derives the two board defaults like tools/connectivity/provision.py, installs them, deletes the copy
//! and starts the radios. Only names, sizes and hashes are in this file; never firmware bytes.
// SPDX-License-Identifier: GPL-2.0-only
use super::image::{self, Ext4, Image};
use crate::commands::storage::media;
use crate::reply::{Failure, Outcome, Reply};
use serde_json::{json, Map, Value};
use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use y2p_core::mounts::mounts_at;
use y2p_core::{
    command,
    digest::sha256_hex,
    json::{Object, ObjectExt},
    py, record, Root,
};

mod copy;
mod files;

pub(crate) use self::copy::inbox;
use self::{copy::*, files::*};
pub use self::{
    copy::{candidates, internal, leftover_waiting},
    files::present,
};

/// docs/knowledge/evidence/m5-firmware-inventory.json (stock v3.2.0 FM, /etc/firmware).
pub const FILES: [(&str, u64, &str); 5] = [
    (
        "WIFI_RAM_CODE_MT6582",
        160368,
        "4def37522e2bfd2d4b78e01bf9f567a37e3f67ab3d7b6646ffb317f5b2b49b82",
    ),
    (
        "WMT_SOC.cfg",
        80,
        "cea0a94b8a88e9c24e6835446ebc3106d7b100c3709ea98e8bb7368689c8a2b5",
    ),
    (
        "mt6572_82_patch_e1_0_hdr.bin",
        33120,
        "fa3be4a92540296de512d64f56272746be10bdfa2db571c8ceada2d40cd616f0",
    ),
    (
        "mt6572_82_patch_e1_1_hdr.bin",
        17476,
        "96eb25489305bd472805db1da33bcc0acd37b5bb21cceda98d7e88ba1179ca37",
    ),
    (
        "modem_1_2g_n.img",
        2424376,
        "5059775975cbf6ab74c43978ca8f65d9a274b83585f456134e39b09f6dc7a4f1",
    ),
];
/// provision.py: generic board defaults derived from the stock reader library.
pub const DEFAULTS: [(&str, u64, &str); 2] = [
    (
        "WIFI.defaults",
        512,
        "eb5e87224e3a493e825179f547fc13924f564a42730ea821b4359b5b36f4defe",
    ),
    (
        "BT.defaults",
        64,
        "f224ff4abfb45a612ef03249e87e4ecfa44b7c1f9f19ee3cc78ae354764d9472",
    ),
];
pub const CUSTOM: (&str, u64, &str) = (
    "/lib/libcustom_nvram.so",
    51224,
    "c6460e9649f539b56449dc19f8d20771402f539be9e73340a74a9ccf45287397",
);

/// The reviewed tables (a test swaps them for its fixture files', as tests/test_radio_firmware.py).
#[derive(Clone)]
pub struct Reviewed {
    pub files: Vec<(String, u64, String)>,
    pub defaults: Vec<(String, u64, String)>,
    pub custom: (String, u64, String),
}

impl Reviewed {
    pub fn stock() -> Self {
        let own = |t: &[(&str, u64, &str)]| {
            t.iter()
                .map(|(n, s, h)| (n.to_string(), *s, h.to_string()))
                .collect()
        };
        Self {
            files: own(&FILES),
            defaults: own(&DEFAULTS),
            custom: (CUSTOM.0.into(), CUSTOM.1, CUSTOM.2.into()),
        }
    }

    fn all(&self) -> impl Iterator<Item = &(String, u64, String)> {
        self.files.iter().chain(self.defaults.iter())
    }
}

const TARGET: &str = "/data/firmware/mt6582";
/// While the firmware is missing the computer sees this folder as the MTP storage "Firmware"
/// (usb.media_views); nothing else of Y2DATA is exported.
pub(crate) const INBOX: &str = "/data/firmware/inbox";
pub const STATE: &str = "/run/y2/radio-firmware.json";
const FOLDER: &str = "Firmware";
const SOURCE: &str = "system.img";
/// With Internal Storage the copy goes there (#39: Y2DATA has room for the zip's 518 MB image, never
/// for an unpacked 860 MB one).
const INTERNAL_INBOX: &str = "/media/internal/music/Firmware";
const SD_INBOX: &str = "/media/sd/Firmware";
const ENOSPC: i64 = 28;
const README: &str =
    "Wi-Fi and Bluetooth need the radio firmware from the Innioasis Y2 firmware package.\n\
Copy (do not move) the file system.img from that package into this folder (about\n\
520 MB). The player checks it, keeps the five small radio files and deletes the copy.\n\
A larger system.img (about 860 MB) is an unpacked copy: if there is not enough space\n\
here, put it into a folder named Firmware on the SD card instead.\n";

/// What radiofw raises (its callers catch OSError and ValueError alike, by text).
#[derive(Debug)]
pub enum Error {
    Value(String),
    Os(String),
}

impl Error {
    pub fn text(&self) -> String {
        match self {
            Error::Value(t) | Error::Os(t) => t.clone(),
        }
    }
}

/// An `OSError` naming no file.
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Error {
        Error::Os(y2p_core::py::os_error(&error, None).0)
    }
}

impl From<image::Error> for Error {
    fn from(error: image::Error) -> Error {
        match error {
            image::Error::Value(t) => Error::Value(t),
            image::Error::Os(t) => Error::Os(t),
            // FileNotFoundError(path): only from a lookup, which every caller turns into a ValueError.
            image::Error::NotFound => Error::Value("not_the_y2_firmware_package".into()),
        }
    }
}

impl From<Failure> for Error {
    fn from(failure: Failure) -> Error {
        match failure {
            Failure::Value(t) => Error::Value(t),
            other => Error::Os(other.message()),
        }
    }
}

fn os_path(error: std::io::Error, path: &Path) -> Error {
    Error::Os(py::os_error(&error, Some(&path.to_string_lossy())).0)
}

type Result<T> = std::result::Result<T, Error>;
type Files = Vec<(String, Vec<u8>)>;

/// (re)start the radios with the installed firmware. Stopping running radios can take
/// longer than a short command limit, which on 49 cut the combined restart off after its stop half
/// (archive 154).
fn start(root: &Root) {
    command::run(root, &["/etc/init.d/S41y2-connectivity", "stop"], 90.0);
    command::run(root, &["/etc/init.d/S41y2-connectivity", "start"], 30.0);
}

/// `y2-platform firmware <action> [path]`.
pub fn run(root: &Root, action: &str, path: Option<&str>) -> Outcome {
    let reviewed = Reviewed::stock();
    let result = (|| -> Result<Value> {
        match action {
            "status" => Ok(state(root)),
            "scan" => scan(root, &reviewed),
            _ => {
                let path = path.filter(|p| !p.is_empty()).ok_or_else(|| {
                    Error::Value("install needs a system.img or a directory".into())
                })?;
                let source = PathBuf::from(path);
                let files = if source.is_dir() {
                    from_directory(&reviewed, &source)?
                } else {
                    from_system_image(&reviewed, &source)?
                };
                install(root, &reviewed, files)?;
                start(root);
                Ok(json!({"state": "Ready"}))
            }
        }
    })();
    match result {
        // A scan whose copy could not be installed exits 1 as an install does (it exited 0 with
        // state Failed); the service ignores the code and reads the state record.
        Ok(value) => Reply::sorted(
            &value,
            u8::from(value.get("state") == Some(&json!("Failed"))),
        ),
        Err(error) => Reply::unsorted(
            &Object::new()
                .with("state", "Failed")
                .with("failure", error.text()),
            1,
        ),
    }
}

#[cfg(test)]
#[path = "radiofw_tests.rs"]
mod radiofw_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Fixture;
    use std::time::Duration;

    #[test]
    fn a_scan_whose_copy_cannot_be_installed_exits_1() {
        let fixture = Fixture::new();
        let image = format!("{INBOX}/{SOURCE}");
        fixture.put(&image, "not a system image");
        // A finished copy: older than the arrival window, and seen unchanged on the pass before.
        let file = fs::File::options()
            .write(true)
            .open(fixture.path(&image))
            .unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(60))
            .unwrap();
        let (seen, arriving) = seen(&file.metadata().unwrap());
        assert!(!arriving);
        fixture.put_json(
            STATE,
            &json!({"schema": 1, "state": "Missing", "seen": seen}),
        );
        let reply = run(&fixture.root, "scan", None).unwrap();
        assert_eq!(reply.code, 1, "{}", reply.stdout);
        assert_eq!(fixture.json(STATE)["state"], json!("Failed"));
        // The next pass reports the same failure, still as one.
        assert_eq!(run(&fixture.root, "scan", None).unwrap().code, 1);
    }

    fn patch(sequence: u8) -> Vec<u8> {
        let mut data = vec![7u8; 64];
        data[18..20].copy_from_slice(b"PS");
        data[24] = 0x20 | sequence;
        data
    }

    fn custom_library() -> Vec<u8> {
        let mut data: Vec<u8> = (0..51224u32).map(|i| (i * 13 % 256) as u8).collect();
        data[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
        data[0x5a8e..0x5a92].copy_from_slice(&[0x04, 0x01, 0, 0]);
        data[0x5a8e + 196..0x5a8e + 198].copy_from_slice(b"\x01\x00");
        data
    }

    /// tests/test_radio_firmware.py: fixture files with the reviewed tables swapped for theirs.
    fn fixture() -> (Reviewed, Files, Vec<u8>) {
        let files: Files = vec![
            ("WIFI_RAM_CODE_MT6582".into(), vec![1u8; 5000]),
            ("WMT_SOC.cfg".into(), b"coex_wmt_ant_mode=1\n".to_vec()),
            ("mt6572_82_patch_e1_0_hdr.bin".into(), patch(2)),
            ("mt6572_82_patch_e1_1_hdr.bin".into(), patch(1)),
            ("modem_1_2g_n.img".into(), vec![9u8; 7000]),
        ];
        let custom = custom_library();
        let derived = defaults(&custom).unwrap();
        let table = |f: &Files| {
            f.iter()
                .map(|(n, d)| (n.clone(), d.len() as u64, sha256_hex(d)))
                .collect()
        };
        let reviewed = Reviewed {
            files: table(&files),
            defaults: table(&derived),
            custom: (CUSTOM.0.into(), custom.len() as u64, sha256_hex(&custom)),
        };
        (reviewed, files, custom)
    }

    #[test]
    fn a_system_image_installs_and_its_copy_goes() {
        let (reviewed, files, custom) = fixture();
        let dir = std::env::temp_dir().join(format!("y2-radiofw-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let tree = dir.join("tree");
        fs::create_dir_all(tree.join("etc/firmware")).unwrap();
        fs::create_dir_all(tree.join("lib")).unwrap();
        for (name, data) in &files {
            fs::write(tree.join("etc/firmware").join(name), data).unwrap();
        }
        fs::write(tree.join("lib/libcustom_nvram.so"), &custom).unwrap();
        let root = Root::new(dir.join("root"));
        fs::create_dir_all(root.path("/run/y2")).unwrap();
        let image = root.path(INBOX).join(SOURCE);
        fs::create_dir_all(image.parent().unwrap()).unwrap();
        let made = std::process::Command::new("mke2fs")
            .args(["-q", "-F", "-b", "4096", "-d"])
            .arg(&tree)
            .arg(&image)
            .arg("8M")
            .output()
            .is_ok_and(|o| o.status.success());
        if !made {
            let _ = fs::remove_dir_all(&dir);
            return; // no host mke2fs
        }
        let old = SystemTime::now() - Duration::from_secs(60);
        fs::File::options()
            .write(true)
            .open(&image)
            .unwrap()
            .set_modified(old)
            .unwrap();
        // The first pass only notes the copy; the second takes it.
        assert_eq!(scan(&root, &reviewed).unwrap(), json!({"state": "Missing"}));
        assert_eq!(scan(&root, &reviewed).unwrap(), json!({"state": "Ready"}));
        assert!(!image.exists());
        assert!(installed(&root, &reviewed));
        let wifi = fs::read(root.path(TARGET).join("WIFI.defaults")).unwrap();
        assert_eq!(wifi.len(), 512);
        assert_eq!(&wifi[4..10], &[0; 6]);
        assert_eq!(
            root.json(STATE).unwrap(),
            json!({"schema": 1, "state": "Ready", "installed_from": "system.img"})
        );
        // A second copy is judged and deleted by leftover() (the stock tables reject it: kept).
        assert_eq!(scan(&root, &reviewed).unwrap(), json!({"state": "Ready"}));
        // from_directory reads the installed set back; checked() refuses a wrong patch order.
        let back = from_directory(&reviewed, &root.path(TARGET)).unwrap();
        let mut swapped = back.clone();
        for item in &mut swapped {
            if item.0.ends_with("_hdr.bin") {
                item.1[24] = 0x21;
            }
        }
        let mut reordered = reviewed.clone();
        for (name, size, sha) in &mut reordered.files {
            if let Some(data) = get(&swapped, name) {
                *size = data.len() as u64;
                *sha = sha256_hex(data);
            }
        }
        assert_eq!(
            checked(&reordered, swapped).unwrap_err().text(),
            "firmware_patch_order"
        );
        let stock = Reviewed::stock();
        assert_eq!(
            checked(&stock, back).unwrap_err().text(),
            "firmware_mismatch:WIFI_RAM_CODE_MT6582"
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
