//! The rest of `network.py` (lease_event/dhcp_event/publish/resolv are `commands/network.rs`): the
//! durable Wi-Fi journal, the DNS probe and its schedule, the supplicant event monitor with the F52
//! password latch Reborn reads, the scan evidence of F13 and the F52 self-heal. The service daemon runs
//! these in process; the connectivity daemon writes the journal (F53).
// SPDX-License-Identifier: GPL-2.0-only
use super::observe_wifi::{clock_value, eq_text, wifi};
use crate::commands::network::{publish, wlan_switched_off};
use crate::reply::Failure;
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    ffi::CString,
    fs, io,
    os::{
        fd::{AsRawFd, RawFd},
        unix::{fs::MetadataExt, net::UnixDatagram},
    },
    path::PathBuf,
    sync::mpsc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use y2p_core::py;
use y2p_core::{
    command,
    connectivity_journal::append_bounded,
    json::{dumps, Object, ObjectExt, Separators},
    mounts::data_ready,
    py::truthy,
    Root,
};

/// Only a password verdict is kept until a connect. A rejected or failed association (CONN_FAILED,
/// AUTH-REJECT) is no password on WPA2-PSK.
pub const CREDENTIAL_ERRORS: [&str; 1] = ["wrong_credentials"];
const LAST_FAILURE_KEYS: [&str; 8] = [
    "error",
    "reason",
    "epoch",
    "failures",
    "scan",
    "scan_monotonic_s",
    "boot_id",
    "monotonic_s",
];
/// F52 (aa96e00a): wpa_supplicant says WRONG_KEY for any 4-way handshake that ends early (events.c
/// could_be_psk_mismatch), also one this player cut short (a Join, the sleep's radio off). A password
/// is wrong only when one network fails this often with no connect between; a single failure is
/// `handshake_failed`, one after our own disconnect is not counted.
pub const WRONG_KEY_REPEAT: i64 = 2;
/// Durable Wi-Fi self-heal rows (F52/F53); never a network name or address.
pub const JOURNAL: &str = "/data/system/platform/wifi-journal.jsonl";
pub const JOURNAL_LIMIT: u64 = 262_144;
/// Supplicant events counted for reports: what it did while it sat DISCONNECTED (F52). Kinds and times
/// only, never the event text.
const EVENT_KINDS: [&str; 11] = [
    "CONNECTED",
    "DISCONNECTED",
    "SSID-TEMP-DISABLED",
    "SSID-REENABLED",
    "ASSOC-REJECT",
    "AUTH-REJECT",
    "NETWORK-NOT-FOUND",
    "SCAN-STARTED",
    "SCAN-RESULTS",
    "SCAN-FAILED",
    "AUTH-TIMEOUT",
];

fn wall() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// Python's `round(x, 1)`: the decimal rounding of the exact binary value, read back as a float.
pub fn round1(x: f64) -> f64 {
    format!("{x:.1}").parse().unwrap_or(x)
}

/// `journal(ctx, event, **fields)`: append one durable row (bounded, rotated once); false when /data
/// is not ours or the row could not be written.
pub fn journal<T: Serialize>(root: &Root, event: &str, fields: &T) -> bool {
    if !data_ready(root) {
        return false;
    }
    let row = Row {
        schema: 1,
        event,
        wall_time: wall(),
        monotonic_s: root.clock().monotonic(),
        boot_id: root.boot_id(),
        fields,
    };
    let Ok(text) = dumps(&row, Separators::Compact) else {
        return false;
    };
    append_bounded(
        root,
        JOURNAL,
        format!("{text}\n").as_bytes(),
        JOURNAL_LIMIT,
        true,
    )
    .is_ok()
}

/// One Wi-Fi journal row: these fields first, then the event's own, in their order (the rows are
/// read as written, never sorted).
#[derive(Serialize)]
struct Row<'a, T> {
    schema: u8,
    event: &'a str,
    wall_time: f64,
    monotonic_s: f64,
    boot_id: Option<String>,
    #[serde(flatten)]
    fields: &'a T,
}

fn value_of(ordered: Object) -> Value {
    Value::Object(y2p_core::json::sorted_object(&ordered))
}

/// Python's `\w` (a Unicode letter, digit or `_`), for `\b`.
fn word(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric() || c == '_')
}

/// `re.search(r'\b(?:KEY|…)=(V{1,max})\b', text).group(1)`, for a value class `V` of word characters:
/// the first `KEY=` after a non-word character whose run of `V` is at most `max` long and is followed
/// by a non-word character.
fn search_value<'a>(
    text: &'a str,
    keys: &[&str],
    class: impl Fn(char) -> bool,
    max: usize,
) -> Option<&'a str> {
    let mut previous = None;
    for (index, c) in text.char_indices() {
        if !word(previous) {
            for key in keys {
                let Some(rest) = text[index..]
                    .strip_prefix(key)
                    .and_then(|r| r.strip_prefix('='))
                else {
                    continue;
                };
                let run = rest.find(|c: char| !class(c)).unwrap_or(rest.len());
                let length = rest[..run].chars().count();
                if (1..=max).contains(&length) && !word(rest[run..].chars().next()) {
                    return Some(&rest[..run]);
                }
            }
        }
        previous = Some(c);
    }
    None
}

/// `re.findall(r'\[[A-Z0-9+_-]{1,24}\]', text)`, joined.
fn flags(text: &str) -> String {
    let class = |c: char| c.is_ascii_uppercase() || c.is_ascii_digit() || "+_-".contains(c);
    let (mut found, mut rest) = (String::new(), text);
    while let Some(start) = rest.find('[') {
        let after = &rest[start + 1..];
        let run = after.find(|c: char| !class(c)).unwrap_or(after.len());
        if (1..=24).contains(&run) && after[run..].starts_with(']') {
            found.push_str(&rest[start..start + run + 2]);
            rest = &after[run + 1..];
        } else {
            rest = after;
        }
    }
    found
}

/// `str.isdigit()` for the ASCII text wpa_cli prints.
fn digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// `re.fullmatch(r'-?\d+', text)`.
fn signed(text: &str) -> bool {
    digits(text.strip_prefix('-').unwrap_or(text))
}

fn int(text: &str) -> i64 {
    text.parse().unwrap_or(i64::MAX)
}

fn rows(output: Option<&str>) -> Vec<Vec<String>> {
    output
        .unwrap_or_default()
        .lines()
        .skip(1)
        .map(|line| line.split('\t').map(str::to_owned).collect())
        .collect()
}

// ---- DNS ---------------------------------------------------------------------------------------

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

// ---- scan evidence (F13) -----------------------------------------------------------------------

fn channel(frequency: i64) -> Value {
    if (2412..=2472).contains(&frequency) {
        json!((frequency - 2407).div_euclid(5))
    } else if frequency == 2484 {
        json!(14)
    } else if (5000..=5900).contains(&frequency) {
        json!((frequency - 5000).div_euclid(5))
    } else {
        Value::Null
    }
}

fn wpa(root: &Root, argv: &[&str]) -> command::Outcome {
    let mut full = vec!["wpa_cli", "-i", "wlan0"];
    full.extend_from_slice(argv);
    command::run(root, &full, 2.0)
}

/// Why the saved network was "not found" (F13: Wi-Fi did not come back after a wake on one player,
/// cause unproven): how many networks the last scan saw, whether a saved one was among them, its best
/// signal and channel, and the regulatory country (country=00 scans channels 12/13 passively). Never a
/// network name or address.
pub fn scan_evidence(root: &Root) -> Object {
    let scan = wpa(root, &["scan_results"]);
    let saved = wpa(root, &["list_networks"]);
    if !scan.ok || !saved.ok {
        return Object::new().with("available", false);
    }
    let saved_rows = rows(saved.output.as_deref());
    let mut names: Vec<&str> = Vec::new();
    for row in &saved_rows {
        if row.len() >= 2 && !row[1].is_empty() && !names.contains(&row[1].as_str()) {
            names.push(&row[1]);
        }
    }
    let found: Vec<Vec<String>> = rows(scan.output.as_deref())
        .into_iter()
        .filter(|row| row.len() >= 3 && digits(&row[1]) && signed(&row[2]))
        .collect();
    let mut result = Object::new()
        .with("available", true)
        .with("networks", found.len())
        .with("saved_networks", names.len())
        .with(
            "band_2ghz",
            found.iter().filter(|r| int(&r[1]) < 3000).count(),
        )
        .with(
            "band_5ghz",
            found.iter().filter(|r| int(&r[1]) >= 5000).count(),
        );
    let hits: Vec<&Vec<String>> = found
        .iter()
        .filter(|row| row.len() >= 5 && names.contains(&row[4].as_str()))
        .collect();
    result.set("saved_seen", !hits.is_empty());
    if !hits.is_empty() {
        // max() keeps the first of equal signals.
        let mut best = hits[0];
        for row in &hits[1..] {
            if int(&row[2]) > int(&best[2]) {
                best = row;
            }
        }
        let frequency = int(&best[1]);
        result
            .set("saved_signal_dbm", int(&best[2]))
            .set("saved_frequency_mhz", frequency)
            .set("saved_channel", channel(frequency))
            .set("saved_access_points", hits.len());
        // F13 (cb904ffb): "not found" with the saved network listed at -66 dBm. scan_results is the
        // BSS table (entries up to 180 s old): how old the entry is, and whether its security matches
        // the saved network's.
        result.set("saved_flags", flags(&best[3]));
        let bss = wpa(root, &["bss", &best[0]]);
        if bss.ok {
            // re.search(r'(?m)^age=(\d+)$', output)
            if let Some(age) = bss
                .output
                .as_deref()
                .unwrap_or_default()
                .split('\n')
                .find_map(|line| line.strip_prefix("age=").filter(|d| digits(d)))
                .and_then(|d| d.parse::<u64>().ok())
            {
                result.set("saved_age_s", age);
            }
        }
        let ids: Vec<&String> = saved_rows
            .iter()
            .filter(|row| row.len() >= 2 && row[1] == best[4] && digits(&row[0]))
            .map(|row| &row[0])
            .collect();
        let mut managements: Vec<String> = Vec::new();
        for network in ids.iter().take(4) {
            let answer = wpa(root, &["get_network", network, "key_mgmt"]);
            let value = y2p_core::root::python_strip(answer.output.as_deref().unwrap_or_default());
            if answer.ok
                && (1..=64).contains(&value.len())
                && value
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b" _-".contains(&b))
                && !managements.iter().any(|m| m == value)
            {
                managements.push(value.to_owned());
            }
        }
        managements.sort();
        result.set("saved_key_mgmt", json!(managements));
    }
    let country = wpa(root, &["get", "country"]);
    let value = y2p_core::root::python_strip(country.output.as_deref().unwrap_or_default());
    if country.ok
        && value.len() == 2
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_uppercase())
    {
        result.set("country", value);
    }
    result
}

// ---- supplicant events --------------------------------------------------------------------------

/// Passive supplicant events. Never issues CONNECT or owns reconnect.
pub struct EventMonitor {
    root: Root,
    pub(crate) sock: Option<UnixDatagram>,
    path: Option<PathBuf>,
    generation: Option<(u64, i128)>,
    started: f64,
    counts: Vec<(String, u64)>,
    last: Vec<(String, f64)>,
    last_disconnect: Value,
    pub(crate) interrupted_handshakes: u64,
    /// The network of the last connect: the one a self-heal selects.
    pub connected_network: Option<i64>,
    /// wpa_supplicant's handshake timeout (≈10 s; how a wrong password ends on the Y2) also
    /// disconnects locally; any other local disconnect cut a handshake short and proves nothing
    /// about the password.
    timed_out: bool,
    interrupted: bool,
    /// A local disconnect with the radio on and no try since: the owner's Disconnect, which a
    /// self-heal must not undo.
    pub held: bool,
}

impl EventMonitor {
    pub const SCAN_EVIDENCE_S: f64 = 30.0;

    pub fn new(root: &Root) -> Self {
        Self {
            root: root.clone(),
            sock: None,
            path: None,
            generation: None,
            started: root.clock().monotonic(),
            counts: Vec::new(),
            last: Vec::new(),
            last_disconnect: Value::Null,
            interrupted_handshakes: 0,
            connected_network: None,
            timed_out: false,
            interrupted: false,
            held: false,
        }
    }

    /// Counts and ages of the supplicant's events for network.json and the journal.
    pub fn summary(&self, now: Option<f64>) -> Object {
        let now = now.unwrap_or_else(|| self.root.clock().monotonic());
        let mut counts = Object::new();
        for (kind, n) in &self.counts {
            counts = counts.with(kind, *n);
        }
        let mut ago = Object::new();
        for (kind, at) in &self.last {
            ago = ago.with(kind, round1(now - at));
        }
        Object::new()
            .with("since_s", round1(now - self.started))
            .with("counts", counts)
            .with("ago_s", ago)
            .with("last_disconnect", self.last_disconnect.clone())
            .with("interrupted_handshakes", self.interrupted_handshakes)
    }

    /// The socket a caller's select waits on, if attached.
    pub fn fd(&self) -> Option<RawFd> {
        self.sock.as_ref().map(AsRawFd::as_raw_fd)
    }

    pub fn close(&mut self) {
        self.sock = None;
        if let Some(path) = self.path.take() {
            let _ = fs::remove_file(path);
        }
        self.generation = None;
    }

    pub fn poll(&mut self) {
        let remote = self.root.path("/run/wpa_supplicant/wlan0");
        if !remote.exists() {
            self.close();
            return;
        }
        if self.attach_and_read(&remote).is_err() {
            self.close();
        }
    }

    fn attach_and_read(&mut self, remote: &std::path::Path) -> Result<(), ()> {
        let meta = fs::metadata(remote).map_err(drop)?;
        let generation = (
            meta.ino(),
            i128::from(meta.ctime()) * 1_000_000_000 + i128::from(meta.ctime_nsec()),
        );
        if self.generation.is_some_and(|g| g != generation) {
            self.close();
            self.held = false;
        }
        if self.sock.is_none() {
            let path = self.root.path("/run/y2/wpa-events.sock");
            match fs::remove_file(&path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(()),
                _ => {}
            }
            self.path = Some(path.clone());
            let sock = UnixDatagram::bind(&path).map_err(drop)?;
            let timeout = Some(Duration::from_millis(200));
            sock.set_read_timeout(timeout).map_err(drop)?;
            sock.set_write_timeout(timeout).map_err(drop)?;
            self.sock = Some(sock);
            let sock = self.sock.as_ref().expect("set above");
            sock.connect(remote).map_err(drop)?;
            sock.send(b"ATTACH").map_err(drop)?;
            let mut answer = [0u8; 1024];
            let n = sock.recv(&mut answer).map_err(drop)?;
            if answer[..n].trim_ascii() != b"OK" {
                return Err(()); // monitor_attach_failed
            }
            sock.set_nonblocking(true).map_err(drop)?;
            self.generation = Some(generation);
        }
        for _ in 0..16 {
            let mut buffer = [0u8; 4096];
            let received = self.sock.as_ref().expect("attached").recv(&mut buffer);
            match received {
                Ok(n) => {
                    let text = String::from_utf8_lossy(&buffer[..n]).into_owned();
                    self.event(&text).map_err(drop)?;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => return Err(()),
            }
        }
        Ok(())
    }

    /// Events wake readiness immediately. DHCP remains owned by wpa-event; the bounded acquisition
    /// cadence also observes its lease publication.
    pub fn wait(&mut self, timeout: f64, wake_fd: Option<RawFd>) {
        let readers: Vec<RawFd> = self.fd().into_iter().chain(wake_fd).collect();
        if readers.is_empty() {
            std::thread::sleep(Duration::from_secs_f64(timeout.max(0.0)));
            return;
        }
        if super::daemon::select(&readers, timeout).is_err() {
            self.close();
        }
    }

    fn count(&mut self, kind: &str) {
        let now = self.root.clock().monotonic();
        match self.counts.iter_mut().find(|(k, _)| k == kind) {
            Some(entry) => entry.1 += 1,
            None => self.counts.push((kind.to_owned(), 1)),
        }
        match self.last.iter_mut().find(|(k, _)| k == kind) {
            Some(entry) => entry.1 = now,
            None => self.last.push((kind.to_owned(), now)),
        }
    }

    /// One supplicant event line (`<3>CTRL-EVENT-… …`).
    pub fn event(&mut self, text: &str) -> Result<(), Failure> {
        let body = match text.strip_prefix('<') {
            Some(rest) if rest.contains('>') => rest.split_once('>').expect("checked").1,
            _ => text,
        };
        let body = y2p_core::root::python_strip(body);
        if body.starts_with("Trying to associate with ") {
            self.held = false;
            return Ok(());
        }
        let kind = if body.starts_with("Authentication with ") && body.ends_with("timed out.") {
            "AUTH-TIMEOUT".to_owned()
        } else if let Some(rest) = body.strip_prefix("CTRL-EVENT-") {
            rest.split(' ').next().unwrap_or_default().to_owned()
        } else {
            return Ok(());
        };
        if EVENT_KINDS.contains(&kind.as_str()) {
            self.count(&kind);
        }
        if kind == "AUTH-TIMEOUT" {
            self.timed_out = true;
            return Ok(());
        }
        if kind == "DISCONNECTED" {
            let code = search_value(body, &["reason"], |c| c.is_ascii_digit(), 5)
                .and_then(|c| c.parse::<i64>().ok());
            let local = body.contains("locally_generated=1");
            self.last_disconnect = Value::Object(
                Object::new()
                    .with("reason", json!(code))
                    .with("local", local)
                    .with("timeout", self.timed_out),
            );
            self.interrupted = local && !self.timed_out;
            self.held = self.interrupted && !wlan_switched_off(&self.root);
            self.timed_out = false;
            return Ok(());
        }
        if [
            "CONNECTED",
            "SSID-TEMP-DISABLED",
            "ASSOC-REJECT",
            "AUTH-REJECT",
        ]
        .contains(&kind.as_str())
        {
            self.held = false;
        }
        let network = search_value(text, &["id"], |c| c.is_ascii_digit(), usize::MAX)
            .and_then(|c| c.parse::<i64>().ok());
        let mut error = match kind.as_str() {
            "SSID-TEMP-DISABLED" => {
                // A sleep blocks the radio mid-handshake: cfg80211 disconnects locally (950a93ac
                // 246 s); the rfkill read covers a missed event.
                let interrupted = self.interrupted || wlan_switched_off(&self.root);
                self.interrupted = false;
                if !text.contains("reason=WRONG_KEY") {
                    Some("authentication_failed")
                } else if interrupted {
                    self.interrupted_handshakes += 1;
                    return Ok(());
                } else {
                    Some("handshake_failed")
                }
            }
            "AUTH-REJECT" => Some("authentication_rejected"),
            "NETWORK-NOT-FOUND" => Some("ap_unavailable"),
            "CONNECTED" => {
                self.connected_network = network;
                None
            }
            _ => return Ok(()),
        };
        let root = &self.root;
        let old = root.json_object("/run/y2/wifi-failure.json");
        let boot = root.boot_id();
        // Failed handshakes per network since the last connect (kept here, so a service restart does
        // not forget the first one).
        let valid = match old.get("wrong_key") {
            Some(Value::Object(w)) => {
                eq_text(w.get("boot_id"), boot.as_deref())
                    && w.get("count")
                        .is_some_and(|c| c.is_boolean() || c.is_i64() || c.is_u64())
            }
            _ => false,
        };
        let mut wrong_key = (valid && kind != "CONNECTED").then(|| old["wrong_key"].clone());
        if error == Some("handshake_failed") {
            let count = match &wrong_key {
                Some(w) if py::eq(w.get("network_id"), Some(&json!(network))) => {
                    y2p_core::py::number(w.get("count")).unwrap_or(0.0) as i64 + 1
                }
                _ => 1,
            };
            wrong_key = Some(value_of(
                Object::new()
                    .with("network_id", json!(network))
                    .with("count", count)
                    .with("boot_id", json!(boot)),
            ));
            if count >= WRONG_KEY_REPEAT {
                error = Some("wrong_credentials");
            }
        }
        let failures = match old.get("failures") {
            None => 0,
            Some(Value::Bool(b)) => i64::from(*b),
            Some(v) => v.as_i64().unwrap_or(0),
        };
        let mut record = Object::new()
            .with("error", json!(error))
            .with("epoch", json!(root.read("/run/y2/wifi-epoch")))
            .with("failures", failures + i64::from(error.is_some()));
        if let Some(w) = wrong_key {
            record.set("wrong_key", w);
        }
        // wpa_supplicant's own code (reason=CONN_FAILED, status_code=15): it separates a password
        // from a timeout or SAE (F31). Never a name.
        if error.is_some() {
            let class = |c: char| c.is_ascii_alphanumeric() || c == '_';
            if let Some(detail) = search_value(text, &["reason", "status_code"], class, 32) {
                record.set("reason", detail);
            }
        }
        // A connect ends a failure run: keep what the last failure showed (F13's scan evidence was
        // gone by the time a report was saved).
        let last = if old.get("error").is_some_and(truthy) {
            Some(Value::Object(old.clone()))
        } else {
            old.get("last_failure").cloned()
        };
        if let Some(Value::Object(last)) = last {
            if last.get("error").is_some_and(truthy) {
                let mut kept = Object::new();
                for key in LAST_FAILURE_KEYS {
                    if let Some(value) = last.get(key) {
                        kept.set(key, value.clone());
                    }
                }
                record.set("last_failure", value_of(kept));
            }
        }
        // Y2Reborn #20: a wrong password stays reported until a connect. The "network not found"
        // that follows (the network is disabled for a while after each failure) never replaces it;
        // it names the network it was.
        if error.is_some_and(|e| CREDENTIAL_ERRORS.contains(&e)) {
            let reason = record.get("reason").cloned().unwrap_or(Value::Null);
            record.set(
                "credential",
                value_of(
                    Object::new()
                        .with("error", json!(error))
                        .with("reason", reason)
                        .with("boot_id", json!(boot))
                        .with("network_id", json!(network))
                        .with("monotonic_s", root.clock().monotonic()),
                ),
            );
        } else if error.is_some() {
            if let Some(Value::Object(credential)) = old.get("credential") {
                if eq_text(credential.get("boot_id"), boot.as_deref()) {
                    record.set("credential", Value::Object(credential.clone()));
                }
            }
        }
        if error == Some("ap_unavailable") {
            // wpa_supplicant reports this after every scan: look at most every 30 s.
            let last = clock_value(old.get("scan_monotonic_s"));
            let cached = eq_text(old.get("boot_id"), boot.as_deref())
                && last.is_some_and(|at| {
                    (0.0..Self::SCAN_EVIDENCE_S).contains(&(root.clock().monotonic() - at))
                })
                && matches!(old.get("scan"), Some(Value::Object(_)));
            if cached {
                record
                    .set("scan", old["scan"].clone())
                    .set("scan_monotonic_s", old["scan_monotonic_s"].clone());
            } else {
                record
                    .set("scan", value_of(scan_evidence(root)))
                    .set("scan_monotonic_s", root.clock().monotonic());
            }
        }
        publish(root, "wifi-failure", &record).map(drop)
    }
}

// ---- self-heal (F52) ----------------------------------------------------------------------------

/// F52: after a start or a wake the supplicant sat DISCONNECTED for up to 17 min with no failure
/// event while the saved network was in range; a Join (SELECT_NETWORK) connected in 3-4 s. Once that
/// lasts IDLE_S, do what the Join does: select the saved network in range and enable the others again
/// (Reborn's join ends so too). Not in range: one scan, so the next look sees fresh results. Backed
/// off per episode; every try is a journal row.
pub struct WifiRecovery {
    root: Root,
    since: Option<f64>,
    tries: u64,
    next_try: f64,
    first_try: Option<f64>,
}

impl WifiRecovery {
    pub const IDLE_S: f64 = 30.0;
    pub const BACKOFF_S: [f64; 4] = [30.0, 60.0, 120.0, 300.0];

    pub fn new(root: &Root) -> Self {
        Self {
            root: root.clone(),
            since: None,
            tries: 0,
            next_try: 0.0,
            first_try: None,
        }
    }

    pub fn observe(
        &mut self,
        value: &Object,
        now: f64,
        monitor: Option<&EventMonitor>,
    ) -> Option<&'static str> {
        let root = &self.root;
        let state = value.get("association").cloned().unwrap_or(Value::Null);
        let completed = state.as_str() == Some("COMPLETED");
        if completed || !value.get("radio_present").is_some_and(truthy) || wlan_switched_off(root) {
            if self.tries > 0 && completed {
                let seconds = round1(now - self.first_try.unwrap_or(now));
                journal(
                    root,
                    "wifi_recovered",
                    &Recovered {
                        tries: self.tries,
                        seconds,
                    },
                );
            }
            self.since = None;
            self.tries = 0;
            self.next_try = 0.0;
            self.first_try = None;
            return None;
        }
        // The supplicant is trying (scan, association), a password verdict stands or the owner
        // disconnected: never interfere.
        let credential = value
            .get("reason")
            .and_then(Value::as_str)
            .is_some_and(|r| CREDENTIAL_ERRORS.contains(&r));
        if !matches!(state.as_str(), Some("DISCONNECTED" | "INACTIVE"))
            || credential
            || monitor.is_some_and(|m| m.held)
        {
            self.since = None;
            return None;
        }
        let since = *self.since.get_or_insert(now);
        if now - since < Self::IDLE_S || now < self.next_try {
            return None;
        }
        let listed = wpa(root, &["list_networks"]);
        let scan = wpa(root, &["scan_results"]);
        self.next_try = now + Self::BACKOFF_S[(self.tries as usize).min(Self::BACKOFF_S.len() - 1)];
        if !(listed.ok && scan.ok) {
            return None;
        }
        let saved: Vec<Vec<String>> = rows(listed.output.as_deref())
            .into_iter()
            .filter(|row| {
                row.len() >= 4
                    && digits(&row[0])
                    && !row[1].is_empty()
                    && !row[3].contains("[DISABLED]")
            })
            .collect();
        if saved.is_empty() {
            return None;
        }
        let mut signal: Vec<(String, i64)> = Vec::new();
        for row in rows(scan.output.as_deref()) {
            if row.len() >= 5 && !row[4].is_empty() && signed(&row[2]) {
                let level = int(&row[2]);
                match signal.iter_mut().find(|(name, _)| *name == row[4]) {
                    Some(entry) => entry.1 = entry.1.max(level),
                    None => signal.push((row[4].clone(), level.max(-999))),
                }
            }
        }
        let strength = |name: &str| signal.iter().find(|(n, _)| n == name).map(|(_, s)| *s);
        let seen: Vec<&Vec<String>> = saved
            .iter()
            .filter(|row| strength(&row[1]).is_some())
            .collect();
        self.tries += 1;
        self.first_try.get_or_insert(now);
        let (action, outcome) = if !seen.is_empty() {
            let last = monitor.and_then(|m| m.connected_network);
            let chosen = seen
                .iter()
                .find(|row| Some(int(&row[0])) == last)
                .copied()
                .unwrap_or_else(|| {
                    // max() by (signal, -id): the first of equal keys.
                    let key =
                        |row: &Vec<String>| (strength(&row[1]).unwrap_or(-999), -int(&row[0]));
                    let mut best = seen[0];
                    for row in &seen[1..] {
                        if key(row) > key(best) {
                            best = row;
                        }
                    }
                    best
                });
            let selected = wpa(root, &["select_network", &chosen[0]]);
            let enabled = wpa(root, &["enable_network", "all"]);
            let outcome = Outcome::Reselect {
                network_id: int(&chosen[0]),
                last_connected: last.is_some_and(|l| chosen[0] == l.to_string()),
                ok: [answered_ok(&selected), answered_ok(&enabled)],
            };
            ("wifi_reselect", outcome)
        } else {
            let answer = wpa(root, &["scan"]);
            (
                "wifi_rescan",
                Outcome::Rescan {
                    ok: answered_ok(&answer),
                },
            )
        };
        let attempt = Attempt {
            state,
            idle_s: round1(now - since),
            tries: self.tries,
            saved: saved.len(),
            in_range: seen.len(),
            scan_entries: signal.len(),
            temp_disabled: saved
                .iter()
                .filter(|row| row[3].contains("TEMP-DISABLED"))
                .count(),
            outcome,
            events: monitor.map(|m| m.summary(Some(now))),
        };
        journal(root, action, &attempt);
        Some(action)
    }
}

/// Whether wpa_cli answered `OK`.
fn answered_ok(answer: &command::Outcome) -> bool {
    y2p_core::root::python_strip(answer.output.as_deref().unwrap_or_default()) == "OK"
}

/// The journal row of a recovery: tries until the supplicant completed, seconds since the first.
#[derive(Serialize)]
struct Recovered {
    tries: u64,
    seconds: f64,
}

/// The journal row of one self-heal try (`wifi_reselect` or `wifi_rescan`).
#[derive(Serialize)]
struct Attempt {
    state: Value,
    idle_s: f64,
    tries: u64,
    saved: usize,
    in_range: usize,
    scan_entries: usize,
    temp_disabled: usize,
    #[serde(flatten)]
    outcome: Outcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    events: Option<Object>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Outcome {
    /// The saved network selected (the last that connected, else the strongest) and the others
    /// enabled again; whether each answered OK.
    Reselect {
        network_id: i64,
        last_connected: bool,
        ok: [bool; 2],
    },
    /// None in range: one scan.
    Rescan { ok: bool },
}

#[cfg(test)]
#[path = "wifi_tests.rs"]
mod wifi_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// A supplicant control socket that answers ATTACH and then sends two events.
    #[test]
    fn the_monitor_attaches_to_the_supplicant_and_counts_its_events() {
        let dir = std::env::temp_dir().join(format!("y2-wpa-{}", std::process::id()));
        fs::create_dir_all(dir.join("run/wpa_supplicant")).unwrap();
        fs::create_dir_all(dir.join("run/y2")).unwrap();
        let server = UnixDatagram::bind(dir.join("run/wpa_supplicant/wlan0")).unwrap();
        let supplicant = std::thread::spawn(move || {
            let mut buffer = [0u8; 64];
            let (n, peer) = server.recv_from(&mut buffer).unwrap();
            assert_eq!(&buffer[..n], b"ATTACH");
            let peer = peer.as_pathname().unwrap().to_owned();
            server.send_to(b"OK\n", &peer).unwrap();
            server
                .send_to(b"<3>CTRL-EVENT-SCAN-STARTED ", &peer)
                .unwrap();
            let connected =
                b"<3>CTRL-EVENT-CONNECTED - Connection to 00:11:22:33:44:55 completed [id=7 id_str=]";
            server.send_to(connected, &peer).unwrap();
        });
        let root = Root::new(&dir);
        let mut monitor = EventMonitor::new(&root);
        monitor.poll();
        supplicant.join().unwrap();
        assert!(monitor.fd().is_some(), "attached");
        assert!(dir.join("run/y2/wpa-events.sock").exists());
        monitor.poll();
        let summary = monitor.summary(None).into_sorted();
        assert_eq!(
            summary["counts"],
            json!({"SCAN-STARTED": 1, "CONNECTED": 1})
        );
        assert_eq!(monitor.connected_network, Some(7));
        assert!(dir.join("run/y2/wifi-failure.json").exists());
        // The supplicant went away: the monitor lets go of its socket.
        fs::remove_file(dir.join("run/wpa_supplicant/wlan0")).unwrap();
        monitor.poll();
        assert!(monitor.fd().is_none());
        assert!(!dir.join("run/y2/wpa-events.sock").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
