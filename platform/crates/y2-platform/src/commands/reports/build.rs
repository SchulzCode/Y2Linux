//! One error report: everything the player records about an error (kernel, platform,
//! update/recovery and Reborn logs and state, hardware identity), every byte through the scrubber,
//! packed as one tar.gz within the receiver's 2 MiB.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    clock, json_dict,
    pyjson::{self, Py, Style},
    pyre,
    scrub::{self, Scrubber, SECTION_MAX},
    secrets, sha256_hex, tarball,
};
use crate::reply::Failure;
use std::{
    cell::Cell,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::{mpsc, Mutex},
};
use y2p_core::{command, root::python_strip, Root};

/// The compressed report (the receiver takes at most 2 MiB; real ones ≈0.5 MB).
pub const BUDGET: usize = 2 * 1024 * 1024;
pub const FOLDER: &str = "Error Reports";
/// Live progress of a report the owner started (Reborn's report screen reads it, like the update's
/// /run/y2/update-progress.json); removed when the run ends.
pub const PROGRESS: &str = "/run/y2/report-progress.json";
pub const TRIGGERS: [&str; 11] = [
    "manual",
    "kernel_crash",
    "kernel_warning",
    "unclean_restart",
    "reborn_crash",
    "reborn_failure",
    "update_failure",
    "sleep_failure",
    "radio_failure",
    "display_failure",
    "storage_error",
];
/// Bound on a file without a size (a seq_file) read through for its tail.
const READ_THROUGH_MAX: usize = 16 * 1024 * 1024;
/// Line-aligned pieces of a log scrubbed in parallel.
const PART: usize = 256 * 1024;
/// The Y2's cores.
const WORKERS: usize = 4;

mod facts;
mod progress;
mod sources;
pub use facts::large_json;
use facts::{hardware, library_statistics, session_projection};
pub use progress::{Bytes, Progress};
use sources::{never, COMMANDS, FILES};

/// The newest `limit` bytes, from the first line start within 4 KiB; and whether it was cut.
pub fn tail(data: &[u8], limit: usize) -> (Vec<u8>, bool) {
    if data.len() <= limit {
        return (data.to_vec(), false);
    }
    let cut = &data[data.len() - limit..];
    match cut.iter().position(|&b| b == b'\n') {
        Some(newline) if newline < 4096 => (cut[newline + 1..].to_vec(), true),
        _ => (cut.to_vec(), true),
    }
}

/// A file's newest SECTION_MAX bytes (and whether it was cut); `None` when it cannot be read.
fn read_bytes(root: &Root, name: &str) -> Option<(Vec<u8>, bool)> {
    let mut file = File::open(root.path(name)).ok()?;
    match file.seek(SeekFrom::End(0)) {
        Err(_) => {
            // seq_file (all of /proc but /proc/sys, debugfs) refuses SEEK_END: until 8.12 every such
            // file was silently left out. Read it through and keep the newest end.
            let (mut data, mut total) = (Vec::new(), 0usize);
            let mut chunk = vec![0u8; 65536];
            while total < READ_THROUGH_MAX {
                let n = file.read(&mut chunk).ok()?;
                if n == 0 {
                    break;
                }
                total += n;
                data.extend_from_slice(&chunk[..n]);
                let keep = SECTION_MAX + 4096;
                if data.len() > keep {
                    data.drain(..data.len() - keep);
                }
            }
            Some(tail(&data, SECTION_MAX))
        }
        Ok(size) => {
            file.seek(SeekFrom::Start(
                size.saturating_sub((SECTION_MAX + 4096) as u64),
            ))
            .ok()?;
            let mut data = Vec::new();
            file.read_to_end(&mut data).ok()?;
            Some(tail(&data, SECTION_MAX))
        }
    }
}

/// A glob result as the device path `Root::read` takes.
pub fn rel(root: &Root, path: &std::path::Path) -> String {
    let base = root.path("/");
    let relative = path.strip_prefix(&base).unwrap_or(path);
    format!("/{}", relative.to_string_lossy().trim_start_matches('/'))
}

/// A file as pieces that scrub independently: logs split at line ends (no pattern spans a
/// line), JSON documents and archives whole.
fn parts(path: &str, data: &[u8]) -> Vec<Vec<u8>> {
    if path.ends_with(".json")
        || path.ends_with(".tar")
        || path.ends_with(".tar.gz")
        || data.len() <= PART
    {
        return vec![data.to_vec()];
    }
    let (mut parts, mut start) = (Vec::new(), 0);
    while start < data.len() {
        let end = data[(start + PART).min(data.len())..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(data.len(), |i| start + PART + i + 1);
        parts.push(data[start..end].to_vec());
        start = end;
    }
    parts
}

/// Each (path, data) scrubbed on the cores; `progress(fraction)` follows the bytes
/// done.
fn scrub_files(
    scrubber: &mut Scrubber,
    items: &[(String, Vec<u8>)],
    progress: impl Fn(f64),
) -> Vec<Vec<u8>> {
    let mut jobs: Vec<(usize, &str, Vec<u8>)> = Vec::new();
    for (n, (path, data)) in items.iter().enumerate() {
        for part in parts(path, data) {
            jobs.push((n, path, part));
        }
    }
    let total = jobs.iter().map(|j| j.2.len()).sum::<usize>().max(1);
    // Big pieces first so no core waits on the last large one.
    let mut order: Vec<usize> = (0..jobs.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(jobs[i].2.len()));
    let next = Mutex::new(order.into_iter());
    let mut results: Vec<Option<Vec<u8>>> = vec![None; jobs.len()];
    let (sender, receiver) = mpsc::channel();
    std::thread::scope(|scope| {
        for _ in 0..WORKERS.min(jobs.len()) {
            let sender = sender.clone();
            let mut worker = scrubber.worker();
            let (jobs, next) = (&jobs, &next);
            scope.spawn(move || loop {
                let next = next
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .next();
                let Some(i) = next else {
                    break;
                };
                let clean = scrub::blob(&mut worker, jobs[i].1, &jobs[i].2);
                if sender
                    .send((i, clean, std::mem::take(&mut worker.counts)))
                    .is_err()
                {
                    break;
                }
            });
        }
        drop(sender);
        let mut done = 0;
        for (i, clean, counts) in receiver {
            done += jobs[i].2.len();
            results[i] = Some(clean);
            for (name, count) in counts {
                *scrubber.counts.entry(name).or_default() += count;
            }
            progress(done as f64 / total as f64);
        }
    });
    let mut cleaned = vec![Vec::new(); items.len()];
    // Every job ran: a worker stops only when the queue is empty.
    for ((owner, _, _), clean) in jobs.iter().zip(results) {
        cleaned[*owner].extend(clean.unwrap_or_default());
    }
    cleaned
}

/// The tar.gz within BUDGET: the largest optional sections are cut first.
fn pack(root: &Root, index: &mut Py, members: &mut [(String, Vec<u8>)]) -> Vec<u8> {
    let mut optional: Vec<usize> = (0..members.len())
        .filter(|&i| members[i].0.starts_with("files/"))
        .collect();
    optional.sort_by_key(|&i| std::cmp::Reverse(members[i].1.len()));
    loop {
        let mut tar = tarball::Writer::new();
        let body = pyjson::dumps(index, Style::Indent).into_bytes();
        let now = || (root.clock().wall_ns() / 1_000_000_000) as i64;
        tar.add(
            &tarball::Info::file("report.json", body.len(), now(), 0o600),
            &body,
        );
        for (name, data) in members.iter() {
            tar.add(&tarball::Info::file(name, data.len(), now(), 0o600), data);
        }
        let packed = tarball::gzip(&tar.finish(), 6, 0);
        if packed.len() <= BUDGET || optional.is_empty() {
            return packed;
        }
        let victim = optional.remove(0);
        let (name, data) = &mut members[victim];
        let half = data.len() / 2;
        data.drain(..half);
        if let Some(section) = index.get_mut("sections").and_then(|s| s.get_mut(name)) {
            section.set("cut", Py::Bool(true));
            section.set("bytes", Py::from(data.len()));
        }
        optional.push(victim);
        optional.sort_by_key(|&i| std::cmp::Reverse(members[i].1.len()));
    }
}

fn random_key() -> Result<[u8; 16], Failure> {
    let mut key = [0u8; 16];
    let mut filled = 0;
    while filled < key.len() {
        // SAFETY: the pointer and length describe the unfilled tail of `key`.
        let n =
            unsafe { libc::getrandom(key[filled..].as_mut_ptr().cast(), key.len() - filled, 0) };
        if n < 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(Failure::from(error));
        }
        filled += n as usize;
    }
    Ok(key)
}

/// The complete report (tar.gz bytes) and its index (report.json).
pub fn build(
    root: &Root,
    triggers: &[String],
    detail: &str,
    topic: Option<&str>,
    install_id: Py,
    now: clock::Instant,
    progress: &Progress,
) -> Result<(Vec<u8>, Py), Failure> {
    if triggers.is_empty() || !triggers.iter().all(|t| TRIGGERS.contains(&t.as_str())) {
        return Err(Failure::value("unknown_trigger"));
    }
    progress.update("collect", 0.0, true, Bytes::NONE);
    let known = secrets::secrets(root)?;
    let mut scrubber = Scrubber::new(&known, random_key()?)?;
    let versions = json_dict(root, "/etc/y2linux/versions.json");
    let release = json_dict(root, "/etc/y2linux/release.json");
    let uptime = root
        .read("/proc/uptime")
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "0".into());
    let uptime = uptime
        .split(pyre::isspace)
        .find(|s| !s.is_empty())
        .unwrap_or("0")
        .to_owned();
    let mut index = Py::dict()
        .with("schema", Py::int(1))
        .with("report_id", Py::Str(super::state::uuid4()?))
        .with("install_id", install_id)
        .with("trigger", Py::str(triggers[0].clone()))
        .with(
            "triggers",
            Py::List(triggers.iter().cloned().map(Py::Str).collect()),
        )
        .with("detail", Py::Str(scrubber.text(detail)))
        .with("topic", topic.into())
        .with("created_utc", Py::Str(clock::isoformat(now)))
        .with(
            "utc_offset_minutes",
            Py::int(clock::utc_offset_minutes(clock::Instant::now(root))),
        )
        .with("uptime_seconds", Py::Str(uptime))
        .with("boot_id", root.boot_id().into())
        .with(
            "release",
            if release.truthy() {
                release
            } else {
                versions.clone()
            },
        )
        .with("versions", versions)
        .with("build_id", root.read("/etc/y2linux/build-id").into())
        .with("kernel", root.read("/proc/sys/kernel/osrelease").into())
        .with("sections", Py::dict())
        .with("scrubbed", Py::dict());
    let mut members: Vec<(String, Vec<u8>)> = Vec::new();
    let mut add = |index: &mut Py, name: &str, data: Vec<u8>, source: Py, cut: bool| {
        if let Some(sections) = index.get_mut("sections") {
            sections.set(
                name,
                Py::dict()
                    .with("bytes", Py::from(data.len()))
                    .with("source", source)
                    .with("cut", Py::Bool(cut)),
            );
        }
        members.push((name.to_owned(), data));
    };
    let hardware = scrubber.value(&hardware(root), None);
    add(
        &mut index,
        "hardware.json",
        pyjson::dumps(&hardware, Style::Indent).into_bytes(),
        Py::str("sysfs"),
        false,
    );
    let session = session_projection(large_json(root, "/data/reborn/state/session.json"))?;
    let session = scrubber.value(&session, None);
    add(
        &mut index,
        "reborn/session.json",
        pyjson::dumps(&session, Style::Indent).into_bytes(),
        Py::str("session.json (projected)"),
        false,
    );
    add(
        &mut index,
        "reborn/library.json",
        pyjson::dumps(&library_statistics(root), Style::Indent).into_bytes(),
        Py::str("library.db (counts)"),
        false,
    );
    // The live commands (≈4 s, mostly waiting) run while the files scrub.
    let runner = {
        let root = root.clone();
        std::thread::spawn(move || {
            COMMANDS
                .iter()
                .map(|(name, argv, timeout, limit)| {
                    (
                        *name,
                        command::run_with(&root, argv, *timeout, *limit, true),
                    )
                })
                .collect::<Vec<_>>()
        })
    };
    let mut seen = std::collections::HashSet::new();
    let mut files: Vec<(String, Vec<u8>, bool)> = Vec::new();
    progress.update("collect", 0.3, false, Bytes::NONE);
    let patterns: Vec<&str> = FILES.iter().copied().filter(|p| !p.is_empty()).collect();
    for (n, pattern) in patterns.iter().enumerate() {
        progress.update(
            "collect",
            0.3 + 0.7 * n as f64 / patterns.len() as f64,
            false,
            Bytes::NONE,
        );
        let paths = if pattern.contains('*') {
            root.glob(pattern)
        } else {
            vec![root.path(pattern)]
        };
        for path in paths {
            let relative = rel(root, &path);
            if seen.contains(&relative) || never(&relative) || !path.is_file() {
                continue;
            }
            seen.insert(relative.clone());
            if let Some((data, cut)) = read_bytes(root, &relative) {
                files.push((relative, data, cut));
            }
        }
    }
    progress.update("scrub", 0.0, true, Bytes::NONE);
    let items: Vec<(String, Vec<u8>)> = files
        .iter()
        .map(|(r, d, _)| (r.clone(), d.clone()))
        .collect();
    let cleaned = scrub_files(&mut scrubber, &items, |fraction| {
        progress.update("scrub", fraction, false, Bytes::NONE)
    });
    progress.update("pack", 0.0, true, Bytes::NONE);
    // The runner thread only collects outcomes (a panic aborts the process before this).
    let commands = runner.join().unwrap_or_default();
    for (name, result) in commands {
        let output = result.output.clone().unwrap_or_default();
        let (data, cut) = tail(output.as_bytes(), SECTION_MAX);
        let clean = match pyjson::loads_bytes(&data) {
            Some(value) => pyjson::dumps(&scrubber.value(&value, None), Style::Indent).into_bytes(),
            None => scrubber.text(&String::from_utf8_lossy(&data)).into_bytes(),
        };
        let source = if result.ok {
            "command".to_owned()
        } else {
            format!("command ({})", result.reason.as_deref().unwrap_or("None"))
        };
        add(
            &mut index,
            &format!("commands/{name}.txt"),
            clean,
            Py::Str(source),
            cut,
        );
    }
    for ((relative, _, cut), clean) in files.iter().zip(cleaned) {
        // Scrubbed archives are re-packed uncompressed (the report itself is gzip).
        let name = relative
            .strip_suffix(".gz")
            .filter(|_| relative.ends_with(".tar.gz"))
            .unwrap_or(relative);
        add(
            &mut index,
            &format!("files{name}"),
            clean,
            Py::str(relative.clone()),
            *cut,
        );
    }
    let counts = scrubber
        .counts
        .iter()
        .map(|(k, v)| (k.clone(), Py::from(*v)))
        .collect();
    index.set("scrubbed", Py::Dict(counts));
    let packed = pack(root, &mut index, &mut members);
    progress.update("pack", 1.0, true, Bytes::NONE);
    Ok((packed, index))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tails_and_parts_follow_python() {
        let data = b"aaaa\nbbbb\ncccc";
        assert_eq!(tail(data, 100), (data.to_vec(), false));
        assert_eq!(tail(data, 7), (b"cccc".to_vec(), true));
        let log: Vec<u8> = (0..PART * 2)
            .map(|i| if i % 100 == 99 { b'\n' } else { b'x' })
            .collect();
        let pieces = parts("/data/logs/system.log", &log);
        assert_eq!(pieces.concat(), log);
        assert!(pieces.len() == 2 && pieces[0].ends_with(b"\n"));
        assert_eq!(parts("/run/y2/x.json", &log).len(), 1);
        assert!(
            never("/data/reborn/state/session.json")
                && never("/x/y.pem")
                && !never("/run/y2/power.json")
        );
    }
}

#[cfg(test)]
#[path = "build_tests.rs"]
mod build_tests;
