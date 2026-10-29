//! What the computer sees over MTP: the media that may be exported (`media_views`), and the
//! responder's configuration and mount expectations for them.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    internal, json, mounts, py, radiofw, read, setup_status, volumes, Failure, Mount, Object, Root,
    Value, NAMES, OLD,
};
use y2p_core::py::PyValue;

pub(super) fn name_of(path: &str) -> Option<&'static str> {
    NAMES.iter().find(|(p, _)| *p == path).map(|(_, n)| *n)
}

pub(super) fn text<'a>(view: &'a Object, key: &str) -> Option<&'a str> {
    view.get(key).and_then(Value::as_str)
}

pub(super) fn view(
    mount: &Mount,
    path: &str,
    uuid: Value,
    filesystem: &str,
    access: &str,
) -> Object {
    let mut value = Object::new();
    value.insert("path".into(), json!(path));
    value.insert("name".into(), json!(name_of(path)));
    value.insert("mount_id".into(), json!(mount.mount_id));
    value.insert("device_id".into(), json!(mount.device_id));
    value.insert("uuid".into(), uuid);
    value.insert("filesystem".into(), json!(filesystem));
    value.insert("access".into(), json!(access));
    value
}

/// `media_views(ctx)`: the host receives user music, never Y2DATA/private/system contents or a LUN.
pub(super) fn media_views(root: &Root) -> Vec<Object> {
    let volumes = volumes(root);
    let mounts = mounts(root);
    let mut result = Vec::new();
    let at = |path: &str| mounts.iter().find(|m| m.path == path);
    // Internal Storage is the Y2Media partition's music folder (volumes[3]); the old /data/music folder
    // on Y2DATA is no longer exported.
    for (volume, path) in [
        (&volumes[3], "/media/internal/music"),
        (&volumes[2], "/media/sd"),
    ] {
        let mount = volume.get("path").and_then(Value::as_str).and_then(at);
        let folder = root.path(path);
        if path == "/media/internal/music" {
            let device = mount.map(|m| root.path(&format!("/sys/dev/block/{}", m.device_id)));
            let fits = device.is_some_and(|d| {
                read(&d.join("start")).as_deref() == Some("3742720")
                    && read(&d.join("size")).as_deref() == Some("11460608")
            });
            if !fits
                || volume.get("uuid").and_then(Value::as_str)
                    != Some("79324c69-6e75-4801-8000-000000000103")
                || mount.is_none_or(|m| m.filesystem != "ext4")
            {
                continue;
            }
        }
        // Existing validated internal partition and platform SD mount claim supply the identity.
        // Never create a view of an absent underlying mount.
        let release = root.json_object("/run/y2/usb-release-sd.json");
        if let Some(mount) = mount.filter(|_| path == "/media/sd") {
            if release
                .get("mount_id")
                .unwrap_or(&Value::Null)
                .py_eq(&json!(mount.mount_id))
            {
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
            let uuid = volume.get("uuid").cloned().unwrap_or(Value::Null);
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
            result.push(view(old, OLD, Value::Null, "vfat", "ro"));
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
            result.push(view(
                data,
                radiofw::INBOX,
                Value::Null,
                &data.filesystem,
                "rw",
            ));
        }
    }
    result
}

/// `identity(view)`.
pub(super) fn identity(view: &Object) -> Value {
    json!([
        view.get("mount_id"),
        view.get("device_id"),
        view.get("uuid"),
        view.get("access")
    ])
}

/// `str(value)` of a record's field (`'%s'`), a dict's keys sorted as the records are written.
fn py_text(value: Option<&Value>) -> String {
    y2p_core::json::sorted(value.cloned().unwrap_or_default()).py_str()
}

/// `expectations(views)`: the mount identity the responder must find when it pins a root.
pub(super) fn expectations(views: &[Object]) -> String {
    views
        .iter()
        .filter(|v| {
            v.get("device_id").is_some_and(py::truthy)
                && !v.get("mount_id").is_none_or(Value::is_null)
        })
        .map(|v| {
            format!(
                "{} {} {}\n",
                py_text(v.get("path")),
                py_text(v.get("device_id")),
                py_text(v.get("mount_id"))
            )
        })
        .collect()
}

/// `config(views)`: only fixed paths and labels reach the responder parser. Both views are always
/// declared, in fixed slots (stable MTP storage IDs); a view that is not currently available is
/// declared `notmounted` and joins or leaves the live session later without a USB re-enumeration.
pub(super) fn config(views: &[Object]) -> Result<String, Failure> {
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
    let mut present: Vec<(&str, &Object)> = Vec::new();
    for v in views {
        let path = text(v, "path").unwrap_or_default();
        let named = name_of(path).is_some_and(|n| text(v, "name") == Some(n));
        if !named || !matches!(text(v, "access"), Some("ro" | "rw")) {
            return Err(Failure::value("unsafe_media_view"));
        }
        match present.iter_mut().find(|(p, _)| *p == path) {
            Some(entry) => entry.1 = v,
            None => present.push((path, v)),
        }
    }
    for (path, name) in NAMES {
        let v = present.iter().find(|(p, _)| *p == path).map(|(_, v)| *v);
        let mut options = vec![if v.is_some_and(|v| text(v, "access") == Some("ro")) {
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
