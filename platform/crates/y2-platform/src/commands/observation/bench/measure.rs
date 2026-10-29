//! One storage benchmark run: sequential and random I/O per block size, syncs, metadata operations
//! and offset-specific integrity regions, each step timed and compared by the passive counters.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    device::{controller_fault, distribution, io_delta, pread, write_all, Io},
    fd::{borrowed, cname, sync},
    scratch::Scratch,
    Error, Result,
};
use serde_json::{json, Map, Value};
use std::{
    io,
    os::fd::{AsRawFd, RawFd},
};
use y2p_core::{
    observation_support::{
        sha2::{Digest, Sha256},
        PyRandom,
    },
    Root,
};

/// What runs once after a measurement's steps (an fdatasync, a directory fsync).
type Finalize<'f> = &'f dyn Fn(&Scratch) -> Result<()>;

/// What a run has measured so far: a failed run shows what it got to.
#[derive(Default)]
pub(super) struct Measured {
    pub(super) measurements: Vec<Value>,
    /// The volume measured and its storage observation, once the scratch directory is open.
    pub(super) volume: Value,
    pub(super) cache_advice: Vec<Value>,
    pub(super) integrity: Vec<Value>,
    /// The counters around the integrity check, once it ran.
    pub(super) integrity_system: Option<Value>,
}

pub(super) struct Bench<'a> {
    pub(super) root: &'a Root,
    pub(super) io: &'a dyn Io,
    pub(super) deadline: f64,
    pub(super) measured: Measured,
}

impl Bench<'_> {
    fn ensure(&self, scratch: &Scratch) -> Result<()> {
        if self.root.clock().monotonic() >= self.deadline {
            return Err(Error {
                class: "TimeoutError".into(),
                text: "benchmark_deadline".into(),
                errno: None,
            });
        }
        scratch.ensure()
    }

    /// `advise_read`: older Y2 kernels omit ADVISE_SYSCALLS (EINVAL on ARM). Preserve verified
    /// readback while explicitly refusing a cold or media-throughput claim. Genuine I/O faults fail.
    fn advise(&mut self, fd: RawFd, operation: &str) -> Result<()> {
        let error = self.io.drop_cache(fd);
        let (state, errno) = match error {
            0 => ("requested_advisory_only", Value::Null),
            libc::EINVAL | libc::ENOSYS | libc::EOPNOTSUPP => ("unavailable", json!(error)),
            other => return Err(Error::os(io::Error::from_raw_os_error(other), None)),
        };
        self.measured
            .cache_advice
            .push(json!({"before": operation, "state": state, "errno": errno}));
        Ok(())
    }

    fn measure(
        &mut self,
        scratch: &mut Scratch,
        name: &str,
        count: usize,
        bytes_per_op: u64,
        mut step: impl FnMut(&mut Scratch, usize) -> Result<()>,
        finalize: Option<Finalize>,
    ) -> Result<()> {
        let mut samples = Vec::with_capacity(count);
        let before_io = self.io.counters(self.root);
        let started = self.root.clock().monotonic_ns();
        for n in 0..count {
            self.ensure(scratch)?;
            let before = self.root.clock().monotonic_ns();
            step(scratch, n)?;
            samples.push(self.root.clock().monotonic_ns() - before);
        }
        if let Some(finalize) = finalize {
            finalize(scratch)?;
        }
        let elapsed = self.root.clock().monotonic_ns() - started;
        let mut measurement = Map::new();
        measurement.insert("operation".into(), json!(name));
        measurement.insert("result".into(), json!("OK"));
        measurement.extend(distribution(
            &samples,
            count as u64 * bytes_per_op,
            Some(elapsed),
        ));
        let system = io_delta(&before_io, &self.io.counters(self.root));
        let fault = controller_fault(&system);
        measurement.insert("system".into(), system);
        self.measured.measurements.push(Value::Object(measurement));
        if fault {
            return Err(Error::raised(
                libc::EIO,
                "controller_fault_during_benchmark",
            ));
        }
        Ok(())
    }

    pub(super) fn run(
        &mut self,
        scratch: &mut Scratch,
        size: usize,
        operations: usize,
        rng: &mut PyRandom,
        block: &[u8],
    ) -> Result<()> {
        let io = self.io;
        let data = scratch.open("data")?;
        let fd = data.as_raw_fd();
        let datasync = move |_: &Scratch| sync(fd, true);
        for bs in [4096usize, 16384, 65536, 1_048_576] {
            let count = size / bs;
            let chunk = &block[..bs];
            self.measure(
                scratch,
                &format!("sequential_write_{bs}"),
                count,
                bs as u64,
                |_, n| write_all(io, fd, chunk, n * bs),
                Some(&datasync),
            )?;
            self.advise(fd, &format!("sequential_read_{bs}"))?;
            self.measure(
                scratch,
                &format!("sequential_read_{bs}"),
                count,
                bs as u64,
                |_, n| {
                    if pread(io, fd, bs, n * bs)? != chunk {
                        Err(Error::raised(libc::EIO, "readback_mismatch"))
                    } else {
                        Ok(())
                    }
                },
                None,
            )?;
            let offsets: Vec<usize> = (0..count.min(operations))
                .map(|_| rng.randrange(count as u64) as usize * bs)
                .collect();
            self.measure(
                scratch,
                &format!("random_write_{bs}"),
                offsets.len(),
                bs as u64,
                |_, n| write_all(io, fd, chunk, offsets[n]),
                Some(&datasync),
            )?;
            self.advise(fd, &format!("random_read_{bs}"))?;
            self.measure(
                scratch,
                &format!("random_read_{bs}"),
                offsets.len(),
                bs as u64,
                |_, n| {
                    if pread(io, fd, bs, offsets[n])? != chunk {
                        Err(Error::raised(libc::EIO, "readback_mismatch"))
                    } else {
                        Ok(())
                    }
                },
                None,
            )?;
        }
        for (name, data_only) in [("fsync", false), ("fdatasync", true)] {
            self.measure(
                scratch,
                name,
                operations.min(64),
                4096,
                |_, _| {
                    write_all(io, fd, &block[..4096], 0)?;
                    sync(fd, data_only)
                },
                None,
            )?;
        }
        drop(data);
        let small = &block[..4096];
        let directory_sync = |s: &Scratch| sync(s.fd(), false);
        self.measure(
            scratch,
            "small_file_write_fsync",
            operations,
            4096,
            |s, n| {
                let file = s.open(&format!("small{n}"))?;
                write_all(io, file.as_raw_fd(), small, 0)?;
                sync(file.as_raw_fd(), false)
            },
            Some(&directory_sync),
        )?;
        for n in 0..operations {
            scratch.unlink(&format!("small{n}"))?;
        }
        self.measure(
            scratch,
            "create",
            operations,
            0,
            |s, n| s.open(&format!("f{n}")).map(drop),
            None,
        )?;
        self.measure(
            scratch,
            "stat",
            operations,
            0,
            |s, n| {
                let path = cname(&format!("f{n}"));
                y2p_core::sys::fstatat(borrowed(s.fd()), &path, libc::AT_SYMLINK_NOFOLLOW)
                    .map(drop)
                    .map_err(|e| Error::os(e, Some(&format!("f{n}"))))
            },
            None,
        )?;
        self.measure(
            scratch,
            "rename",
            operations,
            0,
            |s, n| s.rename(&format!("f{n}"), &format!("g{n}")),
            None,
        )?;
        self.measure(
            scratch,
            "delete",
            operations,
            0,
            |s, n| s.unlink(&format!("g{n}")),
            None,
        )?;
        let renamed = self.measure(
            scratch,
            "file_fsync_rename_directory_fsync",
            operations.min(64),
            4096,
            |s, _| {
                let file = s.open("renameA")?;
                write_all(io, file.as_raw_fd(), small, 0)?;
                sync(file.as_raw_fd(), false)?;
                drop(file);
                s.rename("renameA", "renameB")?;
                sync(s.fd(), false)?;
                s.unlink("renameB")?;
                sync(s.fd(), false)
            },
            None,
        );
        match renamed {
            Err(error)
                if matches!(error.errno, Some(libc::EINVAL | libc::EOPNOTSUPP))
                    && error.class != "ValueError" =>
            {
                self.measured.measurements.push(json!({"operation": "file_fsync_rename_directory_fsync",
                    "result": "UNAVAILABLE", "reason": "filesystem_does_not_support_directory_fsync"}));
            }
            other => other?,
        }
        let before_io = self.io.counters(self.root);
        let integrity = self.regions(scratch)?;
        self.measured.integrity = integrity;
        let system = io_delta(&before_io, &self.io.counters(self.root));
        let fault = controller_fault(&system);
        self.measured.integrity_system = Some(system);
        if fault {
            return Err(Error::raised(
                libc::EIO,
                "controller_fault_during_benchmark",
            ));
        }
        self.ensure(scratch)
    }

    /// `sha256_regions`: offset-specific patterns catch address aliasing as well as changed bytes.
    fn regions(&mut self, scratch: &mut Scratch) -> Result<Vec<Value>> {
        let file = scratch.open("integrity")?;
        let fd = file.as_raw_fd();
        let mut written = Vec::new();
        for (offset, size) in [
            (0usize, 1usize),
            (511, 513),
            (4093, 4099),
            (65543, 65537),
            (1_049_087, 1_048_583),
        ] {
            self.ensure(scratch)?;
            let payload = PyRandom::new((6582 + offset + size) as u64).randbytes(size);
            write_all(self.io, fd, &payload, offset)?;
            sync(fd, false)?;
            written.push((offset, size, hex(&payload)));
        }
        // Verify only after every write: aliased writes must not erase another region undetected.
        // Advisory eviction is file-scoped, never global.
        let mut regions = Vec::new();
        for (offset, size, expected) in written {
            self.advise(fd, &format!("sha256_{offset}_{size}"))?;
            self.ensure(scratch)?;
            let actual = hex(&pread(self.io, fd, size, offset)?);
            if actual != expected {
                return Err(Error::raised(libc::EIO, "sha256_readback_mismatch"));
            }
            regions.push(
                json!({"offset": offset, "bytes": size, "expected_sha256": expected,
                                "readback_sha256": actual, "result": "OK"}),
            );
        }
        Ok(regions)
    }
}

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
