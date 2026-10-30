//! The storage records more than one command reads: the SD operation records (the claim on our SD
//! mount, media-mount.json, written by the mount that made it) with their read-only reason and
//! filesystem check, and `blkid -o export` properties.
// SPDX-License-Identifier: GPL-2.0-only
use super::media::RepairRun;
use super::observe::{Identity, MEDIA_MOUNT, MEDIA_STATE};
use super::sys::device_instance;
use crate::reply::Failure;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use y2p_core::lenient::{self, BootId, Shaped};
use y2p_core::{py::truthy, storage_media::splitlines, Root};

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

/// One SD operation's record: what `media <action>` prints and media.json keeps. The mount's is our
/// claim on the card (media-mount.json): a removed card has no sysfs path any more, and the claim
/// names the exact mount we may still unmount. Reconcile's media-lifecycle.json starts with the
/// inventory it acted on. Read back leniently: a field of another shape reads as missing.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct MediaRecord {
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::or_none"
    )]
    pub schema: Option<u8>,
    /// The SD devices reconcile saw (media-lifecycle.json).
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::or_none"
    )]
    pub inventory: Option<Vec<Inserted>>,
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "attempts")]
    pub attempts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "flag")]
    pub retry: Option<bool>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    pub action: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    pub state: Option<String>,
    #[serde(deserialize_with = "lenient::text")]
    pub reason: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::there"
    )]
    pub boot_id: Option<BootId>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    pub device: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    pub filesystem: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    pub uuid: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    pub cid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<Shaped<Identity>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    pub source_instance: Option<String>,
    /// The card's size in sectors (`format`, `grow`).
    #[serde(skip_serializing_if = "Option::is_none", skip_deserializing)]
    pub sectors: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", skip_deserializing)]
    pub errno: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filesystem_check: Option<Shaped<Check>>,
    #[serde(skip_serializing_if = "Option::is_none", skip_deserializing)]
    pub repair: Option<RepairRun>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::integer"
    )]
    pub mount_id: Option<i64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    pub device_id: Option<String>,
    /// Why our mount is read-only, or a value no version writes (kept as it is).
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "write_read_only",
        deserialize_with = "read_only"
    )]
    pub read_only: Option<Result<ReadOnly, Value>>,
    /// What Reborn reads from a refused command (`media repair`).
    #[serde(skip_serializing_if = "Option::is_none", skip_deserializing)]
    pub failure: Option<String>,
}

/// One SD device reconcile saw: its name, `dev`, disk sequence and CID.
pub type Inserted = (String, Option<String>, Option<String>, Option<String>);

#[allow(clippy::type_complexity)]
fn read_only<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Result<ReadOnly, Value>>, D::Error> {
    let value = Value::deserialize(deserializer)?;
    Ok(Some(ReadOnly::of(&value).ok_or(value)))
}

fn write_read_only<S: serde::Serializer>(
    read_only: &Option<Result<ReadOnly, Value>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match read_only {
        Some(Ok(reason)) => reason.serialize(serializer),
        Some(Err(value)) => value.serialize(serializer),
        None => serializer.serialize_none(),
    }
}

/// The attempts so far: a flag counts as 0 or 1, anything else that is no integer as 0.
fn attempts<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<i64>, D::Error> {
    Ok(Some(match Value::deserialize(deserializer)? {
        Value::Bool(flag) => i64::from(flag),
        other => other.as_i64().unwrap_or(0),
    }))
}

fn flag<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<bool>, D::Error> {
    lenient::truthy(deserializer).map(Some)
}

impl Serialize for ReadOnly {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.value().serialize(serializer)
    }
}

impl MediaRecord {
    /// The start of every operation record.
    pub fn begin(root: &Root, action: &str) -> MediaRecord {
        MediaRecord {
            schema: Some(1),
            action: Some(action.into()),
            state: Some("Starting".into()),
            boot_id: Some(root.boot_id().into()),
            ..MediaRecord::default()
        }
    }

    /// Our claim on the SD mount.
    pub fn claim(root: &Root) -> MediaRecord {
        lenient::read(root.json_object(MEDIA_MOUNT))
    }

    /// The last operation's record.
    pub fn last(root: &Root) -> MediaRecord {
        lenient::read(root.json_object(MEDIA_STATE))
    }

    pub fn set_state(&mut self, state: &str, reason: Option<&str>) {
        self.state = Some(state.into());
        self.reason = reason.map(str::to_owned);
    }

    pub fn state_is(&self, state: &str) -> bool {
        self.state.as_deref() == Some(state)
    }

    /// The later record over this one: its fields where it has them (`reason` always), this one's
    /// elsewhere.
    pub fn absorb(&mut self, later: MediaRecord) {
        let earlier = std::mem::take(self);
        *self = MediaRecord {
            schema: later.schema.or(earlier.schema),
            inventory: later.inventory.or(earlier.inventory),
            attempts: later.attempts.or(earlier.attempts),
            retry: later.retry.or(earlier.retry),
            action: later.action.or(earlier.action),
            state: later.state.or(earlier.state),
            reason: later.reason,
            boot_id: later.boot_id.or(earlier.boot_id),
            device: later.device.or(earlier.device),
            filesystem: later.filesystem.or(earlier.filesystem),
            uuid: later.uuid.or(earlier.uuid),
            cid: later.cid.or(earlier.cid),
            identity: later.identity.or(earlier.identity),
            source_instance: later.source_instance.or(earlier.source_instance),
            sectors: later.sectors.or(earlier.sectors),
            errno: later.errno.or(earlier.errno),
            filesystem_check: later.filesystem_check.or(earlier.filesystem_check),
            repair: later.repair.or(earlier.repair),
            mount_id: later.mount_id.or(earlier.mount_id),
            device_id: later.device_id.or(earlier.device_id),
            read_only: later.read_only.or(earlier.read_only),
            failure: later.failure.or(earlier.failure),
        };
    }

    /// The claim's read-only reason (a claim without one is read-write); `None` for a value no
    /// version writes.
    pub fn read_only(&self) -> Option<ReadOnly> {
        match &self.read_only {
            None => Some(ReadOnly::ReadWrite),
            Some(recorded) => recorded.as_ref().ok().copied(),
        }
    }

    pub fn set_read_only(&mut self, reason: ReadOnly) {
        self.read_only = Some(Ok(reason));
    }

    /// Made by this boot for mount `mount_id` (the kernel recycles mount ids across boots).
    pub fn holds(&self, root: &Root, mount_id: i64) -> bool {
        self.mount_id == Some(mount_id) && self.is_this_boot(root)
    }

    /// Made by this boot (a missing boot id matches an unreadable one).
    pub fn is_this_boot(&self, root: &Root) -> bool {
        self.of_boot(root.boot_id().as_deref())
    }

    /// Made by the boot named `boot` (none: an unreadable id).
    pub fn of_boot(&self, boot: Option<&str>) -> bool {
        self.boot_id.as_ref().unwrap_or(&BootId::Null).is(boot)
    }

    /// A card of this boot whose admission check `media verify` still has to finish (F49).
    pub fn checking(&self, root: &Root) -> bool {
        self.read_only() == Some(ReadOnly::Checking) && self.is_this_boot(root)
    }

    /// The id of the record's card identity (none without one); an identity of another shape is
    /// refused.
    pub fn identity_id(&self) -> Result<Option<&str>, Failure> {
        match &self.identity {
            Some(Shaped::Is(identity)) => Ok(Some(&identity.id)),
            Some(Shaped::Odd(other)) if truthy(other) => {
                Err(Failure::value("card_identity_invalid"))
            }
            _ => Ok(None),
        }
    }

    /// What the record's filesystem check found (none without a check); a check of another shape is
    /// refused.
    pub fn findings(&self) -> Result<Option<Vec<String>>, Failure> {
        match &self.filesystem_check {
            Some(Shaped::Is(check)) => Ok(check.findings.clone()),
            Some(Shaped::Odd(other)) if truthy(other) => {
                Err(Failure::value("filesystem_check_invalid"))
            }
            _ => Ok(None),
        }
    }

    /// The card still inserted that the last mount refused (its check failed), so the player can say
    /// so instead of "no card" (issue #44). A removed or reinserted card has another disk sequence
    /// number.
    pub fn refused(root: &Root) -> Option<RefusedCard> {
        let last = MediaRecord::last(root);
        if last.reason.as_deref() != Some("filesystem_check_failed") {
            return None;
        }
        // `block:<major>:<minor>:diskseq:<n>`: still this card while the same disk sequence is
        // there.
        let instance = last.source_instance.as_deref()?;
        let parts: Vec<&str> = instance.split(':').collect();
        if parts.len() != 5
            || device_instance(root, &parts[1..3].join(":")).as_deref() != Some(instance)
        {
            return None;
        }
        // ['timeout'] for a card too slow to check in 60 s, not one that needs repair (F12).
        let findings = match &last.filesystem_check {
            Some(Shaped::Is(check)) => check.findings.clone().unwrap_or_default(),
            _ => Vec::new(),
        };
        Some(RefusedCard {
            filesystem: last.filesystem,
            findings,
        })
    }
}

/// An SD card the storage group refused to mount after a failed filesystem check (still inserted).
#[derive(Serialize, Debug)]
pub struct RefusedCard {
    pub filesystem: Option<String>,
    pub findings: Vec<String>,
}

/// What a filesystem check found (`filesystem_check` of the records). The checker's output is kept
/// only from a run that passed: a failed one can name files.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Check {
    #[serde(deserialize_with = "lenient::is_true")]
    pub ok: bool,
    #[serde(deserialize_with = "lenient::text")]
    pub reason: Option<String>,
    #[serde(deserialize_with = "lenient::text")]
    pub output: Option<String>,
    #[serde(deserialize_with = "lenient::or_default")]
    pub elapsed_ms: u64,
    /// The classes of what it reported: `timeout`, the FAT line classes, or `other`.
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::texts"
    )]
    pub findings: Option<Vec<String>>,
    /// What FAT's unclassified lines said, without names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other: Option<Value>,
    /// Why a FAT check that failed passed anyway: `unclean_unmount` (#44) or `benign`.
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    pub finding: Option<String>,
    /// Run by `verify` after the card was admitted read-only.
    #[serde(
        skip_serializing_if = "std::ops::Not::not",
        deserialize_with = "lenient::is_true"
    )]
    pub background: bool,
}

impl Check {
    /// The checker ran out of time; that is not damage (F49).
    pub fn timed_out(&self) -> bool {
        self.findings.as_ref().is_some_and(|f| f == &["timeout"])
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

    /// A property's text (none when blkid did not print it).
    pub fn text(&self, key: &str) -> Option<String> {
        self.get(key).map(str::to_owned)
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
        assert_eq!(props.text("LABEL"), None);
        assert!(Properties::parse(None).get("TYPE").is_none());
    }
}
