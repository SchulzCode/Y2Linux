//! Wi-Fi above wpa_supplicant (the lease itself is `commands/network.rs`): the durable Wi-Fi journal,
//! the DNS probe and its schedule, the supplicant event monitor with the F52 password latch Reborn
//! reads, the scan evidence of F13 and the F52 self-heal. The service daemon runs these in process;
//! the connectivity daemon writes the journal (F53).
// SPDX-License-Identifier: GPL-2.0-only
mod dns;
mod journal;
mod monitor;
mod recovery;
mod scan;
mod wpa;

pub use dns::{probe_dns, DnsProbeSchedule};
#[cfg(test)]
pub use journal::JOURNAL;
pub use journal::{journal, round1};
pub use monitor::EventMonitor;
pub use recovery::WifiRecovery;

#[cfg(test)]
#[path = "wifi_tests.rs"]
mod wifi_tests;
