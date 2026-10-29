//! The retained-evidence archive, byte for byte as Python's tarfile (PAX format) writes it: a ustar
//! header per member (a pax header first for a long or non-ASCII name), the data padded to 512 bytes,
//! then the end blocks up to the 10240-byte record.
// SPDX-License-Identifier: GPL-2.0-only
use std::{
    fs::File,
    io::{self, Write},
};

const BLOCK: usize = 512;
const RECORD: usize = 20 * BLOCK;

/// `tarfile.itn` for a value that fits: zero-padded octal and a NUL.
fn itn(value: u64, digits: usize) -> Vec<u8> {
    format!("{value:0width$o}\0", width = digits - 1).into_bytes()
}

/// `tarfile.stn`: the text cut or NUL-padded to its field.
fn stn(text: &[u8], length: usize) -> Vec<u8> {
    let mut field = text[..text.len().min(length)].to_vec();
    field.resize(length, 0);
    field
}

/// `TarInfo._create_header(info, USTAR_FORMAT)` for a file or a pax header.
fn ustar_header(name: &[u8], mode: u64, size: u64, kind: u8) -> Vec<u8> {
    let mut block = Vec::with_capacity(BLOCK);
    block.extend(stn(name, 100));
    block.extend(itn(mode & 0o7777, 8));
    block.extend(itn(0, 8));
    block.extend(itn(0, 8));
    block.extend(itn(size, 12));
    block.extend(itn(0, 12));
    block.extend(b"        ");
    block.push(kind);
    block.extend(stn(b"", 100));
    block.extend(b"ustar\x0000");
    block.extend(stn(b"", 32));
    block.extend(stn(b"", 32));
    block.extend(stn(b"", 8));
    block.extend(stn(b"", 8));
    block.extend(stn(b"", 155));
    block.resize(BLOCK, 0);
    let checksum: u32 = block.iter().map(|b| u32::from(*b)).sum();
    let field = format!("{checksum:06o}\0");
    block[148..155].copy_from_slice(field.as_bytes());
    block
}

/// The name as Python's str (invalid bytes as surrogates), for its ASCII/length tests and sorting.
pub(super) fn code_points(name: &[u8]) -> Vec<u32> {
    let mut points = Vec::new();
    let mut rest = name;
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(text) => {
                points.extend(text.chars().map(u32::from));
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                points.extend(
                    String::from_utf8_lossy(&rest[..valid])
                        .chars()
                        .map(u32::from),
                );
                let bad = error.error_len().unwrap_or(rest.len() - valid);
                points.extend(
                    rest[valid..valid + bad]
                        .iter()
                        .map(|b| 0xdc00 + u32::from(*b)),
                );
                rest = &rest[valid + bad..];
            }
        }
    }
    points
}

/// `TarInfo.tobuf(PAX_FORMAT)` for a regular file of `size` bytes, mode 0o600, mtime 0, owner 0.
fn file_header(name: &[u8], size: u64) -> Vec<u8> {
    {
        let points = code_points(name);
        let ascii = points.iter().all(|p| *p < 0x80);
        let mut out = Vec::new();
        if !ascii || points.len() > 100 {
            // A pax extended header carries the path; surrogates force hdrcharset=BINARY.
            let binary = std::str::from_utf8(name).is_err();
            let mut records = Vec::new();
            if binary {
                records.extend(b"21 hdrcharset=BINARY\n");
            }
            let l = "path".len() + name.len() + 3;
            let mut p = 0;
            loop {
                let n = l + p.to_string().len();
                if n == p {
                    break;
                }
                p = n;
            }
            records.extend(format!("{p} path=").as_bytes());
            records.extend(name);
            records.push(b'\n');
            out.extend(ustar_header(
                b"././@PaxHeader",
                0,
                records.len() as u64,
                b'x',
            ));
            let padded = records.len().div_ceil(BLOCK) * BLOCK;
            records.resize(padded, 0);
            out.extend(records);
        }
        // The ustar name in ASCII with '?' for every other character.
        let ustar: Vec<u8> = points
            .iter()
            .map(|p| if *p < 0x80 { *p as u8 } else { b'?' })
            .collect();
        out.extend(ustar_header(&ustar, 0o600, size, b'0'));
        out
    }
}

pub(super) struct Archive {
    pub(super) file: File,
    offset: usize,
}

impl Archive {
    pub(super) fn new(file: File) -> Archive {
        Archive { file, offset: 0 }
    }

    pub(super) fn add(&mut self, name: &[u8], data: &[u8]) -> io::Result<()> {
        let header = file_header(name, data.len() as u64);
        self.file.write_all(&header)?;
        self.file.write_all(data)?;
        let padding = data.len().div_ceil(BLOCK) * BLOCK - data.len();
        self.file.write_all(&vec![0; padding])?;
        self.offset += header.len() + data.len() + padding;
        Ok(())
    }

    /// `TarFile.close()`: two zero blocks, then zeros up to the next 10240-byte record.
    pub(super) fn close(&mut self) -> io::Result<()> {
        self.offset += 2 * BLOCK;
        let remainder = self.offset % RECORD;
        let tail = 2 * BLOCK + if remainder > 0 { RECORD - remainder } else { 0 };
        self.file.write_all(&vec![0; tail])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn archives_match_python_tarfile() {
        // The bytes host Python's tarfile wrote for these members (recorded, Python 3.15).
        let dir = std::env::temp_dir().join(format!("y2-tar-{}", std::process::id()));
        let file = File::create(&dir).unwrap();
        let mut archive = Archive::new(file);
        let long = format!("pstore/{}", "x".repeat(120));
        for (name, data) in [
            (&b"pm/state"[..], &b"abc"[..]),
            (long.as_bytes(), &b""[..]),
            ("pstore/\u{e9}".as_bytes(), &b"z"[..]),
            (&b"manifest.json"[..], "{}".repeat(300).as_bytes()),
        ] {
            archive.add(name, data).unwrap();
        }
        archive.close().unwrap();
        drop(archive);
        let ours = fs::read(&dir).unwrap();
        fs::remove_file(&dir).unwrap();
        y2p_core::recorded::check(env!("CARGO_MANIFEST_DIR"), "evidence-tarfile.tar", &ours);
    }
}
