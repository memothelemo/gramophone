use std::fmt::Debug;

// We're using ring if both aws-lc-rs and ring are configured.
#[cfg(any(
    all(feature = "ring", not(feature = "aws_lc_rs")),
    all(feature = "aws_lc_rs", feature = "ring")
))]
extern crate ring;

// Since aws-lc-rs claims to be ring-compatible so we don't need to isolate
// each of their implementation in designated modules.
#[cfg(all(feature = "aws_lc_rs", not(feature = "ring")))]
extern crate aws_lc_rs as ring;

pub mod aes256gcm;
pub mod mode;
pub mod xchacha20poly1305;

pub use self::aes256gcm::Aes256Gcm;
pub use self::mode::EncryptMode;
pub use self::xchacha20poly1305::XChaCha20Poly1035;

/// Secret key size to encrypt/decrypt voice packets
pub const AEAD_KEY_LEN: usize = 32;

/// Algoritm-agnostic trait for encrypting and decrypting data,
/// independent of any specific crypto provider or algorithm.
pub trait Aead: Debug + Sync + Send {
    fn mode(&self) -> EncryptMode;
    fn encrypt(&self, nonce: &[u8], aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, AeadError>;
    fn decrypt(&self, nonce: &[u8], aad: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, AeadError>;
}

pub struct AeadError {
    pub(crate) kind: AeadErrorType,
}

impl AeadError {
    #[must_use]
    pub fn kind(&self) -> &AeadErrorType {
        &self.kind
    }
}

impl Debug for AeadError {
    #[cfg(not(test))]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AeadError").finish_non_exhaustive()
    }

    #[cfg(test)]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AeadError")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Display for AeadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // This is on purpose so the attacker cannot decipher the cause of the error.
        f.write_str("aead error")
    }
}

impl std::error::Error for AeadError {}

#[cfg_attr(test, derive(Debug))]
#[non_exhaustive]
pub enum AeadErrorType {
    /// General AEAD error. Nothing too specific.
    Unspecified,

    /// Invalid nonce length.
    InvalidNonceLength { expected: usize },
}
