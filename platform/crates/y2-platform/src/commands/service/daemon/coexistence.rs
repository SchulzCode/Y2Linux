//! `/run/y2/coexistence.json`: the Wi-Fi transfer rate and the Bluetooth link for the radio policy.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;

/// The Wi-Fi transfer rate and the Bluetooth link for the radio policy (`/run/y2/coexistence.json`);
/// provisional, no codec override.
#[derive(Default)]
pub(super) struct Coexistence {
    previous: Option<(f64, f64)>,
    /// This boot's id, read on the first pass: it cannot change while the service runs.
    boot_id: Option<Option<String>>,
}

impl Coexistence {
    pub(super) fn observe(
        &mut self,
        root: &Root,
        wifi: &Object,
        now: f64,
    ) -> Result<Map<String, Value>, Failure> {
        // Counters that are not numbers (the Wi-Fi observation never gives any) count as none.
        let total: f64 = match wifi.get("traffic_counters") {
            Some(Value::Object(stats)) => ["rx_bytes", "tx_bytes"]
                .iter()
                .filter_map(|key| py::number(stats.get(*key)))
                .sum(),
            _ => 0.0,
        };
        let mut rate = None;
        if let Some((previous_time, previous_bytes)) = self.previous {
            if now > previous_time && total >= previous_bytes {
                rate = Some((total - previous_bytes) / (now - previous_time));
            }
        }
        self.previous = Some((now, total));
        let mut value = Map::new();
        value.insert("schema".into(), json!(1));
        let boot_id = self.boot_id.get_or_insert_with(|| root.boot_id());
        value.insert("boot_id".into(), json!(boot_id));
        value.insert("monotonic_s".into(), json!(now));
        value.insert(
            "wifi_active".into(),
            json!(wifi.get("state").and_then(Value::as_str) == Some("Online")),
        );
        value.insert("wifi_bytes_per_second".into(), json!(rate));
        value.insert(
            "wifi_heavy_transfer".into(),
            json!(rate.is_some_and(|r| r >= 1024.0 * 1024.0)),
        );
        value.insert(
            "cpu_pressure".into(),
            json!(root.read("/proc/pressure/cpu")),
        );
        value.insert(
            "bluetooth_link".into(),
            Value::Object(root.json_object("/run/y2/bt-reconnect.json")),
        );
        value.insert("provisional".into(), json!(true));
        value.insert("codec_bitrate_override".into(), Value::Null);
        atomic_json(&root.path("/run/y2/coexistence.json"), &value, false)?;
        Ok(value)
    }
}
