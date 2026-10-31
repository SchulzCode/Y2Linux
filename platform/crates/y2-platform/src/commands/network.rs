//! `wifi-event` and `dhcp-event`: one DHCP lease lifecycle above wpa_supplicant
//! (`wifi-event` from wpa-event, `dhcp-event` from udhcpc's script with its environment).
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::At;
mod ipv4;

use crate::reply::{
    atomic_json, lock, private_directory, unlink_missing_ok, write_text, Failure, Outcome, Reply,
};
use ipv4::{dotted, interface, ipv4, network};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{env, fs, os::unix::fs::PermissionsExt, path::Path};
use y2p_core::lenient::{self, self as record};
use y2p_core::{
    command,
    json::{Object, ObjectExt},
    power_text::python_split,
    py, Root,
};

/// The runtime record `/run/y2/<name>.json`: `value`'s fields in their order, then `schema`, `boot_id`
/// and `monotonic_s` (written sorted; the returned object keeps the order for stdout).
pub(crate) fn publish<T: Serialize + ?Sized>(
    root: &Root,
    name: &str,
    value: &T,
) -> Result<Object, Failure> {
    let Value::Object(mut value) = serde_json::to_value(value).map_err(Failure::value)? else {
        return Err(Failure::value("record_not_an_object"));
    };
    private_directory(&root.path("/run/y2"))?;
    value
        .set("schema", 1)
        .set("boot_id", json!(root.boot_id()))
        .set("monotonic_s", root.clock().monotonic());
    atomic_json(&root.path(&format!("/run/y2/{name}.json")), &value, false)?;
    Ok(value)
}

/// The Wi-Fi rfkill entries, as `/sys/class/rfkill/rfkillN`, in name order.
fn wlan_rfkill(root: &Root) -> Vec<String> {
    let mut entries: Vec<String> = fs::read_dir(root.path("/sys/class/rfkill"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with("rfkill"))
        .map(|name| format!("/sys/class/rfkill/{name}"))
        .collect();
    entries.sort();
    entries.retain(|base| root.read(&format!("{base}/type")).as_deref() == Some("wlan"));
    entries
}

/// The `soft` switch of Wi-Fi's (first) rfkill entry.
pub(crate) fn wlan_rfkill_soft(root: &Root) -> Option<std::path::PathBuf> {
    let base = wlan_rfkill(root).into_iter().next()?;
    Some(root.path(&format!("{base}/soft")))
}

/// True while Wi-Fi is soft-blocked: sleep or the owner turned it off.
pub(crate) fn wlan_switched_off(root: &Root) -> bool {
    wlan_rfkill(root)
        .iter()
        .any(|base| root.read(&format!("{base}/soft")).as_deref() == Some("1"))
}

/// `/etc/resolv.conf` is a production-owned symlink into /run; only Wi-Fi supplies DNS in this
/// product. Never import search strings from DHCP.
fn resolv(root: &Root, servers: &[String]) -> Result<(), Failure> {
    let path = root.path("/run/y2/resolv.conf");
    let id = py::uuid4().at(Path::new("/dev/urandom"))?.replace('-', "");
    let temporary = path.with_file_name(format!(".resolv.conf.{id}"));
    let text: String = servers
        .iter()
        .map(|s| format!("nameserver {s}\n"))
        .collect::<String>()
        + "options timeout:2 attempts:1\n";
    write_text(&temporary, &text)?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o644)).at(&temporary)?;
    fs::rename(&temporary, &path).at(&temporary)
}

const FLUSH: [&[&str]; 2] = [
    &[
        "ip", "-4", "address", "flush", "dev", "wlan0", "scope", "global",
    ],
    &["ip", "-4", "route", "flush", "dev", "wlan0"],
];

/// `/run/y2/wifi-events.json`: the association events of this boot (`wifi-event`), with the real
/// link losses counted apart from sleep's radio-off. Each field reads leniently (another shape
/// reads as none).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct WifiEvents {
    #[serde(deserialize_with = "lenient::or_none")]
    pub link_counts_since_s: Option<f64>,
    #[serde(deserialize_with = "lenient::or_default")]
    pub real_disconnects: u64,
    #[serde(deserialize_with = "lenient::or_default")]
    pub reconnects: u64,
    #[serde(deserialize_with = "lenient::or_default")]
    pub radio_off_disconnects: u64,
    #[serde(deserialize_with = "lenient::or_default")]
    pub connected: u64,
    #[serde(deserialize_with = "lenient::or_default")]
    pub disconnected: u64,
    #[serde(deserialize_with = "lenient::or_default")]
    pub service_restart: u64,
    #[serde(deserialize_with = "lenient::or_none")]
    pub association_active: Option<bool>,
    #[serde(deserialize_with = "lenient::text")]
    pub last_event: Option<String>,
    /// The stamp `publish` writes.
    #[serde(
        deserialize_with = "lenient::text",
        skip_serializing_if = "Option::is_none"
    )]
    pub boot_id: Option<String>,
    #[serde(
        deserialize_with = "lenient::or_none",
        skip_serializing_if = "Option::is_none"
    )]
    pub monotonic_s: Option<f64>,
    #[serde(
        deserialize_with = "lenient::or_none",
        skip_serializing_if = "Option::is_none"
    )]
    pub schema: Option<i64>,
}

impl WifiEvents {
    /// The record (its defaults when missing or no object).
    pub fn read(root: &Root) -> WifiEvents {
        root.read("/run/y2/wifi-events.json")
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }
}

/// `/run/y2/dhcp.json` (also `wifi-event`'s and `dhcp-event`'s stdout) before `publish` adds its
/// identity: the lease state of one association epoch, in this key order.
#[derive(Serialize)]
struct Dhcp<'a> {
    epoch: &'a str,
    state: &'static str,
    error: Option<&'static str>,
    address: Option<String>,
    #[serde(flatten)]
    lease: Option<Lease>,
}

impl<'a> Dhcp<'a> {
    fn new(epoch: &'a str, state: &'static str) -> Self {
        Self {
            epoch,
            state,
            error: None,
            address: None,
            lease: None,
        }
    }

    fn failed(epoch: &'a str, error: &'static str) -> Self {
        Self {
            error: Some(error),
            ..Self::new(epoch, "Failed")
        }
    }
}

/// What an Authenticated lease adds: its DNS servers and what is configured on wlan0 (the prefix
/// length of the address and the routes as destination and gateway), which a renew compares.
#[derive(Serialize)]
struct Lease {
    dns_servers: Vec<String>,
    /// udhcpc's `lease` as it gave it.
    lease_seconds: Option<String>,
    prefix_length: u32,
    routes: Vec<(String, String)>,
}

/// The published lease as `dhcp-event` reads it back.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Published {
    #[serde(deserialize_with = "record::text")]
    state: Option<String>,
    #[serde(deserialize_with = "record::text")]
    epoch: Option<String>,
    #[serde(deserialize_with = "record::text")]
    boot_id: Option<String>,
    #[serde(deserialize_with = "record::text")]
    address: Option<String>,
    #[serde(deserialize_with = "record::texts")]
    dns_servers: Option<Vec<String>>,
    #[serde(deserialize_with = "record::count")]
    prefix_length: Option<u64>,
    #[serde(deserialize_with = "record::pairs")]
    routes: Option<Vec<(String, String)>>,
}

impl Published {
    /// Whether this is the Authenticated lease of this boot and epoch with the same address, prefix,
    /// routes and DNS servers: what wlan0 and resolv.conf already carry.
    fn same(&self, root: &Root, epoch: &str, address: &str, lease: &Lease) -> bool {
        self.state.as_deref() == Some("Authenticated")
            && self.epoch.as_deref() == Some(epoch)
            && self.boot_id == root.boot_id()
            && self.address.as_deref() == Some(address)
            && self.prefix_length == Some(u64::from(lease.prefix_length))
            && self.routes.as_ref() == Some(&lease.routes)
            && self.dns_servers.as_ref() == Some(&lease.dns_servers)
    }
}

/// `y2-platform wifi-event CONNECTED|DISCONNECTED|SERVICE_RESTART`.
pub fn wifi_event(root: &Root, event: &str) -> Outcome {
    let runtime = root.path("/run/y2");
    private_directory(&runtime)?;
    let _lock = lock(&runtime.join("dhcp.lock"))?;
    let epoch = py::uuid4().at(Path::new("/dev/urandom"))?;
    private_directory(&runtime)?;
    write_text(&runtime.join("wifi-epoch"), &format!("{epoch}\n"))?;
    unlink_missing_ok(&runtime.join("dns.json"))?;
    let mut record = WifiEvents::read(root);
    if record.boot_id != root.boot_id() {
        record = WifiEvents::default();
    }
    let active = record
        .association_active
        .unwrap_or(record.last_event.as_deref() == Some("CONNECTED"));
    // Action calls include supervisory cleanup as well as native link events. Keep their historical
    // raw counts, but count a real loss only once after a known connection.
    if record.link_counts_since_s.is_none() {
        record.link_counts_since_s = Some(root.clock().monotonic());
    }
    if event == "DISCONNECTED" && active {
        // Sleep switches the radio off: that is not a lost link (Beta 1 counted every sleep as a real
        // disconnect).
        if wlan_switched_off(root) {
            record.radio_off_disconnects += 1;
        } else {
            record.real_disconnects += 1;
        }
    }
    if event == "CONNECTED" && !active && record.connected > 0 {
        record.reconnects += 1;
    }
    record.association_active = Some(event == "CONNECTED");
    match event {
        "CONNECTED" => record.connected += 1,
        "DISCONNECTED" => record.disconnected += 1,
        _ => record.service_restart += 1,
    }
    record.last_event = Some(event.to_owned());
    publish(root, "wifi-events", &record)?;
    // Leases never survive a new association/service epoch. USB's fixed address and routes are outside
    // this interface-scoped lifecycle.
    for argv in FLUSH {
        command::run(root, argv, command::TIMEOUT);
    }
    resolv(root, &[])?;
    let state = if event == "CONNECTED" {
        "AcquiringIP"
    } else {
        "Unavailable"
    };
    Reply::unsorted(&publish(root, "dhcp", &Dhcp::new(&epoch, state))?, 0)
}

/// Whether udhcpc's environment repeats the Authenticated lease published for this boot and epoch:
/// the same address and DNS servers.
fn renews(
    root: &Root,
    previous: &Published,
    epoch: &str,
    var: &dyn Fn(&str) -> Option<String>,
) -> bool {
    let address = var("ip").and_then(|ip| ipv4(&ip).ok()).map(dotted);
    let servers: Option<Vec<String>> = python_split(&var("dns").unwrap_or_default())
        .iter()
        .take(3)
        .map(|s| ipv4(s).ok().map(dotted))
        .collect();
    previous.state.as_deref() == Some("Authenticated")
        && previous.epoch.as_deref() == Some(epoch)
        && previous.boot_id == root.boot_id()
        && address.is_some()
        && previous.address == address
        && servers.is_some()
        && previous.dns_servers == servers
}

/// Up to three addresses of a space-separated udhcpc variable, each parsed.
fn addresses(var: &dyn Fn(&str) -> Option<String>, name: &str) -> Result<Vec<String>, Failure> {
    python_split(&var(name).unwrap_or_default())
        .iter()
        .take(3)
        .map(|s| ipv4(s).map(dotted))
        .collect()
}

/// The routes of a lease (destination, gateway): RFC3442 classless routes supersede the router
/// option. The list is bounded and holds parsed addresses only, never shell text.
fn routes(var: &dyn Fn(&str) -> Option<String>) -> Result<Vec<(String, String)>, Failure> {
    let gateways = addresses(var, "router")?;
    let classless = var("staticroutes").unwrap_or_default();
    let classless = python_split(&classless);
    if classless.is_empty() {
        return Ok(gateways
            .into_iter()
            .take(1)
            .map(|gateway| ("default".to_owned(), gateway))
            .collect());
    }
    if classless.len() > 32 || classless.len() % 2 == 1 {
        return Err(Failure::value("invalid_classless_routes"));
    }
    classless
        .chunks(2)
        .map(|pair| Ok((network(pair[0])?, dotted(ipv4(pair[1])?))))
        .collect()
}

/// The `ip` commands that put a lease on wlan0: whatever the interface had goes first.
fn configuration(interface: &str, routes: &[(String, String)]) -> Vec<Vec<String>> {
    let words = |argv: &[&str]| argv.iter().map(|w| w.to_string()).collect::<Vec<_>>();
    let mut commands: Vec<Vec<String>> = FLUSH.iter().map(|argv| words(argv)).collect();
    commands.push(words(&["ip", "link", "set", "dev", "wlan0", "up"]));
    commands.push(words(&[
        "ip", "-4", "address", "add", interface, "dev", "wlan0",
    ]));
    for (destination, gateway) in routes {
        let mut argv = words(&["ip", "-4", "route", "replace", destination]);
        if gateway != "0.0.0.0" {
            argv.extend(words(&["via", gateway]));
        }
        argv.extend(words(&["dev", "wlan0", "metric", "100"]));
        commands.push(argv);
    }
    commands
}

/// `y2-platform dhcp-event deconfig|leasefail|nak|bound|renew` with udhcpc's environment.
pub fn dhcp_event(root: &Root, event: &str) -> Outcome {
    // udhcpc's environment; a value that is not UTF-8 reads with surrogate escapes (y2p_core::os_text),
    // which the address errors then show.
    let var = |name: &str| env::var_os(name).map(|v| y2p_core::os_text::fsdecode_os(&v));
    if var("interface").as_deref() != Some("wlan0") {
        return Err(Failure::value("dhcp_interface_not_owned"));
    }
    let runtime = root.path("/run/y2");
    private_directory(&runtime)?;
    let _lock = lock(&runtime.join("dhcp.lock"))?;
    let epoch = root.read("/run/y2/wifi-epoch").filter(|e| !e.is_empty());
    let Some(epoch) = epoch.filter(|e| var("Y2_DHCP_GENERATION").as_ref() == Some(e)) else {
        return Err(Failure::value("stale_dhcp_epoch"));
    };
    // A renew of the same lease keeps its DNS result: deleting it showed dns_unavailable until the
    // next probe, up to 300 s later. The reader still checks epoch, address and freshness.
    let previous: Published = record::read(root.json_object("/run/y2/dhcp.json"));
    if !(event == "renew" && renews(root, &previous, &epoch, &var)) {
        unlink_missing_ok(&runtime.join("dns.json"))?;
    }
    let state = match event {
        "deconfig" | "nak" | "leasefail" => {
            for argv in FLUSH {
                command::run(root, argv, command::TIMEOUT);
            }
            resolv(root, &[])?;
            match event {
                "deconfig" => Dhcp::new(&epoch, "AcquiringIP"),
                "leasefail" => Dhcp::failed(&epoch, "dhcp_timeout"),
                _ => Dhcp::failed(&epoch, "dhcp_nak"),
            }
        }
        "bound" | "renew" => {
            let address = ipv4(&var("ip").unwrap_or_default())?;
            let length = interface(&format!(
                "{}/{}",
                dotted(address),
                var("subnet").unwrap_or_default()
            ))?;
            if address == [0, 0, 0, 0] || (224..=239).contains(&address[0]) || address[0] == 127 {
                return Err(Failure::value("invalid_dhcp_address"));
            }
            let lease = Lease {
                dns_servers: addresses(&var, "dns")?,
                lease_seconds: var("lease"),
                prefix_length: length,
                routes: routes(&var)?,
            };
            let address = dotted(address);
            // Owner decision (2026-10-29): udhcpc renews at half the lease time; a renew of the
            // lease wlan0 already carries changes nothing (flushing and adding it again cut the
            // link for a moment each time). Anything else is configured from scratch.
            if !(event == "renew" && previous.same(root, &epoch, &address, &lease)) {
                let interface = format!("{address}/{length}");
                for argv in configuration(&interface, &lease.routes) {
                    let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
                    if !command::run(root, &argv, command::TIMEOUT).ok {
                        publish(
                            root,
                            "dhcp",
                            &Dhcp::failed(&epoch, "lease_configuration_failed"),
                        )?;
                        return Err(Failure::value("lease_configuration_failed"));
                    }
                }
                resolv(root, &lease.dns_servers)?;
            }
            Dhcp {
                epoch: &epoch,
                state: "Authenticated",
                error: lease.dns_servers.is_empty().then_some("dns_unavailable"),
                address: Some(address),
                lease: Some(lease),
            }
        }
        _ => return Err(Failure::value("unknown_dhcp_event")),
    };
    Reply::unsorted(&publish(root, "dhcp", &state)?, 0)
}

#[cfg(test)]
#[path = "network_tests.rs"]
mod network_tests;
