//! SHA-256 as the records and reports spell it: lowercase hex.
// SPDX-License-Identifier: GPL-2.0-only
use sha2::Digest;

/// Lowercase hex of `bytes`.
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    text
}

/// The SHA-256 of `data` in lowercase hex.
pub fn sha256_hex(data: &[u8]) -> String {
    hex(&sha2::Sha256::digest(data))
}

/// A running SHA-256 of the bytes given to `update` (update payloads, radio firmware, Y2Media copies).
#[derive(Clone, Default)]
pub struct Sha256(sha2::Sha256);

impl Sha256 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    /// The digest as lowercase hex.
    pub fn hexdigest(self) -> String {
        hex(&self.0.finalize())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn digests_match_hashlib() {
        // python3 -c "import hashlib; h=hashlib.sha256(b'a'); h.update(b'bc'); print(h.hexdigest())"
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(super::sha256_hex(b"abc"), abc);
        let mut stream = super::Sha256::new();
        stream.update(b"a");
        stream.update(b"bc");
        assert_eq!(stream.hexdigest(), abc);
        assert_eq!(super::hex(&[0, 15, 255]), "000fff");
    }
}
