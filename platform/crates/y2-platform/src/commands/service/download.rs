//! The release download of `update.py` (urllib in Python) through the `curl` binary, as Reborn does:
//! no TLS stack in this binary (plan §3.7). One request per hop, `curl --include` without following
//! redirects, so this module applies `AllowlistedRedirect` itself: HTTPS only, to the channel's listed
//! hosts, at most 5 hops; authenticity comes from the signed manifest, never from the transport. The
//! request is urllib's: HTTP/1.1, no proxy, `Accept-Encoding: identity`, `Connection: close`, its
//! User-Agent, 10 s for the connection and for each wait on data.
//!
//! Errors keep Python's classes and the texts callers match (Reborn's `update_problem` reads "url",
//! "http", "network"): `<urlopen error …>` (URLError: retried for the manifest), `HTTP Error <code>:
//! <reason>` (HTTPError: never retried), `timed out` (TimeoutError while reading the body).
// SPDX-License-Identifier: GPL-2.0-only
use std::{
    io::Read,
    os::fd::AsRawFd,
    process::{Child, ChildStdout, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
};

/// Set by the SIGTERM handler while a download may be cancelled (`update._cancel`).
pub static CANCELLED: AtomicBool = AtomicBool::new(false);

/// What a read or a request can raise, in Python's terms.
#[derive(Debug)]
pub enum Error {
    /// `urllib.error.HTTPError` (a URLError, but never retried by `stage`).
    Http { code: u16, text: String },
    /// `urllib.error.URLError` before a response arrived.
    Url(String),
    /// `TimeoutError('timed out')` while reading.
    Timeout,
    /// A ValueError raised on the way (`update_redirect_refused`).
    Value(String),
    /// Another OSError while reading (`[Errno 104] Connection reset by peer`).
    Os(String),
    /// SIGTERM while the handler is installed (`Cancelled('download_cancelled')`).
    Cancelled,
}

impl Error {
    pub fn text(&self) -> String {
        match self {
            Error::Http { text, .. } => text.clone(),
            Error::Url(text) => format!("<urlopen error {text}>"),
            Error::Timeout => "timed out".into(),
            Error::Value(text) | Error::Os(text) => text.clone(),
            Error::Cancelled => "download_cancelled".into(),
        }
    }
}

/// `urllib.parse.urlsplit` for what the checks need.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Parts {
    pub scheme: String,
    pub netloc: String,
    pub path: String,
    pub query: String,
    pub fragment: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub hostname: Option<String>,
}

pub fn urlsplit(url: &str) -> Parts {
    let url = url.trim_start_matches(|c: char| c <= ' ');
    let url: String = url
        .chars()
        .filter(|c| !matches!(c, '\t' | '\r' | '\n'))
        .collect();
    let mut parts = Parts::default();
    let mut rest = url.as_str();
    if let Some(colon) = rest.find(':') {
        let scheme = &rest[..colon];
        if !scheme.is_empty()
            && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        {
            parts.scheme = scheme.to_ascii_lowercase();
            rest = &rest[colon + 1..];
        }
    }
    if let Some(after) = rest.strip_prefix("//") {
        let end = after.find(['/', '?', '#']).unwrap_or(after.len());
        parts.netloc = after[..end].to_owned();
        rest = &after[end..];
    }
    if let Some((before, fragment)) = rest.split_once('#') {
        parts.fragment = fragment.to_owned();
        rest = before;
    }
    if let Some((before, query)) = rest.split_once('?') {
        parts.query = query.to_owned();
        rest = before;
    }
    parts.path = rest.to_owned();
    let (userinfo, host) = match parts.netloc.rsplit_once('@') {
        Some((userinfo, host)) => (Some(userinfo), host),
        None => (None, parts.netloc.as_str()),
    };
    if let Some(userinfo) = userinfo {
        match userinfo.split_once(':') {
            Some((user, password)) => {
                parts.username = Some(user.to_owned());
                parts.password = Some(password.to_owned());
            }
            None => parts.username = Some(userinfo.to_owned()),
        }
    }
    let host = if let Some(inner) = host.strip_prefix('[') {
        inner.split(']').next().unwrap_or("")
    } else {
        host.split(':').next().unwrap_or("")
    };
    parts.hostname = (!host.is_empty()).then(|| host.to_ascii_lowercase());
    parts
}

fn has_credentials(parts: &Parts) -> bool {
    parts.username.as_deref().is_some_and(|u| !u.is_empty())
        || parts.password.as_deref().is_some_and(|p| !p.is_empty())
}

/// `update.base_url`.
pub fn base_url(url: &str) -> Result<String, String> {
    let parts = urlsplit(url);
    if parts.scheme != "https"
        || parts.hostname.is_none()
        || has_credentials(&parts)
        || !parts.fragment.is_empty()
        || !parts.query.is_empty()
        || !parts.path.ends_with("/manifest.json")
        || url.chars().any(|c| (c as u32) < 33)
    {
        return Err("HTTPS_manifest_URL_required_without_credentials_query_or_redirect".into());
    }
    Ok(url[..url.len() - "manifest.json".len()].to_owned())
}

/// `AllowlistedRedirect.redirect_request`'s check.
pub(super) fn redirect_allowed(url: &str, hosts: &[String]) -> bool {
    let parts = urlsplit(url);
    parts.scheme == "https"
        && parts
            .hostname
            .as_ref()
            .is_some_and(|h| hosts.iter().any(|allowed| allowed.to_lowercase() == *h))
        && !has_credentials(&parts)
        && !url.chars().any(|c| (c as u32) < 33)
}

/// `quote(newurl, encoding='iso-8859-1', safe=string.punctuation)` then `urljoin(base, newurl)`.
pub(super) fn join(base: &str, location: &str) -> String {
    let mut quoted = String::new();
    for c in location.chars() {
        if c.is_ascii_alphanumeric() || c.is_ascii_punctuation() {
            quoted.push(c);
        } else {
            let mut buffer = [0u8; 4];
            for byte in c.encode_utf8(&mut buffer).bytes() {
                quoted.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    let target = urlsplit(&quoted);
    if !target.scheme.is_empty() {
        return quoted;
    }
    let source = urlsplit(base);
    if quoted.starts_with("//") {
        return format!("{}:{quoted}", source.scheme);
    }
    let origin = format!("{}://{}", source.scheme, source.netloc);
    if quoted.starts_with('/') {
        return origin + &quoted;
    }
    let directory = match source.path.rfind('/') {
        Some(index) => &source.path[..=index],
        None => "/",
    };
    let (path, tail) = match quoted.find(['?', '#']) {
        Some(index) => (&quoted[..index], &quoted[index..]),
        None => (quoted.as_str(), ""),
    };
    let mut segments: Vec<&str> = Vec::new();
    let joined = format!("{directory}{path}");
    let pieces: Vec<&str> = joined.split('/').collect();
    for (index, piece) in pieces.iter().enumerate() {
        match *piece {
            "." => {}
            ".." => {
                if segments.len() > 1 {
                    segments.pop();
                }
            }
            other => segments.push(other),
        }
        if index == pieces.len() - 1 && matches!(*piece, "." | "..") {
            segments.push("");
        }
    }
    format!("{origin}{}{tail}", segments.join("/"))
}

pub const USER_AGENT: &str = "Python-urllib/3.15";
const MAX_REDIRECTIONS: usize = 5;

/// One response body being read (`opener.open(url, timeout=10)`).
pub struct Response {
    child: Child,
    stdout: ChildStdout,
    pending: Vec<u8>,
    pub status: u16,
    finished: bool,
}

impl Drop for Response {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn(url: &str) -> Result<(Child, ChildStdout), Error> {
    let mut child = Command::new("curl")
        .args([
            "--silent",
            "--include",
            "--http1.1",
            "--globoff",
            "--proto",
            "=https",
            "--noproxy",
            "*",
            "--connect-timeout",
            "10",
            "--speed-limit",
            "1",
            "--speed-time",
            "10",
            "--user-agent",
            USER_AGENT,
            "--header",
            "Accept:",
            "--header",
            "Accept-Encoding: identity",
            "--header",
            "Connection: close",
            url,
        ])
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| Error::Url(y2p_core::py::os_error(&e, None).0))?;
    match child.stdout.take() {
        Some(stdout) => Ok((child, stdout)),
        // Not with a piped stdout; refused like a curl that could not start.
        None => {
            let _ = child.kill();
            let _ = child.wait();
            Err(Error::Url("curl gave no output pipe".into()))
        }
    }
}

/// Read from the pipe, waking every quarter second to notice a cancel.
fn read_some(
    stdout: &mut ChildStdout,
    buffer: &mut [u8],
    cancellable: bool,
) -> Result<usize, Error> {
    loop {
        if cancellable && CANCELLED.load(Ordering::SeqCst) {
            return Err(Error::Cancelled);
        }
        let mut poll = libc::pollfd {
            fd: stdout.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd.
        let ready = unsafe { libc::poll(&mut poll, 1, 250) };
        if ready == 0 {
            continue;
        }
        if ready < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(Error::Os(y2p_core::py::os_error(&error, None).0));
        }
        match stdout.read(buffer) {
            Ok(n) => return Ok(n),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(Error::Os(y2p_core::py::os_error(&e, None).0)),
        }
    }
}

/// What a curl failure before any response means to urllib (a URLError's reason).
fn before_response(code: Option<i32>) -> Error {
    Error::Url(match code {
        Some(6) => "[Errno -3] Temporary failure in name resolution".into(),
        Some(7) => "[Errno 111] Connection refused".into(),
        Some(28) => "timed out".into(),
        Some(52) => "Remote end closed connection without response".into(),
        Some(56) => "[Errno 104] Connection reset by peer".into(),
        Some(code @ (35 | 51 | 53 | 54 | 58 | 59 | 60 | 64 | 66 | 77 | 80 | 82 | 83 | 90 | 91)) => {
            format!("[SSL] TLS handshake or certificate check failed (curl {code})")
        }
        Some(code) => format!("curl exit {code}"),
        None => "curl ended by a signal".into(),
    })
}

/// `opener.open(url, timeout=10)` with `AllowlistedRedirect(hosts)` and no proxy. `cancellable`: a
/// SIGTERM ends it with `Error::Cancelled`.
pub fn open(url: &str, hosts: &[String], cancellable: bool) -> Result<Response, Error> {
    let mut url = url.to_owned();
    let mut visited: Vec<String> = Vec::new();
    loop {
        let (mut child, mut stdout) = spawn(&url)?;
        let mut head = Vec::new();
        let mut buffer = [0u8; 16384];
        let split = loop {
            if let Some(index) = head.windows(4).position(|w| w == b"\r\n\r\n") {
                break index + 4;
            }
            let n = match read_some(&mut stdout, &mut buffer, cancellable) {
                Ok(n) => n,
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error);
                }
            };
            if n == 0 {
                let code = child.wait().ok().and_then(|s| s.code());
                return Err(before_response(code));
            }
            head.extend_from_slice(&buffer[..n]);
            if head.len() > 1 << 20 {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Url("got more than 1048576 header bytes".into()));
            }
        };
        let pending = head.split_off(split);
        let text = String::from_utf8_lossy(&head).into_owned();
        let mut lines = text.split("\r\n");
        let status_line = lines.next().unwrap_or("");
        let mut words = status_line.splitn(3, ' ');
        let _version = words.next();
        let status: u16 = words.next().and_then(|c| c.parse().ok()).unwrap_or(0);
        let reason = words.next().unwrap_or("").to_owned();
        let location = lines.find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("location")
                .then(|| value.trim().to_owned())
        });
        let response = Response {
            child,
            stdout,
            pending,
            status,
            finished: false,
        };
        if (200..300).contains(&status) {
            return Ok(response);
        }
        drop(response);
        let redirect = matches!(status, 301 | 302 | 303 | 307 | 308);
        let Some(location) = location.filter(|_| redirect) else {
            return Err(Error::Http {
                code: status,
                text: format!("HTTP Error {status}: {reason}"),
            });
        };
        let next = join(&url, &location);
        if !redirect_allowed(&next, hosts) {
            return Err(Error::Value("update_redirect_refused".into()));
        }
        if visited.iter().filter(|v| **v == next).count() >= 4 || visited.len() >= MAX_REDIRECTIONS
        {
            return Err(Error::Http {
                code: status,
                text: format!(
                    "HTTP Error {status}: The HTTP server returned a redirect error that would lead to an \
                     infinite loop.\nThe last 30x error message was:\n{reason}"
                ),
            });
        }
        visited.push(next.clone());
        url = next;
    }
}

impl Response {
    /// `response.read(limit)`: up to `limit` bytes, empty at the end.
    pub fn read(&mut self, limit: usize, cancellable: bool) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        if !self.pending.is_empty() {
            let take = limit.min(self.pending.len());
            out.extend(self.pending.drain(..take));
        }
        let mut buffer = vec![0u8; 65536];
        while out.len() < limit && !self.finished {
            let want = (limit - out.len()).min(buffer.len());
            let n = read_some(&mut self.stdout, &mut buffer[..want], cancellable)?;
            if n == 0 {
                self.finished = true;
                match self.child.wait().ok().and_then(|s| s.code()) {
                    // A short body ends like urllib's read(): the caller checks the size.
                    Some(0 | 18) => {}
                    Some(28) => return Err(Error::Timeout),
                    Some(56) => {
                        return Err(Error::Os("[Errno 104] Connection reset by peer".into()))
                    }
                    Some(code) => {
                        return Err(Error::Os(format!(
                            "[SSL] connection failed while reading (curl {code})"
                        )))
                    }
                    None => return Err(Error::Os("curl ended by a signal".into())),
                }
                break;
            }
            out.extend_from_slice(&buffer[..n]);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_urls_follow_base_url() {
        for url in [
            "http://example.com/manifest.json",
            "https://u:p@example.com/manifest.json",
            "https://example.com/manifest.json?redirect=other",
            "https://example.com/other",
            "https://example.com/manifest.json#x",
            "https:///manifest.json",
            "https://example.com/a b/manifest.json",
        ] {
            assert!(base_url(url).is_err(), "{url}");
        }
        assert_eq!(
            base_url("https://example.com/release/manifest.json").unwrap(),
            "https://example.com/release/"
        );
        assert_eq!(
            base_url("HTTPS://Example.com:443/manifest.json").unwrap(),
            "HTTPS://Example.com:443/"
        );
    }

    #[test]
    fn redirects_are_joined_and_checked_as_urllib() {
        let hosts = vec!["objects.githubusercontent.com".to_owned()];
        let base = "https://github.com/o/r/releases/download/v1/manifest.json";
        let next = join(base, "https://objects.githubusercontent.com/x?sig=1");
        assert_eq!(next, "https://objects.githubusercontent.com/x?sig=1");
        assert!(redirect_allowed(&next, &hosts));
        assert!(!redirect_allowed(
            "http://objects.githubusercontent.com/x",
            &hosts
        ));
        assert!(!redirect_allowed(
            "https://u@objects.githubusercontent.com/x",
            &hosts
        ));
        assert!(!redirect_allowed(&join(base, "/other"), &hosts));
        assert_eq!(join(base, "/other"), "https://github.com/other");
        assert_eq!(
            join(base, "../x y"),
            "https://github.com/o/r/releases/download/x%20y"
        );
        assert_eq!(join(base, "//cdn.example/x"), "https://cdn.example/x");
    }
}
