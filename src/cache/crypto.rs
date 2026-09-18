//! Cache file envelope encryption (see `contracts/cache-format.md`).
//!
//! Layout: magic `CPC1` (4) | version (2, LE) | XChaCha20-Poly1305 nonce (24) | ciphertext+tag.
//! The 30-byte header is authenticated as associated data.

use chacha20poly1305::{AeadInOut, KeyInit, XChaCha20Poly1305, XNonce};
use zeroize::Zeroizing;

use super::CacheError;

pub const MAGIC: &[u8; 4] = b"CPC1";
pub const ENVELOPE_VERSION: u16 = 1;
pub const HEADER_LEN: usize = 30;
pub const KEY_LEN: usize = 32;

pub type CacheKey = Zeroizing<[u8; KEY_LEN]>;

pub fn generate_key() -> CacheKey {
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    rand::fill(&mut key[..]);
    key
}

fn cipher(key: &[u8; KEY_LEN]) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(&(*key).into())
}

pub fn seal(key: &[u8; KEY_LEN], plaintext: &[u8]) -> Result<Vec<u8>, CacheError> {
    let mut nonce = [0u8; 24];
    rand::fill(&mut nonce[..]);
    let mut header = Vec::with_capacity(HEADER_LEN);
    header.extend_from_slice(MAGIC);
    header.extend_from_slice(&ENVELOPE_VERSION.to_le_bytes());
    header.extend_from_slice(&nonce);

    let mut buffer = plaintext.to_vec();
    cipher(key)
        .encrypt_in_place(&XNonce::from(nonce), &header, &mut buffer)
        .map_err(|_| CacheError::Corrupt)?;
    header.extend_from_slice(&buffer);
    Ok(header)
}

pub fn open(key: &[u8; KEY_LEN], file: &[u8]) -> Result<Zeroizing<Vec<u8>>, CacheError> {
    if file.len() < HEADER_LEN || &file[..4] != MAGIC {
        return Err(CacheError::Corrupt);
    }
    if u16::from_le_bytes([file[4], file[5]]) != ENVELOPE_VERSION {
        return Err(CacheError::Unsupported);
    }
    let (header, body) = file.split_at(HEADER_LEN);
    let nonce: [u8; 24] = header[6..].try_into().map_err(|_| CacheError::Corrupt)?;
    let mut buffer = Zeroizing::new(body.to_vec());
    cipher(key)
        .decrypt_in_place(&XNonce::from(nonce), header, &mut *buffer)
        .map_err(|_| CacheError::Corrupt)?;
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; KEY_LEN] = [7; KEY_LEN];

    #[test]
    fn round_trip() {
        let sealed = seal(&KEY, b"hello cache").unwrap();
        assert_eq!(open(&KEY, &sealed).unwrap().as_slice(), b"hello cache");
    }

    #[test]
    fn header_layout() {
        let sealed = seal(&KEY, b"x").unwrap();
        assert_eq!(&sealed[..4], MAGIC);
        assert_eq!(u16::from_le_bytes([sealed[4], sealed[5]]), 1);
        assert_eq!(sealed.len(), HEADER_LEN + 1 + 16);
    }

    #[test]
    fn nonces_differ() {
        let a = seal(&KEY, b"same").unwrap();
        let b = seal(&KEY, b"same").unwrap();
        assert_ne!(a[6..30], b[6..30]);
        assert_ne!(a, b);
    }

    #[test]
    fn any_flipped_byte_fails() {
        let sealed = seal(&KEY, b"tamper me").unwrap();
        for i in 0..sealed.len() {
            let mut bad = sealed.clone();
            bad[i] ^= 0x01;
            assert!(open(&KEY, &bad).is_err(), "byte {i}");
        }
    }

    #[test]
    fn wrong_key_or_truncation_fails() {
        let sealed = seal(&KEY, b"data").unwrap();
        assert!(open(&[8; KEY_LEN], &sealed).is_err());
        assert!(open(&KEY, &sealed[..sealed.len() - 1]).is_err());
        assert!(open(&KEY, &sealed[..10]).is_err());
        assert!(open(&KEY, b"").is_err());
    }

    #[test]
    fn unsupported_version_is_reported() {
        let mut sealed = seal(&KEY, b"data").unwrap();
        sealed[4] = 9;
        assert!(matches!(open(&KEY, &sealed), Err(CacheError::Unsupported)));
    }

    #[test]
    fn generated_keys_differ() {
        assert_ne!(*generate_key(), *generate_key());
    }
}
