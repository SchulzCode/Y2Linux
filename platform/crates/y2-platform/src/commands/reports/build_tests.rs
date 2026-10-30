//! Error reports (ERROR-REPORTS-PLAN R2, tests/test_error_reports.py; the progress record of
//! test_report_upload.py): no planted secret survives, the evidence does, and the report keeps its
//! budget. The live commands of a report run on their own thread and stay unanswered here
//! (tests/reports.rs answers them).
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::commands::reports::test_player::{self as player, SSID};
use crate::commands::reports::Value;
use crate::testing::Fixture;
use y2p_contract::reports::ReportPhase;

fn built(root: &Root, trigger: &str, detail: &str, topic: Option<&str>) -> (Vec<u8>, Value) {
    let (data, index) = build(
        root,
        &[trigger.to_owned()],
        detail,
        topic,
        Some("install-1".into()),
        clock::Instant::now(root),
        &Progress::quiet(),
    )
    .unwrap();
    (data, serde_json::json!(index))
}

#[test]
fn no_planted_secret_reaches_the_report() {
    let fixture = Fixture::new();
    player::player(fixture.base());
    let (data, index) = built(
        &fixture.root,
        "manual",
        &format!("from {SSID}"),
        Some("sound"),
    );
    let everything = player::members(&data);
    player::assert_no_canary(&everything);
    let scrubbed = index.get("scrubbed").unwrap();
    // The detail, system.log and Reborn's log (dmesg, answered in tests/reports.rs, adds a fourth).
    assert!(
        scrubbed.get("ssid").and_then(Value::as_i64).unwrap_or(0) >= 3,
        "{scrubbed:?}"
    );
    for name in [
        "mac",
        "ip",
        "music",
        "serial",
        "media_path",
        "private_key",
        "bt_name",
        "email",
    ] {
        assert!(scrubbed.get(name).is_some(), "{name} in {scrubbed:?}");
    }
}

#[test]
fn parallel_scrubbing_matches_one_process_and_tokens_hold_across_files() {
    let fixture = Fixture::new();
    player::player(fixture.base());
    let root = &fixture.root;
    let log: String = (0..30000)
        .map(|n| {
            format!(
                "line {n}: wlan0 joined {SSID} from {}\n",
                player::PRIVATE_IP
            )
        })
        .collect();
    fixture.put("/data/logs/system.log.0", &log);
    let known = secrets::secrets(root).unwrap();
    let items: Vec<(String, Vec<u8>)> = [
        "/data/logs/system.log.0",
        "/data/logs/system.log",
        "/data/reborn/logs/reborn-current.jsonl",
    ]
    .iter()
    .map(|path| (path.to_string(), std::fs::read(root.path(path)).unwrap()))
    .collect();
    assert!(parts(&items[0].0, &items[0].1).len() > 2);
    let key = random_key().unwrap();
    let mut one = Scrubber::new(&known, key).unwrap();
    // whole files, one thread
    let alone: Vec<Vec<u8>> = items
        .iter()
        .map(|(path, data)| scrub::blob(&mut one, path, data))
        .collect();
    let mut together = Scrubber::new(&known, key).unwrap();
    let parallel = scrub_files(&mut together, &items, |_| {});
    assert!(parallel == alone);
    assert_eq!(together.counts, one.counts);
    let token = one.text(SSID);
    let contains = |data: &[u8], text: &str| data.windows(text.len()).any(|w| w == text.as_bytes());
    assert!(contains(&parallel[0], &token)); // every piece of the big log
    assert!(contains(&parallel[2], &token)); // and Reborn's log
    let all = parallel.concat();
    assert!(!contains(&all, SSID));
    let key_hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
    assert!(!contains(&all, &key_hex));
    // a new report, new tokens
    assert_ne!(
        Scrubber::new(&known, random_key().unwrap())
            .unwrap()
            .text(SSID),
        token
    );
}

#[test]
fn a_huge_log_is_cut_to_the_budget() {
    let fixture = Fixture::new();
    player::player(fixture.base());
    // 20 MiB of random hex (half of it entropy: it does not compress below the budget).
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut log = Vec::with_capacity(20 << 20);
    while log.len() < 20 << 20 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        log.extend_from_slice(format!("{state:016x}").as_bytes());
    }
    std::fs::write(fixture.path("/data/logs/system.log.0"), &log).unwrap();
    let (data, index) = build(
        &fixture.root,
        &["manual".to_owned()],
        "",
        None,
        Some("i".into()),
        clock::Instant::now(&fixture.root),
        &Progress::quiet(),
    )
    .unwrap();
    assert!(data.len() <= BUDGET);
    let section = index
        .sections
        .0
        .iter()
        .find(|(name, _)| name == "files/data/logs/system.log.0")
        .unwrap();
    assert!(section.1.cut);
}

#[test]
fn files_without_a_size_are_read_through() {
    // seq_file refuses SEEK_END: /proc and debugfs were left out of every report.
    if !std::path::Path::new("/proc/version").is_file() {
        return; // needs procfs
    }
    let (data, cut) = read_bytes(&Root::new("/"), "/proc/version").unwrap();
    assert!(data.starts_with(b"Linux version"));
    assert!(!cut);
    assert!(FILES.contains(&"/sys/kernel/debug/regmap/*-0030/registers"));
}

#[test]
fn progress_writes_at_most_five_times_a_second() {
    let clock = std::sync::Arc::new(y2p_core::clock::Manual::at(10_000_000_000, 10_000_000_000));
    let fixture = Fixture::new().with_clock(clock.clone());
    let progress = Progress::new(&fixture.root, Some("sd"));
    clock.advance(1_000_000_000);
    progress.update(ReportPhase::Scrub, 0.5, false, Bytes::NONE);
    let first = fixture.read(PROGRESS);
    clock.advance(199_000_000);
    progress.update(ReportPhase::Scrub, 0.6, false, Bytes::NONE); // within 0.2 s: skipped
    assert_eq!(fixture.read(PROGRESS), first);
    clock.advance(2_000_000);
    progress.update(ReportPhase::Scrub, 0.6, false, Bytes::NONE); // 0.2 s on
    assert_eq!(fixture.json(PROGRESS)["percent"], 50.4);
    progress.update(ReportPhase::Scrub, 0.7, true, Bytes::NONE);
    let value = fixture.json(PROGRESS);
    assert_eq!(
        (&value["phase"], &value["destination"], &value["schema"]),
        (&"scrub".into(), &"sd".into(), &1.into())
    );
    assert_eq!(value["percent"], 56.8);
    // 1.2 s for 56.8 %: about a second left.
    assert_eq!(value["seconds_left"], 1);
    progress.close();
    progress.close(); // twice is fine
    assert!(!fixture.exists(PROGRESS));
}
