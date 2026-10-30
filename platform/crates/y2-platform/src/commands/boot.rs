//! `y2-platform boot-stage <stage>`: one bounded durable boot journal; an unclean boot
//! is not a reset diagnosis. `application_ready` also arms the runtime power features.
// SPDX-License-Identifier: GPL-2.0-only
use super::power_features;
use crate::reply::At;
use crate::reply::{atomic_json, lock, private_directory, Failure, Outcome, Reply};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
};
use y2p_contract::BootStage;
use y2p_core::mounts::mounts_at;
use y2p_core::{
    json::{dumps, Object, ObjectExt},
    py::{python_number, truthy},
    Root,
};

const RESET_STATUS: &str = "/sys/firmware/y2_pm/reset_status";
/// How many earlier boots the journal keeps.
const HISTORY: usize = 8;

/// The `key=value` pairs of the reset status (`(\w+)=(\S+)`: in each whitespace-separated token the
/// first word followed by `=` and at least one more character; a later key wins).
fn fields(text: &str) -> HashMap<&str, &str> {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mut map = HashMap::new();
    for token in text.split_whitespace() {
        let chars: Vec<(usize, char)> = token.char_indices().collect();
        for start in 0..chars.len() {
            let mut end = start;
            while end < chars.len() && word(chars[end].1) {
                end += 1;
            }
            if end > start && end + 1 < chars.len() && chars[end].1 == '=' {
                map.insert(
                    &token[chars[start].0..chars[end].0],
                    &token[chars[end + 1].0..],
                );
                break;
            }
        }
    }
    map
}

/// The reset cause boot.json records: the MT6582 RGU WDT_STATUS the watchdog driver read at probe,
/// before its first write.
#[derive(Debug, PartialEq, Serialize)]
struct ResetCause {
    /// A cause the RGU status proves; none for zero, unknown bits or an absent file (those never
    /// prove a power-on reset).
    reset_cause: Option<String>,
    reset_cause_reason: String,
    reset_status_raw: Option<String>,
}

fn reset_cause(text: Option<&str>) -> ResetCause {
    let fields = fields(text.unwrap_or_default());
    if fields.get("valid") != Some(&"1") {
        return ResetCause {
            reset_cause: None,
            reset_cause_reason: "not_observed".into(),
            reset_status_raw: None,
        };
    }
    let raw = fields.get("raw").map(|raw| (*raw).to_owned());
    let cause = fields.get("cause").copied().unwrap_or_default();
    const PROVEN: [&str; 5] = [
        "watchdog_timeout",
        "software_reset",
        "watchdog_irq",
        "debug_reset",
        "spm_watchdog",
    ];
    if PROVEN.contains(&cause) {
        return ResetCause {
            reset_cause: Some(cause.to_owned()),
            reset_cause_reason: "rgu_wdt_status_at_probe".into(),
            reset_status_raw: raw,
        };
    }
    let cause = if cause.is_empty() {
        "unreadable"
    } else {
        cause
    };
    ResetCause {
        reset_cause: None,
        reset_cause_reason: format!("rgu_status_{cause}"),
        reset_status_raw: raw,
    }
}

/// A counter of the previous record: a non-negative integer, or a bool (a hand-edited record; it
/// counts as 0 or 1 and is kept as it is while the boot is the same); anything else is 0.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(untagged)]
enum Counter {
    Count(u64),
    Flag(bool),
}

impl Counter {
    fn of(value: Option<&Value>) -> Counter {
        match value {
            Some(Value::Bool(flag)) => Counter::Flag(*flag),
            Some(Value::Number(n)) => Counter::Count(n.as_u64().unwrap_or(0)),
            _ => Counter::Count(0),
        }
    }

    fn count(self) -> u64 {
        match self {
            Counter::Count(n) => n,
            Counter::Flag(flag) => u64::from(flag),
        }
    }
}

/// boot.json (schema 1), written and printed with its keys sorted. The fields taken over from the
/// previous record (`previous_boot_id`, the history, `identity`) keep whatever JSON they held.
#[derive(Debug, Serialize)]
struct BootRecord {
    schema: u8,
    boot_id: String,
    boot_count: u64,
    previous_boot_id: Value,
    last_stage: &'static str,
    stage_monotonic_ns: u64,
    orderly_shutdown: bool,
    consecutive_unclean: Counter,
    recovery_recommended: bool,
    #[serde(flatten)]
    reset: ResetCause,
    /// The last boots before this one, oldest first: boot id, last stage, orderly shutdown, identity.
    history: Vec<Value>,
    /// /etc/y2linux/versions.json of the running build.
    identity: Value,
}

/// The journal after `stage` of boot `boot_id`. A previous record that is not a
/// schema-1 object starts a new journal. A new boot counts one more boot, and one more unclean boot
/// unless the last one ended with `shutdown_complete`; three unclean boots in a row recommend
/// recovery.
fn transition(
    previous: Option<Value>,
    boot_id: &str,
    stage: BootStage,
    identity: Value,
    now_ns: u64,
    reset: ResetCause,
) -> BootRecord {
    let previous = match previous {
        Some(Value::Object(map)) if python_number(map.get("schema")) == Some(1.0) => map,
        _ => Object::new(),
    };
    let known = !previous.is_empty();
    let same = previous.get("boot_id").and_then(Value::as_str) == Some(boot_id);
    let mut unclean = Counter::of(previous.get("consecutive_unclean"));
    let mut history = match previous.get("history") {
        Some(Value::Array(items)) => items.clone(),
        _ => Vec::new(),
    };
    if !same {
        let orderly = previous.get("orderly_shutdown").is_some_and(truthy);
        unclean = Counter::Count(if known && !orderly {
            (unclean.count() + 1).min(1000)
        } else {
            0
        });
        if known {
            history.push(json!({
                "boot_id": previous.value("boot_id"),
                "last_stage": previous.value("last_stage"),
                "orderly_shutdown": previous.value("orderly_shutdown"),
                "identity": previous.value("identity"),
            }));
            history = history.split_off(history.len().saturating_sub(HISTORY));
        }
    }
    BootRecord {
        schema: 1,
        boot_id: boot_id.to_owned(),
        boot_count: Counter::of(previous.get("boot_count")).count() + u64::from(!same),
        previous_boot_id: previous.value(if same { "previous_boot_id" } else { "boot_id" }),
        last_stage: stage.as_str(),
        stage_monotonic_ns: now_ns,
        orderly_shutdown: stage == BootStage::ShutdownComplete,
        consecutive_unclean: unclean,
        recovery_recommended: unclean.count() >= 3,
        reset,
        history,
        identity,
    }
}

/// One line of /run/y2/boot-stages.jsonl, in this key order.
#[derive(Serialize)]
struct StageLine {
    stage: &'static str,
    monotonic_s: f64,
}

pub fn mark(root: &Root, stage: &str) -> Outcome {
    let stage = BootStage::parse(stage).ok_or_else(|| Failure::value("unknown_boot_stage"))?;
    if root.read("/data/.y2data-schema").as_deref() != Some("1") {
        return Err(Failure::value("data_schema_unavailable"));
    }
    // Exactly one mount on /data, of the device the initramfs verified.
    let verified = match &mounts_at(root, "/data")[..] {
        [data] => root.read("/run/y2-data-device").as_deref() == Some(data.source.as_str()),
        _ => false,
    };
    if !verified {
        return Err(Failure::value("verified_data_mount_unavailable"));
    }
    let directory = root.path("/data/system/platform");
    private_directory(&directory)?;
    let _lock = lock(&directory.join(".boot.lock"))?;
    let boot_id = root
        .boot_id()
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
        root.clock().monotonic_ns(),
        reset,
    );
    atomic_json(&directory.join("boot.json"), &value, true)?;
    crate::reply::mkdir_parents(&root.path("/run/y2"), 0o700)?;
    let runtime = root.path("/run/y2/boot-stages.jsonl");
    let mut trail = OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&runtime)
        .at(&runtime)?;
    if trail.metadata().at(&runtime)?.size() < 8192 {
        let line = StageLine {
            stage: stage.as_str(),
            monotonic_s: root.clock().monotonic(),
        };
        let text = dumps(&line).map_err(Failure::value)? + "\n";
        trail.write_all(text.as_bytes()).at(&runtime)?;
    }
    drop(trail);
    // The reset-surviving stage trail of init and rcS (F23); best effort.
    if let Ok(mut pmsg) = OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root.path("/dev/pmsg0"))
    {
        let _ = pmsg.write_all(
            format!(
                "{:.2} platform {}\n",
                root.clock().monotonic(),
                stage.as_str()
            )
            .as_bytes(),
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
    use y2p_core::mounts::mountinfo;

    #[test]
    fn findall_pairs_match_the_regex() {
        // What the pattern `(\w+)=(\S+)` finds in the text.
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
