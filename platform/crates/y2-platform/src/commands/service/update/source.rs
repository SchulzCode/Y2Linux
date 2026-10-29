//! Where a package comes from: the owner's local directory or the release channel over HTTPS
//! (`Io`, the channel, the streams and their copy with its live progress).
// SPDX-License-Identifier: GPL-2.0-only
use super::*;

/// The release channel: the shipped one, with the owner's `name` and `manifest_url` from
/// /data/updates/channel.json where it names them. Redirects go only to the shipped hosts.
pub(super) struct Channel {
    /// Shown as `update status`'s `channel` (null without one).
    pub(super) name: Value,
    pub(super) manifest_url: Option<Value>,
    pub(super) allowed_redirect_hosts: Vec<String>,
}

pub(super) fn channel(root: &Root) -> Channel {
    let shipped = root.json_object(CHANNEL);
    let owner = root.json_object("/data/updates/channel.json");
    let pick = |key: &str| owner.get(key).or_else(|| shipped.get(key)).cloned();
    // Only a list of names: anything else (an empty or non-string entry too) allows no redirect.
    let allowed_redirect_hosts = match shipped.get("allowed_redirect_hosts") {
        Some(Value::Array(items)) => items
            .iter()
            .map(|h| h.as_str().filter(|s| !s.is_empty()).map(str::to_owned))
            .collect::<Option<Vec<String>>>()
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    Channel {
        name: pick("name").unwrap_or(Value::Null),
        manifest_url: pick("manifest_url"),
        allowed_redirect_hosts,
    }
}

/// What `fstatvfs` says about the volume an update writes to.
pub(super) struct Space {
    pub(super) readonly: bool,
    pub(super) available: i128,
    pub(super) files: u64,
    pub(super) favail: u64,
}

/// What a check or download needs beyond the root's files and commands: the clock's TLS readiness,
/// the wall clock, the release server, the free space on Y2DATA, the staging directory's name and
/// the waits for Wi-Fi. [`Player`] is the player's own; unit tests give theirs (update_tests.rs).
pub(super) trait Io {
    /// `y2-platform time`'s record (`tls_ready`, `entropy_ready`).
    fn time_status(&self, root: &Root) -> Map<String, Value> {
        crate::commands::service::time_status(root)
    }

    /// Seconds since the epoch: when the daily check last ran and runs now.
    fn now(&self) -> f64 {
        crate::commands::time::wall()
    }

    /// One file of the release over HTTPS (`url`: the channel's base and the file's name), following
    /// redirects only to `hosts`.
    fn fetch(&self, url: &str, hosts: &[String], cancellable: bool) -> Result<Stream> {
        let response = download::open(url, hosts, cancellable).map_err(Raise::Net)?;
        if response.status != 200 {
            return Err(Raise::value("HTTP_status"));
        }
        Ok(Stream::Http(response))
    }

    /// The free space and inodes of the volume `file` is on.
    fn space(&self, file: &File) -> Result<Space> {
        let info = y2p_core::sys::fstatvfs(file.as_fd())
            .map_err(|e| Raise::Os(py::os_error(&e, None).0))?;
        Ok(Space {
            readonly: info.read_only(),
            available: info.bavail as i128 * info.frsize as i128,
            files: info.files,
            favail: info.favail,
        })
    }

    /// The staging directory's name, `.incoming-<uuid4 hex>`; below a fixture root a host test may
    /// fix it (`Y2_FIXTURE_TOKEN`, tests/platform_cases/service.py).
    fn token(&self, root: &Root) -> Result<String> {
        if !root.is_real() {
            if let Ok(fixed) = std::env::var("Y2_FIXTURE_TOKEN") {
                return Ok(format!(".incoming-{fixed}"));
            }
        }
        let id = py::uuid4().map_err(|e| Raise::Os(py::os_error(&e, None).0))?;
        Ok(format!(".incoming-{}", id.replace('-', "")))
    }

    /// How long a check waits for Wi-Fi to be online.
    fn network_wait(&self) -> f64 {
        NETWORK_WAIT_S
    }

    /// The pause before another manifest attempt.
    fn retry_pause(&self) -> Duration {
        RETRY_PAUSE
    }
}

/// The player's clocks, network and volume.
pub(super) struct Player;

impl Io for Player {}

/// Live download progress for Reborn in RAM (OTA-48): bytes, speed over the last few seconds and time
/// left, at most four times a second. The durable journal on Y2DATA only records the start and the
/// result.
pub(super) struct Progress {
    pub(super) root: Root,
    pub(super) path: PathBuf,
    pub(super) release: Value,
    pub(super) total: i64,
    pub(super) samples: VecDeque<(f64, i64)>,
    pub(super) written: f64,
    pub(super) done: i64,
}

impl Progress {
    const WINDOW: f64 = 4.0;

    pub(super) fn new(root: &Root, release: Value, total: i64) -> Self {
        Self {
            root: root.clone(),
            path: root.path(PROGRESS),
            release,
            total,
            samples: VecDeque::new(),
            written: 0.0,
            done: 0,
        }
    }

    pub(super) fn publish(&mut self, phase: &str, force: bool) {
        let now = self.root.clock().monotonic();
        if !force && now - self.written < 0.25 {
            return;
        }
        self.written = now;
        self.samples.push_back((now, self.done));
        while self.samples.len() > 2 && now - self.samples[0].0 > Self::WINDOW {
            self.samples.pop_front();
        }
        let ((t0, b0), (t1, b1)) = (self.samples[0], self.samples[self.samples.len() - 1]);
        let speed = (t1 - t0 >= 0.5).then(|| ((b1 - b0) as f64 / (t1 - t0)) as i64);
        let left = speed
            .filter(|s| *s != 0 && phase == "download")
            .map(|s| ((self.total - self.done) as f64 / s as f64) as i64);
        let value = json!({"schema": 1, "phase": phase, "release_version": self.release,
                           "bytes_done": self.done, "bytes_total": self.total,
                           "speed_bytes_per_s": speed, "seconds_left": left});
        let _ = record::atomic_json(&self.path, &value, false, 0o644);
    }

    pub(super) fn finish(&mut self, phase: &str) {
        self.publish(phase, true);
    }
}

/// A source of package files: the owner's local directory or the release channel over HTTPS.
pub(super) enum Source {
    Local(PathBuf),
    Remote { base: String, hosts: Vec<String> },
}

pub(super) enum Stream {
    File(File),
    Http(Response),
}

impl Stream {
    fn read(&mut self, limit: usize, cancellable: bool) -> Result<Vec<u8>> {
        match self {
            Stream::File(file) => {
                if cancellable && CANCELLED.load(Ordering::SeqCst) {
                    return Err(Raise::Net(Net::Cancelled));
                }
                let mut out = Vec::new();
                Read::by_ref(file)
                    .take(limit as u64)
                    .read_to_end(&mut out)
                    .map_err(|e| Raise::Os(py::os_error(&e, None).0))?;
                Ok(out)
            }
            Stream::Http(response) => response.read(limit, cancellable).map_err(Raise::Net),
        }
    }
}

/// `str(directory / name)` as pathlib spells it (no empty or `.` components).
pub(super) fn pathlib(directory: &Path, name: &str) -> PathBuf {
    let text = directory.to_string_lossy();
    let mut out = String::new();
    if text.starts_with('/') {
        out.push('/');
    }
    let parts: Vec<&str> = text
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .chain([name])
        .collect();
    out.push_str(&parts.join("/"));
    PathBuf::from(out)
}

impl Source {
    /// `get(name)`: the opened response or file.
    pub(super) fn get(&self, io: &dyn Io, name: &str, cancellable: bool) -> Result<Stream> {
        match self {
            Source::Remote { base, hosts } => {
                io.fetch(&format!("{base}{name}"), hosts, cancellable)
            }
            Source::Local(directory) => {
                let path = pathlib(directory, name);
                let file = OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                    .open(&path)
                    .map_err(|e| Raise::os(e, &path))?;
                let meta = file
                    .metadata()
                    .map_err(|e| Raise::Os(py::os_error(&e, None).0))?;
                if !meta.file_type().is_file() {
                    return Err(Raise::value("local_package_regular_files_required"));
                }
                Ok(Stream::File(file))
            }
        }
    }
}

/// `update.copy_stream`: at most `maximum` bytes (exactly `exact`), within `seconds`, keeping the
/// reserve; returns the count and the SHA-256. Interrupted bytes stay for explicit diagnostics.
#[allow(clippy::too_many_arguments)]
pub(super) fn copy_stream(
    io: &dyn Io,
    source: &mut Stream,
    destination: &Path,
    maximum: i64,
    exact: Option<i64>,
    seconds: f64,
    cancellable: bool,
    mut report: impl FnMut(i64),
) -> Result<(i64, String)> {
    let mut digest = Sha256::new();
    let mut stream = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(destination)
        .map_err(|e| Raise::os(e, destination))?;
    let mut count: i64 = 0;
    let deadline = clock::monotonic() + seconds;
    loop {
        if clock::monotonic() >= deadline {
            return Err(Raise::value("download_deadline"));
        }
        let want = (1024 * 1024).min(maximum - count + 1).max(0) as usize;
        let chunk = source.read(want, cancellable)?;
        if chunk.is_empty() {
            break;
        }
        count += chunk.len() as i64;
        if count > maximum {
            return Err(Raise::value("download_size_limit"));
        }
        free(io, &stream, chunk.len() as i128)?;
        stream
            .write_all(&chunk)
            .map_err(|e| Raise::Os(py::os_error(&e, None).0))?;
        digest.update(&chunk);
        report(count);
    }
    if exact.is_some_and(|e| e != count) {
        return Err(Raise::value("incomplete_download"));
    }
    stream
        .sync_all()
        .map_err(|e| Raise::Os(py::os_error(&e, None).0))?;
    Ok((count, digest.hexdigest()))
}

/// `update.wait_online`: true once Wi-Fi is Online (DNS answered), waiting at most `seconds`.
pub(super) fn wait_online(root: &Root, seconds: f64) -> bool {
    let deadline = root.clock().monotonic() + seconds;
    while root.json_object("/run/y2/network.json").get("state") != Some(&json!("Online")) {
        if root.clock().monotonic() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    true
}
