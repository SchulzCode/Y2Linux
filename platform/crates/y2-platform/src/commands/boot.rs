//! `y2-platform boot-stage <stage>` (`boot.mark`): one bounded durable boot journal; an unclean boot
//! is not a reset diagnosis. `application_ready` also arms the runtime power features.
// SPDX-License-Identifier: GPL-2.0-only
use super::power_features;
use crate::reply::{atomic_json, lock, private_directory, Failure, Outcome, Reply};
use serde_json::{json, Map, Value};
use std::{
    fs::OpenOptions,
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
};
use y2p_contract::BootStage;
use y2p_core::{
    clock,
    json::{dumps, Ordered, Separators},
    py::{number, truthy},
    Root,
};

const RESET_STATUS: &str = "/sys/firmware/y2_pm/reset_status";

/// One mountinfo line (`observe.mountinfo`).
pub struct Mount {
    pub path: String,
    pub source: String,
}

fn unescape(value: &str) -> String {
    let (mut out, mut rest) = (String::new(), value);
    while let Some(index) = rest.find('\\') {
        out.push_str(&rest[..index]);
        let code = &rest[index + 1..];
        match ["040", "011", "012", "134"]
            .iter()
            .find(|c| code.starts_with(**c))
        {
            Some(c) => {
                out.push(char::from(u8::from_str_radix(c, 8).expect("octal")));
                rest = &code[3..];
            }
            None => {
                out.push('\\');
                rest = code;
            }
        }
    }
    out + rest
}

pub fn mountinfo(text: &str) -> Vec<Mount> {
    let mut result = Vec::new();
    for line in text.lines() {
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        let (a, b): (Vec<&str>, Vec<&str>) = (
            left.split_whitespace().collect(),
            right.split_whitespace().collect(),
        );
        if a.len() < 6
            || b.len() < 3
            || !a[0].chars().all(|c| c.is_ascii_digit())
            || a[0].is_empty()
        {
            continue;
        }
        result.push(Mount {
            path: unescape(a[4]),
            source: unescape(b[1]),
        });
    }
    result
}

/// `re.findall(r'(\w+)=(\S+)', text)` into a dict (a later key wins).
fn fields(text: &str) -> Map<String, Value> {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mut map = Map::new();
    for token in text.split_whitespace() {
        let chars: Vec<(usize, char)> = token.char_indices().collect();
        for start in 0..chars.len() {
            let mut end = start;
            while end < chars.len() && word(chars[end].1) {
                end += 1;
            }
            if end > start && end + 1 < chars.len() && chars[end].1 == '=' {
                let key = &token[chars[start].0..chars[end].0];
                map.insert(key.into(), json!(&token[chars[end + 1].0..]));
                break;
            }
        }
    }
    map
}

/// `reset_cause`: the MT6582 RGU WDT_STATUS the watchdog driver read at probe, before its first write.
/// Zero, unknown bits or an absent file never prove a power-on reset.
fn reset_cause(text: Option<&str>) -> [(&'static str, Value); 3] {
    let fields = fields(text.unwrap_or_default());
    if fields.get("valid") != Some(&json!("1")) {
        return [
            ("reset_cause", Value::Null),
            ("reset_cause_reason", json!("not_observed")),
            ("reset_status_raw", Value::Null),
        ];
    }
    let raw = fields.get("raw").cloned().unwrap_or(Value::Null);
    let cause = fields
        .get("cause")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if [
        "watchdog_timeout",
        "software_reset",
        "watchdog_irq",
        "debug_reset",
        "spm_watchdog",
    ]
    .contains(&cause)
    {
        return [
            ("reset_cause", json!(cause)),
            ("reset_cause_reason", json!("rgu_wdt_status_at_probe")),
            ("reset_status_raw", raw),
        ];
    }
    let reason = format!(
        "rgu_status_{}",
        if cause.is_empty() {
            "unreadable"
        } else {
            cause
        }
    );
    [
        ("reset_cause", Value::Null),
        ("reset_cause_reason", json!(reason)),
        ("reset_status_raw", raw),
    ]
}

/// `isinstance(x, int) and x >= 0` (bools included, as in Python) or 0.
fn counter(value: Option<&Value>) -> Value {
    match value {
        Some(v @ Value::Bool(_)) => v.clone(),
        Some(Value::Number(n)) if n.as_u64().is_some() => Value::Number(n.clone()),
        _ => json!(0),
    }
}

/// `boot.transition`.
fn transition(
    previous: Option<Value>,
    boot_id: &str,
    stage: BootStage,
    identity: Value,
    now_ns: u64,
    reset: [(&'static str, Value); 3],
) -> Map<String, Value> {
    let previous = match previous {
        Some(Value::Object(map)) if number(map.get("schema")) == Some(1.0) => map,
        _ => Map::new(),
    };
    let same = previous.get("boot_id").cloned().unwrap_or(Value::Null) == json!(boot_id);
    let count = counter(previous.get("boot_count"));
    let mut unclean = counter(previous.get("consecutive_unclean"));
    if !same {
        unclean = if !previous.is_empty() && !previous.get("orderly_shutdown").is_some_and(truthy) {
            json!((number(Some(&unclean)).unwrap_or(0.0) as u64 + 1).min(1000))
        } else {
            json!(0)
        };
    }
    let mut history = match previous.get("history") {
        Some(Value::Array(items)) => items.clone(),
        _ => Vec::new(),
    };
    if !previous.is_empty() && !same {
        let mut entry = Map::new();
        for key in ["boot_id", "last_stage", "orderly_shutdown", "identity"] {
            entry.insert(
                key.into(),
                previous.get(key).cloned().unwrap_or(Value::Null),
            );
        }
        history.push(Value::Object(entry));
        history = history.split_off(history.len().saturating_sub(8));
    }
    let count = number(Some(&count)).unwrap_or(0.0) as u64 + u64::from(!same);
    let recommended = number(Some(&unclean)).unwrap_or(0.0) >= 3.0;
    let mut value = Map::new();
    value.insert("schema".into(), json!(1));
    value.insert("boot_id".into(), json!(boot_id));
    value.insert("boot_count".into(), json!(count));
    let previous_boot = if same {
        previous.get("previous_boot_id")
    } else {
        previous.get("boot_id")
    };
    value.insert(
        "previous_boot_id".into(),
        previous_boot.cloned().unwrap_or(Value::Null),
    );
    value.insert("last_stage".into(), json!(stage.as_str()));
    value.insert("stage_monotonic_ns".into(), json!(now_ns));
    value.insert(
        "orderly_shutdown".into(),
        json!(stage == BootStage::ShutdownComplete),
    );
    value.insert("consecutive_unclean".into(), unclean);
    value.insert("recovery_recommended".into(), json!(recommended));
    for (key, v) in reset {
        value.insert(key.into(), v);
    }
    value.insert("history".into(), Value::Array(history));
    value.insert("identity".into(), identity);
    value
}

pub fn mark(root: &Root, stage: &str) -> Outcome {
    let stage = BootStage::parse(stage).ok_or_else(|| Failure::value("unknown_boot_stage"))?;
    if root.read("/data/.y2data-schema").as_deref() != Some("1") {
        return Err(Failure::value("data_schema_unavailable"));
    }
    let mounts = mountinfo(&root.read("/proc/self/mountinfo").unwrap_or_default());
    let data: Vec<&Mount> = mounts.iter().filter(|m| m.path == "/data").collect();
    if data.len() != 1
        || Some(data[0].source.as_str()) != root.read("/run/y2-data-device").as_deref()
    {
        return Err(Failure::value("verified_data_mount_unavailable"));
    }
    let directory = root.path("/data/system/platform");
    private_directory(&directory)?;
    let _lock = lock(&directory.join(".boot.lock"))?;
    let boot_id = root
        .read("/proc/sys/kernel/random/boot_id")
        .filter(|b| !b.is_empty())
        .ok_or_else(|| Failure::value("boot_id_unavailable"))?;
    let previous = root.json("/data/system/platform/boot.json");
    let identity = root
        .json("/etc/y2linux/versions.json")
        .unwrap_or(Value::Null);
    let reset = reset_cause(root.read(RESET_STATUS).as_deref());
    let value = transition(
        previous,
        &boot_id,
        stage,
        identity,
        clock::monotonic_ns(),
        reset,
    );
    atomic_json(&directory.join("boot.json"), &value, true)?;
    let runtime = root.path("/run/y2/boot-stages.jsonl");
    crate::reply::mkdir_parents(runtime.parent().expect("parent"), 0o700)?;
    let mut trail = OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&runtime)
        .map_err(|e| Failure::os(e, &runtime))?;
    if trail
        .metadata()
        .map_err(|e| Failure::os(e, &runtime))?
        .size()
        < 8192
    {
        let line = Ordered::new()
            .with("stage", stage.as_str())
            .with("monotonic_s", clock::monotonic());
        let text = dumps(&line, Separators::Spaced).map_err(Failure::value)? + "\n";
        trail
            .write_all(text.as_bytes())
            .map_err(|e| Failure::os(e, &runtime))?;
    }
    drop(trail);
    // The reset-surviving stage trail of init and rcS (F23); best effort.
    if let Ok(mut pmsg) = OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root.path("/dev/pmsg0"))
    {
        let _ = pmsg.write_all(
            format!("{:.2} platform {}\n", clock::monotonic(), stage.as_str()).as_bytes(),
        );
    }
    if stage == BootStage::ApplicationReady {
        power_features::apply(root);
    }
    Reply::sorted(&value, 0)
}

#[cfg(test)]
#[path = "boot_tests.rs"]
mod boot_tests;

#[cfg(test)]
#[path = "boot_reset_cause_tests.rs"]
mod boot_reset_cause_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn findall_pairs_match_the_regex() {
        // re.findall(r'(\w+)=(\S+)', ...) in Python
        let map = fields("valid=1 cause=watchdog_timeout raw=0x4 a-b=c =x=y z= w=1=2");
        assert_eq!(map["valid"], "1");
        assert_eq!(map["raw"], "0x4");
        assert_eq!(map["b"], "c");
        assert_eq!(map["x"], "y");
        assert_eq!(map["w"], "1=2");
        assert!(!map.contains_key("z"));
    }

    #[test]
    fn mountinfo_lines_unescape_paths() {
        let mounts = mountinfo("30 22 179:9 / /data rw - ext4 /dev/mmcblk0p9 rw\nbad line\n31 1 0:1 / /my\\040dir rw - tmpfs t\\134x rw");
        assert_eq!(mounts.len(), 2);
        assert_eq!(
            (mounts[0].path.as_str(), mounts[0].source.as_str()),
            ("/data", "/dev/mmcblk0p9")
        );
        assert_eq!(
            (mounts[1].path.as_str(), mounts[1].source.as_str()),
            ("/my dir", "t\\x")
        );
    }
}

#[cfg(test)]
#[path = "boot_order_tests.rs"]
mod boot_order_tests;
