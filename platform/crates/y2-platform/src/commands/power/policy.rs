//! `power.load_policy` and `power.LowBattery`: UI-independent shutdown policy. Electrical thresholds
//! are owner-qualified data.
// SPDX-License-Identifier: GPL-2.0-only
use super::battery::{BatteryStatus, Confidence};
use serde::Deserialize;
use serde_json::Value;
use y2p_core::{lenient, py::python_number, Root};

pub const POLICY: &str = "/data/system/platform/power-policy.json";

/// A qualified policy.
#[derive(Clone, Debug)]
pub struct Policy {
    /// The document's `qualification_reference` (none when it is no text), for power.json.
    pub qualification_reference: Option<String>,
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

/// power-policy.json as stored: integers only where it has integers (a float or a flag is none).
#[derive(Deserialize, Default)]
#[serde(default)]
struct Stored {
    /// As Python's `==` compared it (`1.0` and `true` count as 1).
    #[serde(deserialize_with = "number")]
    schema: Option<f64>,
    #[serde(deserialize_with = "lenient::is_true")]
    enabled: bool,
    #[serde(deserialize_with = "lenient::integer")]
    critical_uv: Option<i64>,
    #[serde(deserialize_with = "lenient::integer")]
    low_uv: Option<i64>,
    #[serde(deserialize_with = "lenient::integer")]
    recover_uv: Option<i64>,
    #[serde(deserialize_with = "lenient::integer")]
    critical_samples: Option<i64>,
    #[serde(deserialize_with = "lenient::integer")]
    grace_seconds: Option<i64>,
    #[serde(deserialize_with = "lenient::integer")]
    warning_soc: Option<i64>,
    #[serde(deserialize_with = "lenient::integer")]
    critical_soc: Option<i64>,
    #[serde(deserialize_with = "lenient::integer")]
    shutdown_soc: Option<i64>,
    qualification_reference: Option<Reference>,
    source: Option<Reference>,
}

fn number<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    Ok(python_number(Some(&Value::deserialize(deserializer)?)))
}

/// Who or what qualified the policy, as written: a text, or something else (whether Python called
/// it true).
#[derive(Clone, Debug)]
enum Reference {
    Text(String),
    Other(bool),
}

impl<'de> Deserialize<'de> for Reference {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match Value::deserialize(deserializer)? {
            Value::String(text) => Reference::Text(text),
            other => Reference::Other(y2p_core::py::truthy(&other)),
        })
    }
}

impl Reference {
    fn truthy(&self) -> bool {
        match self {
            Reference::Text(text) => !text.is_empty(),
            Reference::Other(truthy) => *truthy,
        }
    }
}

pub fn load_policy(root: &Root) -> (Option<Policy>, Option<&'static str>) {
    let value = match root.json(POLICY) {
        Some(Value::Object(map)) => map,
        _ => root.json_object("/etc/y2linux/power-policy.json"),
    };
    let stored: Stored = lenient::read(value);
    // No derived or fallback voltage thresholds. A reference records who/what
    // qualified the policy; software cannot certify that physical evidence.
    if !matches!(stored.schema, Some(1.0 | 2.0)) || !stored.enabled {
        return (None, Some("thresholds_disabled_PHYSICAL_GATE"));
    }
    let (
        Some(critical_uv),
        Some(low_uv),
        Some(recover_uv),
        Some(critical_samples),
        Some(grace_seconds),
    ) = (
        stored.critical_uv,
        stored.low_uv,
        stored.recover_uv,
        stored.critical_samples,
        stored.grace_seconds,
    )
    else {
        return (None, Some("invalid_policy"));
    };
    // `value.get('qualification_reference') or value.get('source')`.
    let reference = match &stored.qualification_reference {
        Some(reference) if reference.truthy() => Some(reference),
        _ => stored.source.as_ref(),
    };
    let reference_ok = matches!(reference, Some(Reference::Text(text))
        if (1..=256).contains(&text.chars().count()));
    if !(0 < critical_uv
        && critical_uv < low_uv
        && low_uv < recover_uv
        && (1..=60).contains(&critical_samples)
        && (3..=30).contains(&grace_seconds)
        && reference_ok)
    {
        return (None, Some("invalid_policy"));
    }
    let schema2 = stored.schema == Some(2.0);
    let (warning_soc, critical_soc, shutdown_soc) = if schema2 {
        match (stored.warning_soc, stored.critical_soc, stored.shutdown_soc) {
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
            qualification_reference: match stored.qualification_reference {
                Some(Reference::Text(text)) => Some(text),
                _ => None,
            },
        }),
        None,
    )
}

pub use y2p_contract::power::BatteryState;

/// `LowBattery`: the state of the last sample and how many critical samples came in a row.
#[derive(Debug, Default)]
pub struct LowBattery {
    pub state: BatteryState,
    samples: i64,
}

impl LowBattery {
    pub fn observe(
        &mut self,
        policy: Option<&Policy>,
        voltage: Option<i64>,
        present: Option<i64>,
        soc: &BatteryStatus,
    ) -> BatteryState {
        let Some(policy) = policy else {
            (self.state, self.samples) = (BatteryState::Unavailable, 0);
            return self.state;
        };
        let Some(voltage) = voltage.filter(|v| present == Some(1) && *v > 0) else {
            (self.state, self.samples) = (BatteryState::Unavailable, 0);
            return self.state;
        };
        // USB presence does not prove positive net pack current. The same
        // qualified voltage guard applies while connected to a source.
        let valid_soc = soc.percent.filter(|p| (0..=100).contains(p));
        let trustworthy = valid_soc.is_some()
            && matches!(
                soc.confidence,
                Confidence::Hardware | Confidence::CalibratedEstimate
            );
        // F36: on a charger the shown value never falls (battery.Soc.track), so a
        // low one is a start value from before the charge; the voltage guard
        // alone covers a source weaker than the load.
        let charging = soc.charging || soc.status.as_deref() == Some("Charging");
        let soc_shutdown = policy.schema2
            && trustworthy
            && !charging
            && valid_soc.is_some_and(|p| p <= policy.shutdown_soc);
        if voltage <= policy.critical_uv || soc_shutdown {
            self.samples += 1;
            self.state = BatteryState::Critical;
            if self.samples >= policy.critical_samples {
                self.state = BatteryState::ShutdownPending;
            }
        } else {
            self.samples = 0;
            if policy.schema2 && valid_soc.is_some_and(|p| p <= policy.critical_soc) {
                self.state = BatteryState::Critical;
            } else if voltage <= policy.low_uv
                || (policy.schema2 && valid_soc.is_some_and(|p| p <= policy.warning_soc))
            {
                self.state = BatteryState::Low;
            } else if voltage >= policy.recover_uv || self.state == BatteryState::Unavailable {
                self.state = BatteryState::Normal;
            } else if matches!(
                self.state,
                BatteryState::Critical | BatteryState::ShutdownPending
            ) {
                self.state = BatteryState::Low;
            }
        }
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
        let soc = |percent: i64, status: &str| BatteryStatus {
            percent: Some(percent),
            confidence: Confidence::CalibratedEstimate,
            status: Some(status.into()),
            charging: status == "Charging",
            ..BatteryStatus::default()
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
