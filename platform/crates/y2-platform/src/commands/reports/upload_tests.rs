//! Error reports R4 (tests/test_report_upload.py, Upload; test_error_reports.py's queue test): the
//! uploader sends queued reports to the receiver only when it should.
//!
//! Below a fixture root the request is the `curl` command line (its headers in argv), answered with
//! curl's output as `answer` reads it: the receiver's body, then `<http_code> <exitcode> <errormsg>`.
//! The wall clock is the real one: where the Python tests moved `now` on, a retry's time is moved back.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::commands::reports::test_player as player;
use crate::commands::service::daemon::{next_upload, Due};
use crate::testing::{ok, scripted, Fixture};
use serde_json::json;

const KEY: &str = "kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk";

fn setup() -> Fixture {
    let fixture = Fixture::new();
    player::player(fixture.base());
    fixture.put_json(state::UPLOAD_KEY, &json!({"upload_key": KEY}));
    player::online(fixture.base(), "Online", "boot-1");
    player::battery(fixture.base(), 80, false);
    player::clock_ready(fixture.base());
    fixture
}

fn text(value: &Py, key: &str) -> String {
    value
        .get(key)
        .and_then(Py::as_str)
        .unwrap_or_else(|| panic!("{key} in {value:?}"))
        .to_owned()
}

/// `report.queue(ctx, 'manual')`, queued.
fn queued(root: &Root) -> Py {
    let result = queue(
        root,
        &["manual".to_owned()],
        "",
        None,
        &build::Progress::quiet(),
    )
    .unwrap();
    assert_eq!(result.get("queued"), Some(&Py::Bool(true)), "{result:?}");
    result
}

/// A report queued earlier (Python gave queue() an earlier `now`): its file gets an older stamp.
fn earlier(root: &Root, item: &mut Py) {
    let old = text(item, "name");
    let name = format!("20200101T000000Z{}", &old[16..]);
    std::fs::rename(root.path(QUEUE).join(&old), root.path(QUEUE).join(&name)).unwrap();
    item.set("name", Py::Str(name));
}

/// The curl command line `post` makes for one queued report.
fn request(root: &Root, name: &str) -> Vec<String> {
    let data = std::fs::read(root.path(QUEUE).join(name)).unwrap();
    let index = queued_index(&data).unwrap();
    argv(&headers(&data, &index, KEY))
}

/// curl's answer to that request: the receiver's status and body (a stored report's answer names it,
/// as the Worker's does).
fn http(root: &Root, name: &str, code: i64, body: Py) -> (Vec<String>, command::Outcome) {
    let argv = request(root, name);
    let body = if code == 200 || code == 201 {
        let data = std::fs::read(root.path(QUEUE).join(name)).unwrap();
        let id = queued_index(&data)
            .unwrap()
            .get("report_id")
            .cloned()
            .unwrap();
        body.with("report_id", id)
    } else {
        body
    };
    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    ok(
        &refs,
        &format!("{}\n{code} 0 ", pyjson::dumps(&body, pyjson::Style::Spaced)),
    )
}

fn stored(root: &Root, name: &str) -> (Vec<String>, command::Outcome) {
    http(
        root,
        name,
        201,
        Py::dict().with("status", Py::str("stored")),
    )
}

/// `report.upload(ctx)` with these curl answers; its result and the curl requests made.
fn send(root: &Root, answers: Vec<(Vec<String>, command::Outcome)>) -> (Py, Vec<Vec<String>>) {
    send_with(root, answers, None, None)
}

fn send_with(
    root: &Root,
    answers: Vec<(Vec<String>, command::Outcome)>,
    progress: Option<&build::Progress>,
    first: Option<&str>,
) -> (Py, Vec<Vec<String>>) {
    let (result, calls) = scripted(answers, || upload(root, progress, first).unwrap());
    let curls = calls
        .into_iter()
        .map(|(argv, _)| argv)
        .filter(|argv| argv[0] == "curl")
        .collect();
    (result, curls)
}

fn reason(result: &Py) -> String {
    text(result, "reason")
}

fn outcomes(result: &Py) -> Vec<String> {
    match result.get("uploaded") {
        Some(Py::List(items)) => items.iter().map(|i| text(i, "outcome")).collect(),
        other => panic!("{other:?}"),
    }
}

fn uploaded(result: &Py, n: usize) -> Py {
    match result.get("uploaded") {
        Some(Py::List(items)) => items[n].clone(),
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
        Some(Py::List(items)) => items
            .iter()
            .map(|n| n.as_str().unwrap().to_owned())
            .collect(),
        other => panic!("{other:?}"),
    }
}

fn last(root: &Root) -> Py {
    state::status(root).unwrap().get("last").cloned().unwrap()
}

fn retry(root: &Root, name: &str) -> Py {
    state::state(root)
        .get("uploads")
        .and_then(|u| u.get(name))
        .cloned()
        .unwrap_or_else(|| panic!("no retry for {name}"))
}

/// The retry time has come (Python sent again at a later `now`).
fn due_now(root: &Root, name: &str) {
    let mut value = state::state(root);
    value
        .get_mut("uploads")
        .and_then(|u| u.get_mut(name))
        .unwrap()
        .set("next", Py::Float(wall(root) - 1.0));
    state::save(root, &value).unwrap();
}

fn next(root: &Root) -> Option<f64> {
    match next_upload(root) {
        Ok(Due::None) => None,
        Ok(Due::At(at)) => Some(at),
        _ => panic!("not a number"),
    }
}

fn install_id(status: &Py) -> String {
    text(status, "install_id")
}

#[test]
fn a_queued_report_is_sent_with_its_identity_and_leaves_the_queue() {
    let _env = crate::commands::reports::fixture_environment();
    let fixture = setup();
    let root = &fixture.root;
    let install = install_id(&state::set_enabled(root, true).unwrap());
    let item = queued(root);
    let name = text(&item, "name");
    let body = std::fs::read(root.path(QUEUE).join(&name)).unwrap();
    let (result, curls) = send(root, vec![stored(root, &name)]);
    let expected = Py::List(vec![Py::dict()
        .with("name", Py::Str(name.clone()))
        .with("outcome", Py::str("sent"))
        .with("report_id", item.get("report_id").cloned().unwrap())]);
    assert!(
        result.get("uploaded").unwrap().py_eq(&expected),
        "{result:?}"
    );
    let [argv] = &curls[..] else {
        panic!("{curls:?}")
    };
    assert_eq!(argv.last().map(String::as_str), Some(ENDPOINT));
    assert_eq!(&body[..2], b"\x1f\x8b");
    assert_eq!(header(argv, "X-Y2-Upload-Key"), KEY);
    assert_eq!(header(argv, "X-Y2-Schema"), "1");
    assert_eq!(header(argv, "X-Y2-Install-Id"), install);
    assert_eq!(header(argv, "X-Y2-Report-Id"), text(&item, "report_id"));
    let digest: String = Sha256::digest(&body)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(header(argv, "X-Y2-Sha256"), digest);
    assert_eq!(header(argv, "X-Y2-Trigger"), "manual");
    assert_eq!(header(argv, "Content-Type"), "application/gzip");
    assert_eq!(header(argv, "Content-Length"), body.len().to_string());
    let status = state::status(root).unwrap();
    assert_eq!(status.get("queued"), Some(&Py::List(Vec::new())));
    assert_eq!(last(root).get("state"), Some(&Py::str("sent")));
    assert_eq!(status.get("upload"), Some(&Py::Bool(true)));
    assert_eq!(next(root), None);
}

#[test]
fn without_an_upload_key_nothing_is_sent_and_reborn_offers_only_save() {
    let _env = crate::commands::reports::fixture_environment();
    let fixture = setup();
    let root = &fixture.root;
    state::set_enabled(root, true).unwrap();
    queued(root);
    fixture.put_json(state::UPLOAD_KEY, &json!({"upload_key": "short"}));
    assert_eq!(
        state::status(root).unwrap().get("upload"),
        Some(&Py::Bool(false))
    );
    let (result, mut curls) = send(root, Vec::new());
    assert_eq!(reason(&result), "no_upload_key");
    fixture.remove(state::UPLOAD_KEY);
    assert_eq!(
        state::status(root).unwrap().get("upload"),
        Some(&Py::Bool(false))
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
    let _env = crate::commands::reports::fixture_environment();
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
        player::clock_ready(base);
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
    let _env = crate::commands::reports::fixture_environment();
    let fixture = setup();
    let root = &fixture.root;
    state::set_enabled(root, true).unwrap();
    let mut first = queued(root);
    earlier(root, &mut first);
    let first = text(&first, "name");
    let second = text(&queued(root), "name");
    let before = wall(root);
    let busy = http(root, &first, 503, Py::dict().with("error", Py::str("busy")));
    let (result, curls) = send(root, vec![busy]);
    let after = wall(root);
    assert_eq!(outcomes(&result), ["retry"]); // the second was not tried
    assert_eq!(curls.len(), 1);
    assert_eq!(last(root).get("state"), Some(&Py::str("queued"))); // the last report is the second
    assert_eq!(next(root), Some(0.0)); // the second never failed
    let entry = retry(root, &first);
    assert_eq!(entry.get("attempts"), Some(&Py::int(1)));
    assert_eq!(entry.get("error"), Some(&Py::str("HTTP_503")));
    let at = entry.get("next").and_then(Py::number).unwrap();
    assert!(
        (before + RETRY_S[0]..=after + RETRY_S[0]).contains(&at),
        "{at}"
    );
    // A later run sends the untried one; the failed one waits for its time.
    let (result, _) = send(root, vec![stored(root, &second), stored(root, &first)]);
    assert_eq!(outcomes(&result), ["sent"]);
    assert_eq!(next(root), Some(at));
    due_now(root, &first);
    let (result, _) = send(root, vec![stored(root, &first)]);
    assert_eq!(outcomes(&result), ["sent"]);
    assert!(queue_names(root).is_empty());
    assert_eq!(state::state(root).get("uploads"), Some(&Py::dict()));
}

#[test]
fn send_report_now_goes_first_then_takes_the_waiting_ones_along() {
    let _env = crate::commands::reports::fixture_environment();
    let fixture = setup();
    let root = &fixture.root;
    state::set_enabled(root, true).unwrap();
    let mut old = queued(root);
    earlier(root, &mut old);
    let old = text(&old, "name");
    send(root, vec![http(root, &old, 503, Py::dict())]); // old now waits 5 min
    let new = text(&queued(root), "name");
    let progress = build::Progress::new(root, None);
    let answers = vec![stored(root, &new), stored(root, &old)];
    let new_size = header(&request(root, &new), "Content-Length");
    let (result, _) = send_with(root, answers, Some(&progress), Some(&new));
    let names: Vec<(String, String)> = match result.get("uploaded") {
        Some(Py::List(items)) => items
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
    send(root, vec![http(root, &waiting, 503, Py::dict())]);
    let newest = text(&queued(root), "name");
    let answers = vec![http(root, &newest, 503, Py::dict()), stored(root, &waiting)];
    let (result, _) = send_with(root, answers, None, Some(&newest));
    assert_eq!(text(&uploaded(&result, 0), "name"), newest);
    assert!(matches!(result.get("uploaded"), Some(Py::List(l)) if l.len() == 1));
    assert!(queue_names(root).contains(&waiting));
}

#[test]
fn the_last_reports_failure_shows_and_backoff_grows() {
    let _env = crate::commands::reports::fixture_environment();
    let fixture = setup();
    let root = &fixture.root;
    state::set_enabled(root, true).unwrap();
    let item = queued(root);
    let name = text(&item, "name");
    send(root, vec![http(root, &name, 500, Py::dict())]);
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
    due_now(root, &name);
    let before = wall(root);
    send(root, vec![http(root, &name, 502, Py::dict())]);
    let after = wall(root);
    let entry = retry(root, &name);
    assert_eq!(entry.get("attempts"), Some(&Py::int(2)));
    let at = entry.get("next").and_then(Py::number).unwrap();
    assert!(
        (before + RETRY_S[1]..=after + RETRY_S[1]).contains(&at),
        "{at}"
    );
}

#[test]
fn a_full_day_waits_for_the_next_utc_day() {
    let _env = crate::commands::reports::fixture_environment();
    let fixture = setup();
    let root = &fixture.root;
    state::set_enabled(root, true).unwrap();
    let name = text(&queued(root), "name");
    let before = wall(root);
    let full = Py::dict().with("error", Py::str("install_daily_limit"));
    send(root, vec![http(root, &name, 429, full)]);
    let after = wall(root);
    let at = retry(root, &name).get("next").and_then(Py::number).unwrap();
    // 00:05 UTC of the next day
    assert!(at == clock::next_midnight(before) || at == clock::next_midnight(after));
    assert_eq!(at.rem_euclid(86_400.0), 300.0);
    assert!(at > after && at <= after + 86_400.0 + 300.0);
}

#[test]
fn a_refused_report_is_dropped_and_says_why() {
    let _env = crate::commands::reports::fixture_environment();
    let fixture = setup();
    let root = &fixture.root;
    state::set_enabled(root, true).unwrap();
    let name = text(&queued(root), "name");
    let refused = Py::dict().with("error", Py::str("sha256_mismatch"));
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

/// The queued archive with report.json's `created_utc` `seconds` earlier (Python sent at a later
/// `now` instead).
fn made_earlier(root: &Root, name: &str, seconds: f64) {
    let path = root.path(QUEUE).join(name);
    let data = std::fs::read(&path).unwrap();
    let members = tarball::members(&data, true, 64, |_| true).unwrap();
    let mut writer = tarball::Writer::new();
    for (member, body) in members {
        let mut body = body.unwrap();
        if member.name == "report.json" {
            let mut index = pyjson::loads_bytes(&body).unwrap();
            let made = clock::Instant::from_float(wall(root) - seconds);
            index.set("created_utc", Py::Str(clock::isoformat(made)));
            body = pyjson::dumps(&index, pyjson::Style::Indent).into_bytes();
        }
        writer.add(
            &tarball::Info::file(&member.name, body.len(), member.mtime as i64, 0o600),
            &body,
        );
    }
    std::fs::write(&path, tarball::gzip(&writer.finish(), 6, 0)).unwrap();
}

#[test]
fn a_report_unsent_for_three_days_is_dropped_without_sending() {
    let _env = crate::commands::reports::fixture_environment();
    let fixture = setup();
    let root = &fixture.root;
    state::set_enabled(root, true).unwrap();
    let name = text(&queued(root), "name");
    made_earlier(root, &name, GIVE_UP_S + 1.0);
    let (result, curls) = send(root, Vec::new());
    assert_eq!(text(&uploaded(&result, 0), "reason"), "expired");
    assert!(curls.is_empty());
    assert_eq!(last(root).get("state"), Some(&Py::str("dropped")));
}

#[test]
fn a_redirect_is_never_followed() {
    let _env = crate::commands::reports::fixture_environment();
    let fixture = setup();
    let root = &fixture.root;
    state::set_enabled(root, true).unwrap();
    let name = text(&queued(root), "name");
    // curl follows nothing without --location: a 3xx is the answer.
    let (result, curls) = send(root, vec![http(root, &name, 307, Py::dict())]);
    assert_eq!(text(&uploaded(&result, 0), "outcome"), "retry");
    let [argv] = &curls[..] else {
        panic!("{curls:?}")
    };
    assert!(!argv
        .iter()
        .any(|a| a == "--location" || a == "-L" || a.starts_with("--max-redirs")));
    assert_eq!(argv.last().map(String::as_str), Some(ENDPOINT));
    assert_eq!(queue_names(root).len(), 1);
    assert_eq!(retry(root, &name).get("error"), Some(&Py::str("HTTP_307")));
}

#[test]
fn an_untrusted_certificate_sends_nothing() {
    let _env = crate::commands::reports::fixture_environment();
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
    let _env = crate::commands::reports::fixture_environment();
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
    let expected = Py::List(vec![Py::dict()
        .with("name", Py::str(name))
        .with("outcome", Py::str("dropped"))
        .with("reason", Py::str("unreadable"))]);
    assert!(
        result.get("uploaded").unwrap().py_eq(&expected),
        "{result:?}"
    );
    assert!(curls.is_empty());
}

#[test]
fn only_one_upload_runs_at_a_time() {
    let _env = crate::commands::reports::fixture_environment();
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
    let _env = crate::commands::reports::fixture_environment();
    let fixture = setup();
    let root = &fixture.root;
    state::set_enabled(root, true).unwrap();
    let progress = build::Progress::new(root, None);
    let item = queue(root, &["manual".to_owned()], "", None, &progress).unwrap();
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
        json!(item.get("bytes").and_then(Py::as_int).unwrap())
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
    let _env = crate::commands::reports::fixture_environment();
    let fixture = setup();
    let root = &fixture.root;
    state::set_enabled(root, true).unwrap();
    queued(root);
    // Sleep takes the activity lease exclusively.
    let sleep = y2p_core::lock::exclusive(&root.path("/run/y2/activity.lock")).unwrap();
    let result = upload_now(root, None, None).unwrap();
    drop(sleep);
    assert_eq!(reason(&result), "sleep_operation_in_progress");
    assert_eq!(queue_names(root).len(), 1);
}

#[test]
fn queue_is_opt_in_and_bounded() {
    let _env = crate::commands::reports::fixture_environment();
    let fixture = Fixture::new();
    player::player(fixture.base());
    let root = &fixture.root;
    let quiet = build::Progress::quiet();
    let queue_one = |trigger: &str| queue(root, &[trigger.to_owned()], "", None, &quiet).unwrap();
    assert_eq!(reason(&queue_one("manual")), "not_enabled");
    assert_eq!(state::status(root).unwrap().get("enabled"), Some(&Py::None));
    let status = state::set_enabled(root, true).unwrap();
    let uuid = regex::Regex::new(r"^[0-9a-f-]{36}$").unwrap();
    assert!(uuid.is_match(&install_id(&status)));
    let first = queue_one("reborn_crash");
    assert_eq!(first.get("queued"), Some(&Py::Bool(true)));
    assert_eq!(
        reason(&queue_one("reborn_crash")),
        "already_reported_this_boot"
    );
    for _ in 0..4 {
        assert_eq!(queue_one("manual").get("queued"), Some(&Py::Bool(true)));
    }
    assert_eq!(queue_names(root).len(), QUEUE_MAX);
    let mut result = Py::None;
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
        Some(&Py::List(Vec::new()))
    );
}

/// `/v1/report` of url: the path the receiver routes.
fn url_path(url: &str) -> String {
    format!("/{}", url.splitn(4, '/').nth(3).unwrap())
}

#[test]
fn the_receiver_accepts_exactly_the_players_triggers_and_schema() {
    let _env = crate::commands::reports::fixture_environment();
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
        Py::None,
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
