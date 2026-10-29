//! The records an update reads besides its payloads: the native core's journal and answers
//! (`status`, `core`) and download.json as the Software Update page sees it.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;

pub(super) fn object(value: Option<Value>) -> Map<String, Value> {
    match value {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

/// `update.download_record`: download.json, with a download or check whose process is gone shown as
/// interrupted. A user's player (8.17) said "Downloading Update… Keep Wi-Fi on." for a day: the record
/// a download writes first stays `Staging` when the download dies without reaching its own handler
/// (power loss, a kill), and that page offered neither Cancel nor Check for Updates; a restart kept
/// it (it is on /data).
pub(super) fn download_record(root: &Root) -> Value {
    let record = root
        .json("/data/updates/download.json")
        .unwrap_or(Value::Null);
    if matches!(string(record.get("state")), Some("Staging" | "Checking"))
        && !lock_held(root, &root.path("/data/updates/.stage.lock"))
    {
        return json!({"schema": 1, "state": "Failed", "failure": "download_interrupted"});
    }
    record
}

/// `update.status`.
pub fn status(root: &Root) -> Map<String, Value> {
    let path = root.path("/data/updates/state.json");
    let mut value = root.json("/data/updates/state.json");
    if value.is_none() && !path.exists() && path.symlink_metadata().is_err() {
        let ready = root.read("/data/.y2data-schema").as_deref() == Some("1")
            && root.path("/data/updates").is_dir();
        value = Some(
            json!({"schema": 1, "state": if ready { "Idle" } else { "Unavailable" },
                            "failure": if ready { Value::Null } else { json!("data_state_unavailable") }}),
        );
    }
    // The native core writes journal schema 2 since OTA-35 (BOOTIMG + root).
    let mut value = match value {
        Some(Value::Object(map))
            if py::eq_int(map.get("schema"), 1) || py::eq_int(map.get("schema"), 2) =>
        {
            map
        }
        _ => {
            return object(Some(
                json!({"schema": 1, "state": "Failed", "failure": "journal_unreadable"}),
            ))
        }
    };
    value.insert("download".into(), download_record(root));
    value.insert(
        "last_check".into(),
        root.json("/data/updates/last-check.json")
            .unwrap_or(Value::Null),
    );
    value.insert("channel".into(), channel(root).name);
    value.insert("full_system".into(), json!(true));
    value.insert("physical_qualification".into(), json!(false));
    value
}

/// Whether any live process holds a flock on `path` (/proc/locks names the device and inode).
fn lock_held(root: &Root, path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    let dev = meta.dev();
    let (major, minor) = (libc::major(dev) as u64, libc::minor(dev) as u64);
    for line in root.read("/proc/locks").unwrap_or_default().lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 6
            || fields[1] != "FLOCK"
            || fields[4].is_empty()
            || !fields[4].chars().all(|c| c.is_numeric())
        {
            continue;
        }
        let parts: Vec<&str> = fields[5].split(':').collect();
        let [a, b, c] = parts.as_slice() else {
            continue;
        };
        let parsed = (
            u64::from_str_radix(a, 16),
            u64::from_str_radix(b, 16),
            c.parse::<u64>(),
        );
        if let (Ok(a), Ok(b), Ok(c)) = parsed {
            if (a, b, c) == (major, minor, meta.ino()) {
                return true;
            }
        }
    }
    false
}

/// Python's `json.loads` error text for the core's output (a ValueError).
pub(super) fn json_error(text: &str, error: &serde_json::Error) -> String {
    let column = error.column();
    let line = error.line();
    let offset: usize = text
        .split('\n')
        .take(line.saturating_sub(1))
        .map(|l| l.chars().count() + 1)
        .sum::<usize>()
        + column.saturating_sub(1);
    if text.trim().is_empty() {
        return "Expecting value: line 1 column 1 (char 0)".into();
    }
    if error.to_string().starts_with("trailing characters") {
        return format!("Extra data: line {line} column {column} (char {offset})");
    }
    format!("Expecting value: line {line} column {column} (char {offset})")
}

/// `update.core`: one action of the native core; its refusal becomes the error text.
pub fn core(
    root: &Root,
    action: &str,
    directory: Option<&str>,
    timeout: f64,
) -> Result<Map<String, Value>> {
    let mut argv = vec!["/usr/sbin/y2-update-core", action];
    argv.extend(directory);
    let result = command::run_with(root, &argv, timeout, 20000, true);
    if !result.ok {
        let refusal = serde_json::from_str::<Value>(
            result
                .output
                .as_deref()
                .filter(|o| !o.is_empty())
                .unwrap_or("{}"),
        )
        .ok()
        .and_then(|v| v.get("failure").cloned())
        .filter(truthy);
        return Err(Raise::Value(match refusal {
            Some(value) => sorted(value).py_str(),
            None => format!(
                "update_core_{}",
                result
                    .reason
                    .as_deref()
                    .filter(|r| !r.is_empty())
                    .unwrap_or("refused")
            ),
        }));
    }
    let output = result.output.unwrap_or_default();
    match serde_json::from_str::<Value>(&output) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(Raise::value("invalid_update_core_response")),
        Err(error) => Err(Raise::Value(json_error(&output, &error))),
    }
}

pub(super) fn installed_release(root: &Root) -> Value {
    root.json_object("/etc/y2linux/versions.json")
        .get("release_version")
        .cloned()
        .unwrap_or(Value::Null)
}
