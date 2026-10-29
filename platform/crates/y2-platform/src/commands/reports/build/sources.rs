//! What a report reads: the live commands and the files, and what it never reads.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;

/// Live state from tools (name, argv, timeout, output limit); each bounded.
pub(super) const COMMANDS: [(&str, &[&str], f64, usize); 13] = [
    ("dmesg", &["dmesg"], 10.0, 2 << 20),
    ("platform_health", &["y2-platform", "health"], 10.0, 1 << 20),
    ("platform_status", &["y2-platform", "status"], 10.0, 1 << 20),
    (
        "update_status",
        &["y2-platform", "update", "status"],
        10.0,
        256 << 10,
    ),
    (
        "reborn_status",
        &["rebornctl", "status", "--json"],
        5.0,
        1 << 20,
    ),
    (
        "reborn_health",
        &["rebornctl", "health", "--json"],
        5.0,
        1 << 20,
    ),
    (
        "reborn_metrics",
        &["rebornctl", "metrics", "--json"],
        5.0,
        1 << 20,
    ),
    (
        "reborn_audio",
        &["rebornctl", "audio", "--json"],
        5.0,
        1 << 20,
    ),
    (
        "reborn_snapshot",
        &["rebornctl", "snapshot", "--json"],
        5.0,
        1 << 20,
    ),
    (
        "reborn_events",
        &["rebornctl", "events", "--last", "500", "--json"],
        5.0,
        2 << 20,
    ),
    (
        "processes",
        &["ps", "-o", "pid,ppid,stat,rss,vsz,time,args"],
        5.0,
        512 << 10,
    ),
    ("mounts_space", &["df", "-k"], 5.0, 64 << 10),
    (
        "amixer",
        &["amixer", "-c", "Y2Audio", "contents"],
        5.0,
        256 << 10,
    ),
];

/// Files copied whole (logs keep their newest end). Globs are bounded by `Root::glob`.
pub(super) const FILES: [&str; 68] = [
    "/sys/fs/pstore/*",
    "/data/logs/system.log",
    "/data/logs/system.log.0",
    "/data/logs/recovery-last.log",
    "/data/logs/recovery-display.txt",
    "/data/logs/recovery-previous-boot.txt",
    "/proc/version",
    "/proc/cmdline",
    "/proc/meminfo",
    "/proc/vmstat",
    "/proc/loadavg",
    "/proc/interrupts",
    "/proc/softirqs",
    "/proc/stat",
    "/proc/pressure/*",
    "/proc/mounts",
    "/proc/sys/kernel/tainted",
    "/sys/power/suspend_stats/*",
    "/sys/kernel/debug/wakeup_sources",
    "/sys/kernel/debug/suspend_stats",
    "/sys/kernel/debug/regulator/regulator_summary",
    "/sys/kernel/debug/clk/clk_summary",
    // The CS43131 at 0x30; its I2C bus number follows probe order (0 or 1).
    "/sys/kernel/debug/regmap/*-0030/registers",
    "/sys/bus/i2c/devices/*-0030/y2_hp/xtal",
    "/sys/devices/system/cpu/cpu*/cpuidle/state*/usage",
    "/sys/devices/system/cpu/cpu*/cpuidle/state*/time",
    "/sys/devices/system/cpu/cpu*/cpuidle/state*/name",
    "/sys/devices/system/cpu/cpufreq/policy0/stats/time_in_state",
    "/sys/devices/system/cpu/online",
    "/sys/class/thermal/thermal_zone*/temp",
    "/sys/class/thermal/thermal_zone*/type",
    "/sys/class/power_supply/*/uevent",
    "/sys/class/power_supply/BAT0/charging_state",
    "/sys/fs/ext4/*/errors_count",
    "/sys/fs/ext4/*/first_error_time",
    "/sys/devices/platform/11200000.usb/status",
    "/sys/devices/platform/11200000.usb/rxdiag",
    "/data/system/platform/boot.json",
    "/data/system/platform/previous-boot-evidence.json",
    "/data/system/platform/power-journal.jsonl",
    "/data/system/platform/sleep-diagnostics.jsonl",
    "/data/system/platform/suspend-last.json",
    "/data/system/platform/product-sleep-guard.json",
    "/data/system/platform/rtc-policy.json",
    "/data/system/platform/time-last.json",
    "/data/system/platform/battery-ui.json",
    "/data/system/platform/panel.json",
    "/data/system/platform/sd-admissions.json",
    "/data/system/platform/sd-repair-last.json",
    "/data/system/platform/wifi-journal*.jsonl",
    "/data/system/platform/retained/*.tar",
    "/data/updates/*.json",
    "/data/updates/current/manifest.json",
    "/data/updates/health/*.txt",
    "/run/y2/*.json",
    "/run/y2/*.jsonl",
    "/run/y2/*.log",
    "/data/bluetooth/codec-history.json",
    "/data/bluetooth/enabled",
    "/data/network/enabled",
    "/data/network/power-save.json",
    "/data/reborn/logs/*.jsonl",
    "/data/reborn/logs/panic-last.json",
    "/data/reborn/diagnostics/*.tar.gz",
    // (padding entries keep the array length explicit; never matched)
    "",
    "",
    "",
    "",
];

/// Never read into a report, even if a glob above ever matched them.
pub(super) fn never(path: &str) -> bool {
    static CELL: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    pyre::lazy(
        &CELL,
        r"^/(media|data/music|data/ssh|root/\.ssh)/|wpa_supplicant|lastfm\.json|scrobbles\.jsonl|library\.db|/bluez/|machine-id|entropy|/keys?/|\.pem\n?\z|\.key\n?\z|session\.json",
    )
    .is_match(path)
}
