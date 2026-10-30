//! `y2-platform status`: read-only platform snapshots. Null means unobserved, never zero or success.
//! One submodule per section (`cpu`, `memory`, `thermal`, `power`, `storage`, `wifi`, `readiness`);
//! `bluetooth` and `system` are the group's own modules.
// SPDX-License-Identifier: GPL-2.0-only
mod cpu;
mod idle;
mod memory;
mod power;
mod readiness;
mod storage;
mod thermal;
pub(crate) mod wifi;

pub use cpu::{cpu, utilization};
// For the suspend group's receipt tests.
#[cfg(test)]
pub(crate) use cpu::timer_runtime;
pub use memory::memory;
pub use power::{power, printable};
pub use storage::{storage, storage_mode};
pub use thermal::thermal;
pub use wifi::wifi;

use super::{
    bluetooth, system,
    util::{self, int, read, text},
};
use crate::{regex, reply::Failure};
use serde::Serialize;
use serde_json::{json, Map, Value};
use y2p_core::Root;

/// `y2-platform status`: every section of the platform (in this key order). Sections are the JSON
/// their modules build; `audio` is assembled here.
#[derive(Serialize)]
pub struct Snapshot {
    pub schema: &'static str,
    pub record: Map<String, Value>,
    pub cpu: Map<String, Value>,
    pub memory: Map<String, Value>,
    pub thermal: Value,
    pub power: Value,
    pub storage: Value,
    pub wifi: Value,
    pub bluetooth: Value,
    pub system: Value,
    pub audio: Audio,
    pub readiness: Value,
    /// `--reborn`: Reborn's own metrics record.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reborn_metrics: Option<Value>,
}

/// The `audio` section: what ALSA lists and the qualified profile; the headphone jack is not observed.
#[derive(Serialize)]
pub struct Audio {
    pub cards: Option<String>,
    pub pcm: Option<String>,
    pub headphone_jack_present: Option<bool>,
    pub headphone_jack_reason: &'static str,
    pub qualified_profile: Option<Value>,
    pub enabled_profile: Option<Value>,
    pub pcm_contract: Option<Value>,
    pub hw_params: Map<String, Value>,
}

impl Snapshot {
    /// The snapshot as a JSON object.
    pub fn into_map(self) -> Result<Map<String, Value>, Failure> {
        match serde_json::to_value(self).map_err(Failure::value)? {
            Value::Object(map) => Ok(map),
            _ => Err(Failure::value("status_snapshot_unserializable")),
        }
    }

    /// Whether the snapshot can be printed (`printable`): only the power section holds sleep products.
    pub fn printable(&self) -> Result<(), Failure> {
        power::printable_section(&self.power)
    }

    /// The boot this snapshot was taken in (null when unobserved).
    pub fn boot_id(&self) -> &Value {
        self.record.get("boot_id").unwrap_or(&Value::Null)
    }
}

/// One gendisk lifetime of a block device (`block:<dev_t>:diskseq:<n>`), or None: a card removed and
/// inserted again (or another card) has another disk sequence number under the same dev_t.
pub fn device_instance(root: &Root, device_id: &str) -> Option<String> {
    let path = y2p_core::sys::real_path(&root.path(&format!("/sys/dev/block/{device_id}"))).ok()?;
    let disk = if path.join("partition").is_file() {
        path.parent()?.to_path_buf()
    } else {
        path.clone()
    };
    let dev = read(&path.join("dev"));
    let seq = read(&disk.join("diskseq"));
    let dev = dev.filter(|d| d == device_id && regex!(r"\A[0-9]+:[0-9]+\z").is_match(d))?;
    let seq = seq.filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))?;
    (int(&seq)? > 0).then(|| format!("block:{dev}:diskseq:{seq}"))
}

/// The whole snapshot: every section for these processes (`pss`: with their PSS), the CPU section
/// read twice `interval` seconds apart for utilisation when it is not zero.
pub fn snapshot(root: &Root, pids: &[i64], pss: bool, interval: f64) -> Result<Snapshot, Failure> {
    let start = root.clock().monotonic_ns();
    let mut processor = cpu(root)?;
    if interval > 0.0 {
        std::thread::sleep(std::time::Duration::from_secs_f64(interval));
        let later = cpu(root)?;
        let busy = utilization(processor.get("ticks"), later.get("ticks"));
        processor = later;
        processor.insert("utilization_percent".into(), busy);
    }
    let mut hw_params = Map::new();
    let base = root.path("/");
    for path in root.glob("/proc/asound/card*/pcm*/sub*/hw_params") {
        let relative = path
            .strip_prefix(&base)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        hw_params.insert(relative, text(read(&path)));
    }
    let mut result = Snapshot {
        schema: y2p_contract::schema::STATUS,
        record: util::record(root, "observation", Map::new()),
        cpu: processor,
        memory: memory(root, pids, pss)?,
        thermal: thermal(root),
        power: power(root)?,
        storage: storage(root)?,
        wifi: wifi(root)?,
        bluetooth: bluetooth::bluetooth(root)?,
        system: system::system(root)?,
        audio: Audio {
            cards: root.read("/proc/asound/cards"),
            pcm: root.read("/proc/asound/pcm"),
            headphone_jack_present: None,
            headphone_jack_reason: "codec_irq_not_qualified",
            qualified_profile: root.json("/etc/y2linux/audio-qualified.json"),
            enabled_profile: root.json("/etc/y2linux/audio-enabled.json"),
            pcm_contract: root.json("/sys/devices/platform/11220000.audio-controller/pcm_contract"),
            hw_params,
        },
        readiness: Value::Null,
        reborn_metrics: None,
    };
    result.readiness = readiness::readiness(&result)?;
    result.record.insert("result".into(), json!("observed"));
    result
        .record
        .insert("units".into(), json!({"collection_duration": "ns"}));
    result.record.insert(
        "collection_duration_ns".into(),
        json!(root.clock().monotonic_ns() - start),
    );
    Ok(result)
}

#[cfg(test)]
mod tests;
