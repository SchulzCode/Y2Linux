//! The ssh daemon's tests: prerequisites, the passive address events with the signal wakeup pipe,
//! and the Ready heartbeat's address refresh.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::testing::{ok, Fixture};
use serde_json::json;
use std::sync::Arc;

const IP: [&str; 7] = ["ip", "-j", "-4", "addr", "show", "dev", "usb0"];

fn interface(name: &str) -> String {
    json!([{"ifname": name, "addr_info": [{"local": "10.42.0.1", "prefixlen": 24}]}]).to_string()
}

/// Two connected non-blocking ends of a datagram socket pair.
fn socket_pair() -> (OwnedFd, OwnedFd) {
    let mut fds = [-1; 2];
    // SAFETY: a two-element array for the new descriptors, owned right after.
    unsafe {
        assert_eq!(
            libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_DGRAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
                0,
                fds.as_mut_ptr()
            ),
            0
        );
        (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1]))
    }
}

fn pipe() -> (OwnedFd, OwnedFd) {
    let mut fds = [-1; 2];
    // SAFETY: as socket_pair.
    unsafe {
        assert_eq!(
            libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC),
            0
        );
        (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1]))
    }
}

fn send(fd: &OwnedFd, data: &[u8]) {
    // SAFETY: the slice is readable for its length.
    let sent = unsafe { libc::write(fd.as_raw_fd(), data.as_ptr().cast(), data.len()) };
    assert_eq!(sent, data.len() as isize);
}

#[test]
fn ssh_waits_for_crng_and_usb_not_only_a_running_daemon() {
    // The host's CRNG is initialised (the GRND_NONBLOCK read that fails before is not injectable).
    assert!(py::entropy_ready());
    // Every command answered by the root's own runner (unanswered: `fixture`).
    let fixture = Fixture::new().with_answers(Vec::new());
    let root = &fixture.root;
    fixture.put("/root/.ssh/authorized_keys", "fixture-public-key");
    let answering = |ifname: &str| root.clone().with_answers(vec![ok(&IP, &interface(ifname))]);
    let address = usb_address(root);
    assert_eq!(
        prerequisites(root, false, address).1,
        "kernel_crng_not_ready"
    );
    let address = usb_address(&answering("wlan0"));
    assert_eq!(
        prerequisites(root, true, address).1,
        "usb_address_not_ready"
    );
    fixture.put("/data/.y2data-schema", "1");
    let address = usb_address(&answering("usb0"));
    assert_eq!(prerequisites(root, true, address).0, "Ready");
}

#[test]
fn ssh_passive_address_events_and_signal_fd_wake() {
    let (left, right) = socket_pair();
    let mut events = AddressEvents { sock: Some(left) };
    let (rd, wr) = pipe();
    send(&right, b"address-change");
    assert!(events.wait(1.0, rd.as_raw_fd()));
    send(&wr, b"signal");
    assert!(!events.wait(1.0, rd.as_raw_fd()));
    let mut byte = 0u8;
    // SAFETY: one writable byte; the pipe is non-blocking.
    let read = unsafe { libc::read(rd.as_raw_fd(), (&mut byte as *mut u8).cast(), 1) };
    assert_eq!(read, -1);
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::EAGAIN)
    );
}

/// The listener already runs (a stand-in child; dropbear is not on the host) and the loop's ticks are
/// driven directly on the root's clock: once usb0 is ready its address is read once and next only
/// after 60 s, and a Ready daemon waits 5 s.
#[test]
fn ssh_ready_heartbeat_does_not_fork_ip_each_second() {
    let clock = Arc::new(y2p_core::clock::Manual::at(
        1_000_000_000_000,
        1_000_000_000_000,
    ));
    let fixture = Fixture::new()
        .with_answers(vec![ok(&IP, &interface("usb0"))])
        .with_clock(clock.clone());
    fixture
        .put("/root/.ssh/authorized_keys", "owner")
        .put("/data/.y2data-schema", "1")
        .put(
            "/proc/net/tcp",
            "header\n0: 01002A0A:0016 00000000:0000 0A\n",
        )
        .mkdir("/run/y2");
    let (sock, _peer) = socket_pair();
    let listener = Command::new("/bin/sh")
        .args(["-c", "exec sleep 30"])
        .spawn()
        .unwrap();
    let mut daemon = Daemon::new(&fixture.root, AddressEvents { sock: Some(sock) });
    daemon.child = Some(listener);
    let mut waits = Vec::new();
    for _ in 0..5 {
        let ready = daemon.tick().unwrap();
        waits.push(if ready == "Ready" { 5.0 } else { 1.0 });
        daemon.changed = false; // no address event during the wait
        clock.advance(5_000_000_000);
    }
    assert_eq!(fixture.calls().len(), 1);
    assert_eq!(waits, [5.0; 5]);
    assert_eq!(
        fixture.json("/run/y2/ssh.json")["reason"],
        "usb_bound_key_only_listener"
    );
    assert_eq!(fixture.json("/run/y2/ssh.json")["monotonic_s"], 1020.0);
    // 60 s after the first read the address is read again; an address event reads it at once.
    clock.advance(35_000_000_000);
    daemon.tick().unwrap();
    assert_eq!(fixture.calls().len(), 2);
    daemon.changed = true;
    daemon.tick().unwrap();
    assert_eq!(fixture.calls().len(), 3);
    let mut listener = daemon.child.take().unwrap();
    listener.kill().unwrap();
    listener.wait().unwrap();
}
