//! `y2-platform codec-settings [--{sbc,ldac,aac}-quality …] [--{sbc,ldac,aac}-abr on|off]`: the
//! owner's Bluetooth codec quality. Validated codec quality preferences, separated from the active
//! daemon contract: they take effect at the next bluealsad start (a player restart), never by
//! restarting the daemon here. Errors print `{"schema": "org.y2linux.codec-settings/v1", "failure": …}`.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::At;
use crate::{
    commands::codec::{requested, settings, POLICY, SETTINGS_SCHEMA},
    reply::{atomic_json, Failure, Outcome, Reply},
};
use y2p_core::{
    json::{Object, ObjectExt},
    Root,
};

/// The options given (`None` for the others); `on`/`off` already as booleans.
pub struct Changes {
    pub sbc_quality: Option<String>,
    pub sbc_abr: Option<bool>,
    pub ldac_quality: Option<String>,
    pub ldac_abr: Option<bool>,
    pub aac_quality: Option<String>,
    pub aac_abr: Option<bool>,
}

impl Changes {
    fn is_empty(&self) -> bool {
        self.sbc_quality.is_none()
            && self.sbc_abr.is_none()
            && self.ldac_quality.is_none()
            && self.ldac_abr.is_none()
            && self.aac_quality.is_none()
            && self.aac_abr.is_none()
    }
}

/// The settings after the changes, or `{"schema": …, "failure": …}` with exit 1 (Reborn shows it).
pub fn configure(root: &Root, changes: Changes) -> Outcome {
    match configure_inner(root, changes) {
        Ok(value) => Reply::unsorted(&value, 0),
        Err(error) => Reply::unsorted(
            &Object::new()
                .with("schema", SETTINGS_SCHEMA)
                .with("failure", error.message()),
            1,
        ),
    }
}

fn configure_inner(
    root: &Root,
    changes: Changes,
) -> Result<crate::commands::codec::Settings, Failure> {
    if changes.is_empty() {
        return settings(root);
    }
    let path = root.path("/run/y2/codec-settings.lock");
    let _lock = y2p_core::lock::try_exclusive(&path)
        .at(&path)?
        .ok_or_else(Failure::busy)?;
    let mut policy = requested(root)?;
    let ldac = changes.ldac_quality.is_some() || changes.ldac_abr.is_some();
    let aac = changes.aac_quality.is_some() || changes.aac_abr.is_some();
    if let Some(quality) = changes.sbc_quality {
        policy.sbc_quality = quality;
    }
    if let Some(abr) = changes.sbc_abr {
        policy.sbc_abr = abr;
    }
    if let Some(quality) = changes.ldac_quality {
        policy.ldac_quality = quality;
    }
    if let Some(abr) = changes.ldac_abr {
        policy.ldac_abr = abr;
    }
    if let Some(quality) = changes.aac_quality {
        policy.aac_quality = quality;
    }
    if let Some(abr) = changes.aac_abr {
        policy.aac_abr = abr;
    }
    policy.validate()?;
    let supported = settings(root)?.supported;
    if ldac && supported.ldac_quality.is_empty() {
        return Err(Failure::value("ldac_encoder_not_compiled"));
    }
    if aac && supported.aac_quality.is_empty() {
        return Err(Failure::value("aac_encoder_not_compiled"));
    }
    atomic_json(&root.path(POLICY), &policy, true)?;
    settings(root)
}

#[cfg(test)]
#[path = "codec_settings_tests.rs"]
mod codec_settings_tests;
