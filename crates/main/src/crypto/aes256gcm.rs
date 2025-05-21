// ring has fallback (code) implementation of Aes256Gcm in case if the
// CPU using does not support AES extensions.
use super::{AEAD_KEY_LEN, Aead, AeadError, AeadErrorType, ring};

use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use std::fmt::Debug;

/// Nonce size for the particular encryption.
pub const NONCE_LEN: usize = 96 / 8;

pub struct Aes256Gcm {
    key: LessSafeKey,
}

impl Debug for Aes256Gcm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Aes256Gcm").finish_non_exhaustive()
    }
}

#[allow(clippy::missing_panics_doc)]
impl Aes256Gcm {
    #[must_use]
    pub fn new(key: &[u8]) -> Option<Self> {
        (key.len() == AEAD_KEY_LEN)
            .then(|| Self::new_sized(&key.try_into().expect("key size should be AEAD_KEY_LEN")))
    }

    #[must_use]
    pub fn new_sized(key: &[u8; AEAD_KEY_LEN]) -> Self {
        let key = UnboundKey::new(&AES_256_GCM, key)
            .map(LessSafeKey::new)
            .expect("key size is in the correct AEAD key size");

        Self { key }
    }
}

impl Aead for Aes256Gcm {
    #[must_use]
    fn mode(&self) -> super::EncryptMode {
        super::EncryptMode::Aes256Gcm
    }

    fn encrypt(&self, nonce: &[u8], aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, AeadError> {
        if nonce.len() != NONCE_LEN {
            return Err(AeadError {
                kind: AeadErrorType::InvalidNonceLength {
                    expected: NONCE_LEN,
                },
            });
        }

        let mut ciphertext = plaintext.to_vec();
        self.key
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce.try_into().expect("checked with NONCE_LEN")),
                Aad::from(aad),
                &mut ciphertext,
            )
            .map_err(|_| AeadError {
                kind: AeadErrorType::Unspecified,
            })?;

        Ok(ciphertext)
    }

    fn decrypt(&self, nonce: &[u8], aad: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, AeadError> {
        if nonce.len() != NONCE_LEN {
            return Err(AeadError {
                kind: AeadErrorType::InvalidNonceLength {
                    expected: NONCE_LEN,
                },
            });
        }

        if ciphertext.len() < AES_256_GCM.tag_len() {
            return Err(AeadError {
                kind: AeadErrorType::Unspecified,
            });
        }

        let mut buffer = ciphertext.to_vec();
        let plaintext = self
            .key
            .open_in_place(
                Nonce::assume_unique_for_key(nonce.try_into().expect("checked with NONCE_LEN")),
                Aad::from(aad),
                &mut buffer,
            )
            .map_err(|_| AeadError {
                kind: AeadErrorType::Unspecified,
            })?;

        let len = plaintext.len();
        buffer.truncate(len);

        Ok(buffer)
    }
}

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::{Aead, Aes256Gcm};

    const SECRET_KEY: &str = "fb5e9f96f291742023f321c7f4f967a2d90f4e7dadb2d9bd66774ad3ebb3ac5d";
    const NONCE: &str = "a9ecf2241430300505590fdf";

    // Generated with node.js (encoded with hex)
    //
    // const secretKey = Buffer.from("fb5e9f96f291742023f321c7f4f967a2d90f4e7dadb2d9bd66774ad3ebb3ac5d", "hex")
    // const nonce = Buffer.from("a9ecf2241430300505590fdf", "hex")
    //
    // crypto.createCipheriv("aes-256-gcm", secretKey, nonce)
    // cipher.setAAD(Buffer.from("", "hex"))
    //
    // Buffer.concat([cipher.update("Hello, World!"), cipher.final(), cipher.getAuthTag()]).toString("hex"))
    const CIPHERTEXT: &str = "85fd5ab2749514314a225e754dbba94bb84c0f9dd296d3234835ae73a0";

    #[test]
    fn test_decrypt() {
        let key = hex::decode(SECRET_KEY).unwrap();
        let nonce = hex::decode(NONCE).unwrap();
        let ciphertext = hex::decode(CIPHERTEXT).unwrap();

        let aes = Aes256Gcm::new(&key).unwrap();
        let plaintext = aes.decrypt(&nonce, &[], &ciphertext).unwrap();

        assert_eq!(plaintext, b"Hello, World!");
    }

    #[test]
    fn test_encrypt() {
        let key = hex::decode(SECRET_KEY).unwrap();
        let nonce = hex::decode(NONCE).unwrap();

        let aes = Aes256Gcm::new(&key).unwrap();
        let cipher = aes
            .encrypt(&nonce, &[], b"Hello, World!")
            .map(hex::encode)
            .unwrap();

        assert_eq!(cipher, CIPHERTEXT);
    }
}
