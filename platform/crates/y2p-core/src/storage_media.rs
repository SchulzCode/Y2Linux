//! The pure parts of the storage commands: the `fsck.fat -n` line classes (#44, F12), the repair
//! summary, the boot sector's table size, the media identity hash and the disposable-file names of
//! the space cleanup. The patterns are Unicode-aware (`\d`, `\S`, `\s`, `\b`), as the classes were
//! recorded with.
// SPDX-License-Identifier: GPL-2.0-only
pub use crate::text::splitlines;
use crate::text::strip;
use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, sync::OnceLock};

fn full(pattern: &str) -> Regex {
    Regex::new(&format!(r"\A(?:{pattern})\z")).expect("pattern")
}

// fsck.fat -n exits 1 whenever it would change anything, including only the
// dirty bit a FAT card keeps after it was pulled out while mounted (Android,
// Windows and macOS mount such a card; issue #44). Windows also mounts a card
// whose free-cluster count (FSInfo) is stale, whose volume label differs
// between the root directory and the boot sector (Windows renames only the
// former), or with unused clusters still marked used; the Linux FAT driver
// recounts free space and none of them touches a file (Reddit: "needs repair"
// on a card the PC read; F12). Each such line has a class; any other line
// (file chains, directory entries, sizes, names) is a real finding and keeps
// the card out.
const FAT_LINES: &[(Option<&str>, &str)] = &[
    (None, r"fsck\.fat \S+ \(.*\)"),
    (None, r"Leaving filesystem unchanged\."),
    (None, r"\S+: \d+ files, \d+/\d+ clusters"),
    (
        Some("backup"),
        r"There are differences between boot sector and its backup\.",
    ),
    (
        Some("backup"),
        r"This is mostly harmless\. Differences: \(offset:original/backup\)",
    ),
    (
        Some("backup"),
        r"(?:, )?\d+:[0-9a-f]{2}/[0-9a-f]{2}(?:, \d+:[0-9a-f]{2}/[0-9a-f]{2})*",
    ),
    (Some("backup"), r"Not automatically fixing this\."),
    (
        Some("dirty"),
        r"Dirty bit is set\. Fs was not properly unmounted and some data may be corrupt\.",
    ),
    (Some("dirty"), r"Automatically removing dirty bit\."),
    (
        Some("fsinfo_free"),
        r"Free cluster summary wrong \(\d+ vs\. really \d+\)",
    ),
    (
        Some("fsinfo_free"),
        r"Free cluster summary uninitialized \(should be \d+\)",
    ),
    (
        Some("label_mismatch"),
        r"Volume label '.*' stored in root directory and label '.*' stored in boot sector and different\.",
    ),
    (
        Some("label_mismatch"),
        r"Auto-copying volume label from root directory to boot sector\.",
    ),
    // A label written by another system's tools: invalid characters, lowercase.
    (
        Some("label_mismatch"),
        r"Volume label '.*' stored in root directory is not valid\.",
    ),
    (
        Some("label_mismatch"),
        r"Volume label '.*' stored in root directory contains lowercase characters\.",
    ),
    (
        Some("lost_clusters"),
        r"Reclaimed \d+ unused clusters? \(\d+ bytes\)\.",
    ),
    // A card pulled between the two FAT copies (Linux reads the first) and the
    // long-name part of an entry whose short entry was never written: neither is
    // visible to Linux or Windows (host reproduction of interrupted writes).
    (Some("fat_copies"), r"FATs differ but appear to be intact\."),
    (Some("orphan_lfn"), r#"Orphaned long file name part ".*""#),
];

/// The answer line a finding prints next; accepted only right after its finding.
fn fat_follow(line: &str) -> Option<&'static str> {
    Some(match line {
        "Auto-correcting." => "fsinfo_free",
        "Auto-removing label." => "label_mismatch",
        "Auto-changing lowercase characters to uppercase" => "label_mismatch",
        "Using first FAT." => "fat_copies",
        "Auto-deleting." => "orphan_lfn",
        _ => return None,
    })
}

/// The boot sector's own dirty flag (offset 37 on FAT12/16, 65 on FAT32) also
/// differs from its backup on a card pulled while mounted.
const FAT_DIRTY_OFFSETS: [&str; 2] = ["37", "65"];

fn fat_lines() -> &'static [(Option<&'static str>, Regex)] {
    static LINES: OnceLock<Vec<(Option<&'static str>, Regex)>> = OnceLock::new();
    LINES.get_or_init(|| FAT_LINES.iter().map(|(c, p)| (*c, full(p))).collect())
}

/// The class of one stripped line: `Some(None)` for a known neutral line, `None` for 'other'.
fn classify(line: &str) -> Option<Option<&'static str>> {
    fat_lines()
        .iter()
        .find(|(_, pattern)| pattern.is_match(line))
        .map(|(class, _)| *class)
}

/// The class of the line before: neutral, a class name or `other`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Previous {
    Neutral,
    Class(&'static str),
}

fn class_of(line: &str) -> Previous {
    match classify(line) {
        Some(None) => Previous::Neutral,
        Some(Some(class)) => Previous::Class(class),
        None => Previous::Class("other"),
    }
}

/// The non-empty stripped lines of a run with the answer lines that follow their finding left out,
/// each with its class.
fn classified(output: &str) -> Vec<(&str, Previous)> {
    let mut lines = Vec::new();
    let mut previous: Option<Previous> = None;
    for line in splitlines(output) {
        let line = strip(line);
        if line.is_empty() {
            continue;
        }
        if let Some(follows) = fat_follow(line) {
            if previous == Some(Previous::Class(follows)) {
                continue;
            }
        }
        let class = class_of(line);
        lines.push((line, class));
        previous = Some(class);
    }
    lines
}

fn offset_pairs() -> &'static Regex {
    static PAIRS: OnceLock<Regex> = OnceLock::new();
    PAIRS.get_or_init(|| Regex::new(r"\d+:[0-9a-f]{2}/[0-9a-f]{2}").expect("pattern"))
}

/// The classes a `fsck.fat -n` run reported, sorted; 'other' for any line that
/// is not a benign class. Never any file or label text.
pub fn fat_findings(output: Option<&str>) -> Vec<String> {
    let mut found = BTreeSet::new();
    let mut offsets = BTreeSet::new();
    for (line, class) in classified(output.unwrap_or_default()) {
        if class == Previous::Class("backup") && line.contains(':') && line.contains('/') {
            for item in offset_pairs().find_iter(line) {
                offsets.insert(
                    item.as_str()
                        .split(':')
                        .next()
                        .unwrap_or_default()
                        .to_owned(),
                );
            }
        }
        if let Previous::Class(name) = class {
            if name != "backup" {
                found.insert(name.to_owned());
            }
        }
    }
    if offsets
        .iter()
        .any(|o| !FAT_DIRTY_OFFSETS.contains(&o.as_str()))
    {
        found.insert("backup_differs".to_owned());
    }
    found.into_iter().collect()
}

fn quoted() -> &'static Regex {
    static QUOTED: OnceLock<Regex> = OnceLock::new();
    QUOTED.get_or_init(|| Regex::new(r#"'[^']*'|"[^"]*""#).expect("pattern"))
}

fn digits() -> &'static Regex {
    static DIGITS: OnceLock<Regex> = OnceLock::new();
    DIGITS.get_or_init(|| Regex::new(r"\d+").expect("pattern"))
}

/// What the 'other' lines of a `fsck.fat -n` run said, never a name (F12,
/// 29ed4ccb: refusals on 8.25 could not be told from a benign line nobody had classified). A line
/// with a path (dosfstools prints every file and directory as one) only counts; quoted text and
/// numbers are blanked.
pub fn fat_other_shapes(output: Option<&str>, limit: usize) -> Value {
    let mut shapes: Vec<String> = Vec::new();
    let mut paths = 0u64;
    for (line, class) in classified(output.unwrap_or_default()) {
        if class != Previous::Class("other") {
            continue;
        }
        if line.contains('/') {
            paths += 1;
            continue;
        }
        let blanked = quoted().replace_all(line, "'…'");
        let shape: String = digits()
            .replace_all(&blanked, "#")
            .chars()
            .take(100)
            .collect();
        if !shapes.contains(&shape) && shapes.len() < limit {
            shapes.push(shape);
        }
    }
    json!({"shapes": shapes, "path_lines": paths})
}

/// Whether a failed `fsck.fat -n` reported only findings Windows mounts
/// over: an unclean unmount, a stale free count, a label mismatch, unused clusters, a differing backup
/// boot sector.
pub fn fat_benign_only(output: Option<&str>) -> bool {
    let found = fat_findings(output);
    !found.is_empty() && !found.iter().any(|f| f == "other")
}

fn repair_patterns() -> &'static [Regex; 4] {
    static PATTERNS: OnceLock<[Regex; 4]> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            Regex::new(r"Truncating (?:file|second) to \d+ bytes"),
            Regex::new(r"(?m)^\s*(?:Auto-deleting|Deleting|Dropping)\b"),
            Regex::new(r"share clusters"),
            Regex::new(r"Reclaimed (\d+) unused clusters?"),
        ]
        .map(|p| p.expect("pattern"))
    })
}

/// What a repair changed, as counts and name-free line shapes.
pub fn repair_summary(output: Option<&str>) -> Value {
    let text = output.unwrap_or_default();
    let [shortened, removed, shared, reclaimed] = repair_patterns();
    let freed: u64 = reclaimed
        .captures_iter(text)
        .map(|c| {
            // The digits' value (Unicode digits too), saturating.
            c[1].chars()
                .filter_map(|d| d.to_digit(10))
                .fold(0u64, |n, d| {
                    n.saturating_mul(10).saturating_add(u64::from(d))
                })
        })
        .fold(0u64, u64::saturating_add);
    json!({
        "files_shortened": shortened.find_iter(text).count(),
        "entries_removed": removed.find_iter(text).count(),
        "shared_chains": shared.find_iter(text).count(),
        "clusters_freed": freed,
        "shapes": fat_other_shapes(output, 12),
    })
}

fn u16_at(boot: &[u8], offset: usize) -> u64 {
    u64::from(u16::from_le_bytes([boot[offset], boot[offset + 1]]))
}

fn u32_at(boot: &[u8], offset: usize) -> u64 {
    let bytes = [
        boot[offset],
        boot[offset + 1],
        boot[offset + 2],
        boot[offset + 3],
    ];
    u64::from(u32::from_le_bytes(bytes))
}

/// Bytes from the start of the filesystem to its first data cluster (boot sectors, FATs, root
/// directory of FAT12/16; exFAT up to its cluster heap), `None` when the boot sector does not say.
/// ext4 spreads its tables: `None`; so does a boot sector shorter than 512 bytes.
pub fn tables_bytes(boot: &[u8], filesystem: &str) -> Option<u64> {
    if boot.len() < 512 {
        return None;
    }
    if filesystem == "vfat" {
        // The BIOS parameter block from offset 11, little-endian.
        let sector = u16_at(boot, 11);
        let reserved = u16_at(boot, 14);
        let fats = u64::from(boot[16]);
        let roots = u16_at(boot, 17);
        let small = u16_at(boot, 22);
        let size = if small != 0 { small } else { u32_at(boot, 36) };
        if ![512, 1024, 2048, 4096].contains(&sector) || fats == 0 || size == 0 {
            return None;
        }
        return Some((reserved + fats * size) * sector + roots * 32);
    }
    if filesystem == "exfat" && &boot[3..11] == b"EXFAT   " {
        let shift = boot[108];
        return (9..=12).contains(&shift).then(|| u32_at(boot, 88) << shift);
    }
    None
}

/// The SHA-256 of `text` as lowercase hex.
pub fn sha256_hex(text: &[u8]) -> String {
    Sha256::digest(text)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// SHA-256 fed in chunks, as lowercase hex at the end (Y2Media's copy verification).
#[derive(Default)]
pub struct Sha256Stream(Sha256);

impl Sha256Stream {
    pub fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    pub fn hexdigest(self) -> String {
        self.0
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}

/// The disposable files the space cleanup may remove, by category (the whole name must match).
pub fn disposable_name(category: &str, name: &str) -> bool {
    static PATTERNS: OnceLock<[Regex; 3]> = OnceLock::new();
    let [cache, diagnostics, platform] = PATTERNS.get_or_init(|| {
        [
            full(r"[0-9]+-[0-9]+--?[0-9]+\.rgba"),
            full(r"reborn-diagnostic-[0-9]+-[0-9]+\.tar\.gz"),
            full(r"cache-[a-zA-Z0-9]+\.tmp"),
        ]
    });
    match category {
        "cache" => cache.is_match(name),
        "diagnostics" => diagnostics.is_match(name),
        "platform" => platform.is_match(name),
        _ => false,
    }
}

#[cfg(test)]
#[path = "storage_media_tests.rs"]
mod storage_media_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_split_at_every_boundary() {
        assert_eq!(
            splitlines("a\r\nb\rc\n\nd\u{2028}e\n"),
            ["a", "b", "c", "", "d", "e"]
        );
        assert!(splitlines("").is_empty());
        assert_eq!(splitlines("x"), ["x"]);
    }

    #[test]
    fn fat_runs_are_classified() {
        // As recorded for the same texts before the port.
        let dirty = "fsck.fat 4.2 (2021-01-31)\nDirty bit is set. Fs was not properly unmounted and some data may be corrupt.\n Automatically removing dirty bit.\nThere are differences between boot sector and its backup.\nThis is mostly harmless. Differences: (offset:original/backup)\n  65:01/00\n  Not automatically fixing this.\nLeaving filesystem unchanged.\n/dev/mmcblk1p1: 12 files, 3/100 clusters";
        assert_eq!(fat_findings(Some(dirty)), ["dirty"]);
        assert!(fat_benign_only(Some(dirty)));
        let other = "Free cluster summary wrong (5 vs. really 7)\nAuto-correcting.\nAuto-correcting.\n/MUSIC/a.mp3\n  File size is 10 bytes, cluster chain length is 0.\n 3:00/01, 65:01/00\nVolume label 'X' stored in root directory is not valid.";
        assert_eq!(
            fat_findings(Some(other)),
            ["backup_differs", "fsinfo_free", "label_mismatch", "other"]
        );
        assert!(!fat_benign_only(Some(other)));
        assert_eq!(
            fat_other_shapes(Some(other), 5),
            json!({"shapes": ["File size is # bytes, cluster chain length is #."], "path_lines": 1})
        );
        assert!(fat_findings(None).is_empty() && !fat_benign_only(None));
        assert_eq!(
            fat_other_shapes(Some("Bad 'quoted 12' and \"x\" 345"), 5)["shapes"],
            json!(["Bad '…' and '…' #"])
        );
    }

    #[test]
    fn repairs_are_summarised() {
        let text = "Truncating file to 0 bytes\n  Deleting x\nAuto-deleting.\nDeletingx\nReclaimed 3 unused clusters (98304 bytes).\nReclaimed 1 unused cluster\nfiles share clusters";
        assert_eq!(
            repair_summary(Some(text)),
            json!({"files_shortened": 1, "entries_removed": 2, "shared_chains": 1, "clusters_freed": 4,
                   "shapes": {"shapes": ["Truncating file to # bytes", "Deleting x", "Auto-deleting.", "Deletingx", "Reclaimed # unused cluster", "files share clusters"], "path_lines": 0}})
        );
    }

    #[test]
    fn boot_sectors_give_their_tables() {
        let mut boot = vec![0u8; 512];
        boot[11..13].copy_from_slice(&512u16.to_le_bytes());
        boot[14..16].copy_from_slice(&32u16.to_le_bytes());
        boot[16] = 2;
        boot[36..40].copy_from_slice(&8u32.to_le_bytes());
        assert_eq!(tables_bytes(&boot, "vfat"), Some((32 + 16) * 512));
        assert_eq!(tables_bytes(&boot[..100], "vfat"), None);
        assert_eq!(tables_bytes(&boot, "ext4"), None);
        boot[3..11].copy_from_slice(b"EXFAT   ");
        boot[88..92].copy_from_slice(&4096u32.to_le_bytes());
        boot[108] = 9;
        assert_eq!(tables_bytes(&boot, "exfat"), Some(4096 << 9));
        boot[108] = 13;
        assert_eq!(tables_bytes(&boot, "exfat"), None);
    }

    #[test]
    fn names_and_hashes() {
        assert!(disposable_name("cache", "1-2--3.rgba") && !disposable_name("cache", "1-2.rgba"));
        assert!(disposable_name(
            "diagnostics",
            "reborn-diagnostic-1-2.tar.gz"
        ));
        assert!(
            disposable_name("platform", "cache-aZ9.tmp")
                && !disposable_name("platform", "cache-.tmp")
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
