//! A path's last component (and its parent's) as text, the way the records name devices and files.
// SPDX-License-Identifier: GPL-2.0-only
use std::{
    collections::HashMap,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

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

/// `os.path.realpath(path, strict=False)` (`Path.resolve()`): symlinks resolved as far as the path
/// exists, the rest kept as written; a loop leaves the looping link unresolved.
pub fn realpath(path: &Path) -> PathBuf {
    let text = path.to_string_lossy().into_owned();
    let mut rest: Vec<Option<String>> = text.split('/').rev().map(|s| Some(s.to_owned())).collect();
    let mut part_count = rest.len();
    let mut resolved = if text.starts_with('/') {
        "/".to_owned()
    } else {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "/".into())
    };
    let mut seen: HashMap<String, Option<String>> = HashMap::new();
    while part_count > 0 {
        let Some(name) = rest.pop() else { break };
        let Some(name) = name else {
            // A symlink's target is resolved: remember it for the link popped next.
            if let Some(Some(link)) = rest.pop() {
                seen.insert(link, Some(resolved.clone()));
            }
            continue;
        };
        part_count -= 1;
        if name.is_empty() || name == "." {
            continue;
        }
        if name == ".." {
            resolved = match resolved.rfind('/') {
                Some(0) | None => "/".into(),
                Some(index) => resolved[..index].to_owned(),
            };
            continue;
        }
        let candidate = if resolved == "/" {
            format!("/{name}")
        } else {
            format!("{resolved}/{name}")
        };
        let Ok(meta) = std::fs::symlink_metadata(&candidate) else {
            resolved = candidate;
            continue;
        };
        if !meta.file_type().is_symlink() {
            resolved = candidate;
            continue;
        }
        if let Some(known) = seen.get(&candidate) {
            match known {
                Some(target) => resolved = target.clone(),
                None => resolved = candidate, // a symlink loop: kept unresolved
            }
            continue;
        }
        let Ok(target) = std::fs::read_link(&candidate) else {
            resolved = candidate;
            continue;
        };
        let target = String::from_utf8_lossy(target.as_os_str().as_bytes()).into_owned();
        if target.starts_with('/') {
            resolved = "/".into();
        }
        seen.insert(candidate.clone(), None);
        rest.push(Some(candidate));
        rest.push(None);
        let parts: Vec<Option<String>> = target
            .split('/')
            .rev()
            .map(|s| Some(s.to_owned()))
            .collect();
        part_count += parts.len();
        rest.extend(parts);
    }
    PathBuf::from(resolved)
}
