//! The storage records more than one command reads: the claim on our SD mount (media-mount.json,
//! written by the mount that made it) with its read-only reason, and `blkid -o export` properties.
// SPDX-License-Identifier: GPL-2.0-only
use super::observe::MEDIA_MOUNT;
use super::sys::{get, object, Object};
use serde_json::{json, Value};
use std::collections::HashMap;
use y2p_core::{storage_media::splitlines, Root};

/// Why our SD mount is read-only (`read_only` of the claim and the operation records).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadOnly {
    /// `null`: mounted read-write (the owner switch, `sd-always-writable`).
    ReadWrite,
    /// A healthy card between writes: writable only while a write window is open.
    Protected,
    /// Its admission check did not finish in time; `media verify` finishes it (F49).
    Checking,
    /// Its check found errors: Repair (or a computer) makes it writable.
    FilesystemErrors,
    /// Its background check did not finish either: Repair is offered too.
    CheckTimedOut,
}

impl ReadOnly {
    /// The reason a record names; `None` for a value no version of the platform writes.
    pub fn of(value: &Value) -> Option<ReadOnly> {
        Some(match value {
            Value::Null => ReadOnly::ReadWrite,
            Value::String(name) => match name.as_str() {
                "protected" => ReadOnly::Protected,
                "checking" => ReadOnly::Checking,
                "filesystem_errors" => ReadOnly::FilesystemErrors,
                "check_timed_out" => ReadOnly::CheckTimedOut,
                _ => return None,
            },
            _ => return None,
        })
    }

    /// The value a record keeps.
    pub fn value(self) -> Value {
        match self {
            ReadOnly::ReadWrite => Value::Null,
            ReadOnly::Protected => json!("protected"),
            ReadOnly::Checking => json!("checking"),
            ReadOnly::FilesystemErrors => json!("filesystem_errors"),
            ReadOnly::CheckTimedOut => json!("check_timed_out"),
        }
    }

    /// Read-only because the card may be damaged, so Repair is offered.
    pub fn damaged(reason: Option<ReadOnly>) -> bool {
        matches!(
            reason,
            Some(ReadOnly::FilesystemErrors | ReadOnly::CheckTimedOut)
        )
    }
}

/// Our claim on the SD mount (media-mount.json): the record of the mount that made it. A removed
/// card has no sysfs path any more; the claim then names the exact mount we may still unmount.
#[derive(Clone, Debug, Default)]
pub struct Claim(Object);

impl Claim {
    pub fn load(root: &Root) -> Claim {
        Claim(object(root, MEDIA_MOUNT))
    }

    /// A field as the record has it (`null` when missing).
    pub fn get(&self, key: &str) -> &Value {
        get(&self.0, key)
    }

    /// The claim's read-only reason; `None` for an unknown value.
    pub fn read_only(&self) -> Option<ReadOnly> {
        ReadOnly::of(self.get("read_only"))
    }

    /// Made by this boot for mount `mount_id` (the kernel recycles mount ids across boots).
    pub fn holds(&self, root: &Root, mount_id: i64) -> bool {
        *self.get("mount_id") == json!(mount_id) && self.is_this_boot(root)
    }

    /// Made by this boot (a missing boot id matches an unreadable one).
    pub fn is_this_boot(&self, root: &Root) -> bool {
        root.is_this_boot(Some(self.get("boot_id")))
    }

    /// A card of this boot whose admission check `media verify` still has to finish (F49).
    pub fn checking(&self, root: &Root) -> bool {
        self.read_only() == Some(ReadOnly::Checking) && self.is_this_boot(root)
    }

    pub fn into_object(self) -> Object {
        self.0
    }
}

/// The `KEY=value` lines of `blkid -p -o export` (split at the first `=`; a later line names a key
/// again over an earlier one, a line without `=` is left out).
#[derive(Clone, Debug, Default)]
pub struct Properties(HashMap<String, String>);

impl Properties {
    pub fn parse(output: Option<&str>) -> Properties {
        let pairs = splitlines(output.unwrap_or_default())
            .into_iter()
            .filter_map(|line| line.split_once('='));
        Properties(pairs.map(|(k, v)| (k.to_owned(), v.to_owned())).collect())
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    /// A property as a record keeps it: its text, or `null` when blkid did not print it.
    pub fn value(&self, key: &str) -> Value {
        self.get(key).map_or(Value::Null, Value::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_reasons_round_trip() {
        for reason in [
            ReadOnly::ReadWrite,
            ReadOnly::Protected,
            ReadOnly::Checking,
            ReadOnly::FilesystemErrors,
            ReadOnly::CheckTimedOut,
        ] {
            assert_eq!(ReadOnly::of(&reason.value()), Some(reason));
        }
        assert_eq!(ReadOnly::of(&json!("mounted")), None);
        assert_eq!(ReadOnly::of(&json!(1)), None);
        assert!(ReadOnly::damaged(Some(ReadOnly::CheckTimedOut)));
        assert!(!ReadOnly::damaged(Some(ReadOnly::Protected)) && !ReadOnly::damaged(None));
    }

    #[test]
    fn properties_parse_blkid_export() {
        let props = Properties::parse(Some(
            "DEVNAME=/dev/x\nUUID=A-B=C\nTYPE=vfat\nnoise\nTYPE=exfat",
        ));
        assert_eq!(props.get("TYPE"), Some("exfat"));
        assert_eq!(props.get("UUID"), Some("A-B=C"));
        assert_eq!(props.value("LABEL"), Value::Null);
        assert!(Properties::parse(None).get("TYPE").is_none());
    }
}
