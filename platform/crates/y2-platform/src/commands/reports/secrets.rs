//! This player's private strings by class, read from where they are
//! stored (Wi-Fi networks, Bluetooth names, the Last.fm account, SSH keys, serial numbers, volume
//! serials, the songs in the queue).
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    build::{large_json, rel},
    json_at, json_dict,
    pyjson::Py,
    pyre::{self, lazy, S},
    scrub::Known,
};
use crate::reply::Failure;
use regex::Regex;
use std::{collections::BTreeSet, sync::OnceLock};
use y2p_core::{root::python_strip, Root};

/// Never a secret, even when a key comment or account field holds one of them.
const COMMON_WORDS: [&str; 9] = [
    "Y2Linux",
    "y2linux",
    "Reborn",
    "reborn",
    "root",
    "localhost",
    "true",
    "false",
    "null",
];
/// F41: a song called "Ready" or "1.0" also replaced Reborn's shutdown answer and the release inside
/// build IDs. Library names without a letter and the state words reports are read by stay; network,
/// device and account names do not.
const MUSIC_KEPT: [&str; 21] = [
    "Ready",
    "Failed",
    "Normal",
    "Idle",
    "Online",
    "Offline",
    "Charging",
    "Discharging",
    "Full",
    "Connected",
    "Disconnected",
    "Paused",
    "Playing",
    "Stopped",
    "Unknown",
    "Error",
    "None",
    "Starting",
    "Scanning",
    "Bluetooth",
    "Wired",
];

fn strings(value: &Py, out: &mut Vec<String>) {
    match value {
        Py::Str(s) => out.push(s.clone()),
        Py::Dict(items) => items.iter().for_each(|(_, v)| strings(v, out)),
        Py::List(items) => items.iter().for_each(|v| strings(v, out)),
        _ => {}
    }
}

fn key_values(value: &Py, key: &str, out: &mut Vec<String>) {
    match value {
        Py::Dict(items) => {
            for (k, v) in items {
                match v {
                    Py::Str(s) if k == key => out.push(s.clone()),
                    _ => key_values(v, key, out),
                }
            }
        }
        Py::List(items) => items.iter().for_each(|v| key_values(v, key, out)),
        _ => {}
    }
}

/// `_wpa(text)`: ssid/psk/identity/password values of wpa_supplicant networks (quoted or hex).
fn wpa(text: &str, ssid: &mut BTreeSet<String>, secret: &mut BTreeSet<String>) {
    static CELL: OnceLock<Regex> = OnceLock::new();
    let re = lazy(
        &CELL,
        &format!(
            r"(?m)^[{S}]*(ssid|psk|password|identity|anonymous_identity|sae_password)[{S}]*=[{S}]*(.+?)[{S}]*$"
        ),
    );
    for caps in re.captures_iter(text) {
        let (key, mut value) = (&caps[1], caps[2].to_owned());
        if value.starts_with('"') && value.ends_with('"') && value.chars().count() >= 2 {
            value = value[1..value.len() - 1].to_owned();
        } else if key == "ssid"
            && !value.is_empty()
            && value.len() % 2 == 0
            && value.bytes().all(|b| b.is_ascii_hexdigit())
        {
            let bytes: Vec<u8> = (0..value.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&value[i..i + 2], 16).expect("hex"))
                .collect();
            match String::from_utf8(bytes) {
                Ok(text) => value = text,
                Err(_) => continue,
            }
        }
        if key == "ssid" {
            &mut *ssid
        } else {
            &mut *secret
        }
        .insert(value);
    }
}

fn volume_id(value: &str) -> bool {
    static CELL: OnceLock<Regex> = OnceLock::new();
    lazy(
        &CELL,
        r"\A(?:[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}|[0-9A-Fa-f]{16}|[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12})\z",
    )
    .is_match(value)
}

pub fn secrets(root: &Root) -> Result<Known, Failure> {
    let mut found: Vec<(&'static str, BTreeSet<String>)> = super::scrub::CLASSES
        .iter()
        .map(|c| (*c, BTreeSet::new()))
        .collect();
    let class = |name: &str| -> usize {
        found
            .iter()
            .position(|(n, _)| *n == name)
            .expect("known class")
    };
    let (ssid, secret, bt, account, ssh, serial, music) = (
        class("ssid"),
        class("wifi_secret"),
        class("bt_name"),
        class("account"),
        class("ssh"),
        class("serial"),
        class("music"),
    );
    for path in [
        "/data/network/wpa_supplicant.conf",
        "/etc/wpa_supplicant.conf",
    ] {
        let text = root.read_limited(path, 262_144).unwrap_or_default();
        let (mut a, mut b) = (BTreeSet::new(), BTreeSet::new());
        wpa(&text, &mut a, &mut b);
        found[ssid].1.extend(a);
        found[secret].1.extend(b);
    }
    for path in [
        "/run/y2/network.json",
        "/run/y2/wifi-events.json",
        "/run/y2/wifi-failure.json",
    ] {
        let mut out = Vec::new();
        if let Some(value) = json_at(root, path) {
            key_values(&value, "ssid", &mut out);
        }
        found[ssid].1.extend(out);
    }
    static NAMES: OnceLock<Regex> = OnceLock::new();
    let names = lazy(&NAMES, r"(?m)^(?:Name|Alias)=(.+)$");
    let mut infos = root.glob("/data/bluetooth/bluez/*/*/info");
    infos.extend(root.glob("/data/bluetooth/bluez/*/cache/*"));
    for info in infos {
        let text = root
            .read_limited(&rel(root, &info), 65536)
            .unwrap_or_default();
        for caps in names.captures_iter(&text) {
            found[bt].1.insert(python_strip(&caps[1]).to_owned());
        }
    }
    let mut out = Vec::new();
    strings(&json_dict(root, "/data/reborn/state/lastfm.json"), &mut out);
    found[account].1.extend(out);
    let mut keys = vec!["/root/.ssh/authorized_keys".to_owned()];
    keys.extend(
        root.glob("/data/ssh/authorized_keys.d/*")
            .iter()
            .map(|p| rel(root, p)),
    );
    for path in keys {
        let text = root.read_limited(&path, 65536).unwrap_or_default();
        for line in pyre::splitlines(&text) {
            // options? type key comment: everything but the key type is private.
            for part in line.split(pyre::isspace).filter(|p| !p.is_empty()) {
                if !["ssh-", "ecdsa-", "sk-"]
                    .iter()
                    .any(|p| part.starts_with(p))
                {
                    found[ssh].1.insert(part.to_owned());
                }
            }
        }
    }
    let mut cards = root.glob("/sys/class/mmc_host/*/*/cid");
    cards.extend(root.glob("/sys/class/mmc_host/*/*/serial"));
    for path in cards {
        found[serial]
            .1
            .insert(root.read(&rel(root, &path)).unwrap_or_default());
    }
    for path in ["/data/system/dbus/machine-id", "/etc/machine-id"] {
        found[serial].1.insert(root.read(path).unwrap_or_default());
    }
    // Volume serials of the user's cards (FAT XXXX-XXXX, exFAT/NTFS 16 hex, ext4 UUID): Reborn names
    // an SD source by it ({"kind":"sd_card","id":...}).
    for path in [
        "/run/y2/media.json",
        "/run/y2/media-mount.json",
        "/run/y2/storage.json",
        "/run/y2/internal-media.json",
        "/run/y2/mtp-roots.json",
        "/run/y2/media-lifecycle.json",
    ] {
        let mut out = Vec::new();
        if let Some(value) = json_at(root, path) {
            strings(&value, &mut out);
        }
        found[serial]
            .1
            .extend(out.into_iter().filter(|v| volume_id(v)));
    }
    // A record that names the boot (a random ID per boot, kept in report.json) looks like an ext4
    // UUID; it is not a volume serial.
    found[serial].1.remove(&root.boot_id().unwrap_or_default());
    static CPU: OnceLock<Regex> = OnceLock::new();
    let cpu = lazy(&CPU, &format!(r"(?mi)^Serial[{S}]*:[{S}]*([^{S}]+)"));
    if let Some(caps) = cpu.captures(&root.read("/proc/cpuinfo").unwrap_or_default()) {
        found[serial].1.insert(caps[1].to_owned());
    }
    // A bootloader can pass the device serial on the command line.
    static CMDLINE: OnceLock<Regex> = OnceLock::new();
    let cmdline = lazy(
        &CMDLINE,
        &format!(r"(?:\A|[{S}])[^{S}]*serial(?:no)?=([^{S}]+)"),
    );
    for caps in cmdline.captures_iter(&root.read("/proc/cmdline").unwrap_or_default()) {
        found[serial].1.insert(caps[1].to_owned());
    }
    let session = large_json(root, "/data/reborn/state/session.json");
    if let Some(model @ Py::Dict(_)) = session.as_ref().and_then(|s| s.get("model")) {
        // `(model.get('queue') or [])[:5000]`: a string queue holds no dicts; a non-empty object or
        // a number cannot be sliced and is refused.
        let tracks: Vec<Py> = match model.get("queue") {
            Some(Py::List(items)) => items.iter().take(5000).cloned().collect(),
            Some(queue @ (Py::Dict(_) | Py::Int(_) | Py::Float(_) | Py::Bool(_)))
                if queue.truthy() =>
            {
                return Err(Failure::value(super::SESSION_UNREADABLE))
            }
            _ => Vec::new(),
        };
        for track in tracks {
            for key in [
                "title",
                "artist",
                "album",
                "album_artist",
                "filename",
                "genre",
            ] {
                if let Some(Py::Str(value)) = track.get(key) {
                    found[music].1.insert(value.clone());
                }
            }
        }
    }
    // Too short to replace safely (would hit ordinary words and numbers).
    let kept = |name: &str, value: &str| {
        name == "music"
            && (MUSIC_KEPT.contains(&python_strip(value))
                || !value
                    .chars()
                    .any(|c| pyre::isalnum(c) && !pyre::isdecimal(c) && c != '_'))
    };
    Ok(found
        .into_iter()
        .map(|(name, values)| {
            let values = values
                .into_iter()
                .filter(|v| {
                    python_strip(v).chars().count() >= 3
                        && !COMMON_WORDS.contains(&v.as_str())
                        && python_strip(v) != "0000000000000000"
                        && !kept(name, v)
                })
                .collect();
            (name, values)
        })
        .collect())
}

#[cfg(test)]
#[path = "secrets_tests.rs"]
mod secrets_tests;
