//! `y2-platform network-check` (`network_check.check` under `workload.workload("NetworkTransfer")`):
//! explicit owner-run IP and throughput qualification, never run during boot.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    snapshot::{snapshot, Snapshot},
    util::{self, ObservationRecord},
};
use crate::reply::{Failure, Outcome, Reply};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use y2p_core::{command, Root};

/// The peer as a dotted quad, unchanged, or the `AddressValueError` text for it (the error record
/// shows it).
fn ipv4(text: &str) -> Result<String, Failure> {
    let repr = crate::argparse::python::repr;
    let error = |message: String| Failure::raised("AddressValueError", message);
    if text.contains('/') {
        return Err(error(format!("Unexpected '/' in {}", repr(text))));
    }
    if text.is_empty() {
        return Err(error("Address cannot be empty".into()));
    }
    let octets: Vec<&str> = text.split('.').collect();
    if octets.len() != 4 {
        return Err(error(format!("Expected 4 octets in {}", repr(text))));
    }
    for octet in octets {
        // `_parse_octet`, in order: the first bad octet names the error.
        let problem = if octet.is_empty() {
            Some("Empty octet not permitted".to_owned())
        } else if !octet.chars().all(|c| c.is_ascii_digit()) {
            Some(format!("Only decimal digits permitted in {}", repr(octet)))
        } else if octet.len() > 3 {
            Some(format!("At most 3 characters permitted in {}", repr(octet)))
        } else if octet != "0" && octet.starts_with('0') {
            Some(format!(
                "Leading zeros are not permitted in {}",
                repr(octet)
            ))
        } else {
            // At most three ASCII digits here: always a u16.
            let value: u16 = octet.parse().unwrap_or(u16::MAX);
            (value > 255).then(|| format!("Octet {value} (> 255) not permitted"))
        };
        if let Some(problem) = problem {
            return Err(error(format!("{problem} in {}", repr(text))));
        }
    }
    Ok(text.to_owned())
}

/// A command's outcome in the record.
#[derive(Serialize)]
struct Answer {
    ok: bool,
    reason: Option<String>,
    output: Option<String>,
}

impl From<&command::Outcome> for Answer {
    fn from(answer: &command::Outcome) -> Answer {
        Answer {
            ok: answer.ok,
            reason: answer.reason.clone(),
            output: answer.output.clone(),
        }
    }
}

/// iperf3's measurement as it printed it (its JSON), or why there is none.
#[derive(Serialize)]
#[serde(untagged)]
enum Measured {
    Iperf(Value),
    Missing(Answer),
}

/// What iperf3's JSON must hold to count: an `end.sum_sent` and no error.
#[derive(Deserialize, Default)]
#[serde(default)]
struct IperfSummary {
    #[serde(deserialize_with = "y2p_core::lenient::or_none")]
    end: Option<IperfEnd>,
    #[serde(deserialize_with = "y2p_core::lenient::text")]
    error: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct IperfEnd {
    #[serde(deserialize_with = "y2p_core::lenient::or_none")]
    sum_sent: Option<serde_json::Map<String, Value>>,
}

/// `network-check`'s record: the snapshot before, the measurements, the snapshot after.
#[derive(Serialize)]
struct Check {
    schema: &'static str,
    record: ObservationRecord,
    before: Snapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    ping: Option<Answer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    iperf3: Option<Measured>,
    #[serde(skip_serializing_if = "Option::is_none")]
    after: Option<Snapshot>,
}

/// `y2-platform network-check --peer A [--seconds N] [--throughput] [--interface wlan0|usb0]`.
pub fn run(root: &Root, peer: &str, seconds: i64, throughput: bool, interface: &str) -> Outcome {
    // `@workload("NetworkTransfer")`: the lease comes first, then ipaddress reads the peer.
    let _lease = crate::commands::workload::Lease::take(root, "NetworkTransfer")?;
    let peer = ipv4(peer)?;
    if !(1..=600).contains(&seconds) {
        return Err(Failure::value("invalid_network_qualification_parameters"));
    }
    let mut parameters = Map::new();
    parameters.insert("peer".into(), json!(peer));
    parameters.insert("interface".into(), json!(interface));
    parameters.insert("seconds".into(), json!(seconds));
    parameters.insert("throughput_requested".into(), json!(throughput));
    let record = util::record(root, "network-qualification", parameters);
    let before = snapshot(root, &[], false, 0.0)?;
    let mut check = Check {
        schema: "org.y2linux.benchmark/v1",
        record,
        before,
        ping: None,
        iperf3: None,
        after: None,
    };
    if interface == "wlan0" && check.before.wifi.state != "Online" {
        check.record.result = json!("FAILED");
        check.record.failure = json!("wifi_not_online");
        return Reply::sorted(&check, 1);
    }
    let ping = command::run(
        root,
        &["ping", "-I", interface, "-c", "5", "-W", "2", &peer],
        15.0,
    );
    let ok = if throughput {
        // Existing pinned iperf3 package, no new network protocol or package.
        let duration = seconds.to_string();
        let answer = command::run_with(
            root,
            &[
                "iperf3",
                "--bind-dev",
                interface,
                "-c",
                &peer,
                "-t",
                &duration,
                "-J",
            ],
            (seconds + 10) as f64,
            262_144,
            false,
        );
        let printed = answer
            .output
            .as_deref()
            .and_then(|text| serde_json::from_str::<Value>(text).ok());
        let (measured, valid) = match (answer.ok, printed) {
            (true, Some(printed)) => {
                let summary: IperfSummary =
                    serde_json::from_value(printed.clone()).unwrap_or_default();
                let valid = summary.end.and_then(|end| end.sum_sent).is_some()
                    && summary.error.as_deref().is_none_or(str::is_empty);
                (Measured::Iperf(printed), valid)
            }
            (true, None) => (
                Measured::Missing(Answer {
                    ok: false,
                    reason: Some("invalid_iperf_json".into()),
                    output: None,
                }),
                false,
            ),
            (false, _) => (Measured::Missing(Answer::from(&answer)), false),
        };
        check.iperf3 = Some(measured);
        answer.ok && valid && ping.ok
    } else {
        ping.ok
    };
    check.ping = Some(Answer::from(&ping));
    check.record.result = json!(if ok { "OK" } else { "FAILED" });
    check.record.failure = if ok {
        Value::Null
    } else {
        json!("network_test_failed_see_measurements")
    };
    let after = snapshot(root, &[], false, 0.0)?;
    check.after = Some(after);
    Reply::sorted(&check, u8::from(!ok))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peers_parse_as_ipaddress_does() {
        assert_eq!(ipv4("192.168.1.10").ok().as_deref(), Some("192.168.1.10"));
        assert_eq!(ipv4("0.0.0.0").ok().as_deref(), Some("0.0.0.0"));
        for (bad, message) in [
            ("192.168.1", "Expected 4 octets in '192.168.1'"),
            (
                "1.2.3.256",
                "Octet 256 (> 255) not permitted in '1.2.3.256'",
            ),
            (
                "01.2.3.4",
                "Leading zeros are not permitted in '01' in '01.2.3.4'",
            ),
            (
                "1.2.3.4 ",
                "Only decimal digits permitted in '4 ' in '1.2.3.4 '",
            ),
            (
                "a.b.c.d",
                "Only decimal digits permitted in 'a' in 'a.b.c.d'",
            ),
            ("1..2.3", "Empty octet not permitted in '1..2.3'"),
            (
                "1.2.3.4444",
                "At most 3 characters permitted in '4444' in '1.2.3.4444'",
            ),
            ("", "Address cannot be empty"),
            ("1.2.3.4/8", "Unexpected '/' in '1.2.3.4/8'"),
            ("router", "Expected 4 octets in 'router'"),
        ] {
            let error = ipv4(bad).unwrap_err();
            assert_eq!(error.message(), message, "{bad}");
            assert!(matches!(
                error,
                Failure::Os {
                    class: "AddressValueError",
                    ..
                }
            ));
        }
    }
}
