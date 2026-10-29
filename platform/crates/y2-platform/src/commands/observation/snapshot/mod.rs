//! `observe.snapshot`: read-only platform snapshots. Null means unobserved, never zero or success.
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
mod wifi;

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
use serde_json::{json, Map, Value};
use y2p_core::Root;

pub type Snapshot = Map<String, Value>;

/// `observe.device_instance`: one gendisk lifetime (dev_t plus diskseq), or None.
pub fn device_instance(root: &Root, device_id: &str) -> Option<String> {
    let path = std::fs::canonicalize(root.path(&format!("/sys/dev/block/{device_id}"))).ok()?;
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

/// `observe.snapshot(ctx, pids, pss, interval)`.
pub fn snapshot(root: &Root, pids: &[i64], pss: bool, interval: f64) -> Result<Snapshot, Failure> {
    let start = root.clock().monotonic_ns();
    let before = cpu(root)?;
    let mut after = before.clone();
    if interval > 0.0 {
        std::thread::sleep(std::time::Duration::from_secs_f64(interval));
        after = cpu(root)?;
        let busy = utilization(before.get("ticks"), after.get("ticks"));
        after.insert("utilization_percent".into(), busy);
    }
    let mut result = Map::new();
    result.insert("schema".into(), json!(y2p_contract::schema::STATUS));
    result.insert(
        "record".into(),
        Value::Object(util::record(root, "observation", Map::new())),
    );
    result.insert("cpu".into(), Value::Object(after));
    result.insert("memory".into(), Value::Object(memory(root, pids, pss)?));
    result.insert("thermal".into(), thermal(root));
    result.insert("power".into(), power(root)?);
    result.insert("storage".into(), storage(root)?);
    result.insert("wifi".into(), wifi(root)?);
    result.insert("bluetooth".into(), bluetooth::bluetooth(root)?);
    result.insert("system".into(), system::system(root)?);
    let mut hw = Map::new();
    let base = root.path("/");
    for path in root.glob("/proc/asound/card*/pcm*/sub*/hw_params") {
        let relative = path
            .strip_prefix(&base)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        hw.insert(relative, text(read(&path)));
    }
    result.insert("audio".into(), json!({
        "cards": root.read("/proc/asound/cards"), "pcm": root.read("/proc/asound/pcm"),
        "headphone_jack_present": null, "headphone_jack_reason": "codec_irq_not_qualified",
        "qualified_profile": root.json("/etc/y2linux/audio-qualified.json"),
        "enabled_profile": root.json("/etc/y2linux/audio-enabled.json"),
        "pcm_contract": root.json("/sys/devices/platform/11220000.audio-controller/pcm_contract"),
        "hw_params": hw,
    }));
    let ready = readiness::readiness(&result)?;
    result.insert("readiness".into(), ready);
    let record = result["record"].as_object_mut().expect("record");
    record.insert("result".into(), json!("observed"));
    record.insert("units".into(), json!({"collection_duration": "ns"}));
    record.insert(
        "collection_duration_ns".into(),
        json!(root.clock().monotonic_ns() - start),
    );
    Ok(result)
}

#[cfg(test)]
mod tests;
