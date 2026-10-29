//! Y2DATA admission and the bounded cleanup (from tests/test_platform_storage_tools.py).
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::testing::Fixture;
use std::{
    fs::{self, File, FileTimes},
    os::unix::fs::symlink,
    time::{Duration, UNIX_EPOCH},
};

fn volume(state: &str, available: u64) -> Object {
    json!({"space_state": state, "available_bytes": available})
        .as_object()
        .unwrap()
        .clone()
}

#[test]
fn low_space_denies_disposable_writes_but_reserves_checkpoint_budget() {
    let low = volume("LowSpace", 80 << 20);
    assert_eq!(admission(&low, "cache")["allowed"], false);
    assert_eq!(admission(&low, "update")["allowed"], false);
    assert_eq!(admission(&low, "user_state")["allowed"], true);
    // Python asked for 60 MiB of 80; the port never takes a request size (the command asks for
    // none), so the same 20 MiB are left over.
    assert_eq!(
        admission(&volume("LowSpace", 20 << 20), "database")["allowed"],
        false
    );
    assert_eq!(
        admission(&volume("ReadOnlyRisk", 80 << 20), "user_state")["allowed"],
        false
    );
}

fn age(path: &std::path::Path) {
    let old = UNIX_EPOCH + Duration::from_secs(1);
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(FileTimes::new().set_accessed(old).set_modified(old))
        .unwrap();
}

#[test]
fn cleanup_is_bounded_and_cannot_follow_links_or_delete_user_files() {
    let fixture = Fixture::new();
    fixture.put("/data/reborn/cache/1-2-3.rgba", "old cache");
    fixture.put("/data/reborn/cache/4-5-6.rgba", "recent cache");
    fixture.put("/data/music/track.flac", "music");
    fixture.put("/data/reborn/state/settings.json", "settings");
    symlink(
        fixture.path("/data/music/track.flac"),
        fixture.path("/data/reborn/cache/7-8-9.rgba"),
    )
    .unwrap();
    age(&fixture.path("/data/reborn/cache/1-2-3.rgba"));
    for n in 0..4 {
        let name = format!("/data/reborn/diagnostics/reborn-diagnostic-1-{n}.tar.gz");
        fixture.put(&name, "old diagnostic");
        age(&fixture.path(&name));
    }
    let result = disposable_cleanup(&fixture.root, 2, 8 << 20).unwrap();
    assert_eq!(result["files"], 2);
    assert!(!fixture.exists("/data/reborn/cache/1-2-3.rgba"));
    assert!(fixture.exists("/data/reborn/cache/4-5-6.rgba"));
    assert_eq!(fixture.read("/data/music/track.flac"), "music");
    assert_eq!(fixture.read("/data/reborn/state/settings.json"), "settings");
    let link = fs::symlink_metadata(fixture.path("/data/reborn/cache/7-8-9.rgba")).unwrap();
    assert!(link.file_type().is_symlink());
}
