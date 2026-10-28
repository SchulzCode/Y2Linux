//! `hashlib.sha256(...).hexdigest()` for the service group (update payloads, radio firmware), so the
//! command crate needs no hash dependency of its own.
// SPDX-License-Identifier: GPL-2.0-only
use sha2::Digest;

/// A running SHA-256 (`hashlib.sha256()` with `update`).
#[derive(Clone, Default)]
pub struct Sha256(sha2::Sha256);

impl Sha256 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    /// `hexdigest()`: lowercase hex.
    pub fn hexdigest(self) -> String {
        self.0
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}

/// `hashlib.sha256(data).hexdigest()`.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(data);
    digest.hexdigest()
}

#[cfg(test)]
mod tests {
    #[test]
    fn digests_match_hashlib() {
        // python3 -c "import hashlib; print(hashlib.sha256(b'abc').hexdigest())"
        assert_eq!(
            super::sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
