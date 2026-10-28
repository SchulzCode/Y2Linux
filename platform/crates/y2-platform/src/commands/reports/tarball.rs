//! Archives as Python's `tarfile` writes them (PAX format, the default since Python 3.8: a ustar header
//! per member, a `././@PaxHeader` record before one whose name, user or numbers do not fit), so an
//! uncompressed report is byte-identical to Python's for the same members; gzip as `gzip.compress`
//! (header mtime given, OS 255). The deflate stream itself is miniz_oxide's, not zlib's: compressed
//! bytes differ from Python's, the archive inside does not.
// SPDX-License-Identifier: GPL-2.0-only
use flate2::{read::MultiGzDecoder, write::GzEncoder, Compression, GzBuilder};
use std::io::{self, Read, Write};

const BLOCK: usize = 512;
const RECORD: usize = 20 * BLOCK;

/// One member's metadata (`TarInfo`), a regular file.
pub struct Info {
    pub name: String,
    pub mode: u32,
    pub uid: u64,
    pub gid: u64,
    pub size: u64,
    /// An int (`int(time.time())`) or a float (`os.stat().st_mtime`, then also a pax record).
    pub mtime: Mtime,
    pub uname: String,
    pub gname: String,
}

#[derive(Clone, Copy)]
pub enum Mtime {
    Int(i64),
    Float(f64),
}

impl Info {
    pub fn file(name: &str, size: usize, mtime: i64, mode: u32) -> Info {
        Info {
            name: name.into(),
            mode,
            uid: 0,
            gid: 0,
            size: size as u64,
            mtime: Mtime::Int(mtime),
            uname: String::new(),
            gname: String::new(),
        }
    }
}

/// `stn(s, length, 'ascii', 'replace')`.
fn stn(out: &mut Vec<u8>, text: &str, length: usize) {
    let bytes: Vec<u8> = text
        .chars()
        .map(|c| if c.is_ascii() { c as u8 } else { b'?' })
        .collect();
    let take = bytes.len().min(length);
    out.extend_from_slice(&bytes[..take]);
    out.resize(out.len() + length - take, 0);
}

/// `itn(n, digits)` for a value that fits (`create_pax_header` zeroes the others).
fn itn(out: &mut Vec<u8>, value: u64, digits: usize) {
    out.extend_from_slice(format!("{value:0width$o}", width = digits - 1).as_bytes());
    out.push(0);
}

struct Fields<'a> {
    name: &'a str,
    mode: u32,
    uid: u64,
    gid: u64,
    size: u64,
    mtime: u64,
    kind: u8,
    uname: &'a str,
    gname: &'a str,
}

/// `TarInfo._create_header(info, USTAR_FORMAT, 'ascii', 'replace')`.
fn header(fields: &Fields) -> Vec<u8> {
    let mut buf = Vec::with_capacity(BLOCK);
    stn(&mut buf, fields.name, 100);
    itn(&mut buf, u64::from(fields.mode & 0o7777), 8);
    itn(&mut buf, fields.uid, 8);
    itn(&mut buf, fields.gid, 8);
    itn(&mut buf, fields.size, 12);
    itn(&mut buf, fields.mtime, 12);
    buf.extend_from_slice(b"        ");
    buf.push(fields.kind);
    stn(&mut buf, "", 100);
    buf.extend_from_slice(b"ustar\x0000");
    stn(&mut buf, fields.uname, 32);
    stn(&mut buf, fields.gname, 32);
    // devmajor/devminor are empty for anything but a device (Python 3.12+).
    buf.resize(buf.len() + 16, 0);
    stn(&mut buf, "", 155);
    buf.resize(BLOCK, 0);
    let sum: u32 = 256
        + buf[..148].iter().map(|&b| u32::from(b)).sum::<u32>()
        + buf[156..].iter().map(|&b| u32::from(b)).sum::<u32>();
    buf[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
    buf
}

fn pad(out: &mut Vec<u8>) {
    let rest = out.len() % BLOCK;
    if rest > 0 {
        out.resize(out.len() + BLOCK - rest, 0);
    }
}

/// `tarfile.open(fileobj, 'w')`: members appended in order, `close()` writes the end.
#[derive(Default)]
pub struct Writer {
    out: Vec<u8>,
}

impl Writer {
    pub fn new() -> Writer {
        Writer::default()
    }

    /// `addfile(info, data)`.
    pub fn add(&mut self, info: &Info, data: &[u8]) {
        let mut pax: Vec<(&str, String)> = Vec::new();
        for (key, value, length) in [
            ("path", &info.name, 100),
            ("uname", &info.uname, 32),
            ("gname", &info.gname, 32),
        ] {
            if !value.is_ascii() || value.chars().count() > length {
                pax.push((key, value.clone()));
            }
        }
        let limit = |digits: u32| 8u64.pow(digits - 1);
        let mut number = |key: &'static str, value: u64, digits| {
            if value < limit(digits) {
                value
            } else {
                pax.push((key, value.to_string()));
                0
            }
        };
        let uid = number("uid", info.uid, 8);
        let gid = number("gid", info.gid, 8);
        let size = number("size", info.size, 12);
        let mtime = match info.mtime {
            Mtime::Int(t) if t >= 0 && (t as u64) < limit(12) => t as u64,
            Mtime::Int(t) => {
                pax.push(("mtime", t.to_string()));
                0
            }
            Mtime::Float(t) => {
                pax.push(("mtime", y2p_core::json::float_repr(t)));
                let rounded = t.round_ties_even();
                if rounded >= 0.0 && rounded < limit(12) as f64 {
                    rounded as u64
                } else {
                    0
                }
            }
        };
        if !pax.is_empty() {
            let mut records = Vec::new();
            for (key, value) in &pax {
                let length = key.len() + value.len() + 3;
                let mut p = 0usize;
                loop {
                    let n = length + p.to_string().len();
                    if n == p {
                        break;
                    }
                    p = n;
                }
                records.extend_from_slice(format!("{p} {key}={value}\n").as_bytes());
            }
            self.out.extend(header(&Fields {
                name: "././@PaxHeader",
                mode: 0,
                uid: 0,
                gid: 0,
                size: records.len() as u64,
                mtime: 0,
                kind: b'x',
                uname: "",
                gname: "",
            }));
            self.out.extend(records);
            pad(&mut self.out);
        }
        self.out.extend(header(&Fields {
            name: &info.name,
            mode: info.mode,
            uid,
            gid,
            size,
            mtime,
            kind: b'0',
            uname: &info.uname,
            gname: &info.gname,
        }));
        self.out.extend_from_slice(data);
        pad(&mut self.out);
    }

    /// `close()`: two zero blocks, then zeros up to the 10 KiB record.
    pub fn finish(mut self) -> Vec<u8> {
        self.out.resize(self.out.len() + 2 * BLOCK, 0);
        let rest = self.out.len() % RECORD;
        if rest > 0 {
            self.out.resize(self.out.len() + RECORD - rest, 0);
        }
        self.out
    }
}

/// `gzip.compress(data, compresslevel=level, mtime=mtime)`.
pub fn gzip(data: &[u8], level: u32, mtime: u32) -> Vec<u8> {
    let mut encoder: GzEncoder<Vec<u8>> = GzBuilder::new()
        .mtime(mtime)
        .write(Vec::with_capacity(data.len() / 4), Compression::new(level));
    encoder.write_all(data).expect("in memory");
    encoder.finish().expect("in memory")
}

/// `gzip.decompress` (also several members).
pub fn gunzip(data: &[u8], limit: u64) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    MultiGzDecoder::new(data)
        .take(limit)
        .read_to_end(&mut out)?;
    Ok(out)
}

/// Members with the bodies asked for.
pub type Members = Vec<(Member, Option<Vec<u8>>)>;

/// One member read back.
pub struct Member {
    pub name: String,
    pub mtime: u64,
    pub regular: bool,
    pub size: u64,
}

/// The members of an archive (`tarfile.open(...).getmembers()`, at most `limit`), each with its body
/// when `wanted(member)`. `Err` where `tarfile.open` raises (not an archive); a damaged header after
/// the first member ends the list, as tarfile's does.
pub fn members(
    data: &[u8],
    compressed: bool,
    limit: usize,
    wanted: impl Fn(&Member) -> bool,
) -> Result<Members, ()> {
    let raw;
    let data = if compressed {
        raw = gunzip(data, 64 << 20).map_err(|_| ())?;
        &raw[..]
    } else {
        data
    };
    if data.len() < BLOCK || data[..BLOCK].iter().all(|&b| b == 0) {
        return Err(()); // ReadError: empty file
    }
    let mut archive = tar::Archive::new(data);
    let mut found = Vec::new();
    let entries = archive.entries().map_err(|_| ())?;
    for (n, entry) in entries.enumerate() {
        let Ok(mut entry) = entry else {
            if n == 0 {
                return Err(());
            }
            break;
        };
        let kind = entry.header().entry_type();
        if kind.is_pax_global_extensions() {
            continue;
        }
        if found.len() >= limit {
            break;
        }
        let member = Member {
            name: String::from_utf8_lossy(&entry.path_bytes()).into_owned(),
            mtime: entry.header().mtime().unwrap_or(0),
            regular: matches!(
                kind,
                tar::EntryType::Regular | tar::EntryType::Continuous | tar::EntryType::GNUSparse
            ),
            size: entry.size(),
        };
        let body = if wanted(&member) {
            let mut body = Vec::new();
            if entry.read_to_end(&mut body).is_err() {
                break;
            }
            Some(body)
        } else {
            None
        };
        found.push((member, body));
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same members through host Python's tarfile.
    #[test]
    fn archives_are_python_tarfile_bytes() {
        let long = format!("files/{}/x.txt", "d".repeat(120));
        let mut writer = Writer::new();
        writer.add(&Info::file("report.json", 3, 1_761_650_000, 0o600), b"{}\n");
        writer.add(&Info::file(&long, 2, 1_761_650_000, 0o600), b"ab");
        writer.add(&Info::file("files/\u{e9}.log", 0, 5, 0o600), b"");
        let mut owned = Info::file("manifest.json", 1, 0, 0o100644);
        owned.mtime = Mtime::Float(1_761_650_000.123_456_7);
        owned.uname = "root".into();
        owned.gname = "root".into();
        writer.add(&owned, b"x");
        let ours = writer.finish();
        let script = r#"
import io, sys, tarfile
out = io.BytesIO()
long = 'files/' + 'd' * 120 + '/x.txt'
with tarfile.open(fileobj=out, mode='w') as tar:
    for name, data, mtime, mode in (('report.json', b'{}\n', 1761650000, 0o600), (long, b'ab', 1761650000, 0o600),
                                    ('files/é.log', b'', 5, 0o600), ('manifest.json', b'x', 1761650000.1234567, 0o100644)):
        info = tarfile.TarInfo(name)
        info.size, info.mtime, info.mode = len(data), mtime, mode
        if name == 'manifest.json':
            info.uname = info.gname = 'root'
        tar.addfile(info, io.BytesIO(data))
sys.stdout.buffer.write(out.getvalue())
"#;
        let Ok(python) = std::process::Command::new("python3")
            .args(["-I", "-c", script])
            .output()
        else {
            return; // no host Python
        };
        assert!(python.status.success());
        assert_eq!(ours.len(), python.stdout.len());
        assert!(ours == python.stdout, "tar bytes differ from Python's");
        let read = members(&ours, false, 64, |_| true).unwrap();
        assert_eq!(read.len(), 4);
        assert_eq!(read[1].0.name, long);
        assert_eq!(read[0].1.as_deref(), Some(&b"{}\n"[..]));
        assert!(members(b"not a tar", false, 64, |_| true).is_err());
        let packed = gzip(&ours, 6, 0);
        assert_eq!(&packed[..10], b"\x1f\x8b\x08\x00\x00\x00\x00\x00\x00\xff");
        assert_eq!(gunzip(&packed, 1 << 20).unwrap(), ours);
    }
}
