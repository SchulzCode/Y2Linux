//! `y2-platform network-check` (`network_check.check` under `workload.workload("NetworkTransfer")`):
//! explicit owner-run IP and throughput qualification, never run during boot.
// SPDX-License-Identifier: GPL-2.0-only
use super::{snapshot::snapshot, util};
use crate::reply::{private_directory, Failure, Outcome, Reply};
use serde_json::{json, Map, Value};
use std::{
    fs::OpenOptions,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError},
    thread,
    time::Duration,
};
use y2p_core::{command, lock, Root};

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

/// The stop flag, also from a holder that panicked (the flag stays meaningful).
fn locked(flag: &Mutex<bool>) -> MutexGuard<'_, bool> {
    flag.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The semantic QoS lease (`workload.workload`): a shared activity lease (sleep takes it
/// exclusively) and `<class> 3000` written to /dev/y2-workload now and every second until the work
/// ends. Unlike optional frequency hints this is a mandatory operation exclusion; flock dies with the
/// process, a leftover file is inert.
struct Lease {
    _lock: std::fs::File,
    stop: Arc<(Mutex<bool>, Condvar)>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Lease {
    fn take(root: &Root, class: &str) -> Result<Lease, Failure> {
        let runtime = root.path("/run/y2");
        private_directory(&runtime)?;
        let path = runtime.join("activity.lock");
        let lock = match lock::try_shared(&path) {
            Ok(Some(file)) => file,
            Ok(None) => return Err(Failure::value("sleep_operation_in_progress")),
            Err(e) => return Err(Failure::os(e, &path)),
        };
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let payload = format!("{class} 3000\n").into_bytes();
        let mut worker = None;
        // QoS is optional; never fail a transfer on a hint.
        if let Ok(mut device) = OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_CLOEXEC)
            .open(root.path("/dev/y2-workload"))
        {
            // Acquire before work starts; renew on an independent worker even when a blocking
            // transfer subprocess runs for minutes.
            if device.write(&payload).ok() == Some(payload.len()) {
                let stop = Arc::clone(&stop);
                worker = Some(thread::spawn(move || {
                    let (stopped, wake) = &*stop;
                    loop {
                        if *locked(stopped) {
                            return;
                        }
                        if device.write(&payload).ok() != Some(payload.len()) {
                            return;
                        }
                        let (guard, _) = wake
                            .wait_timeout_while(locked(stopped), Duration::from_secs(1), |s| !*s)
                            .unwrap_or_else(PoisonError::into_inner);
                        if *guard {
                            return;
                        }
                    }
                }));
            }
        }
        Ok(Lease {
            _lock: lock,
            stop,
            worker,
        })
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let (stopped, wake) = &*self.stop;
        *locked(stopped) = true;
        wake.notify_all();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn outcome(answer: &command::Outcome) -> Value {
    json!({"ok": answer.ok, "reason": answer.reason, "output": answer.output})
}

/// `y2-platform network-check --peer A [--seconds N] [--throughput] [--interface wlan0|usb0]`.
pub fn run(root: &Root, peer: &str, seconds: i64, throughput: bool, interface: &str) -> Outcome {
    // `@workload("NetworkTransfer")`: the lease comes first, then ipaddress reads the peer.
    let _lease = Lease::take(root, "NetworkTransfer")?;
    let peer = ipv4(peer)?;
    if !(1..=600).contains(&seconds) {
        return Err(Failure::value("invalid_network_qualification_parameters"));
    }
    let mut parameters = Map::new();
    parameters.insert("peer".into(), json!(peer));
    parameters.insert("interface".into(), json!(interface));
    parameters.insert("seconds".into(), json!(seconds));
    parameters.insert("throughput_requested".into(), json!(throughput));
    let mut record = util::record(root, "network-qualification", parameters);
    let before = snapshot(root, &[], false, 0.0)?;
    let mut result = Map::new();
    result.insert("schema".into(), json!("org.y2linux.benchmark/v1"));
    if interface == "wlan0" && before["wifi"]["state"] != "Online" {
        record.insert("result".into(), json!("FAILED"));
        record.insert("failure".into(), json!("wifi_not_online"));
        result.insert("record".into(), Value::Object(record));
        result.insert("before".into(), Value::Object(before));
        super::snapshot::printable(&result)?;
        return Reply::sorted(&result, 1);
    }
    result.insert("before".into(), Value::Object(before));
    let ping = command::run(
        root,
        &["ping", "-I", interface, "-c", "5", "-W", "2", &peer],
        15.0,
    );
    result.insert("ping".into(), outcome(&ping));
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
        let measured = if answer.ok {
            serde_json::from_str::<Value>(answer.output.as_deref().unwrap_or_default())
                .unwrap_or(json!({"ok": false, "reason": "invalid_iperf_json"}))
        } else {
            outcome(&answer)
        };
        let valid = measured.as_object().is_some_and(|m| {
            m.get("end")
                .and_then(Value::as_object)
                .is_some_and(|end| end.get("sum_sent").is_some_and(Value::is_object))
                && !m.get("error").is_some_and(y2p_core::py::truthy)
        });
        result.insert("iperf3".into(), measured);
        answer.ok && valid && ping.ok
    } else {
        ping.ok
    };
    record.insert("result".into(), json!(if ok { "OK" } else { "FAILED" }));
    record.insert(
        "failure".into(),
        if ok {
            Value::Null
        } else {
            json!("network_test_failed_see_measurements")
        },
    );
    result.insert("record".into(), Value::Object(record));
    let after = snapshot(root, &[], false, 0.0)?;
    result.insert("after".into(), Value::Object(after));
    super::snapshot::printable(&result)?;
    Reply::sorted(&result, u8::from(!ok))
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
