//! What the last scan saw of the saved networks (F13), never a network name or address.
// SPDX-License-Identifier: GPL-2.0-only
use super::wpa::{digits, flags, int, rows, signed, wpa};
use serde_json::{json, Value};
use y2p_core::{
    json::{Object, ObjectExt},
    Root,
};

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
