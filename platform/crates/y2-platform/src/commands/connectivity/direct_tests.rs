//! The player's direct reads against fakes: a datagram wpa_supplicant in a temporary directory and
//! netlink dumps built byte by byte; below a fixture root, the fixture commands they replace.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::testing::{ok, Fixture};
use serde_json::{json, Value};
use std::{os::unix::net::UnixDatagram, thread, time::Instant};

/// A fake wpa_supplicant on `<fixture>/wlan0`: each request answered by `answer` (None: never),
/// the requests it saw returned when it stops after `count` of them.
fn supplicant(
    fixture: &Fixture,
    count: usize,
    answer: impl Fn(&[u8]) -> Vec<Vec<u8>> + Send + 'static,
) -> (std::path::PathBuf, thread::JoinHandle<Vec<Vec<u8>>>) {
    let path = fixture.path("/wlan0");
    let server = UnixDatagram::bind(&path).unwrap();
    server
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let handle = thread::spawn(move || {
        let mut seen = Vec::new();
        for _ in 0..count {
            let mut buffer = [0u8; 4096];
            let Ok((n, from)) = server.recv_from(&mut buffer) else {
                break;
            };
            seen.push(buffer[..n].to_vec());
            for reply in answer(&buffer[..n]) {
                server.send_to_addr(&reply, &from).unwrap();
            }
        }
        seen
    });
    (path, handle)
}

fn reply(ok_output: &str) -> Outcome {
    Outcome {
        ok: true,
        reason: None,
        output: Some(ok_output.into()),
    }
}

#[test]
fn control_requests_read_the_reply_wpa_cli_prints() {
    let fixture = Fixture::new();
    let (path, server) = supplicant(&fixture, 3, |request| match request {
        // An unsolicited event first (an attached monitor's; wpa_ctrl_request skips it).
        b"STATUS" => vec![
            b"<3>CTRL-EVENT-SCAN-STARTED ".to_vec(),
            b"bssid=aa:bb:cc:dd:ee:ff\nssid=Home\nwpa_state=COMPLETED\nip_address=192.168.1.40\n"
                .to_vec(),
        ],
        b"SIGNAL_POLL" => vec![b"RSSI=-51\nLINKSPEED=72\nNOISE=9999\nFREQUENCY=2437\n".to_vec()],
        _ => vec![b"FAIL\n".to_vec()],
    });
    assert_eq!(
        control_request(&path, b"STATUS", 3.0),
        reply("bssid=aa:bb:cc:dd:ee:ff\nssid=Home\nwpa_state=COMPLETED\nip_address=192.168.1.40")
    );
    assert_eq!(
        control_request(&path, b"SIGNAL_POLL", 3.0),
        reply("RSSI=-51\nLINKSPEED=72\nNOISE=9999\nFREQUENCY=2437")
    );
    // wpa_cli prints FAIL and exits 0: an ok answer whose text the callers do not parse.
    assert_eq!(control_request(&path, b"PING", 3.0), reply("FAIL"));
    assert_eq!(
        server.join().unwrap(),
        [
            b"STATUS".to_vec(),
            b"SIGNAL_POLL".to_vec(),
            b"PING".to_vec()
        ]
    );
}

#[test]
fn control_replies_end_at_wpa_clis_buffer_and_first_nul() {
    let fixture = Fixture::new();
    let (path, server) = supplicant(&fixture, 2, |request| match request {
        b"STATUS" => vec![b"wpa_state=SCANNING\n\0garbage".to_vec()],
        _ => vec![vec![b'x'; 5000]],
    });
    assert_eq!(
        control_request(&path, b"STATUS", 3.0),
        reply("wpa_state=SCANNING")
    );
    let long = control_request(&path, b"SIGNAL_POLL", 3.0);
    assert_eq!(long.output.map(|o| o.len()), Some(4095));
    server.join().unwrap();
}

#[test]
fn a_supplicant_that_never_answers_is_the_commands_timeout() {
    let fixture = Fixture::new();
    let (path, server) = supplicant(&fixture, 1, |_| Vec::new());
    let started = Instant::now();
    let outcome = control_request(&path, b"STATUS", 0.3);
    let elapsed = started.elapsed();
    assert_eq!(
        outcome,
        Outcome {
            ok: false,
            reason: Some("command_timeout".into()),
            output: None
        }
    );
    assert!(elapsed >= Duration::from_millis(290), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
    assert_eq!(server.join().unwrap(), [b"STATUS".to_vec()]);
}

#[test]
fn no_supplicant_socket_is_the_failed_command() {
    let fixture = Fixture::new();
    let failed = Outcome {
        ok: false,
        reason: Some("command_failed".into()),
        output: None,
    };
    // No socket (wpa_supplicant not started): wpa_cli exits 255.
    assert_eq!(
        control_request(&fixture.path("/wlan0"), b"STATUS", 1.0),
        failed
    );
    // A socket file nobody listens on any more (wpa_supplicant ended): the connect is refused.
    drop(UnixDatagram::bind(fixture.path("/stale")).unwrap());
    assert!(fixture.path("/stale").exists());
    assert_eq!(
        control_request(&fixture.path("/stale"), b"STATUS", 1.0),
        failed
    );
}

#[test]
fn control_clients_leave_no_socket_file() {
    let fixture = Fixture::new();
    let (path, server) = supplicant(&fixture, 1, |_| vec![b"PONG\n".to_vec()]);
    assert_eq!(control_request(&path, b"PING", 3.0), reply("PONG"));
    server.join().unwrap();
    let names: Vec<_> = std::fs::read_dir(fixture.base())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, [std::ffi::OsString::from("wlan0")]);
}

#[test]
fn below_a_fixture_root_the_commands_stay() {
    let answers = vec![
        ok(&["wpa_cli", "-i", "wlan0", "status"], "wpa_state=COMPLETED"),
        ok(&["wpa_cli", "-i", "wlan0", "signal_poll"], "RSSI=-60"),
        ok(&["ip", "-j", "-4", "addr", "show", "dev", "wlan0"], "[]"),
        ok(
            &["ip", "-j", "-4", "route", "show", "default", "dev", "wlan0"],
            r#"[{"dst":"default"}]"#,
        ),
        ok(&["ip", "-j", "-4", "addr", "show", "dev", "usb0"], "[]"),
    ];
    let fixture = Fixture::new().with_answers(answers);
    let root = &fixture.root;
    assert_eq!(
        wpa_cli(root, "status", 3.0).output.as_deref(),
        Some("wpa_state=COMPLETED")
    );
    assert_eq!(
        wpa_cli(root, "signal_poll", 3.0).output.as_deref(),
        Some("RSSI=-60")
    );
    assert_eq!(ipv4_addresses(root, "wlan0", 1.0), Ok(Vec::new()));
    assert_eq!(
        json!(default_route(root, "wlan0", 1.0).unwrap()).to_string(),
        r#"[{"dst":"default","flags":[]}]"#
    );
    assert_eq!(ipv4_addresses(root, "usb0", 1.0), Ok(Vec::new()));
    let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        fixture.calls(),
        [
            (argv(&["wpa_cli", "-i", "wlan0", "status"]), 3.0),
            (argv(&["wpa_cli", "-i", "wlan0", "signal_poll"]), 3.0),
            (
                argv(&["ip", "-j", "-4", "addr", "show", "dev", "wlan0"]),
                1.0
            ),
            (
                argv(&["ip", "-j", "-4", "route", "show", "default", "dev", "wlan0"]),
                1.0
            ),
            (
                argv(&["ip", "-j", "-4", "addr", "show", "dev", "usb0"]),
                1.0
            ),
        ]
    );
}

// Netlink messages built as the kernel sends them.

fn rtattr(kind: u16, data: &[u8]) -> Vec<u8> {
    let mut attribute = Vec::new();
    attribute.extend_from_slice(&((4 + data.len()) as u16).to_ne_bytes());
    attribute.extend_from_slice(&kind.to_ne_bytes());
    attribute.extend_from_slice(data);
    attribute.resize(align(attribute.len()), 0);
    attribute
}

/// An RTM_NEWADDR body: ifaddrmsg then attributes.
fn address(index: u32, prefix: u8, scope: u8, attributes: &[Vec<u8>]) -> Vec<u8> {
    let mut body = vec![libc::AF_INET as u8, prefix, 0x80, scope];
    body.extend_from_slice(&index.to_ne_bytes());
    for attribute in attributes {
        body.extend_from_slice(attribute);
    }
    body
}

/// An RTM_NEWROUTE body: rtmsg (family, dst_len, src_len, tos, table, protocol, scope, type, flags)
/// then attributes.
fn route(
    dst_len: u8,
    protocol: u8,
    scope: u8,
    kind: u8,
    flags: u32,
    attributes: &[Vec<u8>],
) -> Vec<u8> {
    let mut body = vec![
        libc::AF_INET as u8,
        dst_len,
        0,
        0,
        RT_TABLE_MAIN,
        protocol,
        scope,
        kind,
    ];
    body.extend_from_slice(&flags.to_ne_bytes());
    for attribute in attributes {
        body.extend_from_slice(attribute);
    }
    body
}

fn u32_attr(kind: u16, value: u32) -> Vec<u8> {
    rtattr(kind, &value.to_ne_bytes())
}

fn ip_attr(kind: u16, address: [u8; 4]) -> Vec<u8> {
    rtattr(kind, &address)
}

fn message(kind: u16, seq: u32, body: &[u8]) -> Vec<u8> {
    let mut message = Vec::new();
    message.extend_from_slice(&((NLMSG_HEADER + body.len()) as u32).to_ne_bytes());
    message.extend_from_slice(&kind.to_ne_bytes());
    message.extend_from_slice(&2u16.to_ne_bytes()); // NLM_F_MULTI
    message.extend_from_slice(&seq.to_ne_bytes());
    message.extend_from_slice(&0u32.to_ne_bytes());
    message.extend_from_slice(body);
    message.resize(align(message.len()), 0);
    message
}

#[test]
fn dump_messages_collect_the_kind_until_done() {
    let first = address(3, 24, 0, &[]);
    let mut datagram = message(RTM_NEWADDR, 1, &first);
    datagram.extend(message(RTM_NEWADDR, 7, &address(4, 8, 0, &[]))); // another request's
    datagram.extend(message(RTM_NEWROUTE, 1, &route(0, 3, 0, 1, 0, &[]))); // another kind
    datagram.extend(message(NLMSG_ERROR, 1, &0i32.to_ne_bytes())); // an acknowledgement
    let mut payloads = Vec::new();
    assert_eq!(
        messages(&datagram, 1, RTM_NEWADDR, &mut payloads),
        Ok(false)
    );
    assert_eq!(payloads, std::slice::from_ref(&first));
    let done = message(NLMSG_DONE, 1, &0i32.to_ne_bytes());
    assert_eq!(messages(&done, 1, RTM_NEWADDR, &mut payloads), Ok(true));
    let error = message(NLMSG_ERROR, 1, &(-libc::EBUSY).to_ne_bytes());
    assert_eq!(
        messages(&error, 1, RTM_NEWADDR, &mut payloads),
        Err(DumpError::Failed)
    );
    let mut short = message(RTM_NEWADDR, 1, &first);
    short.truncate(20);
    assert_eq!(
        messages(&short, 1, RTM_NEWADDR, &mut payloads),
        Err(DumpError::Failed)
    );
}

#[test]
fn addresses_are_printed_as_ip_lists_them() {
    let label = |text: &str| rtattr(IFA_LABEL, format!("{text}\0").as_bytes());
    let dump = vec![
        address(
            1,
            8,
            254,
            &[ip_attr(IFA_LOCAL, [127, 0, 0, 1]), label("lo")],
        ),
        address(
            3,
            24,
            0,
            &[
                ip_attr(IFA_ADDRESS, [192, 168, 1, 40]),
                ip_attr(IFA_LOCAL, [192, 168, 1, 40]),
                label("wlan0"),
            ],
        ),
        // A secondary address without IFA_LOCAL: ip prints IFA_ADDRESS as local.
        address(
            3,
            16,
            253,
            &[ip_attr(IFA_ADDRESS, [169, 254, 7, 9]), label("wlan0:1")],
        ),
    ];
    let printed = json!(addresses("wlan0", 3, dump.clone()));
    assert_eq!(
        printed.to_string(),
        r#"[{"ifindex":3,"ifname":"wlan0","addr_info":[{"family":"inet","local":"192.168.1.40","prefixlen":24,"scope":"global","label":"wlan0"},{"family":"inet","local":"169.254.7.9","prefixlen":16,"scope":"link","label":"wlan0:1"}]}]"#
    );
    // No address on the interface: no link listed (ip's `[{}]` or `[]` read the same).
    assert!(addresses("usb0", 9, dump).is_empty());
}

#[test]
fn usb0_owner_address_reads_as_ip_printed_it() {
    let dump = vec![address(
        6,
        24,
        0,
        &[
            ip_attr(IFA_ADDRESS, [10, 42, 0, 1]),
            ip_attr(IFA_LOCAL, [10, 42, 0, 1]),
        ],
    )];
    assert_eq!(
        json!(addresses("usb0", 6, dump)),
        json!([{"ifindex": 6, "ifname": "usb0",
                "addr_info": [{"family": "inet", "local": "10.42.0.1", "prefixlen": 24, "scope": "global"}]}])
    );
}

#[test]
fn the_default_route_is_printed_as_ip_prints_it() {
    let wlan0 = u32_attr(RTA_OIF, 3);
    let main = u32_attr(RTA_TABLE, u32::from(RT_TABLE_MAIN));
    // dhcp-event's `ip -4 route replace default via 192.168.123.1 dev wlan0 metric 100` (protocol
    // boot, scope universe): the record of the owner's player (platform-v1 Wi-Fi check, 05-wifi).
    let dhcp_event = route(
        0,
        RTPROT_BOOT,
        0,
        RTN_UNICAST,
        0,
        &[
            main.clone(),
            u32_attr(RTA_PRIORITY, 100),
            ip_attr(RTA_GATEWAY, [192, 168, 123, 1]),
            wlan0.clone(),
        ],
    );
    assert_eq!(
        json!(routes(3, vec![dhcp_event.clone()])).to_string(),
        r#"[{"dst":"default","gateway":"192.168.123.1","metric":100,"flags":[]}]"#
    );
    // Carrier lost before the lease was taken down.
    let linkdown = route(
        0,
        RTPROT_BOOT,
        0,
        RTN_UNICAST,
        16,
        &[
            main.clone(),
            ip_attr(RTA_GATEWAY, [192, 168, 123, 1]),
            wlan0.clone(),
        ],
    );
    assert_eq!(
        json!(routes(3, vec![linkdown])).to_string(),
        r#"[{"dst":"default","gateway":"192.168.123.1","flags":["linkdown"]}]"#
    );
    // A DHCP client's route with a preferred source (as a desktop's NetworkManager adds it).
    let desktop = route(
        0,
        16,
        0,
        RTN_UNICAST,
        0,
        &[
            main.clone(),
            u32_attr(RTA_PRIORITY, 100),
            ip_attr(RTA_PREFSRC, [192, 168, 123, 204]),
            ip_attr(RTA_GATEWAY, [192, 168, 123, 1]),
            wlan0.clone(),
        ],
    );
    assert_eq!(
        json!(routes(3, vec![desktop])).to_string(),
        r#"[{"dst":"default","gateway":"192.168.123.1","protocol":"dhcp","prefsrc":"192.168.123.204","metric":100,"flags":[]}]"#
    );
    // Not listed: the subnet route, another interface's default, another table's, a cloned one, a
    // multipath route (no RTA_OIF).
    let subnet = route(
        24,
        2,
        253,
        RTN_UNICAST,
        0,
        &[
            main.clone(),
            ip_attr(RTA_DST, [192, 168, 123, 0]),
            wlan0.clone(),
        ],
    );
    let usb0 = route(
        0,
        RTPROT_BOOT,
        0,
        RTN_UNICAST,
        0,
        &[main.clone(), u32_attr(RTA_OIF, 6)],
    );
    let other_table = route(
        0,
        RTPROT_BOOT,
        0,
        RTN_UNICAST,
        0,
        &[u32_attr(RTA_TABLE, 100), wlan0.clone()],
    );
    let cloned = route(
        0,
        RTPROT_BOOT,
        0,
        RTN_UNICAST,
        RTM_F_CLONED,
        &[main.clone(), wlan0.clone()],
    );
    let multipath = route(
        0,
        RTPROT_BOOT,
        0,
        RTN_UNICAST,
        0,
        std::slice::from_ref(&main),
    );
    assert!(routes(3, vec![subnet, usb0, other_table, cloned, multipath]).is_empty());
    // An unusual type and scope print as ip names them.
    let odd = route(0, 99, 200, 7, 4, &[main, wlan0]);
    assert_eq!(
        json!(routes(3, vec![odd, dhcp_event])).to_string(),
        r#"[{"type":"unreachable","dst":"default","protocol":"openr","scope":"site","flags":["onlink"]},{"dst":"default","gateway":"192.168.123.1","metric":100,"flags":[]}]"#
    );
}

/// The direct reads against this host's own `ip`, interface by interface (run with
/// `cargo test -- --ignored`: it reads the host's live network state).
#[test]
#[ignore]
fn direct_reads_equal_ip_on_this_host() {
    /// An address as compared: link name, local, prefix length, scope.
    type Shown = (Option<String>, Option<String>, Option<u8>, Option<String>);
    let root = Root::new("/");
    let ip = |argv: &[&str]| -> Value {
        let answer = y2p_core::command::run(&root, argv, 1.0);
        serde_json::from_str(answer.output.as_deref().unwrap_or("[]")).unwrap_or(json!([]))
    };
    for entry in std::fs::read_dir("/sys/class/net").unwrap() {
        let dev = entry.unwrap().file_name().into_string().unwrap();
        let routes: Vec<Route> = serde_json::from_value(ip(&[
            "ip", "-j", "-4", "route", "show", "default", "dev", &dev,
        ]))
        .unwrap();
        assert_eq!(default_route(&root, &dev, 1.0), Ok(routes), "{dev}: route");
        let shown = |links: Vec<Link>| -> Vec<Shown> {
            links
                .into_iter()
                .flat_map(|link| {
                    let name = link.ifname.clone();
                    link.addr_info
                        .into_iter()
                        .map(move |a| (name.clone(), a.local, a.prefixlen, a.scope))
                })
                .collect()
        };
        let links: Vec<Link> =
            serde_json::from_value(ip(&["ip", "-j", "-4", "addr", "show", "dev", &dev])).unwrap();
        assert_eq!(
            ipv4_addresses(&root, &dev, 1.0).map(shown),
            Ok(shown(links)),
            "{dev}: addresses"
        );
    }
    assert_eq!(
        default_route(&root, "y2-no-such-dev", 1.0),
        Err("command_failed".into())
    );
    assert_eq!(
        ipv4_addresses(&root, "y2-no-such-dev", 1.0),
        Err("command_failed".into())
    );
}

/// The generic netlink family lookup as genetlink expects it: a request header, the controller's
/// command 3 (CTRL_CMD_GETFAMILY) version 1, then CTRL_ATTR_FAMILY_NAME "nl80211\0" padded to 4.
#[test]
fn the_nl80211_family_lookup_is_laid_out_as_genetlink_expects() {
    let request = genl_request(16, 3, 7, &[(2, b"nl80211\0")]);
    let mut expected = Vec::new();
    expected.extend_from_slice(&32u32.to_ne_bytes());
    expected.extend_from_slice(&16u16.to_ne_bytes());
    expected.extend_from_slice(&1u16.to_ne_bytes()); // NLM_F_REQUEST
    expected.extend_from_slice(&7u32.to_ne_bytes());
    expected.extend_from_slice(&0u32.to_ne_bytes());
    expected.extend_from_slice(&[3, 1, 0, 0]);
    expected.extend_from_slice(&12u16.to_ne_bytes());
    expected.extend_from_slice(&2u16.to_ne_bytes());
    expected.extend_from_slice(b"nl80211\0");
    assert_eq!(request, expected);
    // A 4-byte attribute (the interface index) needs no padding; a 5-byte one is padded to 8.
    assert_eq!(
        genl_request(28, 62, 1, &[(3, &5u32.to_ne_bytes())]).len(),
        16 + 4 + 8
    );
    assert_eq!(genl_request(28, 62, 1, &[(3, b"abcde")]).len(), 16 + 4 + 12);
}

#[test]
fn link_counters_are_the_statistics_files_by_name() {
    // ifinfomsg (AF_UNSPEC, type, index 3, flags, change), then IFLA_STATS64 with counter i = i + 1
    // and rx_otherhost_dropped, which has no file.
    let mut body = vec![0u8, 0, 1, 0];
    body.extend_from_slice(&3u32.to_ne_bytes());
    body.extend_from_slice(&[0; 8]);
    let counters: Vec<u8> = (1..=25u64).flat_map(u64::to_ne_bytes).collect();
    body.extend(rtattr(3, b"wlan0\0"));
    body.extend(rtattr(IFLA_STATS64, &counters));
    let found = statistics_of(&body).unwrap();
    assert_eq!(found.len(), 24);
    assert_eq!(found.first(), Some(&("collisions", 10)));
    assert_eq!(found.last(), Some(&("tx_window_errors", 21)));
    assert!(found.contains(&("rx_packets", 1)) && found.contains(&("rx_nohandler", 24)));
    assert!(found.windows(2).all(|pair| pair[0].0 < pair[1].0));
    // Too short a block: no counters (the caller reads the files).
    let mut short = body[..IFINFOMSG].to_vec();
    short.extend(rtattr(IFLA_STATS64, &counters[..80]));
    assert_eq!(statistics_of(&short), None);
}

#[test]
fn the_players_link_counters_need_the_interface() {
    assert_eq!(link_statistics("y2-no-such-link", 1.0), Some(Vec::new()));
}

#[test]
fn the_kernels_link_counters_are_its_statistics_files() {
    let files = Path::new("/sys/class/net/lo/statistics");
    let Some(counters) = link_statistics("lo", 2.0).filter(|_| files.is_dir()) else {
        return;
    };
    assert_eq!(counters.len(), STATS64.len());
    for (name, value) in counters {
        assert!(files.join(name).is_file(), "{name}");
        if name.ends_with("_errors") {
            let text = std::fs::read_to_string(files.join(name)).unwrap();
            assert_eq!(text.trim().parse::<u64>().unwrap(), value, "{name}");
        }
    }
}
