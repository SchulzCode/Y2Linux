//! Expected outputs recorded from the Python platform and CPython (repr, round, tarfile, random) while
//! they still existed (2026-10-29), kept below a crate's `tests/recorded/`. The Rust platform is the
//! only implementation now; these files are what its output must stay.
// SPDX-License-Identifier: GPL-2.0-only
use std::{
    fs,
    path::{Path, PathBuf},
};

/// `<crate>/tests/recorded/<name>`; `manifest` is the caller's `env!("CARGO_MANIFEST_DIR")`.
pub fn path(manifest: &str, name: &str) -> PathBuf {
    Path::new(manifest).join("tests/recorded").join(name)
}

/// The recorded bytes.
pub fn bytes(manifest: &str, name: &str) -> Vec<u8> {
    let path = path(manifest, name);
    fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The recorded text.
pub fn text(manifest: &str, name: &str) -> String {
    String::from_utf8(bytes(manifest, name)).expect("recorded text is UTF-8")
}

/// Two recorded lines say the same: the same text, or both JSON with equal values (outputs are no longer
/// kept byte-identical to Python's `json.dumps`: spacing and key order do not count).
fn same_line(want: &str, got: &str) -> bool {
    want == got
        || matches!(
            (
                serde_json::from_str::<serde_json::Value>(want),
                serde_json::from_str::<serde_json::Value>(got),
            ),
            (Ok(a), Ok(b)) if a == b
        )
}

/// `actual` must be the recording `name`, line by line (a JSON line by its value). With
/// `Y2_RECORD_GOLDENS=1` it becomes the recording (after an intended change; review the diff). The first
/// recordings were made while the Python oracles still agreed with these outputs.
pub fn check(manifest: &str, name: &str, actual: &[u8]) {
    let path = path(manifest, name);
    if std::env::var_os("Y2_RECORD_GOLDENS").is_some() {
        fs::create_dir_all(path.parent().expect("parent")).expect("recorded directory");
        fs::write(&path, actual).expect("recording");
        return;
    }
    let expected = bytes(manifest, name);
    let (want, got) = (
        String::from_utf8_lossy(&expected),
        String::from_utf8_lossy(actual),
    );
    let same = want.lines().count() == got.lines().count()
        && want.lines().zip(got.lines()).all(|(a, b)| same_line(a, b));
    if !same {
        let line = want
            .lines()
            .zip(got.lines())
            .position(|(a, b)| !same_line(a, b))
            .or(Some(want.lines().count().min(got.lines().count())));
        panic!(
            "{name} differs from its recording (line {}): recorded {:?}, now {:?}; \
             Y2_RECORD_GOLDENS=1 records an intended change",
            line.map_or(0, |l| l + 1),
            line.and_then(|l| want.lines().nth(l)).unwrap_or(""),
            line.and_then(|l| got.lines().nth(l)).unwrap_or(""),
        );
    }
}
