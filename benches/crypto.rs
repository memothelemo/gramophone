use criterion::{Criterion, black_box, criterion_group, criterion_main};
use gramophone::crypto::EncryptMode;
use gramophone_types::RTP_KEY_LEN;

fn encrypt_bytes(c: &mut Criterion) {
    const SECRET_KEY: &str = "fb5e9f96f291742023f321c7f4f967a2d90f4e7dadb2d9bd66774ad3ebb3ac5d";
    const BIG_PLAINTEXT: &str = include_str!("../assets/plaintexts/lorem.txt");
    const SMALL_PLAINTEXT: &str = "Hello, World!";

    let key: [u8; RTP_KEY_LEN] = hex::decode(SECRET_KEY)
        .expect("valid hex encoded bytes")
        .try_into()
        .expect("valid secret key");

    let modes = &[EncryptMode::Aes256Gcm, EncryptMode::XChaCha20Poly1305];
    let texts = &[BIG_PLAINTEXT, SMALL_PLAINTEXT];

    for mode in modes {
        let mut nonce = vec![0u8; mode.nonce_size()];
        fastrand::fill(&mut nonce);

        let aead = mode.encryptor(&key);
        for plaintext in texts {
            let bench_name = format!(
                "encrypt {} (p: {} bytes)",
                mode.to_request_str(),
                plaintext.len()
            );

            let plaintext = plaintext.as_bytes();
            #[allow(clippy::unwrap_used)]
            c.bench_function(&bench_name, |b| {
                b.iter(|| black_box(aead.encrypt(&nonce, &[], black_box(plaintext)).unwrap()));
            });
        }
    }

    for mode in modes {
        let mut nonce = vec![0u8; mode.nonce_size()];
        fastrand::fill(&mut nonce);

        let aead = mode.encryptor(&key);
        for plaintext in texts {
            let bench_name = format!(
                "decrypt {} (p: {} bytes)",
                mode.to_request_str(),
                plaintext.len()
            );

            let ciphertext = aead
                .encrypt(&nonce, &[], plaintext.as_bytes())
                .expect("should encrypt plaintext");

            #[allow(clippy::unwrap_used)]
            c.bench_function(&bench_name, |b| {
                b.iter(|| black_box(aead.decrypt(&nonce, &[], black_box(&ciphertext)).unwrap()));
            });
        }
    }
}

criterion_group!(benches, encrypt_bytes);
criterion_main!(benches);
