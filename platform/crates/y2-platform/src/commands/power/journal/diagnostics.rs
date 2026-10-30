//! The parts of `sleep_diagnostics` the power sample drives: a row around each USB state change
//! (POWER-FINAL-14), with the USB, network and sleep-parity state; `sleep-diagnostics.jsonl`.
// SPDX-License-Identifier: GPL-2.0-only
use super::{glob_name, json, Dropped};
use crate::commands::time::wall;
use serde_json::Value;
use y2p_core::{
    json::dumps,
    json::{Object, ObjectExt},
    mounts::data_ready,
    power_journal::append_bounded,
    root::read,
    Root,
};

const DIAGNOSTICS: &str = "/data/system/platform/sleep-diagnostics.jsonl";
const DIAGNOSTICS_LIMIT: u64 = 2097152;
const DIAGNOSTICS_ROW: usize = 65536;
pub(crate) const FOLLOW_UP_S: f64 = 30.0;
const MIN_INTERVAL_S: f64 = 10.0;

/// The USB state (journal::usb) as a row's object; a row without it is dropped.
pub fn usb_net(root: &Root) -> Result<Object, Dropped> {
    match serde_json::to_value(super::usb::usb_net(root).map_err(|_| Dropped)?) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Err(Dropped),
    }
}

/// `sleep_diagnostics.Diagnostics`: rows on suspend stages and around USB state changes.
#[derive(Debug, Default)]
pub struct Diagnostics {
    pub failures: u64,
    usb_signature: Option<Vec<(String, Option<String>)>>,
    follow_up: Option<f64>,
    last_row: Option<f64>,
}

impl Diagnostics {
    pub fn record(
        &mut self,
        root: &Root,
        stage: &str,
        now: f64,
        golden: bool,
        usb_reason: Option<&str>,
    ) -> bool {
        match self.row(root, stage, now, golden, usb_reason) {
            Ok(written) => {
                if written {
                    self.last_row = Some(now);
                }
                written
            }
            Err(Dropped) => {
                self.failures += 1;
                false
            }
        }
    }

    fn row(
        &self,
        root: &Root,
        stage: &str,
        now: f64,
        golden: bool,
        usb_reason: Option<&str>,
    ) -> Result<bool, Dropped> {
        if !data_ready(root) {
            return Ok(false);
        }
        let build_id = match json(root, "/etc/y2linux/versions.json") {
            Value::Object(o) => o.value("build_id"),
            _ => Value::Null,
        };
        let mut tail = super::state::kernel_tail(root, 80, 240);
        let mut row = Object::new()
            .with("schema", 1i64)
            .with("stage", stage)
            .with("usb_reason", usb_reason)
            .with("wall_time", wall())
            .with("monotonic_s", now)
            .with("boottime_s", root.clock().boottime_ns() as f64 / 1e9)
            .with("boot_id", root.boot_id())
            .with("build_id", build_id)
            .with(
                "features",
                serde_json::to_value(super::state::features(root, golden)).unwrap_or(Value::Null),
            )
            .with("usb_net", usb_net(root)?)
            .with(
                "kernel_pm",
                root.read_limited("/sys/firmware/y2_pm/state", 8192),
            )
            .with(
                "kernel_tail",
                tail.clone()
                    .map(|t| t.into_iter().map(Value::from).collect::<Vec<_>>()),
            );
        let encode = |row: &Object| dumps(row).map(|t| t + "\n").map_err(|_| Dropped);
        let mut encoded = encode(&row)?;
        while encoded.len() > DIAGNOSTICS_ROW {
            // Too large: drop the oldest quarter of the kernel lines, as long as there are any.
            let Some(lines) = tail.as_mut().filter(|t| !t.is_empty()) else {
                break;
            };
            let cut = (lines.len() / 4).max(1);
            lines.drain(..cut);
            row.set(
                "kernel_tail",
                lines.iter().cloned().map(Value::from).collect::<Vec<_>>(),
            );
            encoded = encode(&row)?;
        }
        if encoded.len() > DIAGNOSTICS_ROW {
            return Err(Dropped); // sleep_diagnostics_row_too_large
        }
        append_bounded(
            root,
            DIAGNOSTICS,
            encoded.as_bytes(),
            DIAGNOSTICS_LIMIT,
            true,
        )
        .map_err(|_| Dropped)?;
        Ok(true)
    }

    /// Called on the existing one-second power sample; cheap when nothing changed.
    pub fn observe_usb(&mut self, root: &Root, now: f64) -> bool {
        let mut signature: Vec<(String, Option<String>)> = root
            .glob("/sys/class/udc/*")
            .iter()
            .map(|p| (glob_name(p), read(&p.join("state"), 64)))
            .collect();
        signature.sort();
        let mut reason = None;
        if self.usb_signature.as_ref().is_some_and(|s| *s != signature) {
            reason = Some("udc_change");
            self.follow_up = Some(now + FOLLOW_UP_S);
        } else if self.follow_up.is_some_and(|f| now >= f) {
            reason = Some("udc_follow_up");
            self.follow_up = None;
        }
        self.usb_signature = Some(signature);
        let Some(reason) = reason else {
            return false;
        };
        if let Some(last) = self.last_row.filter(|last| now - last < MIN_INTERVAL_S) {
            // Rate-limited: a change still gets its follow-up, a follow-up is postponed.
            self.follow_up = Some(if reason == "udc_change" {
                now + FOLLOW_UP_S
            } else {
                last + MIN_INTERVAL_S
            });
            return false;
        }
        self.record(root, "usb", now, false, Some(reason))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use y2p_core::py::PyValue;

    /// POWER-FINAL-14: a UDC state change gives a durable row, then one follow-up 30 s later; rows
    /// closer than 10 s are postponed.
    #[test]
    fn usb_changes_leave_rows_and_one_follow_up() {
        let dir = std::env::temp_dir().join(format!("y2-usb-rows-{}", std::process::id()));
        let put = |name: &str, text: &str| {
            let path = dir.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        };
        put(
            "proc/self/mountinfo",
            "30 22 179:9 / /data rw - ext4 /dev/mmcblk0p9 rw\n",
        );
        put("run/y2-data-device", "/dev/mmcblk0p9\n");
        put("data/.y2data-schema", "1\n");
        put("sys/class/udc/11200000.usb/state", "configured\n");
        let root = Root::new(&dir);
        let rows = || {
            fs::read_to_string(dir.join("data/system/platform/sleep-diagnostics.jsonl"))
                .unwrap_or_default()
                .lines()
                .map(
                    |l| match serde_json::from_slice::<Value>(l.as_bytes()).unwrap() {
                        Value::Object(o) => o.value("usb_reason").py_str(),
                        _ => unreachable!(),
                    },
                )
                .collect::<Vec<_>>()
        };
        let mut diagnostics = Diagnostics::default();
        assert!(!diagnostics.observe_usb(&root, 100.0));
        put("sys/class/udc/11200000.usb/state", "not attached\n");
        assert!(diagnostics.observe_usb(&root, 101.0));
        put("sys/class/udc/11200000.usb/state", "configured\n");
        assert!(!diagnostics.observe_usb(&root, 105.0), "rate-limited");
        assert!(!diagnostics.observe_usb(&root, 120.0));
        assert!(
            diagnostics.observe_usb(&root, 135.0),
            "the follow-up of the postponed change"
        );
        assert!(!diagnostics.observe_usb(&root, 200.0));
        assert_eq!(rows(), ["udc_change", "udc_follow_up"]);
        fs::remove_dir_all(&dir).unwrap();
    }
}
