//! `/proc/self/mountinfo` (`observe.mountinfo`): one parser and one `Mount` for every group.
//!
//! A line is `id parent major:minor root path options [optional…] - fstype source super-options`; the
//! kernel escapes a space, tab, newline and backslash in a field as `\040`, `\011`, `\012`, `\134`.
//! Lines and fields split as Python's `splitlines()` and `split()` (the reference the goldens were
//! recorded from); a line without ` - `, with too few fields or a mount id that is no integer is
//! skipped.
// SPDX-License-Identifier: GPL-2.0-only
use crate::{
    json::Object,
    suspend_text::{split, splitlines},
    Root,
};
use serde_json::json;

/// One mountinfo line (serialised in `value`'s key order).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Mount {
    pub mount_id: i64,
    pub device_id: String,
    /// The mounted directory within its filesystem (`/` for a whole filesystem).
    pub root: String,
    pub path: String,
    pub options: Vec<String>,
    pub filesystem: String,
    pub source: String,
    pub super_options: Vec<String>,
}

impl Mount {
    /// The dict `observe.mountinfo` builds, in its key order.
    pub fn value(&self) -> Object {
        let mut map = Object::new();
        map.insert("mount_id".into(), json!(self.mount_id));
        map.insert("device_id".into(), json!(self.device_id));
        map.insert("root".into(), json!(self.root));
        map.insert("path".into(), json!(self.path));
        map.insert("options".into(), json!(self.options));
        map.insert("filesystem".into(), json!(self.filesystem));
        map.insert("source".into(), json!(self.source));
        map.insert("super_options".into(), json!(self.super_options));
        map
    }

    /// `option in mount['options']` (the per-mount options: `rw`, `ro`, `noatime`, …).
    pub fn has_option(&self, option: &str) -> bool {
        self.options.iter().any(|o| o == option)
    }

    /// Read-only at the mount or at the superblock.
    pub fn read_only(&self) -> bool {
        self.has_option("ro") || self.super_options.iter().any(|o| o == "ro")
    }
}

/// `observe.unescape`: the four octal escapes the kernel writes; any other backslash stays.
pub fn unescape(value: &str) -> String {
    let (mut out, mut rest) = (String::new(), value);
    while let Some(index) = rest.find('\\') {
        out.push_str(&rest[..index]);
        let code = &rest[index + 1..];
        match ["040", "011", "012", "134"]
            .iter()
            .find(|c| code.starts_with(**c))
        {
            Some(c) => {
                out.push(char::from(u8::from_str_radix(c, 8).expect("octal")));
                rest = &code[3..];
            }
            None => {
                out.push('\\');
                rest = code;
            }
        }
    }
    out + rest
}

/// `observe.mountinfo(text)`.
pub fn mountinfo(text: &str) -> Vec<Mount> {
    let mut result = Vec::new();
    for line in splitlines(text) {
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        let (a, b) = (split(left), split(right));
        if a.len() < 6 || b.len() < 3 || !a[0].bytes().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(mount_id) = a[0].parse() else {
            continue;
        };
        result.push(Mount {
            mount_id,
            device_id: a[2].to_owned(),
            root: unescape(a[3]),
            path: unescape(a[4]),
            options: a[5].split(',').map(str::to_owned).collect(),
            filesystem: b[0].to_owned(),
            source: unescape(b[1]),
            super_options: b[2].split(',').map(str::to_owned).collect(),
        });
    }
    result
}

/// `mountinfo(ctx.read('/proc/self/mountinfo'))`: no mounts when it cannot be read.
pub fn mounts(root: &Root) -> Vec<Mount> {
    mountinfo(&root.read("/proc/self/mountinfo").unwrap_or_default())
}

/// The mounts on `path`, in mountinfo order (the last one is on top).
pub fn mounts_at(root: &Root, path: &str) -> Vec<Mount> {
    mounts(root)
        .into_iter()
        .filter(|m| m.path == path)
        .collect()
}

/// `sleep_diagnostics.data_ready`: /data is the platform's own verified Y2DATA mount (schema 1, one
/// mount, from the device the boot verified). Never create persistent state on anything else.
pub fn data_ready(root: &Root) -> bool {
    let data = mounts_at(root, "/data");
    root.read("/data/.y2data-schema").as_deref() == Some("1")
        && data.len() == 1
        && Some(data[0].source.as_str()) == root.read("/run/y2-data-device").as_deref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_unescape_and_keep_every_field() {
        let text = "22 1 179:2 / / ro,relatime - ext4 /dev/mmcblk0p2 ro\n\
                    30 22 179:9 /sub /media/my\\040card rw shared:1 - exfat /dev/a\\134b rw,uid=0\n\
                    bad line\n\
                    x 1 1:1 / /x rw - t s o\n\
                    31 22 0:5 / /short rw - tmpfs\n";
        let mounts = mountinfo(text);
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[1].path, "/media/my card");
        assert_eq!(mounts[1].source, "/dev/a\\b");
        assert_eq!(mounts[1].root, "/sub");
        assert_eq!(mounts[1].filesystem, "exfat");
        assert_eq!(mounts[1].super_options, ["rw", "uid=0"]);
        assert!(mounts[0].read_only() && !mounts[1].read_only() && mounts[1].has_option("rw"));
        assert_eq!(unescape("a\\040b\\x\\011"), "a b\\x\t");
        assert_eq!(
            serde_json::to_string(&mounts[0].value()).unwrap(),
            r#"{"mount_id":22,"device_id":"179:2","root":"/","path":"/","options":["ro","relatime"],"filesystem":"ext4","source":"/dev/mmcblk0p2","super_options":["ro"]}"#
        );
    }
}
