//! One wait for supplicant events and the service's signals (tests/test_platform_integration.py:
//! ReadinessWake): SIGUSR1 wakes the loop, SIGTERM ends it, a supplicant packet wakes it without being
//! lost, and the previous handlers come back.
//!
//! The signals go to this test process: its handlers are this module's while SignalWake lives, and the
//! two tests take turns.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use std::{
    os::unix::net::UnixDatagram,
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

static SIGNALS: Mutex<()> = Mutex::new(());

fn handler(signal: libc::c_int) -> usize {
    // SAFETY: a zeroed sigaction is a valid output buffer; no action is changed.
    unsafe {
        let mut current: libc::sigaction = std::mem::zeroed();
        libc::sigaction(signal, std::ptr::null(), &mut current);
        current.sa_sigaction
    }
}

fn handlers() -> Vec<usize> {
    [libc::SIGUSR1, libc::SIGTERM, libc::SIGINT]
        .into_iter()
        .map(handler)
        .collect()
}

/// `os.kill(os.getpid(), signal)`, only while this module's handler takes it.
fn send(signal: libc::c_int) {
    assert_eq!(handler(signal), on_signal as *const () as usize);
    // SAFETY: a signal to this process, whose handler only touches atomics and the wake pipe.
    unsafe { libc::kill(libc::getpid(), signal) };
}

#[test]
fn control_without_supplicant_and_stop_share_one_wait() {
    let _signals = crate::testing::process_signals();
    let _turn = SIGNALS.lock().unwrap_or_else(|e| e.into_inner());
    RUNNING.store(true, Ordering::SeqCst);
    let old = handlers();
    let mut monitor = EventMonitor::new(&Root::new("/nonexistent"));
    let wake = SignalWake::new().unwrap();
    send(libc::SIGUSR1);
    let start = Instant::now();
    monitor.wait(2.0, Some(wake.fd()));
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(RUNNING.load(Ordering::SeqCst));
    wake.drain();
    let start = Instant::now();
    monitor.wait(0.03, Some(wake.fd()));
    assert!(start.elapsed() >= Duration::from_millis(25));
    let timer = thread::spawn(|| {
        thread::sleep(Duration::from_millis(20));
        send(libc::SIGTERM);
    });
    monitor.wait(2.0, Some(wake.fd()));
    timer.join().unwrap();
    assert!(!RUNNING.load(Ordering::SeqCst));
    drop(wake);
    RUNNING.store(true, Ordering::SeqCst);
    assert_eq!(handlers(), old);
}

#[test]
fn supplicant_and_control_each_wake_without_losing_packets() {
    let _signals = crate::testing::process_signals();
    let _turn = SIGNALS.lock().unwrap_or_else(|e| e.into_inner());
    let mut monitor = EventMonitor::new(&Root::new("/nonexistent"));
    let (socket, sender) = UnixDatagram::pair().unwrap();
    monitor.sock = Some(socket);
    let wake = SignalWake::new().unwrap();
    let timer = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        sender.send(b"link").unwrap();
        sender
    });
    let start = Instant::now();
    monitor.wait(2.0, Some(wake.fd()));
    let sender = timer.join().unwrap();
    assert!(start.elapsed() < Duration::from_secs(1));
    let socket = monitor.sock.as_ref().unwrap();
    socket.set_nonblocking(true).unwrap();
    let mut packet = [0u8; 16];
    let n = socket.recv(&mut packet).unwrap();
    assert_eq!(&packet[..n], b"link");
    // The second wake must work when the supplicant socket is idle.
    send(libc::SIGUSR1);
    let start = Instant::now();
    monitor.wait(2.0, Some(wake.fd()));
    assert!(start.elapsed() < Duration::from_secs(1));
    drop(wake);
    monitor.close();
    drop(sender);
}
