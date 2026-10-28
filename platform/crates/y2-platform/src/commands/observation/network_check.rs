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
    sync::{Arc, Condvar, Mutex},
    thread,
    time::Duration,
};
use y2p_core::{command, lock, Root};

/// A dotted quad `ipaddress.IPv4Address` accepts (and prints back unchanged); None for anything it
/// would word an error for.
fn ipv4(text: &str) -> Option<String> {
    let octets: Vec<&str> = text.split('.').collect();
    let valid = octets.len() == 4
        && octets.iter().all(|o| {
            !o.is_empty()
                && o.len() <= 3
                && o.chars().all(|c| c.is_ascii_digit())
                && (o.len() == 1 || !o.starts_with('0'))
                && o.parse::<u16>().is_ok_and(|v| v <= 255)
        });
    valid.then(|| text.to_owned())
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
                        if *stopped.lock().expect("lease") {
                            return;
                        }
                        if device.write(&payload).ok() != Some(payload.len()) {
                            return;
                        }
                        let guard = stopped.lock().expect("lease");
                        let (guard, _) = wake
                            .wait_timeout_while(guard, Duration::from_secs(1), |s| !*s)
                            .expect("lease");
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
        *stopped.lock().expect("lease") = true;
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
    // ipaddress words its own AddressValueError: Python does those (nothing is changed before).
    let peer = ipv4(peer).ok_or(Failure::Fallback)?;
    let _lease = Lease::take(root, "NetworkTransfer")?;
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
    Reply::sorted(&result, u8::from(!ok))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peers_parse_as_ipaddress_does() {
        assert_eq!(ipv4("192.168.1.10").as_deref(), Some("192.168.1.10"));
        assert_eq!(ipv4("0.0.0.0").as_deref(), Some("0.0.0.0"));
        for bad in [
            "192.168.1",
            "1.2.3.256",
            "01.2.3.4",
            "1.2.3.4 ",
            "a.b.c.d",
            "1..2.3",
        ] {
            assert_eq!(ipv4(bad), None, "{bad}");
        }
    }
}
