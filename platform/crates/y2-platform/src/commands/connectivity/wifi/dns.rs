//! The DNS probe of a usable lease and its schedule.
// SPDX-License-Identifier: GPL-2.0-only
use super::super::observe_wifi::wifi;
use super::wpa::digits;
use crate::commands::network::publish;
use crate::reply::Failure;
use serde::Serialize;
use serde_json::Value;
use std::{ffi::CString, sync::mpsc, time::Duration};
use y2p_core::{
    command,
    json::{Object, ObjectExt},
    py::{self, truthy},
    Root,
};

/// How long a DNS probe waits for its answer.
const LOOKUP_S: f64 = 3.0;

/// The libc resolver in process on a bounded worker (plan §5a), answered as a command is: ok with
/// the number of addresses, `command_failed` on a resolver error, `command_timeout` after
/// `LOOKUP_S` (the lookup is left to end on its own: resolv.conf bounds it to 2 s per server).
fn lookup(hostname: &str) -> command::Outcome {
    let host = hostname.to_owned();
    let (sender, receiver) = mpsc::channel();
    let started = std::thread::Builder::new()
        .name("dns-probe".into())
        .spawn(move || {
            let _ = sender.send(getaddrinfo(&host));
        });
    let failed = |reason: &str| command::Outcome {
        ok: false,
        reason: Some(reason.into()),
        output: None,
    };
    if started.is_err() {
        return failed("command_unavailable");
    }
    match receiver.recv_timeout(Duration::from_secs_f64(LOOKUP_S)) {
        Ok(Some(count)) => command::Outcome {
            ok: true,
            reason: None,
            output: Some(count.to_string()),
        },
        Ok(None) => failed("command_failed"),
        Err(_) => failed("command_timeout"),
    }
}

/// `len(socket.getaddrinfo(host, None, socket.AF_INET))`.
fn getaddrinfo(host: &str) -> Option<usize> {
    let name = CString::new(host).ok()?;
    // SAFETY: zeroed hints are valid (all fields 0/null); the result list is freed below.
    unsafe {
        let mut hints: libc::addrinfo = std::mem::zeroed();
        hints.ai_family = libc::AF_INET;
        let mut list: *mut libc::addrinfo = std::ptr::null_mut();
        if libc::getaddrinfo(name.as_ptr(), std::ptr::null(), &hints, &mut list) != 0 {
            return None;
        }
        let mut count = 0;
        let mut next = list;
        while !next.is_null() {
            count += 1;
            next = (*next).ai_next;
        }
        libc::freeaddrinfo(list);
        Some(count)
    }
}

/// `probe_dns(ctx, observed)`: the published `dns.json`, or None when the lease changed meanwhile.
pub fn probe_dns(root: &Root, observed: &Object) -> Result<Option<Object>, Failure> {
    let empty = |key: &str| !observed.get(key).is_some_and(truthy);
    if empty("ip_addresses") || empty("default_route") {
        return Ok(None);
    }
    let config = root.json_object("/etc/y2linux/network-policy.json");
    let host = |h: &str| {
        (1..=253).contains(&h.len())
            && h.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    };
    let hostname = match config.get("dns_probe_host") {
        None => "pool.ntp.org".to_owned(),
        Some(Value::String(h)) if host(h) => h.clone(),
        Some(_) => return Err(Failure::value("invalid_dns_probe_host")),
    };
    let epoch = root.read("/run/y2/wifi-epoch");
    // Standard libc resolver, bounded; no custom DNS protocol. Below a fixture root the lookup is a
    // request the test answers (`getaddrinfo <host>`: the number of addresses).
    let answer = if root.is_real() {
        lookup(&hostname)
    } else {
        command::run(root, &["getaddrinfo", &hostname], LOOKUP_S)
    };
    let after = wifi(root, Some(observed));
    if root.read("/run/y2/wifi-epoch") != epoch
        || !py::eq(after.get("association_id"), observed.get("association_id"))
        || !py::eq(after.get("ip_addresses"), observed.get("ip_addresses"))
        || !py::eq(after.get("default_route"), observed.get("default_route"))
    {
        return Ok(None);
    }
    let output = answer.output.unwrap_or_default();
    let ok = answer.ok && digits(&output) && !output.trim_start_matches('0').is_empty();
    let value = Dns {
        epoch,
        ok,
        address: observed
            .get("ip_addresses")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .cloned()
            .unwrap_or(Value::Null),
        bssid: observed.value("association_id"),
        probe_host: hostname,
        reason: (!ok).then_some("dns_unavailable"),
    };
    publish(root, "dns", &value).map(Some)
}

/// `/run/y2/dns.json` before `publish` adds its identity: whether this lease's resolver answered.
#[derive(Serialize)]
struct Dns {
    epoch: Option<String>,
    ok: bool,
    /// The lease's first address and the access point it was probed through.
    address: Value,
    bssid: Value,
    probe_host: String,
    reason: Option<&'static str>,
}

/// Probe newly usable leases immediately; bound retries on the same lease.
///
/// A confirmed lease is re-checked every five minutes, not every 30 s: each probe was a Python resolver
/// (≈260 ms CPU on the Y2) and is still a radio exchange. observe.DNS_FRESH_S keeps Online across one
/// late re-check.
#[derive(Default)]
pub struct DnsProbeSchedule {
    key: Option<(String, Value, Value)>,
    deadline: f64,
}

impl DnsProbeSchedule {
    pub const RETRY_S: f64 = 30.0;
    pub const CONFIRMED_S: f64 = 300.0;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn confirmed(&mut self, now: f64) {
        self.deadline = now + Self::CONFIRMED_S;
    }

    pub fn due(&mut self, observed: &Object, epoch: Option<&str>, now: f64) -> bool {
        let epoch = epoch.unwrap_or_default();
        if observed.get("association").and_then(Value::as_str) != Some("COMPLETED")
            || epoch.is_empty()
            || !observed.get("ip_addresses").is_some_and(truthy)
            || !observed.get("default_route").is_some_and(truthy)
        {
            return false;
        }
        let key = (
            epoch.to_owned(),
            observed
                .get("association_id")
                .cloned()
                .unwrap_or(Value::Null),
            observed.get("ip_addresses").cloned().unwrap_or(Value::Null),
        );
        let same = self.key.as_ref().is_some_and(|old| {
            old.0 == key.0
                && py::eq(Some(&old.1), Some(&key.1))
                && py::eq(Some(&old.2), Some(&key.2))
        });
        if !same {
            self.key = Some(key);
            self.deadline = 0.0;
        }
        if now < self.deadline {
            return false;
        }
        self.deadline = now + Self::RETRY_S;
        true
    }
}
