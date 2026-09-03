//! Application-layer end-to-end encryption for sensitive channels.
//!
//! Provides ChaCha20-Poly1305 AEAD encryption that wraps QUIC transport,
//! ensuring that even if the QUIC TLS session is compromised (e.g., by a
//! malicious relay), the application data remains protected.
//!
//! Each session derives a unique key from a shared secret via HKDF-SHA256,
//! with per-message nonces derived from a monotonically increasing counter.

use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, CHACHA20_POLY1305};
use ring::digest::{Context, SHA256};
use ring::hkdf::Salt;

use alldesk_core::error::Error;
use alldesk_core::Result;

/// Tag byte prefixed to encrypted messages to identify the algorithm.
const ALGO_CHACHA20_POLY1305: u8 = 0x01;
const ALGO_AES_256_GCM: u8 = 0x02;

/// Byte length of the AEAD tag (both ChaCha20-Poly1305 and AES-256-GCM use 16 bytes).
const TAG_LEN: usize = 16;

/// Nonce length for both algorithms.
const NONCE_LEN: usize = 12;

/// Key length for both algorithms (256-bit).
const KEY_LEN: usize = 32;

/// HKDF info strings for key derivation.
const KEY_INFO: &[u8] = b"alldesk-e2e-encryption-key";
const _NONCE_INFO: &[u8] = b"alldesk-e2e-encryption-nonce";

/// Encryption algorithm selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CryptoAlgorithm {
    ChaCha20Poly1305,
    Aes256Gcm,
}

impl CryptoAlgorithm {
    fn tag_byte(&self) -> u8 {
        match self {
            CryptoAlgorithm::ChaCha20Poly1305 => ALGO_CHACHA20_POLY1305,
            CryptoAlgorithm::Aes256Gcm => ALGO_AES_256_GCM,
        }
    }

    fn from_tag_byte(tag: u8) -> Option<Self> {
        match tag {
            ALGO_CHACHA20_POLY1305 => Some(CryptoAlgorithm::ChaCha20Poly1305),
            ALGO_AES_256_GCM => Some(CryptoAlgorithm::Aes256Gcm),
            _ => None,
        }
    }
}

/// Holds an AEAD key with its associated algorithm.
struct AeadKey {
    key: LessSafeKey,
    algorithm: CryptoAlgorithm,
}

/// End-to-end encryption context for a single direction (send or receive).
///
/// Derives per-session keys from a shared secret using HKDF-SHA256.
/// Each message uses a unique nonce derived from a monotonically increasing
/// counter to prevent nonce reuse.
pub struct E2ECrypto {
    key: AeadKey,
    /// Monotonically increasing counter for nonce derivation.
    counter: std::sync::atomic::AtomicU64,
    /// Per-counter nonce prefix (first 4 bytes of the nonce).
    nonce_prefix: [u8; 4],
}

impl E2ECrypto {
    /// Derive encryption keys from a shared secret and session ID.
    ///
    /// Uses HKDF-SHA256 with the shared secret as input key material,
    /// and `session_id` as the salt. Both sides must use the same
    /// shared secret and session ID to derive the same keys.
    pub fn new(
        shared_secret: &[u8],
        session_id: &[u8],
        algorithm: CryptoAlgorithm,
    ) -> Result<Self> {
        let key_material = Self::derive_key(shared_secret, session_id)?;
        let unbound_key = match algorithm {
            CryptoAlgorithm::ChaCha20Poly1305 => UnboundKey::new(&CHACHA20_POLY1305, &key_material)
                .map_err(|e| Error::Network(format!("create chacha20 key: {}", e)))?,
            CryptoAlgorithm::Aes256Gcm => UnboundKey::new(&AES_256_GCM, &key_material)
                .map_err(|e| Error::Network(format!("create aes key: {}", e)))?,
        };

        // Derive a 4-byte nonce prefix from the session ID for additional domain separation.
        let mut nonce_prefix = [0u8; 4];
        let mut hasher = Context::new(&SHA256);
        hasher.update(session_id);
        hasher.update(b"nonce-prefix");
        let hash = hasher.finish();
        nonce_prefix.copy_from_slice(&hash.as_ref()[..4]);

        Ok(Self {
            key: AeadKey {
                key: LessSafeKey::new(unbound_key),
                algorithm,
            },
            counter: std::sync::atomic::AtomicU64::new(0),
            nonce_prefix,
        })
    }

    /// Derive a 32-byte key using HKDF-SHA256.
    fn derive_key(shared_secret: &[u8], session_id: &[u8]) -> Result<[u8; KEY_LEN]> {
        let salt = Salt::new(ring::hkdf::HKDF_SHA256, session_id);
        let prk = salt.extract(shared_secret);

        // Use a simple struct to declare our desired output length.
        struct KeyLen;
        impl ring::hkdf::KeyType for KeyLen {
            fn len(&self) -> usize {
                KEY_LEN
            }
        }

        let okm = prk
            .expand(&[KEY_INFO], KeyLen)
            .map_err(|e| Error::Network(format!("hkdf expand: {}", e)))?;

        let mut key = [0u8; KEY_LEN];
        okm.fill(&mut key)
            .map_err(|e| Error::Network(format!("hkdf fill: {}", e)))?;
        Ok(key)
    }

    /// Build a 12-byte nonce from the counter value and prefix.
    /// Format: [nonce_prefix (4 bytes)] [counter (8 bytes big-endian)]
    fn make_nonce(&self, counter: u64) -> Nonce {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        nonce_bytes[..4].copy_from_slice(&self.nonce_prefix);
        nonce_bytes[4..].copy_from_slice(&counter.to_be_bytes());
        Nonce::assume_unique_for_key(nonce_bytes)
    }

    /// Encrypt a plaintext message. Returns ciphertext with algorithm tag prefix.
    ///
    /// Output format: [algo_tag (1)] [counter (8 BE)] [ciphertext+tag]
    /// The counter is included for nonce derivation on the decrypt side.
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let counter = self
            .counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nonce = self.make_nonce(counter);

        // Build in_out buffer: just the plaintext; seal_in_place_append_tag
        // will encrypt in-place and extend with the authentication tag.
        let mut in_out = plaintext.to_vec();

        self.key
            .key
            .seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
            .map_err(|e| Error::Network(format!("encrypt: {}", e)))?;

        // Prepend header: [algo_tag (1)] [counter (8 BE)]
        let mut out = Vec::with_capacity(9 + in_out.len());
        out.push(self.key.algorithm.tag_byte());
        out.extend_from_slice(&counter.to_be_bytes());
        out.extend_from_slice(&in_out);

        Ok(out)
    }

    /// Decrypt a ciphertext produced by `encrypt`.
    pub fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        if ciphertext.len() < 1 + 8 + TAG_LEN {
            return Err(Error::Network("ciphertext too short".into()));
        }

        let algo_tag = ciphertext[0];
        let algo = CryptoAlgorithm::from_tag_byte(algo_tag)
            .ok_or_else(|| Error::Network(format!("unknown algorithm tag: {}", algo_tag)))?;

        if algo != self.key.algorithm {
            return Err(Error::Network(format!(
                "algorithm mismatch: expected {:?}, got {:?}",
                self.key.algorithm, algo
            )));
        }

        let mut counter_bytes = [0u8; 8];
        counter_bytes.copy_from_slice(&ciphertext[1..9]);
        let counter = u64::from_be_bytes(counter_bytes);
        let nonce = self.make_nonce(counter);

        // Copy the encrypted payload so we can decrypt in-place.
        let mut payload = ciphertext[9..].to_vec();
        let plaintext_len = self
            .key
            .key
            .open_in_place(nonce, Aad::empty(), &mut payload)
            .map_err(|e| Error::Network(format!("decrypt: {}", e)))?
            .len();

        payload.truncate(plaintext_len);
        Ok(payload)
    }

    /// Get the algorithm used by this context.
    pub fn algorithm(&self) -> CryptoAlgorithm {
        self.key.algorithm
    }

    /// Get the current counter value (for diagnostics).
    pub fn counter(&self) -> u64 {
        self.counter.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Compute SHA-256 HMAC for message authentication.
/// Used for verifying integrity of unencrypted control messages.
pub fn hmac_verify(key: &[u8], message: &[u8]) -> Result<Vec<u8>> {
    let hmac_key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key);
    let tag = ring::hmac::sign(&hmac_key, message);
    Ok(tag.as_ref().to_vec())
}

/// Verify a HMAC tag.
pub fn hmac_check(key: &[u8], message: &[u8], expected_tag: &[u8]) -> bool {
    let hmac_key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key);
    ring::hmac::verify(&hmac_key, message, expected_tag).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_crypto(algo: CryptoAlgorithm) -> E2ECrypto {
        E2ECrypto::new(b"test-shared-secret-32bytes-long!", b"session-123", algo).unwrap()
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip_chacha20() {
        let crypto = make_crypto(CryptoAlgorithm::ChaCha20Poly1305);
        let plaintext = b"hello remote desktop!";
        let encrypted = crypto.encrypt(plaintext).unwrap();
        let decrypted = crypto.decrypt(&encrypted).unwrap();
        assert_eq!(&decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip_aes256() {
        let crypto = make_crypto(CryptoAlgorithm::Aes256Gcm);
        let plaintext = b"hello remote desktop!";
        let encrypted = crypto.encrypt(plaintext).unwrap();
        let decrypted = crypto.decrypt(&encrypted).unwrap();
        assert_eq!(&decrypted, plaintext);
    }

    #[test]
    fn test_ciphertext_larger_than_plaintext() {
        let crypto = make_crypto(CryptoAlgorithm::ChaCha20Poly1305);
        let plaintext = b"short";
        let encrypted = crypto.encrypt(plaintext).unwrap();
        // 1 (algo) + 8 (counter) + 5 (plaintext) + 16 (tag) = 30
        assert_eq!(encrypted.len(), 1 + 8 + plaintext.len() + TAG_LEN);
    }

    #[test]
    fn test_multiple_messages_different_ciphertexts() {
        let crypto = make_crypto(CryptoAlgorithm::ChaCha20Poly1305);
        let plaintext = b"same message";
        let enc1 = crypto.encrypt(plaintext).unwrap();
        let enc2 = crypto.encrypt(plaintext).unwrap();
        // Different nonces should produce different ciphertexts.
        assert_ne!(enc1, enc2);
        // Both should decrypt correctly.
        assert_eq!(&crypto.decrypt(&enc1).unwrap(), plaintext);
        assert_eq!(&crypto.decrypt(&enc2).unwrap(), plaintext);
    }

    #[test]
    fn test_counter_increments() {
        let crypto = make_crypto(CryptoAlgorithm::ChaCha20Poly1305);
        assert_eq!(crypto.counter(), 0);
        crypto.encrypt(b"a").unwrap();
        assert_eq!(crypto.counter(), 1);
        crypto.encrypt(b"b").unwrap();
        assert_eq!(crypto.counter(), 2);
    }

    #[test]
    fn test_empty_plaintext() {
        let crypto = make_crypto(CryptoAlgorithm::ChaCha20Poly1305);
        let encrypted = crypto.encrypt(b"").unwrap();
        let decrypted = crypto.decrypt(&encrypted).unwrap();
        assert!(decrypted.is_empty());
    }

    #[test]
    fn test_large_plaintext() {
        let crypto = make_crypto(CryptoAlgorithm::ChaCha20Poly1305);
        let large: Vec<u8> = (0..65536).map(|i| (i % 256) as u8).collect();
        let encrypted = crypto.encrypt(&large).unwrap();
        let decrypted = crypto.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, large);
    }

    #[test]
    fn test_decrypt_tampered_ciphertext_fails() {
        let crypto = make_crypto(CryptoAlgorithm::ChaCha20Poly1305);
        let mut encrypted = crypto.encrypt(b"secret data").unwrap();
        // Tamper with a byte in the ciphertext.
        let last = encrypted.len() - 1;
        encrypted[last] ^= 0xFF;
        let result = crypto.decrypt(&encrypted);
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_truncated_ciphertext_fails() {
        let crypto = make_crypto(CryptoAlgorithm::ChaCha20Poly1305);
        let encrypted = crypto.encrypt(b"secret data").unwrap();
        let result = crypto.decrypt(&encrypted[..10]);
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_wrong_key_fails() {
        let crypto1 = E2ECrypto::new(
            b"key-one-32-bytes-long-padding!!",
            b"session",
            CryptoAlgorithm::ChaCha20Poly1305,
        )
        .unwrap();
        let crypto2 = E2ECrypto::new(
            b"key-two-32-bytes-long-padding!!",
            b"session",
            CryptoAlgorithm::ChaCha20Poly1305,
        )
        .unwrap();
        let encrypted = crypto1.encrypt(b"secret").unwrap();
        let result = crypto2.decrypt(&encrypted);
        assert!(result.is_err());
    }

    #[test]
    fn test_algorithm_mismatch_fails() {
        let chacha = make_crypto(CryptoAlgorithm::ChaCha20Poly1305);
        let aes = make_crypto(CryptoAlgorithm::Aes256Gcm);
        let encrypted = chacha.encrypt(b"test").unwrap();
        let result = aes.decrypt(&encrypted);
        assert!(result.is_err());
    }

    #[test]
    fn test_same_secret_same_keys() {
        let c1 = E2ECrypto::new(
            b"shared-key-32-bytes-long-xxxxx!",
            b"same-session",
            CryptoAlgorithm::ChaCha20Poly1305,
        )
        .unwrap();
        let c2 = E2ECrypto::new(
            b"shared-key-32-bytes-long-xxxxx!",
            b"same-session",
            CryptoAlgorithm::ChaCha20Poly1305,
        )
        .unwrap();
        let encrypted = c1.encrypt(b"cross-decrypt").unwrap();
        let decrypted = c2.decrypt(&encrypted).unwrap();
        assert_eq!(&decrypted, b"cross-decrypt");
    }

    #[test]
    fn test_different_sessions_different_keys() {
        let c1 = E2ECrypto::new(
            b"shared-key-32-bytes-long-xxxxx!",
            b"session-1",
            CryptoAlgorithm::ChaCha20Poly1305,
        )
        .unwrap();
        let c2 = E2ECrypto::new(
            b"shared-key-32-bytes-long-xxxxx!",
            b"session-2",
            CryptoAlgorithm::ChaCha20Poly1305,
        )
        .unwrap();
        let encrypted = c1.encrypt(b"test").unwrap();
        let result = c2.decrypt(&encrypted);
        assert!(result.is_err());
    }

    #[test]
    fn test_hmac_roundtrip() {
        let key = b"hmac-key-for-testing!!";
        let message = b"verify this message";
        let tag = hmac_verify(key, message).unwrap();
        assert!(hmac_check(key, message, &tag));
        assert!(!hmac_check(key, b"tampered message", &tag));
        assert!(!hmac_check(b"wrong-key-1234567890abcdef", message, &tag));
    }

    #[test]
    fn test_algorithm_tag_byte_roundtrip() {
        assert_eq!(
            CryptoAlgorithm::ChaCha20Poly1305.tag_byte(),
            ALGO_CHACHA20_POLY1305
        );
        assert_eq!(CryptoAlgorithm::Aes256Gcm.tag_byte(), ALGO_AES_256_GCM);
        assert_eq!(
            CryptoAlgorithm::from_tag_byte(0x01),
            Some(CryptoAlgorithm::ChaCha20Poly1305)
        );
        assert_eq!(
            CryptoAlgorithm::from_tag_byte(0x02),
            Some(CryptoAlgorithm::Aes256Gcm)
        );
        assert_eq!(CryptoAlgorithm::from_tag_byte(0xFF), None);
    }
}
