//! Checks of the in-process pieces no subcommand of this group reaches (the service daemon's supplicant
//! monitor, F52 self-heal and DNS probe, the F53 supervisor paths that need time to pass,
//! `bluetooth.normalize`): steps on a fixture tree with fixed command answers; the result, the
//! subprocess requests and the resulting tree (clock readings normalised) must stay what the Python
//! platform gave for the same steps (tests/recorded/connectivity, recorded while it existed).
// SPDX-License-Identifier: GPL-2.0-only
use super::wifi::Json;
use serde_json::{json, Value};
use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use y2p_core::{
    clock,
    json::{dumps, Separators},
    Root,
};

/// Clock readings, and records of them, by key.
const VOLATILE: [&str; 9] = [
    "monotonic_s",
    "monotonic_us",
    "wall_time",
    "since_s",
    "ago_s",
    "scan_monotonic_s",
    "duration_ns",
    "link_counts_since_s",
    "boottime_s",
];

pub fn normalise(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| {
                    let v = if VOLATILE.contains(&k.as_str()) && !v.is_null() {
                        json!("<volatile>")
                    } else {
                        normalise(v)
                    };
                    (k, v)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(normalise).collect()),
        other => other,
    }
}

fn normalise_ordered(value: Json) -> Json {
    match value {
        Json::Object(pairs) => Json::Object(
            pairs
                .into_iter()
                .map(|(k, v)| {
                    let v = if VOLATILE.contains(&k.as_str()) && v != Json::Value(Value::Null) {
                        Json::Value(json!("<volatile>"))
                    } else {
                        normalise_ordered(v)
                    };
                    (k, v)
                })
                .collect(),
        ),
        Json::Value(v) => Json::Value(normalise(v)),
    }
}

/// Every file below `root`: mode and content (JSON parsed and normalised; journal lines keep their
/// key order, so they are compared as text after normalising).
fn tree(root: &Path) -> Value {
    fn walk(base: &Path, directory: &Path, out: &mut serde_json::Map<String, Value>) {
        let mut entries: Vec<_> = fs::read_dir(directory)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        entries.sort();
        for path in entries {
            let name = path.strip_prefix(base).unwrap().display().to_string();
            let meta = fs::symlink_metadata(&path).unwrap();
            let mode = meta.permissions().mode() & 0o7777;
            if meta.is_dir() {
                out.insert(name, json!({"dir": mode}));
                walk(base, &path, out);
                continue;
            }
            let text = fs::read_to_string(&path).unwrap_or_default();
            let content = if name.ends_with(".jsonl") {
                json!(text
                    .lines()
                    .map(|line| dumps(
                        &normalise_ordered(Json::parse(line).expect("a JSON line")),
                        Separators::Compact
                    )
                    .unwrap())
                    .collect::<Vec<_>>())
            } else {
                serde_json::from_str(&text)
                    .map(normalise)
                    .unwrap_or(Value::String(text))
            };
            out.insert(name, json!({"mode": mode, "content": content}));
        }
    }
    let mut out = serde_json::Map::new();
    walk(root, root, &mut out);
    Value::Object(out)
}

pub struct Outcome {
    pub result: Value,
    pub calls: Vec<Value>,
    pub tree: Value,
}

/// A copy of the player-shaped tree `files` describe (path -> text), in a fresh directory.
fn make_tree(scratch: &Path, side: &str, files: &[(&str, String)]) -> PathBuf {
    let root = scratch.join(side);
    for (name, text) in files {
        let path = root.join(name.trim_start_matches('/'));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
    }
    for private in ["run/y2", "data/system", "data/system/platform"] {
        let path = root.join(private);
        fs::create_dir_all(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    root
}

/// Run `rust` on a copy of `files` with these command answers; the outcome, root paths replaced.
pub fn run(files: &[(&str, String)], answers: Value, rust: impl FnOnce(&Root) -> Value) -> Outcome {
    let scratch = env::temp_dir().join(format!(
        "y2-crosscheck-{}-{}",
        std::process::id(),
        clock::monotonic_ns()
    ));
    fs::create_dir_all(&scratch).unwrap();
    let root = make_tree(&scratch, "rust", files);
    let script = answers
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            let argv: Vec<String> = serde_json::from_value(a["argv"].clone()).unwrap();
            let outcome = y2p_core::command::Outcome {
                ok: a["ok"].as_bool().unwrap(),
                reason: a["reason"].as_str().map(str::to_owned),
                output: a["output"].as_str().map(str::to_owned),
            };
            (argv, outcome)
        })
        .collect();
    let (result, requests) = crate::testing::scripted(script, || rust(&Root::new(&root)));
    let replace = |text: String| text.replace(&root.display().to_string(), "<root>");
    let outcome = Outcome {
        result: normalise(serde_json::from_str(&replace(result.to_string())).unwrap()),
        calls: requests
            .into_iter()
            .map(|(argv, timeout)| json!({"argv": argv, "timeout": timeout}))
            .collect(),
        tree: tree(&root),
    };
    fs::remove_dir_all(&scratch).unwrap();
    outcome
}

/// The outcome must be the recording `connectivity/<name>.json`.
pub fn check(
    name: &str,
    files: &[(&str, String)],
    answers: Value,
    rust: impl FnOnce(&Root) -> Value,
) -> Option<Outcome> {
    let outcome = run(files, answers, rust);
    let text = serde_json::to_string_pretty(
        &json!({"result": outcome.result, "calls": outcome.calls, "tree": outcome.tree}),
    )
    .unwrap()
        + "\n";
    y2p_core::recorded::check(
        env!("CARGO_MANIFEST_DIR"),
        &format!("connectivity/{name}.json"),
        text.as_bytes(),
    );
    Some(outcome)
}

/// The player-shaped files every check starts from: identity, the verified /data mount, a Wi-Fi epoch.
pub fn player() -> Vec<(&'static str, String)> {
    vec![
        ("/proc/sys/kernel/random/boot_id", "boot-a\n".into()),
        (
            "/proc/self/mountinfo",
            "22 1 179:2 / / ro - ext4 /dev/mmcblk0p2 ro\n30 22 179:9 / /data rw,noatime - ext4 /dev/mmcblk0p9 rw\n"
                .into(),
        ),
        ("/run/y2-data-device", "/dev/mmcblk0p9\n".into()),
        ("/data/.y2data-schema", "1\n".into()),
        (
            "/run/y2/wifi-epoch",
            "b6a2f0de-6a1c-4b7e-9c0e-3f1d2a4b5c6d\n".into(),
        ),
        ("/sys/class/rfkill/rfkill0/type", "wlan\n".into()),
        ("/sys/class/rfkill/rfkill0/soft", "0\n".into()),
        ("/sys/class/rfkill/rfkill0/hard", "0\n".into()),
        ("/sys/class/rfkill/rfkill1/type", "bluetooth\n".into()),
        ("/sys/class/rfkill/rfkill1/soft", "1\n".into()),
        (
            "/etc/y2linux/bluetooth-codecs.json",
            json!({"schema": 1, "codecs": {
                "SBC": {"compiled_locally": true, "platform_qualified": true},
                "AAC": {"compiled_locally": true, "distribution_approved": true, "platform_qualified": true},
                "LDAC": {"compiled_locally": true, "owner_private_experiment": true},
                "aptX": {"compiled_locally": false}}})
            .to_string(),
        ),
    ]
}

pub fn answer(argv: &[&str], output: &str) -> Value {
    json!({"argv": argv, "ok": true, "reason": null, "output": output})
}

pub fn refused(argv: &[&str]) -> Value {
    json!({"argv": argv, "ok": false, "reason": "command_failed", "output": null})
}
