//! tests/test_boot_order.py's record test: early-boot records are written before the kernel's random
//! pool is ready, so publishing one must not ask for randomness.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::root::tests::tempdir::Dir;
use serde_json::json;
use std::os::unix::fs::MetadataExt;

#[test]
fn platform_records_do_not_need_the_random_pool() {
    // Python blocked os.urandom while it wrote; here the code that names the temporary file is
    // pinned: no random source between `atomic_json` and the next function.
    let source = include_str!("record.rs");
    let body = &source[source.find("pub fn atomic_json").unwrap()
        ..source.find("pub fn private_directory").unwrap()];
    for random in ["getrandom", "urandom", "uuid"] {
        assert!(!body.contains(random), "{random}");
    }
    let dir = Dir::new();
    let target = dir.path().join("record.json");
    atomic_json(&target, &json!({"a": 1}), true, 0o600).unwrap();
    atomic_json(&target, &json!({"a": 2}), false, 0o600).unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap().trim(), "{\"a\":2}");
    assert_eq!(fs::metadata(&target).unwrap().mode() & 0o777, 0o600);
    let left: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".record"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}
