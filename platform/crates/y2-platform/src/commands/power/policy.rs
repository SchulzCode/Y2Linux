//! `power.load_policy` and `power.LowBattery`: UI-independent shutdown policy. Electrical thresholds
//! are owner-qualified data.
// SPDX-License-Identifier: GPL-2.0-only
use serde_json::{Map, Value};
use y2p_core::{
    json::Object,
    py::PyValue,
    py::{eq_int, exact_int, is_true},
    Root,
};

pub const POLICY: &str = "/data/system/platform/power-policy.json";

/// A qualified policy (`value` is the whole document, as Python keeps it).
#[derive(Clone, Debug)]
pub struct Policy {
    pub value: Map<String, Value>,
    pub schema2: bool,
    pub critical_uv: i64,
    pub low_uv: i64,
    pub recover_uv: i64,
    pub critical_samples: i64,
    pub grace_seconds: i64,
    pub warning_soc: i64,
    pub critical_soc: i64,
    pub shutdown_soc: i64,
}

/// `ctx.json(name, default)` with a dict default: an object, else the default.
fn json_dict(root: &Root, name: &str, default: Map<String, Value>) -> Map<String, Value> {
    match root.json(name) {
        Some(Value::Object(map)) => map,
        _ => default,
    }
}

pub fn load_policy(root: &Root) -> (Option<Policy>, Option<&'static str>) {
    let value = json_dict(
        root,
        POLICY,
        root.json_object("/etc/y2linux/power-policy.json"),
    );
    // No derived or fallback voltage thresholds. A reference records who/what
    // qualified the policy; software cannot certify that physical evidence.
    let schema = value.get("schema");
    if !(eq_int(schema, 1) || eq_int(schema, 2)) || !is_true(value.get("enabled")) {
        return (None, Some("thresholds_disabled_PHYSICAL_GATE"));
    }
    let int = |key: &str| exact_int(value.get(key));
    let (
        Some(critical_uv),
        Some(low_uv),
        Some(recover_uv),
        Some(critical_samples),
        Some(grace_seconds),
    ) = (
        int("critical_uv"),
        int("low_uv"),
        int("recover_uv"),
        int("critical_samples"),
        int("grace_seconds"),
    )
    else {
        return (None, Some("invalid_policy"));
    };
    // `value.get('qualification_reference') or value.get('source')`.
    let reference = match value.get("qualification_reference") {
        Some(v) if y2p_core::py::truthy(v) => Some(v),
        _ => value.get("source"),
    };
    let reference_ok = reference
        .and_then(Value::as_str)
        .is_some_and(|s| (1..=256).contains(&s.chars().count()));
    if !(0 < critical_uv
        && critical_uv < low_uv
        && low_uv < recover_uv
        && (1..=60).contains(&critical_samples)
        && (3..=30).contains(&grace_seconds)
        && reference_ok)
    {
        return (None, Some("invalid_policy"));
    }
    let schema2 = eq_int(schema, 2);
    let (warning_soc, critical_soc, shutdown_soc) = if schema2 {
        match (int("warning_soc"), int("critical_soc"), int("shutdown_soc")) {
            (Some(w), Some(c), Some(s)) if 0 <= s && s < c && c < w && w <= 50 => (w, c, s),
            _ => return (None, Some("invalid_policy")),
        }
    } else {
        (0, 0, 0)
    };
    (
        Some(Policy {
            schema2,
            critical_uv,
            low_uv,
            recover_uv,
            critical_samples,
            grace_seconds,
            warning_soc,
            critical_soc,
            shutdown_soc,
            value,
        }),
        None,
    )
}

/// `LowBattery`: Unavailable, Normal, Low, Critical or ShutdownPending after `critical_samples`
/// consecutive critical samples.
#[derive(Debug)]
pub struct LowBattery {
    pub state: &'static str,
    samples: i64,
}

impl Default for LowBattery {
    fn default() -> Self {
        Self {
            state: "Unavailable",
            samples: 0,
        }
    }
}

impl LowBattery {
    pub fn observe(
        &mut self,
        policy: Option<&Policy>,
        voltage: Option<i64>,
        present: Option<i64>,
        soc: &Object,
    ) -> &'static str {
        let Some(policy) = policy else {
            (self.state, self.samples) = ("Unavailable", 0);
            return self.state;
        };
        let Some(voltage) = voltage.filter(|v| present == Some(1) && *v > 0) else {
            (self.state, self.samples) = ("Unavailable", 0);
            return self.state;
        };
        // USB presence does not prove positive net pack current. The same
        // qualified voltage guard applies while connected to a source.
        let percent = soc.get("percent").and_then(Value::exact_int);
        let valid_soc = percent.filter(|p| (0..=100).contains(p));
        let trustworthy = valid_soc.is_some()
            && soc
                .get("confidence")
                .is_some_and(|c| c.is_str("hardware") || c.is_str("calibrated_estimate"));
        // F36: on a charger the shown value never falls (battery.Soc.track), so a
        // low one is a start value from before the charge; the voltage guard
        // alone covers a source weaker than the load.
        let charging = matches!(soc.get("charging"), Some(Value::Bool(true)))
            || soc.get("status").is_some_and(|s| s.is_str("Charging"));
        let soc_shutdown = policy.schema2
            && trustworthy
            && !charging
            && valid_soc.is_some_and(|p| p <= policy.shutdown_soc);
        if voltage <= policy.critical_uv || soc_shutdown {
            self.samples += 1;
            self.state = "Critical";
            if self.samples >= policy.critical_samples {
                self.state = "ShutdownPending";
            }
        } else {
            self.samples = 0;
            if policy.schema2 && valid_soc.is_some_and(|p| p <= policy.critical_soc) {
                self.state = "Critical";
            } else if voltage <= policy.low_uv
                || (policy.schema2 && valid_soc.is_some_and(|p| p <= policy.warning_soc))
            {
                self.state = "Low";
            } else if voltage >= policy.recover_uv || self.state == "Unavailable" {
                self.state = "Normal";
            } else if matches!(self.state, "Critical" | "ShutdownPending") {
                self.state = "Low";
            }
        }
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use y2p_core::json::ObjectExt;

    fn root_with(policy: &str) -> (std::path::PathBuf, Root) {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "y2-policy-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(dir.join("data/system/platform")).unwrap();
        fs::write(dir.join("data/system/platform/power-policy.json"), policy).unwrap();
        let root = Root::new(&dir);
        (dir, root)
    }

    #[test]
    fn policies_are_validated_as_python_does() {
        let stock = r#"{"schema": 2, "enabled": true, "source": "s", "critical_uv": 3400000, "low_uv": 3500000,
            "recover_uv": 3600000, "critical_samples": 5, "grace_seconds": 10, "warning_soc": 20,
            "critical_soc": 10, "shutdown_soc": 5}"#;
        for (text, reason) in [
            (stock.to_owned(), None),
            (stock.replace("\"schema\": 2", "\"schema\": 2.0"), None),
            (stock.replace("\"schema\": 2", "\"schema\": true"), None),
            (
                stock.replace("\"enabled\": true", "\"enabled\": 1"),
                Some("thresholds_disabled_PHYSICAL_GATE"),
            ),
            (
                stock.replace("\"grace_seconds\": 10", "\"grace_seconds\": 2"),
                Some("invalid_policy"),
            ),
            (
                stock.replace("\"source\": \"s\"", "\"source\": \"\""),
                Some("invalid_policy"),
            ),
            (
                stock.replace(
                    "\"source\": \"s\"",
                    "\"qualification_reference\": 0, \"source\": \"x\"",
                ),
                None,
            ),
            (
                stock.replace("\"shutdown_soc\": 5", "\"shutdown_soc\": 10"),
                Some("invalid_policy"),
            ),
            (
                stock.replace("\"critical_uv\": 3400000", "\"critical_uv\": 3400000.0"),
                Some("invalid_policy"),
            ),
        ] {
            let (dir, root) = root_with(&text);
            let (policy, found) = load_policy(&root);
            assert_eq!(found, reason, "{text}");
            assert_eq!(policy.is_some(), reason.is_none());
            fs::remove_dir_all(dir).unwrap();
        }
        let (dir, root) = root_with(stock);
        let policy = load_policy(&root).0.unwrap();
        let mut low = LowBattery::default();
        let soc = |percent: i64, status: &str| {
            Object::new()
                .with("percent", percent)
                .with("confidence", "calibrated_estimate")
                .with("status", status)
                .with("charging", status == "Charging")
        };
        assert_eq!(
            low.observe(
                Some(&policy),
                Some(3_800_000),
                Some(1),
                &soc(50, "Discharging")
            ),
            "Normal"
        );
        assert_eq!(
            low.observe(
                Some(&policy),
                Some(3_800_000),
                Some(1),
                &soc(15, "Discharging")
            ),
            "Low"
        );
        assert_eq!(
            low.observe(
                Some(&policy),
                Some(3_800_000),
                Some(1),
                &soc(8, "Discharging")
            ),
            "Critical"
        );
        // F36: a low start value on a charger never shuts down by itself.
        for _ in 0..6 {
            assert_eq!(
                low.observe(Some(&policy), Some(3_800_000), Some(1), &soc(4, "Charging")),
                "Critical"
            );
        }
        for expected in [
            "Critical",
            "Critical",
            "Critical",
            "Critical",
            "ShutdownPending",
            "ShutdownPending",
        ] {
            assert_eq!(
                low.observe(
                    Some(&policy),
                    Some(3_800_000),
                    Some(1),
                    &soc(4, "Discharging")
                ),
                expected
            );
        }
        assert_eq!(
            low.observe(
                Some(&policy),
                Some(3_550_000),
                Some(1),
                &soc(30, "Discharging")
            ),
            "Low"
        );
        assert_eq!(
            low.observe(None, Some(3_550_000), Some(1), &soc(30, "Discharging")),
            "Unavailable"
        );
        assert_eq!(
            low.observe(
                Some(&policy),
                Some(3_550_000),
                Some(1),
                &soc(30, "Discharging")
            ),
            "Normal"
        );
        assert_eq!(
            low.observe(
                Some(&policy),
                Some(3_550_000),
                None,
                &soc(30, "Discharging")
            ),
            "Unavailable"
        );
        fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
#[path = "policy_tests.rs"]
mod policy_tests;
