//! What a ported subcommand returns: its stdout and exit code, an error `main.py` would print as
//! `org.y2linux.error/v1`, or "let Python do it" for inputs the Python platform handles in a way not
//! worth reproducing (it would crash, or parse something exotic). A command may fall back only before
//! it changed anything Python would not change the same way again.
// SPDX-License-Identifier: GPL-2.0-only
use serde::Serialize;
use std::{
    fs, io,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::Path,
};
use y2p_core::{
    json::{dumps, dumps_sorted, Ordered, Separators},
    py,
};

pub struct Reply {
    /// One line without its newline, as `print()` takes it.
    pub stdout: String,
    pub code: u8,
}

impl Reply {
    /// `print(json.dumps(result, sort_keys=True))`.
    pub fn sorted<T: Serialize + ?Sized>(value: &T, code: u8) -> Result<Reply, Failure> {
        Ok(Reply {
            stdout: dumps_sorted(value, Separators::Spaced).map_err(Failure::value)?,
            code,
        })
    }

    /// `print(json.dumps(result))`: insertion order (an `Ordered`, or a struct's field order).
    pub fn unsorted<T: Serialize + ?Sized>(value: &T, code: u8) -> Result<Reply, Failure> {
        Ok(Reply {
            stdout: dumps(value, Separators::Spaced).map_err(Failure::value)?,
            code,
        })
    }

    pub fn text(text: String) -> Reply {
        Reply {
            stdout: text,
            code: 0,
        }
    }
}

#[derive(Debug)]
pub enum Failure {
    /// Hand the whole command to the Python platform.
    Fallback,
    /// `raise ValueError(text)`.
    Value(String),
    /// An `OSError`: Python's `str(error)` and its class name.
    Os { text: String, class: &'static str },
}

impl Failure {
    pub fn value(error: impl ToString) -> Failure {
        Failure::Value(error.to_string())
    }

    /// An OSError from a call that was given `path` (Python includes it in the text).
    pub fn os(error: io::Error, path: &Path) -> Failure {
        let (text, class) = py::os_error(&error, Some(&path.to_string_lossy()));
        Failure::Os { text, class }
    }

    /// An OSError raised with a plain message (`raise OSError('dormant budget readback')`).
    pub fn os_message(text: &str) -> Failure {
        Failure::Os {
            text: text.into(),
            class: "OSError",
        }
    }

    /// `str(error)`.
    pub fn message(&self) -> String {
        match self {
            Failure::Fallback => String::new(),
            Failure::Value(text) | Failure::Os { text, .. } => text.clone(),
        }
    }

    /// `getattr(error, 'strerror', None) or str(error)`.
    pub fn strerror(&self) -> String {
        match self {
            Failure::Os { text, .. } => text
                .strip_prefix("[Errno ")
                .and_then(|rest| rest.split_once("] "))
                .map(|(_, message)| message.split(": '").next().unwrap_or(message).to_owned())
                .unwrap_or_else(|| text.clone()),
            other => other.message(),
        }
    }

    /// main.py's record for an error no command caught (exit 1).
    pub fn record(&self) -> Ordered {
        let kind = match self {
            Failure::Os { class, .. } => class,
            _ => "ValueError",
        };
        Ordered::new()
            .with("schema", y2p_contract::schema::ERROR)
            .with("state", "Failed")
            .with("reason", self.message())
            .with("type", kind.to_string())
    }
}

pub type Outcome = Result<Reply, Failure>;

// ---- file operations with Python's error texts -------------------------------------------------

/// `Path.write_text(text)`: open for writing (created 0666 & umask, truncated), write.
pub fn write_text(path: &Path, text: &str) -> Result<(), Failure> {
    fs::write(path, text).map_err(|e| Failure::os(e, path))
}

/// `Path.unlink(missing_ok=True)`.
pub fn unlink_missing_ok(path: &Path) -> Result<(), Failure> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(Failure::os(e, path)),
        _ => Ok(()),
    }
}

/// `Path.mkdir(mode=mode, parents=True, exist_ok=True)`: parents with default permissions.
pub fn mkdir_parents(path: &Path, mode: u32) -> Result<(), Failure> {
    if path.is_dir() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| Failure::os(e, parent))?;
    }
    match fs::DirBuilder::new().mode(mode).create(path) {
        Err(e) if !(e.kind() == io::ErrorKind::AlreadyExists && path.is_dir()) => {
            Err(Failure::os(e, path))
        }
        _ => Ok(()),
    }
}

/// `os.open(path, O_RDWR | O_CREAT | O_NOFOLLOW, 0o600)` + `fcntl.flock(fd, LOCK_EX)`.
pub fn lock(path: &Path) -> Result<fs::File, Failure> {
    y2p_core::lock::exclusive(path).map_err(|e| Failure::os(e, path))
}

/// `boot.private_directory`.
pub fn private_directory(path: &Path) -> Result<(), Failure> {
    y2p_core::record::private_directory(path).map_err(|e| match e.kind() {
        io::ErrorKind::InvalidData => Failure::value(e),
        _ => Failure::os(e, path),
    })
}

/// `common.atomic_json`.
pub fn atomic_json<T: Serialize + ?Sized>(
    path: &Path,
    value: &T,
    durable: bool,
) -> Result<(), Failure> {
    y2p_core::record::atomic_json(path, value, durable, 0o600).map_err(|e| Failure::os(e, path))
}

/// `os.open(path, flags | O_CREAT, mode)` for a file whose content does not matter.
pub fn create(path: &Path, exclusive: bool) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .create_new(exclusive)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC)
        .open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_read_as_python_prints_them() {
        let error = Failure::os(
            io::Error::from_raw_os_error(libc::ENOENT),
            Path::new("/sys/module/spm/x"),
        );
        assert_eq!(
            error.message(),
            "[Errno 2] No such file or directory: '/sys/module/spm/x'"
        );
        assert_eq!(error.strerror(), "No such file or directory");
        assert_eq!(
            Failure::value("invalid_power_feature").strerror(),
            "invalid_power_feature"
        );
        assert_eq!(
            dumps(&Failure::value("unknown_boot_stage").record(), Separators::Spaced).unwrap(),
            "{\"schema\": \"org.y2linux.error/v1\", \"state\": \"Failed\", \"reason\": \"unknown_boot_stage\", \
             \"type\": \"ValueError\"}"
        );
    }
}
