use std::fmt::Debug;

pub mod mode;
pub use self::mode::EncryptMode;

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
