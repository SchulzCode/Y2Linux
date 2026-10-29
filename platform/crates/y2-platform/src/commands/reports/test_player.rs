//! A synthetic player with a secret in every place one could be (the error-report tests: unit tests
//! of `reports` and tests/reports.rs, which includes this file by path). Only std and the crate's
//! dependencies: no item of this crate.
// SPDX-License-Identifier: GPL-2.0-only
#![allow(dead_code)] // each test module uses part of it
use serde_json::json;
use std::{collections::BTreeMap, fs, io::Read, path::Path};

pub const SSID: &str = "CanaryNet Home";
pub const PSK: &str = "canary-pass-9876";
pub const BT_NAME: &str = "Canary Buds Pro";
pub const BT_MAC: &str = "14:14:7D:AB:CD:EF";
pub const LASTFM_USER: &str = "canaryuser42";
pub const LASTFM_KEY: &str = "c4n4rys3ss10nk3y";
pub const SSH_BLOB: &str = "AAAACcanaryKEYblob0123";
pub const SSH_COMMENT: &str = "canary@laptop";
pub const CID: &str = "150100464e58324d4206ec82a80434f9";
pub const MACHINE_ID: &str = "6f1d2c3b4a5e6f708192a3b4c5d6e7f8";
pub const CPU_SERIAL: &str = "00000000deadbeef";
pub const TITLE: &str = "Canary Song Title";
pub const ARTIST: &str = "Canary Band";
pub const ZONE: &str = "Europe/Berlin";
pub const PRIVATE_IP: &str = "192.168.123.45";
pub const PUBLIC_IP: &str = "93.184.216.34";
pub const EMAIL: &str = "canary@example.org";
pub const BSSID: &str = "3C:22:FB:9A:BC:DE";
/// Every planted secret, as it may appear (MACs also in BlueZ's underscore form).
pub const CANARIES: [&str; 23] = [
    SSID,
    PSK,
    BT_NAME,
    "AB:CD:EF",
    "AB_CD_EF",
    LASTFM_USER,
    LASTFM_KEY,
    SSH_BLOB,
    SSH_COMMENT,
    CID,
    MACHINE_ID,
    "deadbeef",
    TITLE,
    ARTIST,
    ZONE,
    "123.45",
    PUBLIC_IP,
    EMAIL,
    "9A:BC:DE",
    "Canary Band/",
    "canary-station-token",
    "NEVER-canary-value",
    "C4A2-77E1",
];

/// Tokens without their per-report hash: `<ssid#3fa2c1>` -> `<ssid#>`.
pub fn plain(text: &str) -> String {
    regex::Regex::new("#[0-9a-f]{6}>")
        .unwrap()
        .replace_all(text, "#>")
        .into_owned()
}

/// Write `data` at the device path `name` below `base` (parents made).
pub fn write(base: &Path, name: &str, data: impl AsRef<[u8]>) {
    let target = base.join(name.trim_start_matches('/'));
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(target, data).unwrap();
}

/// A tar (gzip-compressed when asked) of (name, body) members.
pub fn archive(items: &[(&str, &[u8])], compressed: bool) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (name, body) in items {
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(1_761_000_000);
        header.set_cksum();
        builder.append_data(&mut header, name, *body).unwrap();
    }
    let data = builder.into_inner().unwrap();
    if !compressed {
        return data;
    }
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut encoder, &data).unwrap();
    encoder.finish().unwrap()
}

pub fn player(base: &Path) {
    write(
        base,
        "/data/network/wpa_supplicant.conf",
        format!("ctrl_interface=/run/wpa\nnetwork={{\n ssid=\"{SSID}\"\n psk=\"{PSK}\"\n bssid={BSSID}\n}}\n"),
    );
    write(
        base,
        &format!("/data/bluetooth/bluez/A2:5F:D7:57:80:23/{BT_MAC}/info"),
        format!("[General]\nName={BT_NAME}\nAlias={BT_NAME}\n"),
    );
    write(
        base,
        "/data/bluetooth/codec-history.json",
        json!({"peers": {BT_MAC: {"codec": "AAC"}}}).to_string(),
    );
    write(
        base,
        "/data/reborn/state/lastfm.json",
        json!({"user": LASTFM_USER, "session_key": LASTFM_KEY}).to_string(),
    );
    write(
        base,
        "/root/.ssh/authorized_keys",
        format!("ssh-ed25519 {SSH_BLOB} {SSH_COMMENT}\nssh-ed25519 {SSH_BLOB} Y2Linux\n"),
    );
    let card = "/sys/class/mmc_host/mmc0/mmc0:0001/";
    for (name, value) in [
        ("cid", CID),
        ("serial", "0xec82a804"),
        ("name", "FNX2MB"),
        ("manfid", "0x000015"),
        ("date", "03/2017"),
        ("prv", "0x6"),
        ("fwrev", "0x0600000000000000"),
    ] {
        write(base, &format!("{card}{name}"), format!("{value}\n"));
    }
    write(
        base,
        "/data/system/dbus/machine-id",
        format!("{MACHINE_ID}\n"),
    );
    write(
        base,
        "/proc/cpuinfo",
        format!("processor\t: 0\nCPU part\t: 0xc07\nSerial\t\t: {CPU_SERIAL}\n"),
    );
    write(base, "/proc/meminfo", "MemTotal:         950636 kB\n");
    write(base, "/proc/sys/kernel/random/boot_id", "boot-1\n");
    write(base, "/proc/sys/kernel/tainted", "0\n");
    write(base, "/proc/uptime", "123.4 400.0\n");
    write(
        base,
        "/sys/firmware/y2_boot/lcm",
        "lcm=0-st7701_hvga_dsi_vdo_boe\n",
    );
    write(base, "/sys/module/panel/parameters/panel_kind", "2\n");
    let path = format!("/media/internal/music/{ARTIST}/{TITLE}.flac");
    let mut queue = vec![
        json!({"id": 7, "title": TITLE, "artist": ARTIST, "album": "Canary Album",
        "path": path, "filename": format!("{TITLE}.flac"), "codec": "flac", "sample_rate": 96000,
        "duration_ms": 1000}),
    ];
    queue.extend((0..3000).map(
        |n| json!({"id": n, "title": format!("Filler {n}"), "path": format!("/media/sd/f{n}.mp3")}),
    ));
    write(
        base,
        "/data/reborn/state/session.json",
        json!({"schema": 2, "model": {"queue": queue, "queue_position": 0, "position_ms": 500,
            "playback": "paused", "output": {"kind": "wired"}, "settings": {"volume": 47,
            "hp_level": "low", "time_zone": ZONE, "music_directory": "/media/internal/music"}}})
        .to_string(),
    );
    let events = [
        json!({"subsystem": "alsa", "event": "prepared", "fields": {"device": format!("bluealsa:DEV={BT_MAC},PROFILE=a2dp")}}),
        json!({"subsystem": "wifi", "event": "connected", "message": format!("joined {SSID} from {PRIVATE_IP}"), "fields": {"ssid": SSID}}),
        json!({"subsystem": "lastfm", "event": "sent", "fields": {"url": format!("https://ws.audioscrobbler.com/2.0/?sk={LASTFM_KEY}&user={LASTFM_USER}")}}),
        json!({"subsystem": "scanner", "event": "unreadable", "message": format!("failed to open {path}: EIO")}),
        json!({"subsystem": "core", "event": "mail", "message": format!("contact {EMAIL} via {PUBLIC_IP}, usb 10.42.0.1")}),
        json!({"subsystem": "radio", "event": "stream", "message": "opened https://stream.example.net/live/canary-station-token"}),
    ];
    let lines: Vec<String> = events.iter().map(|e| e.to_string()).collect();
    write(
        base,
        "/data/reborn/logs/reborn-current.jsonl",
        lines.join("\n") + "\n",
    );
    write(
        base,
        "/data/reborn/logs/panic-last.json",
        json!({"message": format!("index out of bounds playing {TITLE}"),
            "location": "app/reborn/src/playback.rs:1200"})
        .to_string(),
    );
    write(
        base,
        "/data/logs/system.log",
        format!(
            "kernel: mmc0: new HS200 card cid {CID}\nkernel: wlan0: associate with {BSSID} (SSID='{SSID}')\n\
             kernel: bt peer dev_{} connected\nkernel: reborn_audio::native ok at 20:39:14, kernel 6.18.56\n\
             kernel: Y2Linux stays Y2Linux\n",
            BT_MAC.replace(':', "_")
        ),
    );
    write(
        base,
        "/sys/fs/pstore/console-ramoops-0",
        "<0>Kernel panic - not syncing: Attempted to kill init!\n",
    );
    // A file a report glob matches but must never read (the NEVER list).
    write(
        base,
        "/run/y2/wpa_supplicant-debug.log",
        "NEVER-canary-value\n",
    );
    write(
        base,
        "/run/y2/media.json",
        json!({"sources": [{"kind": "sd_card", "uuid": "C4A2-77E1", "mount": "/media/sd"}]})
            .to_string(),
    );
    write(
        base,
        "/data/reborn/logs/splash-boot.jsonl",
        json!({"event": "source", "fields": {"source": {"kind": "sd_card", "id": "C4A2-77E1"}}})
            .to_string()
            + "\n",
    );
    write(
        base,
        "/run/y2/network.json",
        json!({"ssid": SSID, "ip": PRIVATE_IP, "gateway": "192.168.123.1"}).to_string(),
    );
    write(
        base,
        "/data/updates/state.json",
        json!({"state": "Acknowledged", "sequence": 6900}).to_string(),
    );
    write(
        base,
        "/data/system/platform/previous-boot-evidence.json",
        json!({"previous_orderly_shutdown": true, "previous_boot_id": "boot-0", "reset_cause": null})
            .to_string(),
    );
    // A retained boot archive and a Reborn diagnostic bundle, both holding a secret.
    let body = json!({"wifi": {"ssid": SSID}, "note": format!("peer {BT_NAME}")}).to_string();
    for (name, compressed) in [
        ("/data/system/platform/retained/boot-x.tar", false),
        (
            "/data/reborn/diagnostics/reborn-diagnostic-1-2.tar.gz",
            true,
        ),
    ] {
        write(
            base,
            name,
            archive(&[("diagnostic.json", body.as_bytes())], compressed),
        );
    }
    let db = base.join("data/reborn/library.db");
    let connection = y2p_core::reports_sqlite::Database::open(&db.to_string_lossy(), 1000).unwrap();
    connection
        .query("create table tracks (id integer, title text, artist text, path text, codec text, sample_rate integer)")
        .unwrap();
    connection
        .query(&format!(
            "insert into tracks values (1, '{TITLE}', '{ARTIST}', '{path}', 'flac', 96000)"
        ))
        .unwrap();
}

/// `time` says `tls_ready` (the Python tests patched timekeeping.status): an NTP anchor of this boot
/// made now and a build floor a day back.
pub fn clock_ready(base: &Path) {
    clock_ready_on(base, &y2p_core::clock::Kernel);
}

/// `clock_ready` for a root reading `clock` (`time` compares the anchor with its monotonic and
/// boottime readings, and with the kernel's wall clock).
pub fn clock_ready_on(base: &Path, clock: &dyn y2p_core::clock::Clock) {
    let wall = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    write(
        base,
        "/etc/y2linux/build-epoch",
        format!("{}\n", wall as i64 - 86_400),
    );
    write(
        base,
        "/run/y2/time.json",
        json!({"schema": 1, "boot_id": "boot-1", "epoch": wall,
            "monotonic_s": clock.monotonic(),
            "boottime_s": clock.boottime_ns() as f64 / 1e9, "source": "ntp"})
        .to_string(),
    );
}

pub fn online(base: &Path, state: &str, boot: &str) {
    write(
        base,
        "/run/y2/network.json",
        json!({"state": state, "boot_id": boot}).to_string(),
    );
}

pub fn battery(base: &Path, percent: i64, charging: bool) {
    write(
        base,
        "/run/y2/power.json",
        json!({"boot_id": "boot-1", "battery": {"percent": percent, "charging": charging,
            "status": if charging { "Charging" } else { "Discharging" }}})
        .to_string(),
    );
}

/// `/proc/self/mountinfo` with `paths` mounted (vfat, `options`), each made a directory.
pub fn mount(base: &Path, paths: &[&str], options: &str) {
    let mut lines = vec!["22 1 179:2 / / ro,relatime - ext4 /dev/root ro".to_owned()];
    for (n, path) in paths.iter().enumerate() {
        fs::create_dir_all(base.join(path.trim_start_matches('/'))).unwrap();
        lines.push(format!(
            "{} 22 179:{} / {path} {options} - vfat /dev/mmcblk1p1 rw",
            30 + n,
            8 + n
        ));
    }
    write(base, "/proc/self/mountinfo", lines.join("\n") + "\n");
}

/// What the player's tools answer (`runner` of the Python tests): dmesg and every rebornctl
/// command a report or a check runs, each naming a secret.
pub fn answers() -> Vec<(Vec<&'static str>, String)> {
    let dmesg = format!(
        "[    1.0] wlan0: authenticated with {BSSID} SSID {SSID}\n[    2.0] mtk-dsi: handoff ok\n"
    );
    let reborn =
        json!({"bluetooth": {"devices": [{"name": BT_NAME, "address": BT_MAC, "connected": true}]},
        "wifi": {"ssid": SSID, "ip": PRIVATE_IP}, "subsystems": {}})
        .to_string();
    let mut answers = vec![(vec!["dmesg"], dmesg)];
    for argv in [
        &["rebornctl", "status", "--json"][..],
        &["rebornctl", "health", "--json"],
        &["rebornctl", "metrics", "--json"],
        &["rebornctl", "audio", "--json"],
        &["rebornctl", "snapshot", "--json"],
        &["rebornctl", "events", "--last", "500", "--json"],
    ] {
        answers.push((argv.to_vec(), reborn.clone()));
    }
    answers
}

/// Every member of a report (tar.gz), and of the archives inside it as `outer!inner`.
pub fn members(data: &[u8]) -> BTreeMap<String, Vec<u8>> {
    fn read(data: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        for entry in tar::Archive::new(data).entries().unwrap() {
            let mut entry = entry.unwrap();
            let name = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
            let mut body = Vec::new();
            entry.read_to_end(&mut body).unwrap();
            out.push((name, body));
        }
        out
    }
    let mut raw = Vec::new();
    flate2::read::GzDecoder::new(data)
        .read_to_end(&mut raw)
        .unwrap();
    let mut found = BTreeMap::new();
    for (name, body) in read(&raw) {
        if name.ends_with(".tar") || name.ends_with(".tar.gz") {
            let inner = if body.starts_with(b"\x1f\x8b") {
                let mut out = Vec::new();
                flate2::read::GzDecoder::new(&body[..])
                    .read_to_end(&mut out)
                    .unwrap();
                out
            } else {
                body.clone()
            };
            for (item, data) in read(&inner) {
                found.insert(format!("{name}!{item}"), data);
            }
        }
        found.insert(name, body);
    }
    found
}

/// No canary in any member's name or body.
pub fn assert_no_canary(members: &BTreeMap<String, Vec<u8>>) {
    for (name, body) in members {
        let text = String::from_utf8_lossy(body);
        for canary in CANARIES {
            assert!(!text.contains(canary), "{canary} in {name}");
            assert!(!name.contains(canary), "{canary} in the name {name}");
        }
    }
}
