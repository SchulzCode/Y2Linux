//! What the last scan saw of the saved networks (F13), never a network name or address.
// SPDX-License-Identifier: GPL-2.0-only
use super::wpa::{digits, flags, int, rows, signed, wpa};
use serde::{Deserialize, Serialize};
use y2p_core::Root;

fn channel(frequency: i64) -> Option<i64> {
    if (2412..=2472).contains(&frequency) {
        Some((frequency - 2407).div_euclid(5))
    } else if frequency == 2484 {
        Some(14)
    } else if (5000..=5900).contains(&frequency) {
        Some((frequency - 5000).div_euclid(5))
    } else {
        None
    }
}

/// The scan evidence: absent fields were not observed (no scan, no saved network seen).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct ScanEvidence {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub networks: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_networks: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub band_2ghz: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub band_5ghz: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_seen: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_signal_dbm: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_frequency_mhz: Option<i64>,
    /// The channel of that frequency (null where the frequency is none of the bands').
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_channel: Option<Option<i64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_access_points: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_flags: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_age_s: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_key_mgmt: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
}

/// Why the saved network was "not found" (F13: Wi-Fi did not come back after a wake on one player,
/// cause unproven): how many networks the last scan saw, whether a saved one was among them, its best
/// signal and channel, and the regulatory country (country=00 scans channels 12/13 passively). Never a
/// network name or address.
pub fn scan_evidence(root: &Root) -> ScanEvidence {
    let scan = wpa(root, &["scan_results"]);
    let saved = wpa(root, &["list_networks"]);
    if !scan.ok || !saved.ok {
        return ScanEvidence::default();
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
    let mut result = ScanEvidence {
        available: true,
        networks: Some(found.len()),
        saved_networks: Some(names.len()),
        band_2ghz: Some(found.iter().filter(|r| int(&r[1]) < 3000).count()),
        band_5ghz: Some(found.iter().filter(|r| int(&r[1]) >= 5000).count()),
        ..ScanEvidence::default()
    };
    let hits: Vec<&Vec<String>> = found
        .iter()
        .filter(|row| row.len() >= 5 && names.contains(&row[4].as_str()))
        .collect();
    result.saved_seen = Some(!hits.is_empty());
    if !hits.is_empty() {
        // The strongest; of equal signals the first listed.
        let mut best = hits[0];
        for row in &hits[1..] {
            if int(&row[2]) > int(&best[2]) {
                best = row;
            }
        }
        let frequency = int(&best[1]);
        result.saved_signal_dbm = Some(int(&best[2]));
        result.saved_frequency_mhz = Some(frequency);
        result.saved_channel = Some(channel(frequency));
        result.saved_access_points = Some(hits.len());
        // F13 (cb904ffb): "not found" with the saved network listed at -66 dBm. scan_results is the
        // BSS table (entries up to 180 s old): how old the entry is, and whether its security matches
        // the saved network's.
        result.saved_flags = Some(flags(&best[3]));
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
                result.saved_age_s = Some(age);
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
        result.saved_key_mgmt = Some(managements);
    }
    let country = wpa(root, &["get", "country"]);
    let value = y2p_core::root::python_strip(country.output.as_deref().unwrap_or_default());
    if country.ok
        && value.len() == 2
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_uppercase())
    {
        result.country = Some(value.to_owned());
    }
    result
}
