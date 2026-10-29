//! Random access to an Android sparse image (or a raw ext4 image) and just enough ext2/3/4 to read a
//! few named regular files from the stock `system.img`.
// SPDX-License-Identifier: GPL-2.0-only
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
use y2p_core::py;

/// Why an image cannot be read: a ValueError (its own reasons, or `struct.error` texts for a
/// truncated structure, CONTRACT.md accepted differences), an OSError, or FileNotFoundError(path)
/// from a lookup.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Value(String),
    Os(String),
    NotFound,
}

type Result<T> = std::result::Result<T, Error>;

fn value(text: &str) -> Error {
    Error::Value(text.into())
}

fn os(error: std::io::Error) -> Error {
    Error::Os(py::os_error(&error, None).0)
}

fn field(buffer: &[u8], offset: usize, size: usize) -> Result<&[u8]> {
    buffer.get(offset..offset + size).ok_or_else(|| {
        Error::Value(format!(
            "unpack_from requires a buffer of at least {} bytes for unpacking {size} bytes at offset \
             {offset} (actual buffer size is {})",
            offset + size,
            buffer.len()
        ))
    })
}

fn u16_at(buffer: &[u8], offset: usize) -> Result<u16> {
    let bytes = field(buffer, offset, 2)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn u32_at(buffer: &[u8], offset: usize) -> Result<u32> {
    let bytes = field(buffer, offset, 4)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

const RAW: u16 = 0xCAC1;
const FILL: u16 = 0xCAC2;
const DONT_CARE: u16 = 0xCAC3;
const CRC: u16 = 0xCAC4;

enum Data {
    Raw(u64),
    Fill(Vec<u8>),
    Zero,
}

struct Chunk {
    start: u128,
    blocks: u128,
    data: Data,
}

pub struct Image {
    file: File,
    block: u128,
    chunks: Vec<Chunk>,
    sparse: bool,
}

impl Image {
    pub fn open(path: &Path) -> Result<Image> {
        let mut file = File::open(path)
            .map_err(|e| Error::Os(py::os_error(&e, Some(&path.to_string_lossy())).0))?;
        let mut header = Vec::new();
        Read::by_ref(&mut file)
            .take(28)
            .read_to_end(&mut header)
            .map_err(os)?;
        if header.len() == 28 && u32_at(&header, 0)? == 0xED26_FF3A {
            let major = u16_at(&header, 4)?;
            let file_hdr = u16_at(&header, 8)? as u64;
            let chunk_hdr = u16_at(&header, 10)? as u64;
            let block = u32_at(&header, 12)? as u128;
            let total = u32_at(&header, 16)? as u128;
            let count = u32_at(&header, 20)?;
            if major != 1 || file_hdr < 28 || chunk_hdr < 12 || !block.is_multiple_of(4) {
                return Err(value("unsupported_sparse_image"));
            }
            let (mut offset, mut at) = (file_hdr, 0u128);
            let mut chunks = Vec::new();
            for _ in 0..count {
                file.seek(SeekFrom::Start(offset)).map_err(os)?;
                let mut head = Vec::new();
                Read::by_ref(&mut file)
                    .take(12)
                    .read_to_end(&mut head)
                    .map_err(os)?;
                if head.len() != 12 {
                    return Err(Error::Value("unpack requires a buffer of 12 bytes".into()));
                }
                let kind = u16_at(&head, 0)?;
                let blocks = u32_at(&head, 4)? as u128;
                let size = u32_at(&head, 8)? as u64;
                let data = offset + chunk_hdr;
                match kind {
                    RAW => {
                        if size as u128 != chunk_hdr as u128 + blocks * block {
                            return Err(value("sparse_chunk_size"));
                        }
                        chunks.push(Chunk {
                            start: at,
                            blocks,
                            data: Data::Raw(data),
                        });
                    }
                    FILL => {
                        file.seek(SeekFrom::Start(data)).map_err(os)?;
                        let mut pattern = Vec::new();
                        Read::by_ref(&mut file)
                            .take(4)
                            .read_to_end(&mut pattern)
                            .map_err(os)?;
                        chunks.push(Chunk {
                            start: at,
                            blocks,
                            data: Data::Fill(pattern),
                        });
                    }
                    DONT_CARE => chunks.push(Chunk {
                        start: at,
                        blocks,
                        data: Data::Zero,
                    }),
                    CRC => {}
                    _ => return Err(value("sparse_chunk_type")),
                }
                at += blocks;
                offset += size;
            }
            if at != total {
                return Err(value("sparse_block_count"));
            }
            return Ok(Image {
                file,
                block,
                chunks,
                sparse: true,
            });
        }
        let size = file.metadata().map_err(os)?.len() as u128;
        Ok(Image {
            file,
            block: 4096,
            chunks: vec![Chunk {
                start: 0,
                blocks: size.div_ceil(4096),
                data: Data::Raw(0),
            }],
            sparse: false,
        })
    }

    pub fn read(&mut self, mut offset: u128, mut length: u128) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        while length > 0 {
            if self.block == 0 {
                return Err(value("integer division or modulo by zero"));
            }
            let (block, within) = (offset / self.block, offset % self.block);
            let Some(chunk) = self
                .chunks
                .iter()
                .find(|c| c.start <= block && block < c.start + c.blocks)
            else {
                return Err(value("image_offset_outside"));
            };
            let take = length.min((chunk.start + chunk.blocks - block) * self.block - within);
            let take_usize = usize::try_from(take).map_err(|_| value("image_offset_outside"))?;
            match &chunk.data {
                Data::Raw(data) => {
                    let at = *data as u128 + (block - chunk.start) * self.block + within;
                    let at = u64::try_from(at).map_err(|_| value("image_offset_outside"))?;
                    self.file.seek(SeekFrom::Start(at)).map_err(os)?;
                    let mut part = Vec::with_capacity(take_usize.min(1 << 26));
                    Read::by_ref(&mut self.file)
                        .take(take as u64)
                        .read_to_end(&mut part)
                        .map_err(os)?;
                    if part.len() != take_usize && self.sparse {
                        return Err(value("sparse_short_read"));
                    }
                    part.resize(take_usize, 0);
                    out.extend_from_slice(&part);
                }
                Data::Fill(pattern) => {
                    // pattern * (take // 4 + 2), sliced from within % 4.
                    let start = (within % 4) as usize;
                    let repeated: Vec<u8> = pattern
                        .iter()
                        .copied()
                        .cycle()
                        .take(pattern.len() * (take_usize / 4 + 2))
                        .collect();
                    let end = (start + take_usize).min(repeated.len());
                    out.extend_from_slice(repeated.get(start..end).unwrap_or(&[]));
                }
                Data::Zero => out.resize(out.len() + take_usize, 0),
            }
            offset += take;
            length -= take;
        }
        Ok(out)
    }
}

pub struct Ext4 {
    image: Image,
    bs: u128,
    ipg: u128,
    isize: u128,
    incompat: u32,
    dsize: u128,
    gdt: u128,
}

const S_IFMT: u16 = 0o170000;
pub const S_IFDIR: u16 = 0o040000;
pub const S_IFREG: u16 = 0o100000;

impl Ext4 {
    pub fn new(mut image: Image) -> Result<Ext4> {
        let sb = image.read(1024, 1024)?;
        if u16_at(&sb, 56)? != 0xEF53 {
            return Err(value("not_ext4"));
        }
        let log = u32_at(&sb, 24)?;
        let bs = if log > 64 {
            u128::MAX / 4
        } else {
            1024u128 << log
        };
        let first = u32_at(&sb, 20)? as u128;
        let ipg = u32_at(&sb, 40)? as u128;
        let rev = u32_at(&sb, 76)?;
        let isize = if rev != 0 {
            u16_at(&sb, 88)? as u128
        } else {
            128
        };
        let incompat = u32_at(&sb, 96)?;
        let mut dsize = if incompat & 0x80 != 0 {
            u16_at(&sb, 254)? as u128
        } else {
            32
        };
        if dsize == 0 {
            dsize = 32;
        }
        Ok(Ext4 {
            image,
            bs,
            ipg,
            isize,
            incompat,
            dsize,
            gdt: (first + 1).saturating_mul(bs),
        })
    }

    fn block(&mut self, n: u128, count: u128) -> Result<Vec<u8>> {
        self.image
            .read(n.saturating_mul(self.bs), count.saturating_mul(self.bs))
    }

    fn inode(&mut self, number: u128) -> Result<Vec<u8>> {
        if self.ipg == 0 {
            return Err(value("integer division or modulo by zero"));
        }
        let (group, index) = ((number - 1) / self.ipg, (number - 1) % self.ipg);
        let desc = self.image.read(self.gdt + group * self.dsize, self.dsize)?;
        let mut table = u32_at(&desc, 8)? as u128;
        if self.dsize >= 64 {
            table |= (u32_at(&desc, 0x28)? as u128) << 32;
        }
        self.image.read(
            table.saturating_mul(self.bs) + index * self.isize,
            self.isize,
        )
    }

    fn data(&mut self, number: u128, limit: u64) -> Result<(u16, Vec<u8>)> {
        let raw = self.inode(number)?;
        let mode = u16_at(&raw, 0)?;
        let size = u32_at(&raw, 4)? as u64 | (u32_at(&raw, 0x6C)? as u64) << 32;
        if size > limit {
            return Err(value("ext4_file_too_large"));
        }
        let flags = u32_at(&raw, 0x20)?;
        let blocks = field(&raw, 0x28, 60)?.to_vec();
        if flags & 0x1000_0000 != 0 {
            return Err(value("ext4_inline_data"));
        }
        let runs = if flags & 0x80000 != 0 {
            self.extents(&blocks, 8)?
        } else {
            self.indirect(&blocks, size as u128)?
        };
        let size = size as usize;
        let mut out = Vec::new();
        for (logical, physical, count) in runs {
            let need = logical.saturating_mul(self.bs);
            if (out.len() as u128) < need {
                // Beyond the file's size nothing is kept (out[:size]).
                out.resize(need.min(size as u128 + 1) as usize, 0);
            }
            let wanted = count.saturating_mul(self.bs);
            let room = (size as u128).saturating_sub(out.len() as u128);
            if room == 0 {
                continue;
            }
            if physical != 0 {
                let part = self
                    .image
                    .read(physical.saturating_mul(self.bs), wanted.min(room))?;
                out.extend_from_slice(&part);
            } else {
                out.resize(out.len() + wanted.min(room) as usize, 0);
            }
        }
        out.resize(size.max(out.len()), 0);
        out.truncate(size);
        Ok((mode, out))
    }

    fn extents(&mut self, node: &[u8], guard: u32) -> Result<Vec<(u128, u128, u128)>> {
        let magic = u16_at(node, 0)?;
        let entries = u16_at(node, 2)?;
        let depth = u16_at(node, 6)?;
        if magic != 0xF30A || guard == 0 {
            return Err(value("ext4_extent_header"));
        }
        let mut runs = Vec::new();
        for i in 0..entries as usize {
            let e = node
                .get(12 + 12 * i..(24 + 12 * i).min(node.len()))
                .unwrap_or(&[]);
            if depth != 0 {
                let leaf = u32_at(e, 4)? as u128 | (u16_at(e, 8)? as u128) << 32;
                let block = self.block(leaf, 1)?;
                runs.extend(self.extents(&block, guard - 1)?);
            } else {
                if e.len() != 12 {
                    return Err(value("unpack requires a buffer of 12 bytes"));
                }
                let logical = u32_at(e, 0)? as u128;
                let mut length = u16_at(e, 4)? as u128;
                let hi = u16_at(e, 6)? as u128;
                let lo = u32_at(e, 8)? as u128;
                let uninit = length > 32768;
                if uninit {
                    length -= 32768;
                }
                runs.push((logical, if uninit { 0 } else { hi << 32 | lo }, length));
            }
        }
        Ok(runs)
    }

    fn indirect(&mut self, blocks: &[u8], size: u128) -> Result<Vec<(u128, u128, u128)>> {
        let pointers: Vec<u128> = (0..15)
            .map(|i| u32_at(blocks, 4 * i).map(|v| v as u128))
            .collect::<Result<_>>()?;
        let per = self.bs / 4;
        let want = if self.bs == 0 {
            0
        } else {
            size.div_ceil(self.bs)
        };
        let mut found: Vec<u128> = pointers[..12].to_vec();
        for (level, block) in [(1u32, pointers[12]), (2, pointers[13]), (3, pointers[14])] {
            if found.len() as u128 >= want {
                break;
            }
            let base = found.len() as u128;
            let walked = self.walk(block, level, per, want, base)?;
            found.extend(walked);
        }
        found.truncate(want.min(found.len() as u128) as usize);
        Ok(found
            .into_iter()
            .enumerate()
            .filter(|(_, b)| *b != 0)
            .map(|(i, b)| (i as u128, b, 1))
            .collect())
    }

    /// One indirect level; `base` is how many block numbers were found before it (the walk stops once
    /// `want` are known: a hole's zeros beyond that are never needed).
    fn walk(
        &mut self,
        block: u128,
        level: u32,
        per: u128,
        want: u128,
        base: u128,
    ) -> Result<Vec<u128>> {
        if block == 0 {
            let zeros = per.saturating_pow(level).min(want.saturating_sub(base));
            return Ok(vec![0; zeros as usize]);
        }
        if level == 0 {
            return Ok(vec![block]);
        }
        let data = self.block(block, 1)?;
        let mut out = Vec::new();
        for i in 0..per as usize {
            if base + out.len() as u128 >= want {
                break;
            }
            let b = u32_at(&data, 4 * i)? as u128;
            let below = self.walk(b, level - 1, per, want, base + out.len() as u128)?;
            out.extend(below);
        }
        Ok(out)
    }

    fn lookup(&mut self, path: &str) -> Result<u128> {
        let mut number = 2;
        for name in path.split('/').filter(|p| !p.is_empty()) {
            let (mode, entries) = self.data(number, 1 << 24)?;
            if mode & S_IFMT != S_IFDIR {
                return Err(value("ext4_not_a_directory"));
            }
            let filetype = self.incompat & 0x2 != 0;
            let (mut at, mut found) = (0usize, None);
            while at + 8 <= entries.len() {
                let ino = u32_at(&entries, at)?;
                let rec = u16_at(&entries, at + 4)? as usize;
                let length = if filetype {
                    entries[at + 6] as usize
                } else {
                    u16_at(&entries, at + 6)? as usize
                };
                if rec < 8 {
                    break;
                }
                let end = (at + 8 + length).min(entries.len());
                if ino != 0 && entries.get(at + 8..end) == Some(name.as_bytes()) {
                    found = Some(ino);
                    break;
                }
                at += rec;
            }
            number = found.ok_or(Error::NotFound)? as u128;
        }
        Ok(number)
    }

    /// `Ext4.read`: a regular file's bytes, at most `limit`.
    pub fn read(&mut self, path: &str, limit: u64) -> Result<Vec<u8>> {
        let number = self.lookup(path)?;
        let (mode, data) = self.data(number, limit)?;
        if mode & S_IFMT != S_IFREG {
            return Err(value("ext4_not_a_regular_file"));
        }
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn mke2fs(tree: &Path, image: &Path, options: &[&str], size: &str) -> bool {
        Command::new("mke2fs")
            .args(["-q", "-F"])
            .args(options)
            .arg("-d")
            .arg(tree)
            .arg(image)
            .arg(size)
            .output()
            .is_ok_and(|o| o.status.success())
    }

    /// Android sparse encoding as tests/test_radio_firmware.py's `sparse()`.
    fn sparse(raw: &[u8], block: usize) -> Vec<u8> {
        let blocks: Vec<Vec<u8>> = raw
            .chunks(block)
            .map(|c| {
                let mut c = c.to_vec();
                c.resize(block, 0);
                c
            })
            .collect();
        let zero = vec![0u8; block];
        let mut chunks: Vec<Vec<u8>> = Vec::new();
        let mut i = 0;
        while i < blocks.len() {
            let mut j = i;
            let mut chunk = Vec::new();
            if blocks[i] == zero {
                while j < blocks.len() && blocks[j] == zero {
                    j += 1;
                }
                chunk.extend_from_slice(&DONT_CARE.to_le_bytes());
                chunk.extend_from_slice(&0u16.to_le_bytes());
                chunk.extend_from_slice(&((j - i) as u32).to_le_bytes());
                chunk.extend_from_slice(&12u32.to_le_bytes());
            } else {
                while j < blocks.len() && blocks[j] != zero {
                    j += 1;
                }
                chunk.extend_from_slice(&RAW.to_le_bytes());
                chunk.extend_from_slice(&0u16.to_le_bytes());
                chunk.extend_from_slice(&((j - i) as u32).to_le_bytes());
                chunk.extend_from_slice(&((12 + (j - i) * block) as u32).to_le_bytes());
                for b in &blocks[i..j] {
                    chunk.extend_from_slice(b);
                }
            }
            chunks.push(chunk);
            i = j;
        }
        let mut fill = Vec::new();
        fill.extend_from_slice(&FILL.to_le_bytes());
        fill.extend_from_slice(&0u16.to_le_bytes());
        fill.extend_from_slice(&2u32.to_le_bytes());
        fill.extend_from_slice(&16u32.to_le_bytes());
        fill.extend_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]);
        chunks.push(fill);
        let mut out = Vec::new();
        out.extend_from_slice(&0xED26_FF3Au32.to_le_bytes());
        for v in [1u16, 0, 28, 12] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for v in [
            block as u32,
            (blocks.len() + 2) as u32,
            chunks.len() as u32,
            0,
        ] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for c in chunks {
            out.extend(c);
        }
        out
    }

    #[test]
    fn files_are_read_from_raw_sparse_and_indirect_images() {
        let dir = std::env::temp_dir().join(format!("y2-image-{}", std::process::id()));
        let tree = dir.join("tree");
        std::fs::create_dir_all(tree.join("etc/firmware")).unwrap();
        let big: Vec<u8> = (0..300_000u32).map(|i| (i * 7 % 251) as u8).collect();
        std::fs::write(tree.join("etc/firmware/big.bin"), &big).unwrap();
        std::fs::write(tree.join("etc/firmware/small.cfg"), b"coex=1\n").unwrap();
        for (options, size) in [
            (&["-b", "4096"][..], "16M"),
            (
                &["-b", "1024", "-O", "^extent,^64bit", "-t", "ext3"][..],
                "4M",
            ),
        ] {
            let raw = dir.join("raw.img");
            if !mke2fs(&tree, &raw, options, size) {
                return; // no host mke2fs
            }
            let mut fs = Ext4::new(Image::open(&raw).unwrap()).unwrap();
            assert_eq!(fs.read("/etc/firmware/big.bin", 1 << 20).unwrap(), big);
            assert_eq!(fs.read("/etc/firmware/small.cfg", 7).unwrap(), b"coex=1\n");
            assert_eq!(
                fs.read("/etc/firmware/small.cfg", 6),
                Err(value("ext4_file_too_large"))
            );
            assert_eq!(fs.read("/etc/firmware/none", 6), Err(Error::NotFound));
            assert_eq!(
                fs.read("/etc/firmware", 1 << 20),
                Err(value("ext4_not_a_regular_file"))
            );
            assert_eq!(
                fs.read("/etc/firmware/small.cfg/x", 6),
                Err(value("ext4_not_a_directory"))
            );
            let block = if options[1] == "4096" { 4096 } else { 1024 };
            let packed = dir.join("sparse.img");
            std::fs::write(&packed, sparse(&std::fs::read(&raw).unwrap(), block)).unwrap();
            let mut fs = Ext4::new(Image::open(&packed).unwrap()).unwrap();
            assert_eq!(fs.read("/etc/firmware/big.bin", 1 << 20).unwrap(), big);
        }
        let text = dir.join("text.img");
        std::fs::write(&text, b"not an image").unwrap();
        assert_eq!(
            Ext4::new(Image::open(&text).unwrap()).err(),
            Some(value("not_ext4"))
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn fill_chunks_repeat_their_pattern() {
        let mut image = sparse(&[], 4096);
        // Only the fill chunk (2 blocks of aa bb cc dd).
        let path = std::env::temp_dir().join(format!("y2-fill-{}", std::process::id()));
        image.truncate(image.len());
        std::fs::write(&path, &image).unwrap();
        let mut img = Image::open(&path).unwrap();
        assert_eq!(
            img.read(5, 6).unwrap(),
            [0xbb, 0xcc, 0xdd, 0xaa, 0xbb, 0xcc]
        );
        assert_eq!(img.read(8192, 1), Err(value("image_offset_outside")));
        std::fs::remove_file(&path).unwrap();
    }
}
