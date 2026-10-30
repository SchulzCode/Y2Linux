//! The report's own projections: the hardware's identity (sysfs, the bootloaders' hashes), Reborn's
//! session without its queue, the library's counts. Never a name, title or path.
// SPDX-License-Identifier: GPL-2.0-only
use super::{pyre, python_strip, rel, sha256_hex, Failure, File, Root, SeekFrom, Value};
use crate::commands::reports::{loads_text, object, ValueExt};
use std::io::Read;
use std::io::Seek;
pub use y2p_core::py::strerror;

/// A JSON record of up to 16 MiB (`json_at` reads 128 KiB; Reborn's session with its whole queue is
/// larger); `None` for `null` or no JSON.
pub fn large_json(root: &Root, name: &str) -> Option<Value> {
    let text = root.read_limited(name, 16 * 1024 * 1024);
    match loads_text(text.as_deref().filter(|t| !t.is_empty()).unwrap_or("null")) {
        Some(Value::Null) | None => None,
        other => other,
    }
}

fn read(root: &Root, name: &str) -> Value {
    root.read(name).into()
}

/// Every setting and technical playback fact of session.json; no queue entries.
pub(super) fn session_projection(session: Option<Value>) -> Result<Value, Failure> {
    let Some(session @ Value::Object(_)) = session else {
        return Ok(Value::Null);
    };
    let model = match session.get("model") {
        Some(m @ Value::Object(_)) => m.clone(),
        _ => object(),
    };
    let queue = match model.get("queue") {
        Some(Value::Array(q)) => q.clone(),
        _ => Vec::new(),
    };
    let position = model.get("queue_position").cloned().unwrap_or(Value::Null);
    let index = position.as_i64();
    let current = index
        .filter(|&i| 0 <= i && (i as usize) < queue.len())
        .map(|i| queue[i as usize].clone());
    const TECHNICAL: [&str; 9] = [
        "codec",
        "sample_rate",
        "channels",
        "bitrate",
        "bits_per_sample",
        "duration_ms",
        "source_id",
        "online",
        "artwork",
    ];
    // Reborn writes its settings as an object; anything else shows as null (the report is still
    // made: a broken session must not keep a crash from being reported).
    let mut settings = match model.get("settings") {
        Some(settings @ Value::Object(_)) => settings.clone(),
        _ => Value::Null,
    };
    if let Some(Value::String(zone)) = settings.get("time_zone") {
        let set = if zone.is_empty() { "" } else { "set" };
        settings.set("time_zone", Value::from(set));
    }
    let current_track = match current {
        Some(track @ Value::Object(_)) => {
            let mut out = object();
            for key in TECHNICAL {
                if let Some(value) = track.get(key) {
                    out.set(key, value.clone());
                }
            }
            out
        }
        _ => Value::Null,
    };
    let mut other: Vec<String> = match &model {
        Value::Object(items) => items
            .iter()
            .map(|(k, _)| k.clone())
            .filter(|k| !matches!(k.as_str(), "queue" | "queue_entry_ids" | "settings"))
            .collect(),
        _ => Vec::new(),
    };
    other.sort();
    let get = |key: &str| model.get(key).cloned().unwrap_or(Value::Null);
    Ok(object()
        .with(
            "schema",
            session.get("schema").cloned().unwrap_or(Value::Null),
        )
        .with("settings", settings)
        .with("playback", get("playback"))
        .with("position_ms", get("position_ms"))
        .with("output", get("output"))
        .with("queue_length", Value::from(queue.len()))
        .with("queue_position", position)
        .with("current_track", current_track)
        .with(
            "other_keys",
            Value::Array(other.into_iter().map(Value::String).collect()),
        ))
}

/// The library's counts only (sqlite, read-only); never a name, title or path.
pub(super) fn library_statistics(root: &Root) -> Value {
    use y2p_core::reports_sqlite::{Database, Error, Value as Sql};
    let path = root.path("/data/reborn/library.db");
    if !path.exists() {
        return Value::Null;
    }
    let database = match Database::open(
        &format!("file:{}?mode=ro&immutable=1", path.to_string_lossy()),
        1000,
    ) {
        Ok(database) => database,
        Err(error) => return object().with("error", Value::from(error.class())),
    };
    let size = std::fs::metadata(&path).map_or(0, |m| m.len());
    let mut result = object().with("bytes", Value::from(size));
    let value = |v: &Sql| match v {
        Sql::Null | Sql::Blob(_) => Value::Null,
        Sql::Integer(i) => Value::from(*i),
        Sql::Real(f) => Value::from(*f),
        Sql::Text(t) => Value::from(t.clone()),
    };
    let outcome = (|| -> Result<(), Error> {
        let tables: Vec<String> = database
            .query("select name from sqlite_master where type='table'")?
            .into_iter()
            .map(|row| match row.into_iter().next() {
                Some(Sql::Text(t)) => t,
                _ => String::new(),
            })
            .collect();
        result.set("tables", object());
        for table in &tables {
            if !table.is_empty()
                && table
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                let rows = database.query(&format!("select count(*) from \"{table}\""))?;
                let count = rows
                    .first()
                    .and_then(|r| r.first())
                    .map_or(Value::Null, value);
                if let Some(tables) = result.get_mut("tables") {
                    tables.set(table, count);
                }
            }
        }
        let columns: Vec<String> = if tables.iter().any(|t| t == "tracks") {
            database
                .query("pragma table_info(tracks)")?
                .into_iter()
                .filter_map(|row| match row.into_iter().nth(1) {
                    Some(Sql::Text(t)) => Some(t),
                    _ => None,
                })
                .collect()
        } else {
            Vec::new()
        };
        for column in [
            "codec",
            "sample_rate",
            "channels",
            "source_id",
            "bits_per_sample",
            "online",
        ] {
            if columns.iter().any(|c| c == column) {
                let rows = database.query(&format!(
                    "select \"{column}\", count(*) from tracks group by 1 order by 2 desc limit 32"
                ))?;
                let list = rows
                    .iter()
                    .map(|row| {
                        let first = row.first().map_or(Value::Null, value);
                        let count = row.get(1).map_or(Value::Null, value);
                        Value::Array(vec![first, count])
                    })
                    .collect();
                result.set(&format!("by_{column}"), Value::Array(list));
            }
        }
        let version = database.query("pragma user_version")?;
        result.set(
            "user_version",
            version
                .first()
                .and_then(|r| r.first())
                .map_or(Value::Null, value),
        );
        Ok(())
    })();
    if let Err(error) = outcome {
        result.set("error", Value::from(error.class()));
    }
    // Source IDs are SD card filesystem IDs: keep their number, not the value.
    if let Some(Value::Array(rows)) = result.get("by_source_id").cloned() {
        let renamed = rows
            .into_iter()
            .enumerate()
            .map(|(i, row)| {
                let count = match row {
                    Value::Array(pair) => pair.get(1).cloned().unwrap_or(Value::Null),
                    _ => Value::Null,
                };
                Value::Array(vec![Value::String(format!("source#{}", i + 1)), count])
            })
            .collect();
        result.set("by_source_id", Value::Array(renamed));
    }
    result
}

fn first(root: &Root, pattern: &str) -> Option<String> {
    root.glob(pattern).first().map(|p| rel(root, p))
}

/// The stock LK and preloader regions (production-partitions.json): which bootloader a player carries
/// (3.2.0 vs 3.2.1, #45/#46).
const BOOTLOADERS: [(&str, &str, u64, usize); 2] = [
    ("lk", "/dev/mmcblk0", 0x1d2_0000, 0x6_0000),
    ("preloader", "/dev/mmcblk0boot0", 0, 0x4_0000),
];

/// The hardware's identity: board, panel, storage chips, input devices, bootloader hashes.
pub(super) fn hardware(root: &Root) -> Value {
    let compatible = root
        .read("/proc/device-tree/compatible")
        .unwrap_or_default()
        .replace('\0', " ");
    let mut data = object()
        .with("compatible", Value::from(python_strip(&compatible)))
        .with("cpuinfo", read(root, "/proc/cpuinfo"))
        .with("meminfo_total", Value::Null);
    let mut boot = object();
    for name in ["metadata", "lcm", "normal_boot"] {
        boot.set(name, read(root, &format!("/sys/firmware/y2_boot/{name}")));
    }
    let mut panel = object();
    for name in ["panel_kind", "panel_hint", "power_down", "stock_suspend"] {
        panel.set(
            name,
            read(root, &format!("/sys/module/panel/parameters/{name}")),
        );
    }
    data.set("y2_boot", boot);
    data.set("panel", panel);
    data.set("storage", object());
    data.set(
        "connectivity",
        read(root, "/sys/devices/platform/18070000.connectivity/status"),
    );
    data.set("input_devices", read(root, "/proc/bus/input/devices"));
    data.set("partitions", read(root, "/proc/partitions"));
    data.set("bootloaders", object());
    for line in pyre::splitlines(&root.read("/proc/meminfo").unwrap_or_default()) {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            data.set("meminfo_total", Value::from(python_strip(rest)));
        }
    }
    for host in ["mmc0", "mmc1"] {
        let Some(card) = first(root, &format!("/sys/class/mmc_host/{host}/{host}:*")) else {
            continue;
        };
        let mut fields = object();
        for name in [
            "type",
            "name",
            "manfid",
            "oemid",
            "prv",
            "rev",
            "fwrev",
            "hwrev",
            "date",
            "csd",
            "ocr",
            "life_time",
            "pre_eol_info",
            "enhanced_area_size",
            "erase_size",
            "preferred_erase_size",
        ] {
            fields.set(name, read(root, &format!("{card}/{name}")));
        }
        let ext_csd = first(root, &format!("/sys/kernel/debug/{host}/{host}:*/ext_csd"));
        fields.set("ext_csd", ext_csd.map_or(Value::Null, |p| read(root, &p)));
        fields.set("ios", read(root, &format!("/sys/kernel/debug/{host}/ios")));
        if let Some(storage) = data.get_mut("storage") {
            storage.set(host, fields);
        }
    }
    for (name, device, offset, length) in BOOTLOADERS {
        let result = (|| -> std::io::Result<Vec<u8>> {
            let mut file = File::open(root.path(device))?;
            file.seek(SeekFrom::Start(offset))?;
            let mut blob = Vec::new();
            file.take(length as u64).read_to_end(&mut blob)?;
            Ok(blob)
        })();
        let entry = match result {
            Ok(blob) => object()
                .with("sha256", Value::String(sha256_hex(&blob)))
                .with("bytes", Value::from(blob.len()))
                .with(
                    "lk_v320_prefix_sha256",
                    if name == "lk" {
                        Value::String(sha256_hex(&blob[..blob.len().min(242_440)]))
                    } else {
                        Value::Null
                    },
                ),
            Err(error) => object().with("error", Value::String(strerror(&error))),
        };
        if let Some(loaders) = data.get_mut("bootloaders") {
            loaders.set(name, entry);
        }
    }
    data
}
