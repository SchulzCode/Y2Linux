//! The RTC alarm (`rtc-alarm`). The other clock commands are tested on the command line
//! (tests/connectivity.rs).
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::testing::Fixture;
use std::{ffi::CString, fs, os::unix::ffi::OsStrExt};

fn armed(fixture: &Fixture, seconds: i64) -> Value {
    serde_json::from_str(&alarm(&fixture.root, seconds).unwrap().stdout).unwrap()
}

#[test]
fn alarm_replacement_cancel_and_failed_readback() {
    let fixture = Fixture::new();
    fixture
        .put("/sys/class/rtc/rtc0/since_epoch", "1790000000")
        .put("/sys/class/rtc/rtc0/wakealarm", "1");
    assert_eq!(armed(&fixture, 60)["alarm_epoch"], 1790000060);
    assert_eq!(armed(&fixture, 0)["state"], "Disabled");
    assert!(matches!(
        alarm(&fixture.root, 604801),
        Err(Failure::Value(_))
    ));
    // A driver that reads back another alarm than the one written: wakealarm is a FIFO this thread
    // plays the driver on. It takes the disable and the alarm (one reader across both writes), then
    // answers the readback with 1 and takes what comes after.
    let path = fixture.path("/sys/class/rtc/rtc0/wakealarm");
    fs::remove_file(&path).unwrap();
    let name = CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: mkfifo with a valid NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let driver = std::thread::spawn(move || {
        let mut armed = String::new();
        let mut device = fs::File::open(&path).unwrap();
        while !armed.ends_with("1790000060\n") {
            let mut chunk = [0u8; 64];
            match std::io::Read::read(&mut device, &mut chunk).unwrap() {
                // No writer between the two writes.
                0 => std::thread::sleep(std::time::Duration::from_millis(1)),
                n => armed.push_str(std::str::from_utf8(&chunk[..n]).unwrap()),
            }
        }
        drop(device);
        fs::write(&path, "1\n").unwrap();
        (armed, fs::read_to_string(&path).unwrap())
    });
    let refused = alarm(&fixture.root, 60).err().expect("refused");
    assert_eq!(refused.message(), "rtc_alarm_readback_mismatch");
    let (armed, after) = driver.join().unwrap();
    assert_eq!(armed, "0\n1790000060\n");
    // Disabled again after the mismatch.
    assert_eq!(after, "0\n");
}
