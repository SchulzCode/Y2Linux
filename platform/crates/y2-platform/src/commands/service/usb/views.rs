//! What the computer sees over MTP: the media that may be exported (`media_views`), and the
//! responder's configuration and mount expectations for them.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    internal, mounts, radiofw, read, setup_status, volume_at, Failure, Mount, Root, Value, NAMES,
    OLD,
};
use serde::Serialize;
use y2p_core::volume::{Y2MEDIA_SIZE, Y2MEDIA_START, Y2MEDIA_UUID};

pub(super) fn name_of(path: &str) -> Option<&'static str> {
    NAMES.iter().find(|(p, _)| *p == path).map(|(_, n)| *n)
}

/// One medium the computer may see: written into usb-transfer.json's `views` (sorted), the
/// responder's configuration and its mount expectations.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub(super) struct View {
    pub path: &'static str,
    /// The fixed label of `path` (`NAMES`).
    pub name: Option<&'static str>,
    pub mount_id: i64,
    pub device_id: String,
    pub uuid: Option<String>,
    pub filesystem: String,
    /// `ro` or `rw`.
    pub access: &'static str,
}

/// What must stay the same for a pinned root to stay valid: the mount, its device, its filesystem
/// and the access it was declared with.
pub(super) type Identity = (i64, String, Option<String>, &'static str);

pub(super) fn view(
    mount: &Mount,
    path: &'static str,
    uuid: Option<String>,
    filesystem: &str,
    access: &'static str,
) -> View {
    View {
        path,
        name: name_of(path),
        mount_id: mount.mount_id,
        device_id: mount.device_id.clone(),
        uuid,
        filesystem: filesystem.to_owned(),
        access,
    }
}

/// `media_views(ctx)`: the host receives user music, never Y2DATA/private/system contents or a LUN.
pub(super) fn media_views(root: &Root) -> Vec<View> {
    let (internal, sd) = (
        volume_at(root, "/media/internal"),
        volume_at(root, "/media/sd"),
    );
    let mounts = mounts(root);
    let mut result = Vec::new();
    let at = |path: &str| mounts.iter().find(|m| m.path == path);
    // Internal Storage is the Y2Media partition's music folder; the old /data/music folder
    // on Y2DATA is no longer exported.
    for (volume, path) in [(&internal, "/media/internal/music"), (&sd, "/media/sd")] {
        let mount = volume.get("path").and_then(Value::as_str).and_then(at);
        let folder = root.path(path);
        if path == "/media/internal/music" {
            let device = mount.map(|m| root.path(&format!("/sys/dev/block/{}", m.device_id)));
            let fits = device.is_some_and(|d| {
                read(&d.join("start")).as_deref() == Some(Y2MEDIA_START)
                    && read(&d.join("size")).as_deref() == Some(Y2MEDIA_SIZE)
            });
            if !fits
                || volume.get("uuid").and_then(Value::as_str) != Some(Y2MEDIA_UUID)
                || mount.is_none_or(|m| m.filesystem != "ext4")
            {
                continue;
            }
        }
        // Existing validated internal partition and platform SD mount claim supply the identity.
        // Never create a view of an absent underlying mount.
        let release = root.json_object("/run/y2/usb-release-sd.json");
        if let Some(mount) = mount.filter(|_| path == "/media/sd") {
            if release.get("mount_id").and_then(Value::as_i64) == Some(mount.mount_id) {
                continue;
            }
        }
        let state = volume.get("state").and_then(Value::as_str);
        let Some(mount) = mount else { continue };
        let nested = mounts.iter().any(|m| {
            m.path.starts_with(&format!("{path}/"))
                || (path == "/media/internal/music" && m.path == path)
        });
        let symlink = folder
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink());
        if matches!(state, Some("Ready" | "Degraded")) && folder.is_dir() && !symlink && !nested {
            let access = if mount.read_only() { "ro" } else { "rw" };
            let uuid = volume
                .get("uuid")
                .and_then(Value::as_str)
                .map(str::to_owned);
            result.push(view(mount, path, uuid, &mount.filesystem, access));
        }
    }
    // STORAGE-SETUP case 3: the stock storage, read-only, until the listener sets up Internal Storage,
    // so the old files can be copied off first.
    let old = at(OLD);
    let entry = internal::partition(root);
    if let (Some(old), Some(entry)) = (old, entry) {
        let device = format!(
            "/dev/{}",
            entry.file_name().unwrap_or_default().to_string_lossy()
        );
        if old.source == device
            && old.filesystem == "vfat"
            && setup_status(root).get("state").and_then(Value::as_str) == Some("Waiting")
        {
            result.push(view(old, OLD, None, "vfat", "ro"));
        }
    }
    // RELEASE-49: while the radio firmware is missing, one empty folder of Y2DATA is shown as
    // "Firmware" for the owner's system.img copy. With Internal Storage the copy goes into its
    // Firmware folder instead (#39: Y2DATA is too small for an unpacked image).
    let data = at("/data");
    if !radiofw::present(root) && radiofw::internal(root) {
        // Internal Storage → Firmware, before the computer lists it.
        let _ = radiofw::inbox(root);
    } else if let Some(data) = data.filter(|d| !d.has_option("ro") && !radiofw::present(root)) {
        if radiofw::inbox(root).is_ok() {
            result.push(view(data, radiofw::INBOX, None, &data.filesystem, "rw"));
        }
    }
    result
}

impl View {
    /// `identity(view)`.
    pub(super) fn identity(&self) -> Identity {
        (
            self.mount_id,
            self.device_id.clone(),
            self.uuid.clone(),
            self.access,
        )
    }
}

/// `expectations(views)`: the mount identity the responder must find when it pins a root.
pub(super) fn expectations(views: &[View]) -> String {
    views
        .iter()
        .filter(|v| !v.device_id.is_empty())
        .map(|v| format!("{} {} {}\n", v.path, v.device_id, v.mount_id))
        .collect()
}

/// `config(views)`: only fixed paths and labels reach the responder parser. Both views are always
/// declared, in fixed slots (stable MTP storage IDs); a view that is not currently available is
/// declared `notmounted` and joins or leaves the live session later without a USB re-enumeration.
pub(super) fn config(views: &[View]) -> Result<String, Failure> {
    let mut lines: Vec<String> = [
        "loop_on_disconnect 1",
        "manufacturer \"Y2Linux\"",
        "product \"Y2\"",
        "serial \"Y2LINUX-DEV-01\"",
        "interface \"MTP\"",
        "firmware_version \"Platform 1.0\"",
        "mtp_extensions \"\"",
        "usb_class 0x6",
        "usb_subclass 0x1",
        "usb_protocol 0x1",
        "perceived_device_type 0x2",
        "usb_functionfs_mode 1",
        "usb_dev_path \"/dev/ffs-mtp/ep0\"",
        "usb_epin_path \"/dev/ffs-mtp/ep1\"",
        "usb_epout_path \"/dev/ffs-mtp/ep2\"",
        "usb_epint_path \"/dev/ffs-mtp/ep3\"",
        "usb_max_packet_size 0x200",
        "usb_max_rd_buffer_size 0x10000",
        "usb_max_wr_buffer_size 0x10000",
        "read_buffer_cache_size 0x100000",
        "sync_when_close 0",
        "no_inotify 1",
        "umask 077",
    ]
    .map(str::to_owned)
    .to_vec();
    let mut present: Vec<(&str, &View)> = Vec::new();
    for v in views {
        let path = v.path;
        let named = name_of(path).is_some_and(|n| v.name == Some(n));
        if !named || !matches!(v.access, "ro" | "rw") {
            return Err(Failure::value("unsafe_media_view"));
        }
        match present.iter_mut().find(|(p, _)| *p == path) {
            Some(entry) => entry.1 = v,
            None => present.push((path, v)),
        }
    }
    for (path, name) in NAMES {
        let v = present.iter().find(|(p, _)| *p == path).map(|(_, v)| *v);
        let mut options = vec![if v.is_some_and(|v| v.access == "ro") {
            "ro"
        } else {
            "rw"
        }];
        if path == "/media/sd" {
            options.push("removable");
        }
        if v.is_none() {
            options.push("notmounted");
        }
        lines.push(format!(
            "storage \"{path}\" \"{name}\" \"{}\"",
            options.join(",")
        ));
    }
    Ok(lines.join("\n") + "\n")
}
