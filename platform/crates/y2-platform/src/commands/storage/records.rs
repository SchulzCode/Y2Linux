//! The storage records more than one command reads: the SD operation records (the claim on our SD
//! mount, media-mount.json, written by the mount that made it) with their read-only reason and
//! filesystem check, and `blkid -o export` properties.
// SPDX-License-Identifier: GPL-2.0-only
use super::observe::{MEDIA_MOUNT, MEDIA_STATE};
use super::sys::device_instance;
use crate::reply::Failure;
use serde::Serialize;
use std::collections::HashMap;
use y2p_core::lenient::{self, Shaped};
use y2p_core::{py::truthy, storage_media::splitlines, Root};

pub use y2p_contract::storage::{Check, Inserted, MediaRecord, ReadOnly};

/// What a record says about the player now: its boot, the record files, a refused card.
pub trait OnPlayer: Sized {
    /// The start of every operation record.
    fn begin(root: &Root, action: &str) -> Self;

    /// Our claim on the SD mount.
    fn claim(root: &Root) -> Self;

    /// The last operation's record.
    fn last(root: &Root) -> Self;

    /// Made by this boot for mount `mount_id` (the kernel recycles mount ids across boots).
    fn holds(&self, root: &Root, mount_id: i64) -> bool;

    /// Made by this boot (a missing boot id matches an unreadable one).
    fn is_this_boot(&self, root: &Root) -> bool;

    /// A card of this boot whose admission check `media verify` still has to finish (F49).
    fn checking(&self, root: &Root) -> bool;

    /// The id of the record's card identity (none without one); an identity of another shape is
    /// refused.
    fn identity_id(&self) -> Result<Option<&str>, Failure>;

    /// What the record's filesystem check found (none without a check); a check of another shape is
    /// refused.
    fn findings(&self) -> Result<Option<Vec<String>>, Failure>;

    /// The card still inserted that the last mount refused (its check failed), so the player can say
    /// so instead of "no card" (issue #44). A removed or reinserted card has another disk sequence
    /// number.
    fn refused(root: &Root) -> Option<RefusedCard>;
}

impl OnPlayer for MediaRecord {
    /// The start of every operation record.
    fn begin(root: &Root, action: &str) -> MediaRecord {
        MediaRecord {
            schema: Some(1),
            action: Some(action.into()),
            state: Some("Starting".into()),
            boot_id: Some(root.boot_id().into()),
            ..MediaRecord::default()
        }
    }

    /// Our claim on the SD mount.
    fn claim(root: &Root) -> MediaRecord {
        lenient::read(root.json_object(MEDIA_MOUNT))
    }

    /// The last operation's record.
    fn last(root: &Root) -> MediaRecord {
        lenient::read(root.json_object(MEDIA_STATE))
    }

    /// Made by this boot for mount `mount_id` (the kernel recycles mount ids across boots).
    fn holds(&self, root: &Root, mount_id: i64) -> bool {
        self.mount_id == Some(mount_id) && self.is_this_boot(root)
    }

    /// Made by this boot (a missing boot id matches an unreadable one).
    fn is_this_boot(&self, root: &Root) -> bool {
        self.of_boot(root.boot_id().as_deref())
    }

    /// A card of this boot whose admission check `media verify` still has to finish (F49).
    fn checking(&self, root: &Root) -> bool {
        self.read_only() == Some(ReadOnly::Checking) && self.is_this_boot(root)
    }

    /// The id of the record's card identity (none without one); an identity of another shape is
    /// refused.
    fn identity_id(&self) -> Result<Option<&str>, Failure> {
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
    fn findings(&self) -> Result<Option<Vec<String>>, Failure> {
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
    fn refused(root: &Root) -> Option<RefusedCard> {
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
