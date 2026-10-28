//! What `transfer` needs of `observe.storage` and `internal_media`: the `/data` and `/media/internal`
//! volume records (state, generation, space), and `internal_media.ready`. Every volume is computed
//! in the consumer order (root, Y2DATA, SD, Y2Media) with the same `blkid` requests, so the commands
//! asked for are Python's; the SD card's own identity and claim checks, the block list, the
//! controllers and the setup status only feed fields transfer never reads (and run no command), so
//! they are not part of this port.
// SPDX-License-Identifier: GPL-2.0-only
use serde_json::{json, Value};
use std::{
    ffi::CString,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};
use y2p_core::{command, py::eq_int, Root};

pub const INTERNAL: &str = "/media/internal";
const MARKER: &str = "/run/y2/internal-media.json";

#[derive(Clone, Debug)]
pub struct Mount {
    pub mount_id: i64,
    pub device_id: String,
    pub path: String,
    pub options: Vec<String>,
    pub source: String,
    pub super_options: Vec<String>,
}

/// `observe.unescape`: the octal escapes of mountinfo fields.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(at) = rest.find('\\') {
        out.push_str(&rest[..at]);
        let code = rest.get(at + 1..at + 4);
        match code.and_then(|c| ["040", "011", "012", "134"].contains(&c).then_some(c)) {
            Some(c) => {
                out.push(char::from(u8::from_str_radix(c, 8).unwrap_or(b'?')));
                rest = &rest[at + 4..];
            }
            None => {
                out.push('\\');
                rest = &rest[at + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// `observe.mountinfo`.
pub fn mountinfo(text: Option<&str>) -> Vec<Mount> {
    let mut result = Vec::new();
    for line in text.unwrap_or_default().lines() {
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        let (a, b): (Vec<&str>, Vec<&str>) = (
            left.split_whitespace().collect(),
            right.split_whitespace().collect(),
        );
        if a.len() < 6 || b.len() < 3 || !a[0].bytes().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(mount_id) = a[0].parse() else {
            continue;
        };
        result.push(Mount {
            mount_id,
            device_id: a[2].to_string(),
            path: unescape(a[4]),
            options: a[5].split(',').map(str::to_owned).collect(),
            source: unescape(b[1]),
            super_options: b[2].split(',').map(str::to_owned).collect(),
        });
    }
    result
}

/// One entry of `storage(ctx)['volumes']`, the fields transfer reads.
#[derive(Clone, Debug)]
pub struct Volume {
    pub path: &'static str,
    pub state: &'static str,
    pub generation: Option<String>,
    pub space_state: &'static str,
    pub available_bytes: Option<u64>,
}

/// `observe.space_state`: product capacity budget, not electrical thresholds. Never delete user data.
fn space_state(available: u64, total: u64, readonly: bool, error: bool) -> &'static str {
    const MIB: u64 = 1024 * 1024;
    if error {
        "Failed"
    } else if readonly {
        "ReadOnlyRisk"
    } else if available < (32 * MIB).max(total / 20) {
        "CriticalSpace"
    } else if available < (96 * MIB).max(total / 10) {
        "LowSpace"
    } else {
        "Normal"
    }
}

/// `Path.resolve()` (strict=False): symlinks resolved as far as the path exists.
fn resolve(path: &Path) -> PathBuf {
    if let Ok(real) = path.canonicalize() {
        return real;
    }
    let mut missing = Vec::new();
    let mut existing = path.to_path_buf();
    while !existing.as_os_str().is_empty() {
        if let Ok(real) = existing.canonicalize() {
            let mut real = real;
            for part in missing.iter().rev() {
                real.push(part);
            }
            return real;
        }
        match (existing.file_name(), existing.parent()) {
            (Some(name), Some(parent)) => {
                missing.push(name.to_os_string());
                existing = parent.to_path_buf();
            }
            _ => break,
        }
    }
    path.to_path_buf()
}

struct Space {
    total: u64,
    available: u64,
    files: u64,
    favail: u64,
}

fn statvfs(path: &Path) -> Option<Space> {
    let name = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: a NUL-terminated path and a writable statvfs buffer.
    let mut vfs: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(name.as_ptr(), &mut vfs) } != 0 {
        return None;
    }
    let frsize = vfs.f_frsize as u64;
    Some(Space {
        total: vfs.f_blocks as u64 * frsize,
        available: vfs.f_bavail as u64 * frsize,
        files: vfs.f_files as u64,
        favail: vfs.f_favail as u64,
    })
}

/// `observe.storage(ctx)['volumes']`.
pub fn volumes(root: &Root) -> Vec<Volume> {
    let mounts = mountinfo(root.read("/proc/self/mountinfo").as_deref());
    let boot_id = root.read("/proc/sys/kernel/random/boot_id");
    let mut volumes = Vec::new();
    // Index order is a consumer contract: root, Y2DATA, SD, then Y2Media (internal user media).
    for target in ["/", "/data", "/media/sd", INTERNAL] {
        let found: Vec<&Mount> = mounts.iter().filter(|m| m.path == target).collect();
        // Stacked mounts are ambiguous to consumers; fail closed.
        let [item] = found[..] else {
            volumes.push(Volume {
                path: target,
                state: "Unavailable",
                generation: None,
                space_state: "Unavailable",
                available_bytes: None,
            });
            continue;
        };
        let generation = boot_id
            .as_deref()
            .filter(|b| !b.is_empty())
            .map(|b| format!("{b}:{}:{}", item.mount_id, item.device_id));
        let device = root.path(&format!("/sys/dev/block/{}", item.device_id));
        let real = resolve(&device).to_string_lossy().into_owned();
        let expected = if target == "/media/sd" {
            "11240000.mmc"
        } else {
            "11230000.mmc"
        };
        let controller_valid = real.contains(expected) && device.exists();
        let mut state = if controller_valid { "Ready" } else { "Failed" };
        if controller_valid {
            let answer = command::run(
                root,
                &["blkid", "-p", "-s", "UUID", "-o", "value", &item.source],
                command::TIMEOUT,
            );
            let uuid = if answer.ok {
                answer.output.filter(|o| !o.is_empty())
            } else {
                None
            };
            let expected_uuid = match target {
                "/" => Some("79324c69-6e75-4801-8000-000000000101"),
                "/data" => Some("79324c69-6e75-4801-8000-000000000102"),
                INTERNAL => Some("79324c69-6e75-4801-8000-000000000103"),
                _ => None,
            };
            if expected_uuid.is_some() && uuid.as_deref() != expected_uuid {
                state = "Failed";
            }
        }
        let readonly =
            item.options.iter().any(|o| o == "ro") || item.super_options.iter().any(|o| o == "ro");
        // The SD card's protected/checking read-only mounts (media-mount.json) are not ported: only
        // the SD volume uses them.
        let protected = false;
        let (space_state, available_bytes) = match statvfs(&root.path(target)) {
            Some(space) => {
                let mut space_state = space_state(
                    space.available,
                    space.total,
                    readonly && !protected,
                    state == "Failed",
                );
                if space.files > 0 && space.favail == 0 {
                    space_state = "CriticalSpace";
                }
                (space_state, Some(space.available))
            }
            None => {
                state = "Failed";
                ("Failed", None)
            }
        };
        if state == "Ready" && readonly && !protected {
            state = "Degraded";
        }
        volumes.push(Volume {
            path: target,
            state,
            generation,
            space_state,
            available_bytes,
        });
    }
    volumes
}

/// The volume at `path` (always one of the four targets).
pub fn volume(root: &Root, path: &str) -> Volume {
    volumes(root)
        .into_iter()
        .find(|v| v.path == path)
        .expect("one of the four volume targets")
}

/// `space.admission(volume, requested_bytes, 'transfer')['allowed']`.
pub fn admitted(volume: &Volume, requested: u64) -> bool {
    if matches!(
        volume.space_state,
        "ReadOnlyRisk" | "Failed" | "Unavailable"
    ) {
        return false;
    }
    let reserve = 96 * 1024 * 1024;
    volume
        .available_bytes
        .is_some_and(|free| free as i128 - requested as i128 >= reserve)
}

/// `internal_media.ready`: true only while the recorded mount is still the live, exact Y2Media mount.
pub fn internal_ready(root: &Root) -> bool {
    let claim = root.json_object(MARKER);
    let current: Vec<Mount> = mountinfo(root.read("/proc/self/mountinfo").as_deref())
        .into_iter()
        .filter(|m| m.path == INTERNAL)
        .collect();
    let boot = root.read("/proc/sys/kernel/random/boot_id");
    claim.get("state").and_then(Value::as_str) == Some("Ready")
        && current.len() == 1
        && claim.get("boot_id").cloned().unwrap_or(Value::Null) == json!(boot)
        && eq_int(claim.get("mount_id"), current[0].mount_id)
        && claim.get("device_id").and_then(Value::as_str) == Some(current[0].device_id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mountinfo_reads_as_observe() {
        let mounts = mountinfo(Some(
            "22 1 179:2 / / ro - ext4 /dev/mmcblk0p2 ro\n\
             bad line\n\
             31 22 179:8 / /media/My\\040Music rw,noatime - ext4 /dev/mmcblk0p8 rw,x\n",
        ));
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[1].path, "/media/My Music");
        assert_eq!(mounts[1].mount_id, 31);
        assert_eq!(mounts[1].super_options, ["rw", "x"]);
        assert_eq!(unescape("a\\134b\\9"), "a\\b\\9");
        assert_eq!(space_state(10, 100, false, false), "CriticalSpace");
        assert_eq!(space_state(1 << 40, 1 << 41, false, false), "Normal");
    }
}
