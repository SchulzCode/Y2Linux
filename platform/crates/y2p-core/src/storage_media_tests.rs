//! fsck.fat's findings as the SD admission classifies them (from tests/test_platform_storage_tools.py
//! and test_filesystems.py): texts dosfstools printed, and real checker runs on newly created host
//! scratch images.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::root::tests::tempdir::Dir;
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output},
};

// fsck.fat 4.2 -n on host images holding what an interrupted write leaves
// (2026-10-23, scratchpad tools/fatdamage.py): FAT copies differ, an
// orphaned long name; and three real ones (chain shorter than the size, a
// free first cluster, a shared chain).
const FAT_COPIES: &str =
    "FATs differ but appear to be intact.\n  Using first FAT.\nReclaimed 1 unused cluster (4096 bytes).\n";
const ORPHAN: &str = "Orphaned long file name part \"Song 2.mp3\"\n  Auto-deleting.\n\
Reclaimed 79 unused clusters (323584 bytes).\n";
const INTERRUPTED: [(&str, &str); 3] = [
    (
        "chain shorter",
        "/Music/Song 0.mp3\n  File size is 300000 bytes, cluster chain length is 16384 bytes.\n  \
         Truncating file to 16384 bytes.\n",
    ),
    (
        "free first cluster",
        "/Music/Song 1.mp3\n  Contains a free cluster (78). Assuming EOF.\n",
    ),
    (
        "shared chain",
        "/Music/Song 0.mp3  and\n/Music/Song 1.mp3\n  share clusters.\n  Truncating second to 0 bytes.\n",
    ),
];
const LOST: &str = "Reclaimed 4 unused clusters (16384 bytes).\n";

#[test]
fn interrupted_write_leftovers_linux_never_sees_are_admitted() {
    assert_eq!(
        fat_findings(Some(FAT_COPIES)),
        ["fat_copies", "lost_clusters"]
    );
    assert_eq!(fat_findings(Some(ORPHAN)), ["lost_clusters", "orphan_lfn"]);
    assert!(fat_benign_only(Some(FAT_COPIES)) && fat_benign_only(Some(ORPHAN)));
    for (name, lines) in INTERRUPTED {
        assert!(!fat_benign_only(Some(lines)), "{name}");
    }
    // The answers count only right after their own finding.
    assert!(!fat_benign_only(Some(&format!(
        "{LOST}  Using first FAT.\n"
    ))));
    assert!(!fat_benign_only(Some(&format!("{LOST}  Auto-deleting.\n"))));
}

// ---- real images (test_filesystems.py) ----------------------------------------------------------

fn tool(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|d| {
            d.join(name)
                .metadata()
                .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
        })
    })
}

/// A sparse 64 MiB scratch image.
fn image(directory: &Dir) -> std::path::PathBuf {
    let path = directory.path().join("scratch.img");
    File::create_new(&path).unwrap().set_len(64 << 20).unwrap();
    path
}

fn digest(path: &Path) -> String {
    sha256_hex(&fs::read(path).unwrap())
}

fn run(locale: &str, argv: &[&str]) -> Output {
    Command::new(argv[0])
        .args(&argv[1..])
        .env("LC_ALL", locale)
        .output()
        .unwrap()
}

/// The tool as the setup ran it (LC_ALL=C.UTF-8): its exit status.
fn made(argv: &[&str]) -> i32 {
    run("C.UTF-8", argv).status.code().unwrap()
}

/// `fsck.fat -n` as the platform runs it (LC_ALL=C): exit status and stdout.
fn check(image: &Path) -> (i32, String) {
    let output = run("C", &["fsck.fat", "-n", image.to_str().unwrap()]);
    (
        output.status.code().unwrap(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

fn edit(image: &Path, offset: u64, data: &[u8]) {
    let mut file = File::options().write(true).open(image).unwrap();
    file.seek(SeekFrom::Start(offset)).unwrap();
    file.write_all(data).unwrap();
}

/// FAT32 boot sector fields: bytes per sector, reserved sectors, FAT count, sectors per FAT,
/// sectors per cluster, FSInfo sector.
struct Geometry {
    sector: u64,
    reserved: u64,
    fats: u64,
    fat_sectors: u64,
    cluster: u64,
    fsinfo: u64,
}

fn geometry(image: &Path) -> Geometry {
    let mut boot = [0u8; 512];
    File::open(image).unwrap().read_exact(&mut boot).unwrap();
    let u16_at = |o: usize| u64::from(u16::from_le_bytes([boot[o], boot[o + 1]]));
    Geometry {
        sector: u16_at(11),
        reserved: u16_at(14),
        fats: u64::from(boot[16]),
        fat_sectors: u64::from(u32::from_le_bytes(boot[36..40].try_into().unwrap())),
        cluster: u64::from(boot[13]),
        fsinfo: u16_at(48),
    }
}

#[test]
fn fat32_unicode_long_names_hashes_and_read_only_dirty_check() {
    if !tool("mkfs.fat") || !tool("fsck.fat") || !tool("mcopy") {
        eprintln!("skipped: native FAT tools required");
        return;
    }
    let directory = Dir::new();
    let image = image(&directory);
    let name = image.to_str().unwrap();
    assert_eq!(made(&["mkfs.fat", "-F", "32", name]), 0);
    let original = directory
        .path()
        .join(format!("音楽-Été-{}.flac", "a".repeat(180)));
    let data: Vec<u8> = (0..4096).flat_map(|_| 0..=255u8).collect();
    fs::write(&original, &data).unwrap();
    assert_eq!(
        made(&["mcopy", "-i", name, original.to_str().unwrap(), "::"]),
        0
    );
    let copy = directory.path().join("readback");
    let inside = format!("::{}", original.file_name().unwrap().to_str().unwrap());
    assert_eq!(
        made(&["mcopy", "-i", name, &inside, copy.to_str().unwrap()]),
        0
    );
    assert_eq!(digest(&original), digest(&copy));
    let before = digest(&image);
    assert_eq!(made(&["fsck.fat", "-n", name]), 0);
    assert_eq!(digest(&image), before);
    // Only this new image: clear FAT32's clean-unmount bit in both FATs.
    let g = geometry(&image);
    for index in 0..g.fats {
        let offset = (g.reserved + index * g.fat_sectors) * g.sector + 4;
        let mut flags = [0u8; 4];
        let mut file = File::open(&image).unwrap();
        file.seek(SeekFrom::Start(offset)).unwrap();
        file.read_exact(&mut flags).unwrap();
        let flags = u32::from_le_bytes(flags) & !(1 << 27);
        edit(&image, offset, &flags.to_le_bytes());
    }
    let dirty = digest(&image);
    let checked = run("C.UTF-8", &["fsck.fat", "-n", name]);
    assert!(String::from_utf8_lossy(&checked.stdout).contains("Dirty bit is set"));
    assert!(!checked.status.success());
    assert_eq!(digest(&image), dirty);
    // Issue #44: the Linux driver also sets the boot sector's dirty
    // flag (offset 65), so it differs from its backup. A card left
    // like this (pulled while mounted) is admitted; checker output as
    // the platform runs it (LC_ALL=C).
    edit(&image, 65, &[1]);
    let (status, unclean) = check(&image);
    assert_eq!(status, 1);
    assert!(unclean.contains("65:01/00"));
    assert!(fat_benign_only(Some(&unclean)), "{unclean}");
    // A real finding on top (a file longer than its clusters) is not.
    let root = (g.reserved + g.fats * g.fat_sectors) * g.sector;
    let mut entries = vec![0u8; (g.sector * g.cluster) as usize];
    let mut file = File::open(&image).unwrap();
    file.seek(SeekFrom::Start(root)).unwrap();
    file.read_exact(&mut entries).unwrap();
    let index = (0..entries.len())
        .step_by(32)
        .find(|&i| ![0x0F, 0x08].contains(&entries[i + 11]) && ![0, 0xE5].contains(&entries[i]))
        .unwrap();
    edit(
        &image,
        root + index as u64 + 28,
        &(4u32 << 20).to_le_bytes(),
    );
    let (status, broken) = check(&image);
    assert_ne!(status, 0);
    assert!(!fat_benign_only(Some(&broken)), "{broken}");
}

#[test]
fn fat_findings_windows_mounts_over_on_real_images() {
    // F12 (Reddit "needs repair", fine on the PC): what a card pulled while
    // mounted or relabelled on Windows looks like to the player's checker.
    if !tool("mkfs.fat") || !tool("fsck.fat") {
        eprintln!("skipped: native FAT tools required");
        return;
    }
    let directory = Dir::new();
    let image = image(&directory);
    assert_eq!(
        made(&["mkfs.fat", "-F", "32", "-n", "Y2", image.to_str().unwrap()]),
        0
    );
    let g = geometry(&image);
    edit(&image, 65, &[1]); // dirty (#44)
    edit(&image, g.fsinfo * g.sector + 488, &1234u32.to_le_bytes()); // stale free count
    for index in 0..g.fats {
        // an unused chain still marked used
        let fat = (g.reserved + index * g.fat_sectors) * g.sector;
        for cluster in 5000u32..5004 {
            let next = if cluster < 5003 {
                cluster + 1
            } else {
                0x0FFF_FFFF
            };
            edit(&image, fat + u64::from(cluster) * 4, &next.to_le_bytes());
        }
    }
    edit(&image, 3, b"MSWIN4.1"); // backup differs elsewhere
    let before = digest(&image);
    let (status, output) = check(&image);
    assert_eq!(status, 1, "{output}");
    assert_eq!(digest(&image), before);
    assert_eq!(
        fat_findings(Some(&output)),
        ["backup_differs", "dirty", "fsinfo_free", "lost_clusters"]
    );
    assert!(fat_benign_only(Some(&output)), "{output}");
}
