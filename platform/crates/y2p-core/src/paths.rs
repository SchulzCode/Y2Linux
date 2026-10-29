//! A path's last component (and its parent's) as text, the way the records name devices and files.
// SPDX-License-Identifier: GPL-2.0-only
use std::path::Path;

/// The file name, empty when there is none.
pub fn name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The parent directory's name (a sysfs attribute's device), empty when there is none.
pub fn parent_name(path: &Path) -> String {
    path.parent().map(name).unwrap_or_default()
}
