//! What the `observation` port group needs beyond the shared primitives: the regular expression and
//! hash crates for the multicall binary, Python's `random.Random` (bench-storage's data and offsets
//! are part of its output: CONTRACT.md "What a port must not miss") and `common.command` with one
//! inherited descriptor (`pass_fds`, bench-library's scratch directory).
// SPDX-License-Identifier: GPL-2.0-only
pub use regex;
pub use sha2;

use crate::{command, Root};
use std::{
    io::Read,
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

/// CPython's `random.Random`: MT19937 seeded by `init_by_array` with the 32-bit words of `abs(seed)`.
pub struct PyRandom {
    state: [u32; 624],
    index: usize,
}

impl PyRandom {
    /// `random.Random(seed)` for an int seed.
    pub fn new(seed: u64) -> Self {
        let mut key = Vec::new();
        let mut rest = seed;
        while rest != 0 {
            key.push(rest as u32);
            rest >>= 32;
        }
        if key.is_empty() {
            key.push(0);
        }
        let mut random = PyRandom {
            state: [0; 624],
            index: 624,
        };
        random.init_genrand(19_650_218);
        let (mut i, mut j) = (1usize, 0usize);
        for _ in 0..624.max(key.len()) {
            let previous = random.state[i - 1] ^ (random.state[i - 1] >> 30);
            random.state[i] = (random.state[i] ^ previous.wrapping_mul(1_664_525))
                .wrapping_add(key[j])
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= 624 {
                random.state[0] = random.state[623];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
        }
        for _ in 0..623 {
            let previous = random.state[i - 1] ^ (random.state[i - 1] >> 30);
            random.state[i] =
                (random.state[i] ^ previous.wrapping_mul(1_566_083_941)).wrapping_sub(i as u32);
            i += 1;
            if i >= 624 {
                random.state[0] = random.state[623];
                i = 1;
            }
        }
        random.state[0] = 0x8000_0000;
        random
    }

    fn init_genrand(&mut self, seed: u32) {
        self.state[0] = seed;
        for i in 1..624 {
            let previous = self.state[i - 1] ^ (self.state[i - 1] >> 30);
            self.state[i] = previous.wrapping_mul(1_812_433_253).wrapping_add(i as u32);
        }
        self.index = 624;
    }

    fn next_u32(&mut self) -> u32 {
        if self.index >= 624 {
            for k in 0..624 {
                let y = (self.state[k] & 0x8000_0000) | (self.state[(k + 1) % 624] & 0x7fff_ffff);
                let mut value = self.state[(k + 397) % 624] ^ (y >> 1);
                if y & 1 != 0 {
                    value ^= 0x9908_b0df;
                }
                self.state[k] = value;
            }
            self.index = 0;
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }

    /// `getrandbits(k)` for k <= 64.
    pub fn getrandbits(&mut self, k: u32) -> u64 {
        if k == 0 {
            return 0;
        }
        if k <= 32 {
            return u64::from(self.next_u32() >> (32 - k));
        }
        let low = u64::from(self.next_u32());
        let high = u64::from(self.next_u32() >> (64 - k));
        low | (high << 32)
    }

    /// `randbytes(n)`: `getrandbits(n * 8).to_bytes(n, 'little')`, one 32-bit word after another,
    /// the last one keeping its high bits.
    pub fn randbytes(&mut self, n: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(n + 4);
        let mut bits = n * 8;
        while bits > 0 {
            let mut word = self.next_u32();
            if bits < 32 {
                word >>= 32 - bits;
            }
            out.extend_from_slice(&word.to_le_bytes());
            bits = bits.saturating_sub(32);
        }
        out.truncate(n);
        out
    }

    /// `randrange(n)` for n > 0 (`_randbelow_with_getrandbits`).
    pub fn randrange(&mut self, n: u64) -> u64 {
        let k = 64 - n.leading_zeros();
        loop {
            let r = self.getrandbits(k);
            if r < n {
                return r;
            }
        }
    }
}

/// `ctx.command(argv, timeout, limit, pass_fds=(fd,))`: as `command::run_with`, the descriptor kept
/// open under its own number in the child (and only there). Below a fixture root nothing starts.
pub fn run_passing_fd(
    root: &Root,
    argv: &[&str],
    timeout: f64,
    limit: usize,
    fd: libc::c_int,
) -> command::Outcome {
    if !root.is_real() {
        return command::run_with(root, argv, timeout, limit, false);
    }
    let unavailable = command::Outcome {
        ok: false,
        reason: Some("command_unavailable".into()),
        output: None,
    };
    let Some((program, args)) = argv.split_first() else {
        return unavailable;
    };
    let mut child = Command::new(program);
    child
        .args(args)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // SAFETY: setsid() and fcntl() are async-signal-safe; they run in the forked child before exec.
    unsafe {
        child.pre_exec(move || {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let flags = libc::fcntl(fd, libc::F_GETFD);
            if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = match child.spawn() {
        Ok(child) => child,
        Err(_) => return unavailable,
    };
    let pid = child.id() as libc::pid_t;
    let deadline = Instant::now() + Duration::from_secs_f64(timeout.max(0.0));
    // Spawned with a piped stdout: always there.
    let mut stdout = child.stdout.take().expect("piped stdout");
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        while let Ok(n) = stdout.read(&mut chunk) {
            if n == 0 || sender.send(chunk[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut output = Vec::new();
    let mut failure = None;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            failure = Some("command_timeout");
            break;
        }
        match receiver.recv_timeout(remaining) {
            Ok(chunk) => {
                output.extend_from_slice(&chunk);
                if output.len() > limit {
                    failure = Some("command_output_limit");
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                failure = Some("command_timeout");
                break;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let mut status = None;
    while failure.is_none() {
        match child.try_wait() {
            Ok(Some(exit)) => status = Some(exit),
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(1));
                continue;
            }
            _ => failure = Some("command_timeout"),
        }
        break;
    }
    if failure.is_some() {
        command::kill_tree(pid);
        let until = Instant::now() + Duration::from_millis(50);
        while Instant::now() < until && !matches!(child.try_wait(), Ok(Some(_))) {
            thread::sleep(Duration::from_millis(1));
        }
    }
    let ok = failure.is_none() && status.is_some_and(|s| s.success());
    command::Outcome {
        ok,
        reason: failure
            .map(str::to_owned)
            .or((!ok).then(|| "command_failed".to_owned())),
        output: ok.then(|| crate::root::python_strip(&String::from_utf8_lossy(&output)).to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_generator_is_pythons() {
        // python3 -c "import random; r=random.Random(6582); print(r.randbytes(5).hex(),
        //   r.getrandbits(40), r.randrange(256), r.randrange(1))" and Random(6583).randbytes(1)
        let mut random = PyRandom::new(6582);
        let bytes: String = random
            .randbytes(5)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(bytes, "f38189ae7c");
        assert_eq!(random.getrandbits(40), 224_505_261_151);
        assert_eq!(random.randrange(256), 141);
        assert_eq!(random.randrange(1), 0);
        assert_eq!(PyRandom::new(6583).randbytes(1), [0x4f]);
    }

    #[test]
    fn long_runs_match_python_random() {
        // random.Random(1049087): randbytes(70001) (as its SHA-256), randrange(4096), randrange(3),
        // randrange(1 << 40), recorded from host Python 3.15.
        use sha2::{Digest, Sha256};
        let mut random = PyRandom::new(1_049_087);
        let digest: String = Sha256::digest(random.randbytes(70001))
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let line = format!(
            "{digest} {} {} {}\n",
            random.randrange(4096),
            random.randrange(3),
            random.randrange(1 << 40)
        );
        crate::recorded::check(
            env!("CARGO_MANIFEST_DIR"),
            "random-1049087.txt",
            line.as_bytes(),
        );
    }

    #[test]
    fn a_passed_descriptor_reaches_the_child() {
        let file = std::fs::File::open("/dev/null").unwrap();
        use std::os::fd::AsRawFd;
        let fd = file.as_raw_fd();
        let script = format!("test -e /proc/self/fd/{fd} && echo yes");
        let outcome = run_passing_fd(&Root::new("/"), &["/bin/sh", "-c", &script], 2.0, 100, fd);
        assert_eq!(outcome.output.as_deref(), Some("yes"));
        let outcome = command::run(&Root::new("/"), &["/bin/sh", "-c", &script], 2.0);
        assert!(!outcome.ok, "not inherited without pass_fds");
    }
}
