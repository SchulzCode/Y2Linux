//! Saving a report onto the player's own storage (tests/test_error_reports.py: Report; the save step of
//! test_report_upload.py): no opt-in, mounted writable storage only, never through a link.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::commands::reports::test_player as player;
use crate::commands::reports::{loads_bytes, Value};
use crate::testing::Fixture;

fn setup() -> Fixture {
    let fixture = Fixture::new();
    player::player(fixture.base());
    fixture
}

fn destinations(root: &Root) -> Value {
    state::status(root)
        .unwrap()
        .get("destinations")
        .cloned()
        .unwrap()
}

fn names(list: &[&str]) -> Value {
    Value::Array(list.iter().map(|n| Value::from(*n)).collect())
}

fn save(root: &Root, destination: &str, topic: Option<&str>) -> Value {
    save_to(root, destination, topic, &build::Progress::quiet()).unwrap()
}

fn text(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap().to_owned()
}

#[test]
fn a_report_is_saved_onto_mounted_storage_without_opting_in() {
    let fixture = setup();
    let root = &fixture.root;
    assert_eq!(destinations(root), names(&[]));
    assert_eq!(
        save(root, "sd", Some("battery")).get("reason"),
        Some(&Value::from("not_mounted"))
    );
    player::mount(
        fixture.base(),
        &["/media/sd", "/media/internal"],
        "rw,relatime",
    );
    // Internal Storage over MTP is the music folder: without it, nowhere to put the file.
    assert_eq!(destinations(root), names(&["sd"]));
    fixture.mkdir("/media/internal/music");
    let status = state::status(root).unwrap();
    assert_eq!(
        status.get("destinations"),
        Some(&names(&["internal", "sd"]))
    );
    assert_eq!(status.get("upload"), Some(&Value::Bool(false))); // no upload key in this tree
    let first = save(root, "sd", Some("battery"));
    let second = save(root, "sd", Some("battery"));
    assert_eq!(first.get("saved"), Some(&Value::Bool(true)));
    let shape = regex::Regex::new(r"^Error Reports/y2linux-report-\d{8}-\d{6}\.tar\.gz$").unwrap();
    assert!(shape.is_match(&text(&first, "file")), "{first:?}");
    let folder = fixture.path("/media/sd/Error Reports");
    let mut hidden: Vec<bool> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().starts_with('.'))
        .collect();
    hidden.sort();
    assert_eq!(hidden, [false, false]);
    // Two reports of the same second (Python's fixed `now`): the second gets "-2".
    let other = fixture.path("/media/other");
    std::fs::create_dir_all(&other).unwrap();
    let stamp = "20261019-120000";
    let one = write_into(&other, "", b"x", stamp).unwrap();
    let two = write_into(&other, "", b"x", stamp).unwrap();
    assert_eq!(one, "y2linux-report-20261019-120000.tar.gz");
    assert_eq!(two, format!("{}-2.tar.gz", &one[..one.len() - 7]));
    let data = std::fs::read(fixture.path("/media/sd").join(text(&first, "file"))).unwrap();
    let everything = player::members(&data);
    let index = loads_bytes(&everything["report.json"]).unwrap();
    assert_eq!(
        (
            index.get("trigger"),
            index.get("topic"),
            index.get("install_id")
        ),
        (
            Some(&Value::from("manual")),
            Some(&Value::from("battery")),
            Some(&Value::Null)
        )
    );
    player::assert_no_canary(&everything);
    let last = state::status(root).unwrap().get("last").cloned().unwrap();
    assert_eq!(
        (
            text(&last, "state"),
            text(&last, "destination"),
            text(&last, "file")
        ),
        ("saved".to_owned(), "sd".to_owned(), text(&second, "file"))
    );
    let status = state::status(root).unwrap();
    assert_eq!(status.get("enabled"), Some(&Value::Null)); // saving is not an answer to the question
    assert_eq!(status.get("queued"), Some(&Value::Array(Vec::new()))); // and queues nothing
    state::set_enabled(root, true).unwrap();
    let saved = save(root, "internal", None);
    assert!(!fixture.exists("/media/internal/Error Reports"));
    let data = std::fs::read(
        fixture
            .path("/media/internal/music")
            .join(text(&saved, "file")),
    )
    .unwrap();
    let index = loads_bytes(&player::members(&data)["report.json"]).unwrap();
    assert_eq!(
        index.get("install_id"),
        state::status(root).unwrap().get("install_id")
    );
}

#[test]
fn saving_refuses_read_only_storage_and_a_folder_link() {
    let fixture = setup();
    let root = &fixture.root;
    player::mount(fixture.base(), &["/media/sd"], "ro,relatime");
    assert_eq!(destinations(root), names(&[]));
    assert_eq!(
        save(root, "sd", None).get("reason"),
        Some(&Value::from("not_mounted"))
    );
    player::mount(fixture.base(), &["/media/sd"], "rw,relatime");
    let elsewhere = fixture.path("/data/elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, fixture.path("/media/sd/Error Reports")).unwrap();
    assert_eq!(
        save(root, "sd", None).get("reason"),
        Some(&Value::from("write_failed"))
    );
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
    let refused = save_to(root, "usb", None, &build::Progress::quiet());
    assert!(matches!(refused, Err(Failure::Value(ref text)) if text == "unknown_destination"));
}

#[test]
fn saving_shows_the_save_step_last() {
    let fixture = setup();
    let root = &fixture.root;
    player::mount(fixture.base(), &["/media/sd"], "rw,relatime");
    let progress = build::Progress::new(root, Some("sd"));
    let result = save_to(root, "sd", None, &progress).unwrap();
    assert_eq!(result.get("saved"), Some(&Value::Bool(true)));
    let shown = fixture.json(build::PROGRESS);
    assert_eq!(shown["phase"], "save");
    assert_eq!(shown["percent"], 100.0);
    assert_eq!(shown["destination"], "sd");
    assert_eq!(
        shown["bytes_total"],
        result.get("bytes").and_then(Value::as_i64).unwrap()
    );
    progress.close();
}
