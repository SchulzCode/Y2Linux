//! The reads every daemon tick makes, without a process: wpa_supplicant's control socket in place
//! of `wpa_cli -i wlan0 status|signal_poll`, rtnetlink in place of `ip -j -4 addr show dev X` and
//! `ip -j -4 route show default dev X`. On the player each answers as the command's
//! `command::Outcome` would (the same `ok`, reasons and output text the callers parse); below a
//! fixture root each stays the fixture command it was (same argv and timeout), as the DNS probe's
//! `getaddrinfo` does, so goldens, recordings and tests see the same requests.
// SPDX-License-Identifier: GPL-2.0-only
use serde::{Deserialize, Serialize};
use std::{
    ffi::CString,
    io,
    net::Ipv4Addr,
    os::{
        fd::{AsRawFd, OwnedFd},
        linux::net::SocketAddrExt,
        unix::net::{SocketAddr, UnixDatagram},
    },
    path::Path,
    sync::atomic::{AtomicU32, Ordering},
    time::{Duration, Instant},
};
use y2p_core::{command::Outcome, Root};

/// wpa_supplicant's control socket for wlan0 (`ctrl_interface=/run/wpa_supplicant`; wpa_cli's
/// `/var/run/wpa_supplicant/wlan0` is the same file).
pub const WPA_CONTROL: &str = "/run/wpa_supplicant/wlan0";

/// `wpa_cli -i wlan0 <command>` (`status`, `signal_poll`): the reply wpa_cli prints.
pub fn wpa_cli(root: &Root, command: &str, timeout: f64) -> Outcome {
    if root.is_real() {
        control_request(
            Path::new(WPA_CONTROL),
            command.to_ascii_uppercase().as_bytes(),
            timeout,
        )
    } else {
        y2p_core::command::run(root, &["wpa_cli", "-i", "wlan0", command], timeout)
    }
}

/// `ip -j -4 addr show dev <dev>`: the interface's IPv4 addresses as `ip -j` lists them (one link
/// with `ifindex`, `ifname` and `addr_info`; none without addresses). The link's own fields (flags,
/// mtu, …) and the address lifetimes are left out. Err is the reason the query failed
/// (`command_failed`, `command_timeout`, or `invalid_ip_output` for a fixture's odd output).
pub fn ipv4_addresses(root: &Root, dev: &str, timeout: f64) -> Result<Vec<Link>, String> {
    if root.is_real() {
        netlink_answer(dev, RTM_NEWADDR, timeout, |index, dump| {
            addresses(dev, index, dump)
        })
    } else {
        fixture_answer(
            root,
            &["ip", "-j", "-4", "addr", "show", "dev", dev],
            timeout,
        )
    }
}

/// `ip -j -4 route show default dev <dev>`: the main table's default routes through `dev`, each
/// with the fields iproute2 prints (`ip/iproute.c` print_route).
pub fn default_route(root: &Root, dev: &str, timeout: f64) -> Result<Vec<Route>, String> {
    if root.is_real() {
        netlink_answer(dev, RTM_NEWROUTE, timeout, routes)
    } else {
        fixture_answer(
            root,
            &["ip", "-j", "-4", "route", "show", "default", "dev", dev],
            timeout,
        )
    }
}

/// Below a fixture root: the recorded `ip -j` command's output read as `T` (empty output is none).
fn fixture_answer<T: serde::de::DeserializeOwned + Default>(
    root: &Root,
    argv: &[&str],
    timeout: f64,
) -> Result<T, String> {
    let answer = y2p_core::command::run(root, argv, timeout);
    if !answer.ok {
        return Err(answer.reason.unwrap_or_else(|| "command_failed".into()));
    }
    match answer.output.as_deref().map(str::trim) {
        None | Some("") => Ok(T::default()),
        Some(text) => serde_json::from_str(text).map_err(|_| "invalid_ip_output".into()),
    }
}

/// One link of `ip -j -4 addr` with its IPv4 addresses.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Link {
    #[serde(default)]
    pub ifindex: Option<u32>,
    #[serde(default)]
    pub ifname: Option<String>,
    #[serde(default)]
    pub addr_info: Vec<Address>,
}

/// One address of a link (`local` is IFA_LOCAL, else IFA_ADDRESS, as ip prints it).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Address {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local: Option<String>,
    pub prefixlen: Option<u8>,
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// One route as `ip -j route` prints it (a field ip leaves out is absent).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Route {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dst: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway: Option<String>,
    /// Printed by ip only without a `dev` filter (never by the netlink read).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefsrc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metric: Option<u32>,
    pub flags: Vec<String>,
}

fn failed(reason: &str) -> Outcome {
    Outcome {
        ok: false,
        reason: Some(reason.into()),
        output: None,
    }
}

/// A distinct client name per request in this process (wpa_ctrl's `wpa_ctrl_<pid>-<counter>`).
static NEXT_CLIENT: AtomicU32 = AtomicU32::new(0);

/// One control-interface request as wpa_cli makes it (`wpa_ctrl_request`): a datagram socket bound
/// to a client address, the request sent to `socket`, the first reply that is not an unsolicited
/// `<…>` event read back, at most 4095 bytes and up to its first NUL (wpa_cli's buffer and printf).
///
/// The client address is abstract (`\0y2-wpa-ctrl-<pid>-<n>`): wpa_supplicant answers the address
/// recvfrom gave it (ctrl_iface_unix.c), and an abstract name leaves no file behind. Outcomes as
/// the command runner gave for wpa_cli: no socket, a refused connect or a failed send is
/// `command_failed` (wpa_cli exits 255); no reply (or no room to send) within `timeout` is
/// `command_timeout` (the runner killed wpa_cli after `timeout`, before its own 10 s request
/// timeout); a reply, also `FAIL`, is ok (wpa_cli prints it and exits 0).
pub(crate) fn control_request(socket: &Path, request: &[u8], timeout: f64) -> Outcome {
    let deadline = Instant::now() + Duration::from_secs_f64(timeout.max(0.0));
    let name = format!(
        "y2-wpa-ctrl-{}-{}",
        std::process::id(),
        NEXT_CLIENT.fetch_add(1, Ordering::Relaxed)
    );
    let Ok(sock) = SocketAddr::from_abstract_name(name.as_bytes())
        .and_then(|address| UnixDatagram::bind_addr(&address))
    else {
        return failed("command_failed");
    };
    if sock.connect(socket).is_err() {
        return failed("command_failed");
    }
    let left = |deadline: Instant| {
        let left = deadline.saturating_duration_since(Instant::now());
        (!left.is_zero()).then_some(left)
    };
    let Some(wait) = left(deadline) else {
        return failed("command_timeout");
    };
    if sock.set_write_timeout(Some(wait)).is_err() {
        return failed("command_failed");
    }
    match sock.send(request) {
        Ok(_) => {}
        // A full queue: wpa_ctrl_request retries for 5 s, past the runner's deadline.
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            ) =>
        {
            return failed("command_timeout")
        }
        Err(_) => return failed("command_failed"),
    }
    let mut reply = [0u8; 4095];
    loop {
        let Some(wait) = left(deadline) else {
            return failed("command_timeout");
        };
        if sock.set_read_timeout(Some(wait)).is_err() {
            return failed("command_failed");
        }
        match sock.recv(&mut reply) {
            Ok(n) if n > 0 && reply[0] == b'<' => continue,
            Ok(n) => {
                let text = &reply[..n];
                let text = text.split(|&b| b == 0).next().unwrap_or_default();
                let text = String::from_utf8_lossy(text);
                return Outcome {
                    ok: true,
                    reason: None,
                    output: Some(y2p_core::root::python_strip(&text).to_owned()),
                };
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return failed("command_timeout")
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return failed("command_failed"),
        }
    }
}

// rtnetlink (linux/netlink.h, linux/rtnetlink.h, linux/if_addr.h).
const NLMSG_HEADER: usize = 16;
const NLMSG_ERROR: u16 = 2;
const NLMSG_DONE: u16 = 3;
const NLM_F_REQUEST: u16 = 1;
const NLM_F_DUMP: u16 = 0x300;
pub(crate) const RTM_NEWADDR: u16 = 20;
const RTM_GETADDR: u16 = 22;
pub(crate) const RTM_NEWROUTE: u16 = 24;
const RTM_GETROUTE: u16 = 26;
pub(crate) const RTM_NEWLINK: u16 = 16;
const RTM_GETLINK: u16 = 18;
const IFADDRMSG: usize = 8;
const IFINFOMSG: usize = 16;
const IFLA_STATS64: u16 = 23;
const RTMSG: usize = 12;
pub(crate) const IFA_ADDRESS: u16 = 1;
pub(crate) const IFA_LOCAL: u16 = 2;
pub(crate) const IFA_LABEL: u16 = 3;
pub(crate) const RTA_DST: u16 = 1;
pub(crate) const RTA_OIF: u16 = 4;
pub(crate) const RTA_GATEWAY: u16 = 5;
pub(crate) const RTA_PRIORITY: u16 = 6;
pub(crate) const RTA_PREFSRC: u16 = 7;
pub(crate) const RTA_TABLE: u16 = 15;
pub(crate) const RT_TABLE_MAIN: u8 = 254;
pub(crate) const RTN_UNICAST: u8 = 1;
pub(crate) const RTPROT_BOOT: u8 = 3;
pub(crate) const RTM_F_CLONED: u32 = 0x200;

/// The request for a dump of `kind` (RTM_GETADDR or RTM_GETROUTE of the IPv4 family, RTM_GETLINK of
/// every family).
fn dump_request(kind: u16, seq: u32) -> Vec<u8> {
    let (body, family) = match kind {
        RTM_GETADDR => (IFADDRMSG, libc::AF_INET as u8),
        RTM_GETLINK => (IFINFOMSG, libc::AF_UNSPEC as u8),
        _ => (RTMSG, libc::AF_INET as u8),
    };
    let mut request = Vec::with_capacity(NLMSG_HEADER + body);
    request.extend_from_slice(&((NLMSG_HEADER + body) as u32).to_ne_bytes());
    request.extend_from_slice(&kind.to_ne_bytes());
    request.extend_from_slice(&(NLM_F_REQUEST | NLM_F_DUMP).to_ne_bytes());
    request.extend_from_slice(&seq.to_ne_bytes());
    request.extend_from_slice(&0u32.to_ne_bytes());
    request.push(family);
    request.resize(NLMSG_HEADER + body, 0);
    request
}

/// What a dump could not do: the command's reason.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DumpError {
    Failed,
    Timeout,
}

/// The payloads (after the netlink header) of the dump's `kind` messages, in the kernel's order.
fn dump(kind: u16, timeout: f64) -> Result<Vec<Vec<u8>>, DumpError> {
    let deadline = Instant::now() + Duration::from_secs_f64(timeout.max(0.0));
    let sock = y2p_core::sys::netlink_route(0, false).map_err(|_| DumpError::Failed)?;
    let seq = 1;
    let request = dump_request(
        match kind {
            RTM_NEWADDR => RTM_GETADDR,
            RTM_NEWLINK => RTM_GETLINK,
            _ => RTM_GETROUTE,
        },
        seq,
    );
    send(&sock, &request)?;
    let mut payloads = Vec::new();
    let mut buffer = vec![0u8; 65_536];
    loop {
        let received = receive(&sock, &mut buffer, deadline)?;
        if messages(&buffer[..received], seq, kind, &mut payloads)? {
            return Ok(payloads);
        }
    }
}

/// Send one request to the kernel.
fn send(sock: &OwnedFd, request: &[u8]) -> Result<(), DumpError> {
    // SAFETY: a zeroed sockaddr_nl is the kernel's address; `request` is a valid buffer.
    let sent = unsafe {
        let mut kernel: libc::sockaddr_nl = std::mem::zeroed();
        kernel.nl_family = libc::AF_NETLINK as libc::sa_family_t;
        libc::sendto(
            sock.as_raw_fd(),
            request.as_ptr().cast(),
            request.len(),
            0,
            (&kernel as *const libc::sockaddr_nl).cast(),
            std::mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
        )
    };
    if sent != request.len() as isize {
        return Err(DumpError::Failed);
    }
    Ok(())
}

/// One datagram from the kernel before `deadline`: its length in `buffer`.
fn receive(sock: &OwnedFd, buffer: &mut [u8], deadline: Instant) -> Result<usize, DumpError> {
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(DumpError::Timeout);
        }
        let wait = libc::timeval {
            tv_sec: left.as_secs() as libc::time_t,
            tv_usec: left.subsec_micros().max(1) as libc::suseconds_t,
        };
        // SAFETY: a valid timeval for SO_RCVTIMEO and a valid buffer for recv.
        let received = unsafe {
            libc::setsockopt(
                sock.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_RCVTIMEO,
                (&wait as *const libc::timeval).cast(),
                std::mem::size_of::<libc::timeval>() as libc::socklen_t,
            );
            libc::recv(
                sock.as_raw_fd(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                0,
            )
        };
        if received >= 0 {
            return Ok(received as usize);
        }
        match io::Error::last_os_error().kind() {
            io::ErrorKind::Interrupted => continue,
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => return Err(DumpError::Timeout),
            _ => return Err(DumpError::Failed),
        }
    }
}

// Generic netlink (linux/genetlink.h) and nl80211 (linux/nl80211.h).
const GENL_ID_CTRL: u16 = 16;
const CTRL_CMD_GETFAMILY: u8 = 3;
const CTRL_ATTR_FAMILY_ID: u16 = 1;
const CTRL_ATTR_FAMILY_NAME: u16 = 2;
const GENL_HEADER: usize = 4;
const NL80211_CMD_GET_POWER_SAVE: u8 = 62;
const NL80211_ATTR_IFINDEX: u16 = 3;
const NL80211_ATTR_PS_STATE: u16 = 93;

/// One generic netlink request (`kind` the family id, `command`, attributes) as bytes.
pub(crate) fn genl_request(
    kind: u16,
    command: u8,
    seq: u32,
    attributes: &[(u16, &[u8])],
) -> Vec<u8> {
    let mut body = vec![command, 1, 0, 0];
    for (attribute, data) in attributes {
        let length = 4 + data.len();
        body.extend_from_slice(&(length as u16).to_ne_bytes());
        body.extend_from_slice(&attribute.to_ne_bytes());
        body.extend_from_slice(data);
        body.resize(align(body.len()), 0);
    }
    let mut request = Vec::with_capacity(NLMSG_HEADER + body.len());
    request.extend_from_slice(&((NLMSG_HEADER + body.len()) as u32).to_ne_bytes());
    request.extend_from_slice(&kind.to_ne_bytes());
    request.extend_from_slice(&NLM_F_REQUEST.to_ne_bytes());
    request.extend_from_slice(&seq.to_ne_bytes());
    request.extend_from_slice(&0u32.to_ne_bytes());
    request.extend_from_slice(&body);
    request
}

/// The reply (after the netlink header) of a generic netlink request.
fn genl(
    sock: &OwnedFd,
    request: &[u8],
    seq: u32,
    kind: u16,
    deadline: Instant,
) -> Result<Vec<u8>, DumpError> {
    send(sock, request)?;
    let mut buffer = vec![0u8; 16_384];
    loop {
        let received = receive(sock, &mut buffer, deadline)?;
        let mut payloads = Vec::new();
        let done = messages(&buffer[..received], seq, kind, &mut payloads)?;
        if let Some(reply) = payloads.into_iter().next() {
            return Ok(reply);
        }
        if done {
            return Err(DumpError::Failed);
        }
    }
}

/// `iw dev <dev> get power_save`'s answer (`Power save: on|off`) from nl80211's power-save state.
fn read_power_save(dev: &str, timeout: f64) -> Result<String, DumpError> {
    let deadline = Instant::now() + Duration::from_secs_f64(timeout.max(0.0));
    let index = interface_index(dev).ok_or(DumpError::Failed)?;
    let sock =
        y2p_core::sys::netlink(libc::NETLINK_GENERIC, 0, false).map_err(|_| DumpError::Failed)?;
    let ask = genl_request(
        GENL_ID_CTRL,
        CTRL_CMD_GETFAMILY,
        1,
        &[(CTRL_ATTR_FAMILY_NAME, b"nl80211\0")],
    );
    let family = genl(&sock, &ask, 1, GENL_ID_CTRL, deadline)?;
    let family = attribute(&attributes(&family, GENL_HEADER), CTRL_ATTR_FAMILY_ID)
        .and_then(|id| Some(u16::from_ne_bytes(id.get(..2)?.try_into().ok()?)))
        .ok_or(DumpError::Failed)?;
    let ask = genl_request(
        family,
        NL80211_CMD_GET_POWER_SAVE,
        2,
        &[(NL80211_ATTR_IFINDEX, &index.to_ne_bytes())],
    );
    let reply = genl(&sock, &ask, 2, family, deadline)?;
    let state = attribute(&attributes(&reply, GENL_HEADER), NL80211_ATTR_PS_STATE)
        .and_then(u32_of)
        .ok_or(DumpError::Failed)?;
    Ok(format!(
        "Power save: {}",
        if state == 1 { "on" } else { "off" }
    ))
}

/// `iw dev <dev> get power_save`: on the player nl80211 answers in process (the same text, the
/// same outcomes: no interface or a refused request `command_failed`, no answer in time
/// `command_timeout`); below a fixture root the fixture command as before.
pub fn power_save(root: &Root, dev: &str, timeout: f64) -> Outcome {
    if !root.is_real() {
        return y2p_core::command::run(
            root,
            &["/usr/sbin/iw", "dev", dev, "get", "power_save"],
            timeout,
        );
    }
    match read_power_save(dev, timeout) {
        Ok(text) => Outcome {
            ok: true,
            reason: None,
            output: Some(text),
        },
        Err(DumpError::Timeout) => failed("command_timeout"),
        Err(DumpError::Failed) => failed("command_failed"),
    }
}

/// The messages of one datagram: `kind` payloads of request `seq` appended; whether the dump ended
/// (NLMSG_DONE). An error message (not an acknowledgement) or a malformed header fails the dump.
pub(crate) fn messages(
    mut data: &[u8],
    seq: u32,
    kind: u16,
    payloads: &mut Vec<Vec<u8>>,
) -> Result<bool, DumpError> {
    while data.len() >= NLMSG_HEADER {
        let length = u32::from_ne_bytes(data[0..4].try_into().unwrap_or_default()) as usize;
        let message = u16::from_ne_bytes([data[4], data[5]]);
        let number = u32::from_ne_bytes(data[8..12].try_into().unwrap_or_default());
        if length < NLMSG_HEADER || length > data.len() {
            return Err(DumpError::Failed);
        }
        let body = &data[NLMSG_HEADER..length];
        if number == seq {
            match message {
                NLMSG_DONE => return Ok(true),
                NLMSG_ERROR => {
                    let code = body
                        .get(..4)
                        .map_or(-1, |b| i32::from_ne_bytes(b.try_into().unwrap_or_default()));
                    if code != 0 {
                        return Err(DumpError::Failed);
                    }
                }
                _ if message == kind => payloads.push(body.to_vec()),
                _ => {}
            }
        }
        data = data.get(align(length)..).unwrap_or_default();
    }
    Ok(false)
}

fn align(length: usize) -> usize {
    (length + 3) & !3
}

/// The attributes after a fixed message header of `fixed` bytes: (type, data) in order.
pub(crate) fn attributes(body: &[u8], fixed: usize) -> Vec<(u16, &[u8])> {
    let mut found = Vec::new();
    let mut rest = body.get(align(fixed)..).unwrap_or_default();
    while rest.len() >= 4 {
        let length = u16::from_ne_bytes([rest[0], rest[1]]) as usize;
        let kind = u16::from_ne_bytes([rest[2], rest[3]]) & 0x3fff;
        if length < 4 || length > rest.len() {
            break;
        }
        found.push((kind, &rest[4..length]));
        rest = rest.get(align(length)..).unwrap_or_default();
    }
    found
}

fn attribute<'a>(attributes: &[(u16, &'a [u8])], kind: u16) -> Option<&'a [u8]> {
    attributes.iter().find(|(k, _)| *k == kind).map(|(_, d)| *d)
}

fn ipv4(data: &[u8]) -> Option<String> {
    let octets: [u8; 4] = data.get(..4)?.try_into().ok()?;
    Some(Ipv4Addr::from(octets).to_string())
}

fn u32_of(data: &[u8]) -> Option<u32> {
    Some(u32::from_ne_bytes(data.get(..4)?.try_into().ok()?))
}

/// `struct rtnl_link_stats64` (linux/if_link.h) in field order: the names its counters have below
/// `/sys/class/net/<dev>/statistics/` (`rx_otherhost_dropped` has no file there).
const STATS64: [&str; 24] = [
    "rx_packets",
    "tx_packets",
    "rx_bytes",
    "tx_bytes",
    "rx_errors",
    "tx_errors",
    "rx_dropped",
    "tx_dropped",
    "multicast",
    "collisions",
    "rx_length_errors",
    "rx_over_errors",
    "rx_crc_errors",
    "rx_frame_errors",
    "rx_fifo_errors",
    "rx_missed_errors",
    "tx_aborted_errors",
    "tx_carrier_errors",
    "tx_fifo_errors",
    "tx_heartbeat_errors",
    "tx_window_errors",
    "rx_compressed",
    "tx_compressed",
    "rx_nohandler",
];

/// The files of `/sys/class/net/<dev>/statistics/` as (name, value), sorted by name, from one
/// RTM_GETLINK dump's IFLA_STATS64 (the same `dev_get_stats` the files print): empty without the
/// interface, None when the dump fails or carries no counters for it (the caller reads the files).
pub fn link_statistics(dev: &str, timeout: f64) -> Option<Vec<(&'static str, u64)>> {
    let Some(index) = interface_index(dev) else {
        return Some(Vec::new());
    };
    let dump = dump(RTM_NEWLINK, timeout).ok()?;
    let body = dump
        .iter()
        .find(|body| body.len() >= IFINFOMSG && u32_of(&body[4..8]) == Some(index))?;
    statistics_of(body)
}

/// The counters of one RTM_NEWLINK payload, as [`link_statistics`] gives them.
pub(crate) fn statistics_of(body: &[u8]) -> Option<Vec<(&'static str, u64)>> {
    let data = attribute(&attributes(body, IFINFOMSG), IFLA_STATS64)?;
    let mut found: Vec<(&'static str, u64)> = STATS64
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let bytes = data.get(i * 8..i * 8 + 8)?.try_into().ok()?;
            Some((*name, u64::from_ne_bytes(bytes)))
        })
        .collect::<Option<_>>()?;
    found.sort_by_key(|(name, _)| *name);
    Some(found)
}

/// `if_nametoindex` (how ip resolves `dev`): None for no such interface.
fn interface_index(dev: &str) -> Option<u32> {
    let name = CString::new(dev).ok()?;
    // SAFETY: a valid NUL-terminated name.
    let index = unsafe { libc::if_nametoindex(name.as_ptr()) };
    (index != 0).then_some(index)
}

/// A dump of `kind` (RTM_NEWADDR or RTM_NEWROUTE) messages, printed as `ip -j` would: no such interface or a failed dump
/// is `command_failed` (ip exits 1 or 2), a dump that does not end within `timeout` is
/// `command_timeout` (the runner's deadline).
fn netlink_answer<T>(
    dev: &str,
    kind: u16,
    timeout: f64,
    read: impl FnOnce(u32, Vec<Vec<u8>>) -> T,
) -> Result<T, String> {
    let Some(index) = interface_index(dev) else {
        return Err("command_failed".into());
    };
    match dump(kind, timeout) {
        Ok(payloads) => Ok(read(index, payloads)),
        Err(DumpError::Timeout) => Err("command_timeout".into()),
        Err(DumpError::Failed) => Err("command_failed".into()),
    }
}

/// `ip -j -4 addr show dev <dev>` from an RTM_NEWADDR dump: the addresses of interface `index`, in
/// the kernel's order. `local` is IFA_LOCAL, else IFA_ADDRESS (as ip prints it); `scope` is
/// iproute2's name of ifa_scope (`global`, `site`, `link`, `host`, `nowhere`, else the number).
pub(crate) fn addresses(dev: &str, index: u32, dump: Vec<Vec<u8>>) -> Vec<Link> {
    let mut found = Vec::new();
    for body in &dump {
        if body.len() < IFADDRMSG
            || body[0] != libc::AF_INET as u8
            || u32_of(&body[4..8]) != Some(index)
        {
            continue;
        }
        let attributes = attributes(body, IFADDRMSG);
        let local = attribute(&attributes, IFA_LOCAL)
            .or_else(|| attribute(&attributes, IFA_ADDRESS))
            .and_then(ipv4);
        found.push(Address {
            family: Some("inet".into()),
            local: local.map(|l| l.to_string()),
            prefixlen: Some(body[1]),
            scope: Some(scope_name(body[3])),
            label: attribute(&attributes, IFA_LABEL).map(|label| {
                let label = label.split(|&b| b == 0).next().unwrap_or_default();
                String::from_utf8_lossy(label).into_owned()
            }),
        });
    }
    if found.is_empty() {
        return Vec::new();
    }
    vec![Link {
        ifindex: Some(index),
        ifname: Some(dev.into()),
        addr_info: found,
    }]
}

/// `ip -j -4 route show default dev <dev>` from an RTM_NEWROUTE dump (ip/iproute.c filter_nlmsg and
/// print_route with `filter.tb = RT_TABLE_MAIN`, `default` as an exact prefix, `dev` as oif): IPv4
/// routes of the main table, not cloned, with dst_len 0 and RTA_OIF `index`; each printed with
/// `type` (not unicast), `dst`, `gateway`, `protocol` (not boot), `scope` (not universe), `prefsrc`,
/// `metric` and `flags`, in that order (`dev` is not printed: it is the filter).
///
/// Not printed (the route dhcp-event installs, `ip -4 route replace default via <gw> dev wlan0
/// metric 100`, never has them): `tos`, `from`, `nhid`, `via`, `encap`, `newdst`, `mark`, `realms`,
/// `uid`, `metrics`, `pref`, `iif`, `cache`; a multipath route (no RTA_OIF) is not listed.
pub(crate) fn routes(index: u32, dump: Vec<Vec<u8>>) -> Vec<Route> {
    let mut found = Vec::new();
    for body in &dump {
        if body.len() < RTMSG || body[0] != libc::AF_INET as u8 {
            continue;
        }
        let (dst_len, header_table, protocol, scope, kind) =
            (body[1], body[4], body[5], body[6], body[7]);
        let flags = u32_of(&body[8..12]).unwrap_or_default();
        let attributes = attributes(body, RTMSG);
        let table = attribute(&attributes, RTA_TABLE)
            .and_then(u32_of)
            .unwrap_or(u32::from(header_table));
        if table != u32::from(RT_TABLE_MAIN)
            || flags & RTM_F_CLONED != 0
            || dst_len != 0
            || attribute(&attributes, RTA_OIF).and_then(u32_of) != Some(index)
        {
            continue;
        }
        let dst = match attribute(&attributes, RTA_DST).and_then(ipv4) {
            Some(address) => format!("{address}/{dst_len}"),
            None => "default".into(),
        };
        found.push(Route {
            kind: (kind != RTN_UNICAST).then(|| type_name(kind).to_string()),
            dst: Some(dst),
            gateway: attribute(&attributes, RTA_GATEWAY)
                .and_then(ipv4)
                .map(|a| a.to_string()),
            dev: None,
            protocol: (protocol != RTPROT_BOOT).then(|| protocol_name(protocol).to_string()),
            scope: (scope != 0).then(|| scope_name(scope)),
            prefsrc: attribute(&attributes, RTA_PREFSRC)
                .and_then(ipv4)
                .map(|a| a.to_string()),
            metric: attribute(&attributes, RTA_PRIORITY).and_then(u32_of),
            flags: flag_names(flags).into_iter().map(String::from).collect(),
        });
    }
    found
}

/// iproute2's rt_scopes names.
fn scope_name(scope: u8) -> String {
    match scope {
        0 => "global".into(),
        200 => "site".into(),
        253 => "link".into(),
        254 => "host".into(),
        255 => "nowhere".into(),
        other => other.to_string(),
    }
}

/// iproute2's rt_protos names (the table built into ip and the file it ships).
fn protocol_name(protocol: u8) -> String {
    let name = match protocol {
        0 => "unspec",
        1 => "redirect",
        2 => "kernel",
        3 => "boot",
        4 => "static",
        8 => "gated",
        9 => "ra",
        10 => "mrt",
        11 => "zebra",
        12 => "bird",
        13 => "dnrouted",
        14 => "xorp",
        15 => "ntk",
        16 => "dhcp",
        18 => "keepalived",
        42 => "babel",
        84 => "ovn",
        99 => "openr",
        186 => "bgp",
        187 => "isis",
        188 => "ospf",
        189 => "rip",
        192 => "eigrp",
        other => return other.to_string(),
    };
    name.into()
}

/// iproute2's rtnl_rtntype_n2a.
fn type_name(kind: u8) -> String {
    let name = match kind {
        0 => "none",
        1 => "unicast",
        2 => "local",
        3 => "broadcast",
        4 => "anycast",
        5 => "multicast",
        6 => "blackhole",
        7 => "unreachable",
        8 => "prohibit",
        9 => "throw",
        10 => "nat",
        11 => "xresolve",
        other => return other.to_string(),
    };
    name.into()
}

/// ip/iproute.c print_rt_flags, in its order.
fn flag_names(flags: u32) -> Vec<&'static str> {
    const NAMES: [(u32, &str); 11] = [
        (1, "dead"),
        (4, "onlink"),
        (2, "pervasive"),
        (8, "offload"),
        (64, "trap"),
        (0x100, "notify"),
        (16, "linkdown"),
        (32, "unresolved"),
        (0x4000, "rt_offload"),
        (0x8000, "rt_trap"),
        (0x2000_0000, "rt_offload_failed"),
    ];
    NAMES
        .iter()
        .filter(|(bit, _)| flags & bit != 0)
        .map(|(_, name)| *name)
        .collect()
}

#[cfg(test)]
#[path = "direct_tests.rs"]
mod direct_tests;
