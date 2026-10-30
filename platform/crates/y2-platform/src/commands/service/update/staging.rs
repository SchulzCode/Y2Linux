//! `update check` and `update stage`: room on Y2DATA, the signed manifest, the payloads and the
//! package queued with the core; SIGTERM cancels until the commit point.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    atomic_json, channel, copy_stream, core, download, fs, installed_release, journal_state, json,
    lock, pathlib, private_directory, py, set_signal, shortened, signal_disposition, storage,
    string, truthy, wait_online, Disposition, File, Handler, Io, Map, Net, OpenOptions, Ordering,
    Path, PathBuf, Progress, Raise, Result, Root, Source, Value, BACKUPS, CANCELLED,
    CHECK_PROGRESS, FILES, IN_PROGRESS, KEPT_LOGS, MANIFEST_ATTEMPTS, RESERVE,
};
use std::ops::ControlFlow;
use std::os::fd::AsFd;
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;

fn payload_file(key: &str) -> Option<&'static str> {
    match key {
        "rootfs" => Some("rootfs.ext4.gz"),
        "bootimg" => Some("bootimg.img.gz"),
        "recovery" => Some("recovery.img.gz"),
        _ => None,
    }
}

/// The volume must keep the reserve after `size` more bytes.
pub(super) fn free(io: &dyn Io, file: &File, size: i128) -> Result<()> {
    let info = io.space(file)?;
    if info.readonly || info.available < size + RESERVE || (info.files != 0 && info.favail < 128) {
        return Err(Raise::value("update_data_space_reserve"));
    }
    Ok(())
}

fn sorted_entries(directory: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = fs::read_dir(directory)
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    entries.sort();
    entries
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Data the player can lose without losing anything a user sees: rotated and
/// diagnostic logs, rotated platform journals and finished qualification runs. Never music,
/// settings, the library or the rollback copies.
pub(super) fn disposable(root: &Root) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let logs = root.path("/data/logs");
    if logs.is_dir() {
        found.extend(
            sorted_entries(&logs)
                .into_iter()
                .filter(|p| !KEPT_LOGS.contains(&name_of(p).as_str())),
        );
    }
    let platform = root.path("/data/system/platform");
    if platform.is_dir() {
        found.extend(sorted_entries(&platform).into_iter().filter(|p| {
            let name = name_of(p);
            name.ends_with(".previous.jsonl")
                || name == "cpu-idle-qualification"
                || name.starts_with("fix01-")
                || name.starts_with("fix02-")
        }));
    }
    let system = root.path("/data/system");
    if system.is_dir() {
        found.extend(
            sorted_entries(&system)
                .into_iter()
                .filter(|p| y2p_core::root::fnmatch("cpu-final-qualification-*", &name_of(p))),
        );
    }
    found
}

fn tree_bytes(path: &Path) -> u64 {
    let mut total = 0;
    for entry in fs::read_dir(path).into_iter().flatten().flatten() {
        let Ok(meta) = entry.path().symlink_metadata() else {
            continue;
        };
        if meta.file_type().is_dir() {
            total += tree_bytes(&entry.path());
        } else if meta.file_type().is_file() {
            total += meta.len();
        }
    }
    total
}

/// One entry without following links; the bytes freed.
fn remove_disposable(path: &Path) -> std::io::Result<u64> {
    let info = path.symlink_metadata()?;
    if info.file_type().is_dir() {
        let size = tree_bytes(path);
        fs::remove_dir_all(path)?;
        return Ok(size);
    }
    fs::remove_file(path)?;
    Ok(if info.file_type().is_file() {
        info.len()
    } else {
        0
    })
}

/// Count the rollback copies the recovery replaces, then remove disposable data until the download
/// fits (OTA-46). Returns the bytes removed; `NoSpace` with the total the
/// update needs when even that is not enough.
fn make_room(root: &Root, io: &dyn Io, file: &File, size: i128) -> Result<i128> {
    let directory = root.path("/data/updates");
    let mut reclaimable: i128 = 0;
    for name in BACKUPS {
        if let Ok(meta) = directory.join(name).symlink_metadata() {
            if meta.file_type().is_file() {
                reclaimable += meta.len() as i128;
            }
        }
    }
    let need = size + RESERVE - reclaimable;
    let fits = || -> Result<bool> {
        let info = io.space(file)?;
        Ok(!info.readonly && info.available >= need && !(info.files != 0 && info.favail < 128))
    };
    let mut removed: i128 = 0;
    if fits()? {
        return Ok(removed);
    }
    for path in disposable(root) {
        if let Ok(size) = remove_disposable(&path) {
            removed += size as i128;
        }
        if fits()? {
            return Ok(removed);
        }
    }
    // Last: the kept package of the running release (OTA-50). Without it the recovery backs up the
    // live root instead, which is only slower.
    let kept = directory.join("current");
    let size: Option<i128> = if kept.is_dir() {
        fs::read_dir(&kept).ok().map(|entries| {
            entries
                .flatten()
                .map(|e| e.path().symlink_metadata().map_or(0, |m| m.len() as i128))
                .sum()
        })
    } else {
        Some(0)
    };
    if let Some(size) = size {
        if remove_package(&kept).is_ok() {
            removed += size;
            if fits()? {
                return Ok(removed);
            }
        }
    }
    Err(Raise::NoSpace(need))
}

/// Only these known disposable files; unexpected contents stop cleanup.
pub(super) fn remove_package(directory: &Path) -> Result<()> {
    if !directory.exists() {
        return Ok(());
    }
    let meta = directory
        .symlink_metadata()
        .map_err(|e| Raise::os(e, directory))?;
    if !meta.file_type().is_dir() {
        return Err(Raise::value("unsafe_staging_directory"));
    }
    let names: Vec<String> = fs::read_dir(directory)
        .map_err(|e| Raise::os(e, directory))?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    if names.iter().any(|n| !FILES.contains(&n.as_str())) {
        return Err(Raise::value("unknown_staging_contents"));
    }
    for name in &names {
        let file = directory.join(name);
        let info = file.symlink_metadata().map_err(|e| Raise::os(e, &file))?;
        let links = fs::metadata(&file)
            .map_err(|e| Raise::os(e, &file))?
            .nlink();
        if !info.file_type().is_file() || links != 1 {
            return Err(Raise::value("unsafe_staging_file"));
        }
    }
    for name in &names {
        let file = directory.join(name);
        fs::remove_file(&file).map_err(|e| Raise::os(e, &file))?;
    }
    fs::remove_dir(directory).map_err(|e| Raise::os(e, directory))
}

/// The SIGTERM disposition while `stage` runs (`signal.signal(SIGTERM, …)`), restored when it ends.
struct Terminate {
    previous: Option<Disposition>,
}

pub(super) extern "C" fn cancel(_: libc::c_int) {
    CANCELLED.store(true, Ordering::SeqCst);
}

impl Terminate {
    fn save() -> Self {
        Self {
            previous: signal_disposition(libc::SIGTERM).ok(),
        }
    }

    fn set(handler: Handler) {
        // SAFETY: a plain handler (an atomic store) or SIG_IGN, no SA_RESTART so reads are interrupted.
        let _ = unsafe { set_signal(libc::SIGTERM, handler, 0) };
    }
}

impl Drop for Terminate {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            previous.restore(libc::SIGTERM);
        }
    }
}

pub(super) struct Staging<'a> {
    root: &'a Root,
    io: &'a dyn Io,
    directory: PathBuf,
    incoming: PathBuf,
    token: String,
    check_only: bool,
    live: Option<Progress>,
    summary: Value,
    previous: Value,
}

/// A core answer without the manifest's fields (only a broken core gives one).
fn malformed() -> Raise {
    Raise::value("invalid_update_core_response")
}

/// One payload of a signed release: its key (rootfs, bootimg, recovery), size and SHA-256.
struct Payload {
    key: String,
    bytes: i64,
    sha256: Value,
}

/// What the core's `check-manifest` answers about a signed release.
struct Manifest {
    /// The answer itself (what `update check` prints).
    answer: Map<String, Value>,
    /// In the core's order.
    payloads: Vec<Payload>,
    /// What the payloads download.
    total: i64,
    release: Value,
    sequence: Value,
    /// The root filesystem's unpacked size (only a download needs it).
    raw_bytes: Option<i64>,
}

impl Manifest {
    fn read(answer: Map<String, Value>) -> Result<Self> {
        let Some(Value::Object(items)) = answer.get("payloads") else {
            return Err(malformed());
        };
        let payloads = items
            .iter()
            .map(|(key, item)| {
                Ok(Payload {
                    key: key.clone(),
                    bytes: py::exact_int(item.get("bytes")).ok_or_else(malformed)?,
                    sha256: item.get("sha256").cloned().unwrap_or(Value::Null),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let raw_bytes = items
            .get("rootfs")
            .and_then(|rootfs| py::exact_int(rootfs.get("raw_bytes")));
        let (Some(release), Some(sequence)) = (
            answer.get("release_version").cloned(),
            answer.get("sequence").cloned(),
        ) else {
            return Err(malformed());
        };
        Ok(Self {
            total: payloads.iter().map(|p| p.bytes).sum(),
            payloads,
            release,
            sequence,
            raw_bytes,
            answer,
        })
    }

    fn notes(&self) -> Value {
        self.answer.get("notes").cloned().unwrap_or(json!(""))
    }

    /// download.json after a check: the release on offer (Update Now).
    fn offer(&self) -> Value {
        json!({"schema": 1, "state": "Checked", "release_version": self.release,
               "sequence": self.sequence, "download_bytes": self.total, "notes": self.notes(),
               "reborn_version": self.answer.get("reborn_version").cloned().unwrap_or(json!(""))})
    }
}

impl Staging<'_> {
    fn run(
        &mut self,
        file: &File,
        url: Option<&str>,
        source: Option<&str>,
        volume: &storage::Volume,
    ) -> Result<Value> {
        let cleaned = self.begin(file)?;
        let source = self.source(url, source)?;
        if let ControlFlow::Break(up_to_date) = self.fetch_manifest(&source)? {
            return Ok(up_to_date);
        }
        let manifest = match self.check_manifest()? {
            ControlFlow::Break(up_to_date) => return Ok(up_to_date),
            ControlFlow::Continue(manifest) => manifest,
        };
        self.summary = manifest.offer();
        if self.check_only {
            remove_package(&self.incoming)?;
            atomic_json(&self.record(), &self.summary, false)?;
            return Ok(Value::Object(manifest.answer));
        }
        self.download(file, &source, manifest, volume, cleaned)
    }

    /// download.json, what the Software Update page shows.
    fn record(&self) -> PathBuf {
        self.directory.join("download.json")
    }

    /// Everything before the manifest: the lock held, no update pending, earlier leftovers gone, room
    /// for the manifest, the staging directory and download.json saying a check or download runs.
    /// Returns the bytes cleaned.
    fn begin(&mut self, file: &File) -> Result<i128> {
        let (root, io) = (self.root, self.io);
        // stage() refused a busy lock already; this one is held, or fails for another reason.
        match lock::try_fd(file.as_fd(), lock::Kind::Exclusive) {
            Ok(true) => {}
            Ok(false) => return Err(Raise::Os(py::os_error(&lock::would_block(), None).0)),
            Err(e) => return Err(Raise::from(e)),
        }
        if !journal_state(root).terminal() {
            return Err(Raise::value("update_already_pending"));
        }
        // An earlier interrupted download is disposable; bounded, named cleanup.
        let old: Vec<PathBuf> = fs::read_dir(&self.directory)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| {
                        y2p_core::root::fnmatch(".incoming-*", &e.file_name().to_string_lossy())
                    })
                    .map(|e| e.path())
                    .collect()
            })
            .unwrap_or_default();
        if old.len() > 8 {
            return Err(Raise::value("excess_staging_owner_inspection_required"));
        }
        for item in &old {
            remove_package(item)?;
        }
        // A cancelled or failed package is never installed; free its space before this download is
        // measured (OTA-46).
        if !self.check_only {
            remove_package(&self.directory.join("pending"))?;
        }
        // Even the manifest needs the reserve: a nearly full volume is cleaned first, so a check can
        // still find the release (OTA-46).
        let cleaned = make_room(root, io, file, 64 * 1024)?;
        // What the Software Update page showed: a failed check keeps it.
        self.previous = root
            .json("/data/updates/download.json")
            .unwrap_or(Value::Null);
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&self.incoming)
            .map_err(|e| Raise::os(e, &self.incoming))?;
        // A check only reads the signed manifest; it never shows as a download.
        atomic_json(
            &self.record(),
            &json!({"schema": 1, "state": if self.check_only { "Checking" } else { "Staging" },
                    "id": self.token}),
            true,
        )?;
        Ok(cleaned)
    }

    /// The release channel (TLS needs a set clock and entropy, and Wi-Fi online), or the owner's
    /// local package directory.
    fn source(&self, url: Option<&str>, source: Option<&str>) -> Result<Source> {
        let (root, io) = (self.root, self.io);
        let Some(url) = url else {
            let directory = PathBuf::from(source.unwrap_or_default());
            let shown = pathlib(&directory, "");
            let shown = PathBuf::from(shown.to_string_lossy().trim_end_matches('/'));
            let meta = directory
                .symlink_metadata()
                .map_err(|e| Raise::os(e, &shown))?;
            if !meta.file_type().is_dir() {
                return Err(Raise::value("local_package_directory_required"));
            }
            return Ok(Source::Local(directory));
        };
        let clock = io.time_status(root);
        if !clock.get("tls_ready").is_some_and(truthy)
            || !clock.get("entropy_ready").is_some_and(truthy)
        {
            return Err(Raise::value("clock_or_entropy_not_ready_for_TLS"));
        }
        if !wait_online(root, io.network_wait()) {
            return Err(Raise::value("network_not_online"));
        }
        let base = download::base_url(url).map_err(Raise::Value)?;
        Ok(Source::Remote {
            base,
            hosts: channel(root).allowed_redirect_hosts,
        })
    }

    /// The manifest and its signature into the staging directory; a channel that publishes nothing
    /// yet (404 to a check) is up to date.
    fn fetch_manifest(&self, source: &Source) -> Result<ControlFlow<Value>> {
        let (root, io) = (self.root, self.io);
        let remote = matches!(source, Source::Remote { .. });
        for (name, maximum) in [("manifest.json", 16384), ("manifest.sig", 64)] {
            for attempt in 1..=MANIFEST_ATTEMPTS {
                let copied = source.get(io, name, false).and_then(|mut stream| {
                    let exact = name.ends_with(".sig").then_some(64);
                    copy_stream(
                        io,
                        &mut stream,
                        &self.incoming.join(name),
                        maximum,
                        exact,
                        30.0,
                        false,
                        |_| {},
                    )
                });
                match copied {
                    Ok(_) => break,
                    Err(Raise::Net(Net::Http { code, .. }))
                        if self.check_only && code == 404 && name == "manifest.json" =>
                    {
                        // Nothing published on the channel yet: nothing newer to install.
                        return self.up_to_date(Some(false)).map(ControlFlow::Break);
                    }
                    Err(Raise::Net(Net::Url(_) | Net::Timeout))
                        if remote && attempt != MANIFEST_ATTEMPTS =>
                    {
                        // A lookup or handshake that failed while Wi-Fi was still settling: try again
                        // once it is online.
                        let _ = fs::remove_file(self.incoming.join(name));
                        std::thread::sleep(io.retry_pause());
                        wait_online(root, 10.0);
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(ControlFlow::Continue(()))
    }

    /// The core's verdict on the signed manifest; to a check, a release no newer than this one is
    /// up to date.
    fn check_manifest(&self) -> Result<ControlFlow<Value, Manifest>> {
        let device = format!("/data/updates/{}", self.token);
        match core(self.root, "check-manifest", Some(&device), 15.0) {
            Ok(answer) => Manifest::read(answer).map(ControlFlow::Continue),
            Err(Raise::Value(error)) if self.check_only && error.contains("downgrade/replay") => {
                // The channel offers this release (or an older one): up to date.
                self.up_to_date(None).map(ControlFlow::Break)
            }
            Err(error) => Err(error),
        }
    }

    /// A check that found nothing newer: download.json says so (`published` false: the channel has
    /// no release yet).
    fn up_to_date(&self, published: Option<bool>) -> Result<Value> {
        remove_package(&self.incoming)?;
        let installed = installed_release(self.root);
        let mut record = json!({"schema": 1, "state": "Checked", "up_to_date": true,
                                "release_version": installed});
        if let Some(published) = published {
            record["published"] = json!(published);
        }
        atomic_json(&self.record(), &record, false)?;
        Ok(json!({"state": "UpToDate", "release_version": installed}))
    }

    /// The payloads, checked against the manifest, then the package queued with the core.
    fn download(
        &mut self,
        file: &File,
        source: &Source,
        manifest: Manifest,
        volume: &storage::Volume,
        mut cleaned: i128,
    ) -> Result<Value> {
        let (root, io) = (self.root, self.io);
        let raw = manifest.raw_bytes.ok_or_else(malformed)?;
        // Until the package is committed below, SIGTERM (Reborn's Cancel) stops the download and
        // leaves the release offered as before.
        Terminate::set(Handler::Call(cancel));
        // Root payload plus its backup during installation; refuse early, not in recovery.
        cleaned += make_room(
            root,
            io,
            file,
            manifest.total as i128 + raw.div_euclid(4) as i128,
        )?;
        if CANCELLED.load(Ordering::SeqCst) {
            return Err(Raise::Net(Net::Cancelled));
        }
        // Music and the screen come first: download and check yield the CPU.
        // SAFETY: nice(2) only lowers this process's priority.
        unsafe { libc::nice(10) };
        let live = self.live.insert(Progress::new(
            root,
            manifest.release.clone(),
            manifest.total,
        ));
        live.finish("download");
        let mut count: i64 = 0;
        for payload in &manifest.payloads {
            let name = payload_file(&payload.key).ok_or_else(malformed)?;
            let mut stream = source.get(io, name, true)?;
            let (got, digest) = copy_stream(
                io,
                &mut stream,
                &self.incoming.join(name),
                payload.bytes,
                Some(payload.bytes),
                900.0,
                true,
                |done| {
                    live.done = count + done;
                    live.publish("download", false);
                },
            )?;
            drop(stream);
            if payload.sha256.as_str() != Some(digest.as_str()) {
                return Err(Raise::value("download_hash_mismatch"));
            }
            count += got;
        }
        live.done = count;
        // The queue below decodes and checks every payload once (OTA-48: the separate check before it
        // decoded the same files a second time).
        let _ = fs::remove_file(root.path(CHECK_PROGRESS));
        live.finish("verify");
        if CANCELLED.load(Ordering::SeqCst) {
            return Err(Raise::Net(Net::Cancelled));
        }
        if storage::data_volume(root)?.generation != volume.generation {
            return Err(Raise::value("update_data_generation_changed"));
        }
        // The commit point: from here the package is queued, never half-cancelled.
        Terminate::set(Handler::Ignore);
        if CANCELLED.load(Ordering::SeqCst) {
            return Err(Raise::Net(Net::Cancelled));
        }
        let pending = self.directory.join("pending");
        remove_package(&pending)?;
        fs::rename(&self.incoming, &pending).map_err(|e| {
            Raise::Os(format!(
                "{} -> {}",
                py::os_error(&e, Some(&self.incoming.to_string_lossy())).0,
                py::repr(&pending.to_string_lossy())
            ))
        })?;
        let parent = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(&self.directory)
            .map_err(|e| Raise::os(e, &self.directory))?;
        parent.sync_all()?;
        drop(parent);
        let result = core(root, "queue", None, 300.0)?;
        if let Some(live) = self.live.as_mut() {
            live.finish("done");
        }
        atomic_json(
            &self.record(),
            &json!({"schema": 1, "state": "Verified", "bytes": count,
                    "release_version": manifest.release,
                    "notes": manifest.notes(),
                    "backup_budget": "recovery captures the compressed previous root and BOOTIMG before any write",
                    "reserve_bytes": RESERVE as i64, "cleaned_bytes": cleaned as i64}),
            true,
        )?;
        Ok(Value::Object(result))
    }
}

/// Check the signed manifest, and unless `check_only` download the payloads and queue
/// the package with the core.
pub(super) fn stage(
    root: &Root,
    io: &dyn Io,
    source: Option<&str>,
    url: Option<&str>,
    check_only: bool,
) -> Result<Value> {
    let source = source.filter(|s| !s.is_empty());
    let url = url.filter(|u| !u.is_empty());
    if source.is_some() == url.is_some() {
        return Err(Raise::value(
            "exactly_one_local_package_or_HTTPS_manifest_required",
        ));
    }
    let volume = storage::data_volume(root)?;
    if root
        .path("/data/system/platform/maintenance-pending")
        .exists()
    {
        return Err(Raise::value("maintenance_in_progress"));
    }
    let directory = root.path("/data/updates");
    private_directory(&directory)?;
    let lock = directory.join(".stage.lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&lock)
        .map_err(|e| Raise::os(e, &lock))?;
    // A second check or download while one runs (the daily automatic check meeting the owner's) is
    // refused before anything is written: the running one keeps its download.json.
    if let Ok(false) = lock::try_fd(file.as_fd(), lock::Kind::Exclusive) {
        return Err(Raise::value(IN_PROGRESS));
    }
    let token = io.token(root)?;
    let terminate = Terminate::save();
    CANCELLED.store(false, Ordering::SeqCst);
    let mut staging = Staging {
        root,
        io,
        incoming: directory.join(&token),
        directory,
        token,
        check_only,
        live: None,
        summary: Value::Null,
        previous: Value::Null,
    };
    let result = staging.run(&file, url, source, &volume);
    let download = staging.directory.join("download.json");
    let outcome = match result {
        Ok(value) => Ok(value),
        Err(error) if error.cancelled() => {
            let _ = remove_package(&staging.incoming);
            atomic_json(&download, &staging.summary, true)?;
            if let Some(live) = staging.live.as_mut() {
                live.finish("failed");
            }
            Err(error)
        }
        Err(error) => {
            let text = error.text();
            let mut record = json!({"schema": 1, "state": "Failed", "failure": text});
            if check_only && string(staging.previous.get("state")) == Some("Checked") {
                // A check that couldn't reach the server keeps the release the page already offered
                // (Update Now stays); the error goes to the caller.
                let mut kept = staging.previous.as_object().cloned().unwrap_or_default();
                kept.insert("last_check_failure".into(), json!(shortened(&text, 120)));
                record = Value::Object(kept);
            }
            if let Raise::NoSpace(needed) = error {
                record["needed_bytes"] = json!(needed as i64);
                let _ = remove_package(&staging.incoming);
            }
            atomic_json(&download, &record, true)?;
            if let Some(live) = staging.live.as_mut() {
                live.finish("failed");
            }
            Err(error)
        }
    };
    drop(terminate);
    drop(file);
    outcome
}
