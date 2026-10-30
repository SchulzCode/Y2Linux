//! Error reports R4 (tests/test_report_upload.py, Upload; test_error_reports.py's queue test): the
//! uploader sends queued reports to the receiver only when it should.
//!
//! Below a fixture root the request is the `curl` command line (its headers in argv), answered with
//! curl's output as `answer` reads it: the receiver's body, then `<http_code> <exitcode> <errormsg>`.
//! The root reads a test clock (2026-10-29 12:00 UTC) that moves only when a test moves it on.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::commands::reports::test_player as player;
use crate::commands::reports::{object, Value, ValueExt};
use crate::commands::service::daemon::{next_upload, Due};
use crate::testing::{ok, Fixture};
use serde_json::json;
use std::sync::Arc;
use y2p_core::clock::Manual;

const KEY: &str = "kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk";
/// The test clock's wall time at the start (seconds since the epoch).
const START: f64 = 1_793_275_200.0;

fn setup() -> Fixture {
    setup_with(&Arc::new(clock_at_start()))
}

fn clock_at_start() -> Manual {
    Manual::at(1_000_000_000_000, 1_000_000_000_000).with_wall(START as u64 * 1_000_000_000)
}

/// Move the test clock `seconds` on; the clock was synchronised again meanwhile (`time` holds the
/// anchor to the clock's monotonic and boottime readings).
fn later(fixture: &Fixture, clock: &Manual, seconds: f64) {
    clock.advance((seconds * 1e9) as u64);
    player::clock_ready_on(fixture.base(), clock);
}

fn setup_with(clock: &Arc<Manual>) -> Fixture {
    let fixture = Fixture::new().with_clock(clock.clone());
    player::player(fixture.base());
    fixture.put_json(state::UPLOAD_KEY, &json!({"upload_key": KEY}));
    player::online(fixture.base(), "Online", "boot-1");
    player::battery(fixture.base(), 80, false);
    player::clock_ready_on(fixture.base(), &**clock);
    fixture
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{key} in {value:?}"))
        .to_owned()
}

/// `report.queue(ctx, 'manual')`, queued.
fn queued(root: &Root) -> Value {
    let result = queue(
        root,
        &["manual".to_owned()],
        "",
        None,
        &build::Progress::quiet(),
    )
    .unwrap();
    assert!(result.queued, "{result:?}");
    serde_json::json!(result)
}

/// A report queued earlier (Python gave queue() an earlier `now`): its file gets an older stamp.
fn earlier(root: &Root, item: &mut Value) {
    let old = text(item, "name");
    let name = format!("20200101T000000Z{}", &old[16..]);
    std::fs::rename(root.path(QUEUE).join(&old), root.path(QUEUE).join(&name)).unwrap();
    item.set("name", Value::String(name));
}

/// The curl command line `post` makes for one queued report.
fn request(root: &Root, name: &str) -> Vec<String> {
    let data = std::fs::read(root.path(QUEUE).join(name)).unwrap();
    let index = queued_index(&data).unwrap();
    argv(&headers(&data, &index, KEY))
}

/// curl's answer to that request: the receiver's status and body (a stored report's answer names it,
/// as the Worker's does).
fn http(root: &Root, name: &str, code: i64, body: Value) -> (Vec<String>, command::Outcome) {
    let argv = request(root, name);
    let body = if code == 200 || code == 201 {
        let data = std::fs::read(root.path(QUEUE).join(name)).unwrap();
        let id = Value::from(queued_index(&data).unwrap().report_id.unwrap());
        body.with("report_id", id)
    } else {
        body
    };
    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    ok(
        &refs,
        &format!(
            "{}\n{code} 0 ",
            serde_json::to_string(&body).unwrap_or_default()
        ),
    )
}

fn stored(root: &Root, name: &str) -> (Vec<String>, command::Outcome) {
    http(
        root,
        name,
        201,
        object().with("status", Value::from("stored")),
    )
}

/// `report.upload(ctx)` with these curl answers; its result and the curl requests made.
fn send(root: &Root, answers: Vec<(Vec<String>, command::Outcome)>) -> (Value, Vec<Vec<String>>) {
    send_with(root, answers, None, None)
}

fn send_with(
    root: &Root,
    answers: Vec<(Vec<String>, command::Outcome)>,
    progress: Option<&build::Progress>,
    first: Option<&str>,
) -> (Value, Vec<Vec<String>>) {
    let answering = root.clone().with_answers(answers);
    let result = serde_json::json!(upload(&answering, progress, first).unwrap());
    let curls = answering
        .calls()
        .into_iter()
        .map(|(argv, _)| argv)
        .filter(|argv| argv[0] == "curl")
        .collect();
    (result, curls)
}

fn reason(result: &Value) -> String {
    text(result, "reason")
}

fn outcomes(result: &Value) -> Vec<String> {
    match result.get("uploaded") {
        Some(Value::Array(items)) => items.iter().map(|i| text(i, "outcome")).collect(),
        other => panic!("{other:?}"),
    }
}

fn uploaded(result: &Value, n: usize) -> Value {
    match result.get("uploaded") {
        Some(Value::Array(items)) => items[n].clone(),
        other => panic!("{other:?}"),
    }
}

/// `--header 'Name: value'` of a curl command line.
fn header(argv: &[String], name: &str) -> String {
    argv.windows(2)
        .filter(|w| w[0] == "--header")
        .find_map(|w| w[1].strip_prefix(&format!("{name}: ")).map(str::to_owned))
        .unwrap_or_else(|| panic!("{name} in {argv:?}"))
}

fn queue_names(root: &Root) -> Vec<String> {
    match state::status(root).unwrap().get("queued") {
        Some(Value::Array(items)) => items
            .iter()
            .map(|n| n.as_str().unwrap().to_owned())
            .collect(),
        other => panic!("{other:?}"),
    }
}

fn last(root: &Root) -> Value {
    state::status(root).unwrap().get("last").cloned().unwrap()
}

fn retry(root: &Root, name: &str) -> Value {
    serde_json::json!(state::state(root))
        .get("uploads")
        .and_then(|u| u.get(name))
        .cloned()
        .unwrap_or_else(|| panic!("no retry for {name}"))
}

/// The clock moved on just past `name`'s retry time.
fn due_now(fixture: &Fixture, clock: &Manual, name: &str) {
    let root = &fixture.root;
    let at = retry(root, name)
        .get("next")
        .and_then(Value::as_f64)
        .unwrap();
    later(fixture, clock, at - wall(root) + 1.0);
}

fn next(root: &Root) -> Option<f64> {
    match next_upload(root) {
        Ok(Due::None) => None,
        Ok(Due::At(at)) => Some(at),
        _ => panic!("not a number"),
    }
}

fn install_id(status: &Value) -> String {
    text(status, "install_id")
}

#[test]
fn a_queued_report_is_sent_with_its_identity_and_leaves_the_queue() {
    let fixture = setup();
    let root = &fixture.root;
    let install = install_id(&state::set_enabled(root, true).unwrap());
    let item = queued(root);
    let name = text(&item, "name");
    let body = std::fs::read(root.path(QUEUE).join(&name)).unwrap();
    let (result, curls) = send(root, vec![stored(root, &name)]);
    let expected = Value::Array(vec![object()
        .with("name", Value::String(name.clone()))
        .with("outcome", Value::from("sent"))
        .with("report_id", item.get("report_id").cloned().unwrap())]);
    assert!(result.get("uploaded").unwrap().eq(&expected), "{result:?}");
    let [argv] = &curls[..] else {
        panic!("{curls:?}")
    };
    assert_eq!(argv.last().map(String::as_str), Some(ENDPOINT));
    assert_eq!(&body[..2], b"\x1f\x8b");
    assert_eq!(header(argv, "X-Y2-Upload-Key"), KEY);
    assert_eq!(header(argv, "X-Y2-Schema"), "1");
    assert_eq!(header(argv, "X-Y2-Install-Id"), install);
    assert_eq!(header(argv, "X-Y2-Report-Id"), text(&item, "report_id"));
    use sha2::{Digest, Sha256};
    let digest: String = Sha256::digest(&body)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(header(argv, "X-Y2-Sha256"), digest);
    assert_eq!(header(argv, "X-Y2-Trigger"), "manual");
    assert_eq!(header(argv, "Content-Type"), "application/gzip");
    assert_eq!(header(argv, "Content-Length"), body.len().to_string());
    let status = state::status(root).unwrap();
    assert_eq!(status.get("queued"), Some(&Value::Array(Vec::new())));
    assert_eq!(last(root).get("state"), Some(&Value::from("sent")));
    assert_eq!(status.get("upload"), Some(&Value::Bool(true)));
    assert_eq!(next(root), None);
}

#[test]
fn without_an_upload_key_nothing_is_sent_and_reborn_offers_only_save() {
    let fixture = setup();
    let root = &fixture.root;
    state::set_enabled(root, true).unwrap();
    queued(root);
    fixture.put_json(state::UPLOAD_KEY, &json!({"upload_key": "short"}));
    assert_eq!(
        state::status(root).unwrap().get("upload"),
        Some(&Value::Bool(false))
    );
    let (result, mut curls) = send(root, Vec::new());
    assert_eq!(reason(&result), "no_upload_key");
    fixture.remove(state::UPLOAD_KEY);
    assert_eq!(
        state::status(root).unwrap().get("upload"),
        Some(&Value::Bool(false))
    );
    let (result, more) = send(root, Vec::new());
    assert_eq!(reason(&result), "no_upload_key");
    curls.extend(more);
    assert!(curls.is_empty());
    assert_eq!(queue_names(root).len(), 1);
}

/// One way a ready player stops being ready to upload.
type Change<'a> = Box<dyn Fn() + 'a>;

#[test]
fn reports_wait_for_opt_in_wifi_clock_battery_and_no_update() {
    let fixture = setup();
    let root = &fixture.root;
    let base = fixture.base();
    assert_eq!(reason(&send(root, Vec::new()).0), "not_enabled");
    state::set_enabled(root, true).unwrap();
    let name = text(&queued(root), "name");
    let ready = || {
        player::online(base, "Online", "boot-1");
        player::battery(base, 80, false);
        fixture.put_json(
            "/data/updates/state.json",
            &json!({"state": "Acknowledged"}),
        );
        fixture.remove("/data/system/platform/maintenance-pending");
        player::clock_ready_on(base, fixture.root.clock());
    };
    let cases: Vec<(Change, &str)> = vec![
        (
            Box::new(|| player::online(base, "Connecting", "boot-1")),
            "offline",
        ),
        // an earlier boot's record
        (
            Box::new(|| player::online(base, "Online", "boot-0")),
            "offline",
        ),
        (Box::new(|| player::battery(base, 14, false)), "battery_low"),
        // unknown: wait
        (
            Box::new(|| {
                fixture.put("/run/y2/power.json", "{}");
            }),
            "battery_low",
        ),
        (
            Box::new(|| {
                fixture.put_json("/data/updates/state.json", &json!({"state": "Queued"}));
            }),
            "update_in_progress",
        ),
        (
            Box::new(|| {
                fixture.put("/data/system/platform/maintenance-pending", "");
            }),
            "update_in_progress",
        ),
        (
            Box::new(|| {
                fixture.remove("/run/y2/time.json");
            }),
            "clock_not_ready",
        ),
    ];
    let mut curls = Vec::new();
    for (change, expected) in cases {
        ready();
        change();
        let (result, more) = send(root, Vec::new());
        assert_eq!(reason(&result), expected);
        curls.extend(more);
    }
    assert!(curls.is_empty());
    // Charging overrides a low battery.
    ready();
    player::battery(base, 5, true);
    let (result, _) = send(root, vec![stored(root, &name)]);
    assert_eq!(text(&uploaded(&result, 0), "outcome"), "sent");
}

#[test]
fn server_errors_back_off_and_stop_the_run_then_succeed() {
    let clock = Arc::new(clock_at_start());
    let fixture = setup_with(&clock);
    let root = &fixture.root;
    state::set_enabled(root, true).unwrap();
    let mut first = queued(root);
    earlier(root, &mut first);
    let first = text(&first, "name");
    let second = text(&queued(root), "name");
    let busy = http(
        root,
        &first,
        503,
        object().with("error", Value::from("busy")),
    );
    let (result, curls) = send(root, vec![busy]);
    assert_eq!(outcomes(&result), ["retry"]); // the second was not tried
    assert_eq!(curls.len(), 1);
    assert_eq!(last(root).get("state"), Some(&Value::from("queued"))); // the last report is the second
    assert_eq!(next(root), Some(0.0)); // the second never failed
    let entry = retry(root, &first);
    assert_eq!(entry.get("attempts"), Some(&Value::from(1)));
    assert_eq!(entry.get("error"), Some(&Value::from("HTTP_503")));
    let at = entry.get("next").and_then(Value::as_f64).unwrap();
    assert_eq!(at, START + RETRY_S[0]);
    // A later run sends the untried one; the failed one waits for its time.
    later(&fixture, &clock, 60.0);
    let (result, _) = send(root, vec![stored(root, &second), stored(root, &first)]);
    assert_eq!(outcomes(&result), ["sent"]);
    assert_eq!(next(root), Some(at));
    due_now(&fixture, &clock, &first);
    let (result, _) = send(root, vec![stored(root, &first)]);
    assert_eq!(outcomes(&result), ["sent"]);
    assert!(queue_names(root).is_empty());
    assert!(state::state(root).uploads.is_empty());
}

#[test]
fn send_report_now_goes_first_then_takes_the_waiting_ones_along() {
    let fixture = setup();
    let root = &fixture.root;
    state::set_enabled(root, true).unwrap();
    let mut old = queued(root);
    earlier(root, &mut old);
    let old = text(&old, "name");
    send(root, vec![http(root, &old, 503, object())]); // old now waits 5 min
    let new = text(&queued(root), "name");
    let progress = build::Progress::new(root, None);
    let answers = vec![stored(root, &new), stored(root, &old)];
    let new_size = header(&request(root, &new), "Content-Length");
    let (result, _) = send_with(root, answers, Some(&progress), Some(&new));
    let names: Vec<(String, String)> = match result.get("uploaded") {
        Some(Value::Array(items)) => items
            .iter()
            .map(|i| (text(i, "name"), text(i, "outcome")))
            .collect(),
        other => panic!("{other:?}"),
    };
    // new first, old without waiting
    assert_eq!(names, [(new, "sent".to_owned()), (old, "sent".to_owned())]);
    assert!(queue_names(root).is_empty());
    // the bar follows the new one only
    let shown = fixture.json(build::PROGRESS);
    assert_eq!(shown["bytes_total"].to_string(), new_size);
    progress.close();
    // Without a working connection for the new one, the old one keeps its time.
    let waiting = text(&queued(root), "name");
    send(root, vec![http(root, &waiting, 503, object())]);
    let newest = text(&queued(root), "name");
    let answers = vec![http(root, &newest, 503, object()), stored(root, &waiting)];
    let (result, _) = send_with(root, answers, None, Some(&newest));
    assert_eq!(text(&uploaded(&result, 0), "name"), newest);
    assert!(matches!(result.get("uploaded"), Some(Value::Array(l)) if l.len() == 1));
    assert!(queue_names(root).contains(&waiting));
}

#[test]
fn the_last_reports_failure_shows_and_backoff_grows() {
    let clock = Arc::new(clock_at_start());
    let fixture = setup_with(&clock);
    let root = &fixture.root;
    state::set_enabled(root, true).unwrap();
    let item = queued(root);
    let name = text(&item, "name");
    send(root, vec![http(root, &name, 500, object())]);
    let shown = last(root);
    assert_eq!(
        (
            text(&shown, "state"),
            text(&shown, "reason"),
            text(&shown, "report_id")
        ),
        (
            "failed".to_owned(),
            "HTTP_500".to_owned(),
            text(&item, "report_id")
        )
    );
    due_now(&fixture, &clock, &name);
    let now = wall(root);
    send(root, vec![http(root, &name, 502, object())]);
    let entry = retry(root, &name);
    assert_eq!(entry.get("attempts"), Some(&Value::from(2)));
    let at = entry.get("next").and_then(Value::as_f64).unwrap();
    assert_eq!(at, now + RETRY_S[1]);
}

#[test]
fn a_full_day_waits_for_the_next_utc_day() {
    let fixture = setup();
    let root = &fixture.root;
    state::set_enabled(root, true).unwrap();
    let name = text(&queued(root), "name");
    let full = object().with("error", Value::from("install_daily_limit"));
    send(root, vec![http(root, &name, 429, full)]);
    let at = retry(root, &name)
        .get("next")
        .and_then(Value::as_f64)
        .unwrap();
    // 00:05 UTC of the next day
    assert_eq!(at, clock::next_midnight(START));
    assert_eq!(at, START + 12.0 * 3600.0 + 300.0);
}

#[test]
fn a_refused_report_is_dropped_and_says_why() {
    let fixture = setup();
    let root = &fixture.root;
    state::set_enabled(root, true).unwrap();
    let name = text(&queued(root), "name");
    let refused = object().with("error", Value::from("sha256_mismatch"));
    let (result, _) = send(root, vec![http(root, &name, 400, refused)]);
    assert_eq!(
        text(&uploaded(&result, 0), "reason"),
        "refused_400_sha256_mismatch"
    );
    assert!(queue_names(root).is_empty());
    let shown = last(root);
    assert_eq!(
        (text(&shown, "state"), text(&shown, "reason")),
        (
            "dropped".to_owned(),
            "refused_400_sha256_mismatch".to_owned()
        )
    );
}

#[test]
fn a_report_unsent_for_three_days_is_dropped_without_sending() {
    let clock = Arc::new(clock_at_start());
    let fixture = setup_with(&clock);
    let root = &fixture.root;
    state::set_enabled(root, true).unwrap();
    queued(root);
    later(&fixture, &clock, GIVE_UP_S + 1.0);
    let (result, curls) = send(root, Vec::new());
    assert_eq!(text(&uploaded(&result, 0), "reason"), "expired");
    assert!(curls.is_empty());
    assert_eq!(last(root).get("state"), Some(&Value::from("dropped")));
}

#[test]
fn a_redirect_is_never_followed() {
    let fixture = setup();
    let root = &fixture.root;
    state::set_enabled(root, true).unwrap();
    let name = text(&queued(root), "name");
    // curl follows nothing without --location: a 3xx is the answer.
    let (result, curls) = send(root, vec![http(root, &name, 307, object())]);
    assert_eq!(text(&uploaded(&result, 0), "outcome"), "retry");
    let [argv] = &curls[..] else {
        panic!("{curls:?}")
    };
    assert!(!argv
        .iter()
        .any(|a| a == "--location" || a == "-L" || a.starts_with("--max-redirs")));
    assert_eq!(argv.last().map(String::as_str), Some(ENDPOINT));
    assert_eq!(queue_names(root).len(), 1);
    assert_eq!(
        retry(root, &name).get("error"),
        Some(&Value::from("HTTP_307"))
    );
}

#[test]
fn an_untrusted_certificate_sends_nothing() {
    let fixture = setup();
    let root = &fixture.root;
    state::set_enabled(root, true).unwrap();
    let name = text(&queued(root), "name");
    // curl refuses the certificate before any byte of the report is sent (exit 60).
    let argv = request(root, &name);
    let refused = command::Outcome {
        ok: false,
        reason: Some("command_failed".into()),
        output: Some(
            "\n000 60 SSL certificate problem: unable to get local issuer certificate".into(),
        ),
    };
    let (result, curls) = send(root, vec![(argv, refused)]);
    let item = uploaded(&result, 0);
    assert_eq!(text(&item, "outcome"), "retry");
    assert_eq!(text(&item, "reason"), "SSLCertVerificationError");
    assert_eq!(curls.len(), 1);
    assert_eq!(queue_names(root).len(), 1);
}

#[test]
fn a_damaged_queue_file_is_dropped() {
    let fixture = setup();
    let root = &fixture.root;
    state::set_enabled(root, true).unwrap();
    let name = "20261019T120000Z-x.tar.gz";
    std::fs::create_dir_all(root.path(QUEUE)).unwrap();
    std::fs::write(
        root.path(QUEUE).join(name),
        tarball::gzip(b"not a tar", 6, 0),
    )
    .unwrap();
    let (result, curls) = send(root, Vec::new());
    let expected = Value::Array(vec![object()
        .with("name", Value::from(name))
        .with("outcome", Value::from("dropped"))
        .with("reason", Value::from("unreadable"))]);
    assert!(result.get("uploaded").unwrap().eq(&expected), "{result:?}");
    assert!(curls.is_empty());
}

#[test]
fn only_one_upload_runs_at_a_time() {
    let fixture = setup();
    let root = &fixture.root;
    state::set_enabled(root, true).unwrap();
    queued(root);
    let lock = y2p_core::lock::exclusive(&root.path(QUEUE).join(".upload.lock")).unwrap();
    let (result, curls) = send(root, Vec::new());
    assert_eq!(reason(&result), "busy");
    drop(lock);
    assert!(curls.is_empty());
}

#[test]
fn the_owners_screen_follows_build_and_upload_and_the_record_goes_away() {
    let fixture = setup();
    let root = &fixture.root;
    state::set_enabled(root, true).unwrap();
    let progress = build::Progress::new(root, None);
    let item = json!(queue(root, &["manual".to_owned()], "", None, &progress).unwrap());
    let name = text(&item, "name");
    // The build ends with its pack step (at most five writes a second: the forced ones show).
    let built = fixture.json(build::PROGRESS);
    assert_eq!(built["phase"], "pack");
    assert_eq!(built["percent"], 82.0);
    let (result, curls) = send_with(root, vec![stored(root, &name)], Some(&progress), None);
    assert_eq!(text(&uploaded(&result, 0), "outcome"), "sent");
    let shown = fixture.json(build::PROGRESS);
    assert_eq!(shown["phase"], "upload");
    assert!(shown["percent"].as_f64().unwrap() >= built["percent"].as_f64().unwrap()); // never goes back
    assert!(shown["seconds_left"].is_null() || shown["seconds_left"].as_i64().unwrap() >= 0);
    assert_eq!(
        shown["bytes_total"],
        json!(item.get("bytes").and_then(Value::as_i64).unwrap())
    );
    // The request carries the whole body (fixture curl: its length and identity).
    let [argv] = &curls[..] else {
        panic!("{curls:?}")
    };
    assert_eq!(
        header(argv, "Content-Length"),
        shown["bytes_total"].to_string()
    );
    assert_eq!(header(argv, "X-Y2-Report-Id"), text(&item, "report_id"));
    progress.close();
    assert!(!fixture.exists(build::PROGRESS));
    // On the player the body is streamed to curl's stdin and the screen follows it (a shell stands
    // in for curl and checks the length).
    let streamed = build::Progress::new(root, None);
    let data = vec![1u8; 100_000];
    let script = "n=$(wc -c); printf '{}\\n201 0 '; test \"$n\" -eq 100000";
    let outcome = stream(&["/bin/sh", "-c", script], &data, Some(&streamed));
    assert!(outcome.ok, "{outcome:?}");
    assert_eq!(answer(&outcome).0, Some(201));
    let shown = fixture.json(build::PROGRESS);
    assert_eq!(shown["phase"], "upload");
    assert_eq!(shown["bytes_total"], 100_000);
    streamed.close();
    assert!(!fixture.exists(build::PROGRESS));
}

#[test]
fn a_sleep_in_progress_leaves_the_report_queued() {
    let fixture = setup();
    let root = &fixture.root;
    state::set_enabled(root, true).unwrap();
    queued(root);
    // Sleep takes the activity lease exclusively.
    let sleep = y2p_core::lock::exclusive(&root.path("/run/y2/activity.lock")).unwrap();
    let result = json!(upload_now(root, None, None));
    drop(sleep);
    assert_eq!(reason(&result), "sleep_operation_in_progress");
    assert_eq!(queue_names(root).len(), 1);
}

#[test]
fn queue_is_opt_in_and_bounded() {
    let fixture = Fixture::new();
    player::player(fixture.base());
    let root = &fixture.root;
    let quiet = build::Progress::quiet();
    let queue_one =
        |trigger: &str| json!(queue(root, &[trigger.to_owned()], "", None, &quiet).unwrap());
    assert_eq!(reason(&queue_one("manual")), "not_enabled");
    assert_eq!(
        state::status(root).unwrap().get("enabled"),
        Some(&Value::Null)
    );
    let status = state::set_enabled(root, true).unwrap();
    let uuid = regex::Regex::new(r"^[0-9a-f-]{36}$").unwrap();
    assert!(uuid.is_match(&install_id(&status)));
    let first = queue_one("reborn_crash");
    assert_eq!(first.get("queued"), Some(&Value::Bool(true)));
    assert_eq!(
        reason(&queue_one("reborn_crash")),
        "already_reported_this_boot"
    );
    for _ in 0..4 {
        assert_eq!(queue_one("manual").get("queued"), Some(&Value::Bool(true)));
    }
    assert_eq!(queue_names(root).len(), QUEUE_MAX);
    let mut result = Value::Null;
    for trigger in [
        "kernel_warning",
        "sleep_failure",
        "radio_failure",
        "storage_error",
    ] {
        result = queue_one(trigger);
    }
    assert_eq!(reason(&result), "daily_limit"); // 5 automatic a day
    let old = install_id(&state::status(root).unwrap());
    assert_ne!(install_id(&state::new_install_id(root).unwrap()), old);
    assert_eq!(
        state::set_enabled(root, false).unwrap().get("queued"),
        Some(&Value::Array(Vec::new()))
    );
}

/// `/v1/report` of url: the path the receiver routes.
fn url_path(url: &str) -> String {
    format!("/{}", url.splitn(4, '/').nth(3).unwrap())
}

#[test]
fn the_receiver_accepts_exactly_the_players_triggers_and_schema() {
    let source = std::fs::read_to_string(
        crate::testing::repository().join("tools/reports/worker/src/index.js"),
    )
    .unwrap();
    let list = regex::Regex::new(r"(?s)TRIGGERS = new Set\(\[(.*?)\]\)")
        .unwrap()
        .captures(&source)
        .unwrap()[1]
        .to_owned();
    let triggers: std::collections::BTreeSet<String> = regex::Regex::new(r"'([a-z_]+)'")
        .unwrap()
        .captures_iter(&list)
        .map(|c| c[1].to_owned())
        .collect();
    let ours: std::collections::BTreeSet<String> =
        build::TRIGGERS.iter().map(|t| t.to_string()).collect();
    assert_eq!(triggers, ours);
    // The schema a built report carries is the one the request names.
    let fixture = Fixture::new();
    let (data, index) = build::build(
        &fixture.root,
        &["manual".to_owned()],
        "",
        None,
        None,
        clock::Instant::now(&fixture.root),
        &build::Progress::quiet(),
    )
    .unwrap();
    let schema = headers(&data, &index, KEY)
        .into_iter()
        .find(|(name, _)| *name == "X-Y2-Schema")
        .unwrap()
        .1;
    assert!(source.contains(&format!("const SCHEMA = '{schema}';")));
    assert!(source.contains(&format!(
        "MAX_BYTES = {} * 1024 * 1024",
        build::BUDGET / (1024 * 1024)
    )));
    assert!(source.contains(&format!("'{}'", url_path(ENDPOINT))));
}
