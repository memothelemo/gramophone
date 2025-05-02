use criterion::{Criterion, black_box, criterion_group, criterion_main};
use gramophone::crypto::EncryptMode;
use gramophone_types::RTP_KEY_LEN;

fn encrypt_bytes(c: &mut Criterion) {
    const SECRET_KEY: &str = "fb5e9f96f291742023f321c7f4f967a2d90f4e7dadb2d9bd66774ad3ebb3ac5d";
    const BIG_PLAINTEXT: &str = include_str!("../assets/plaintexts/lorem.txt");
    const SMALL_PLAINTEXT: &str = "Hello, World!";

    let key: [u8; RTP_KEY_LEN] = hex::decode(SECRET_KEY).unwrap().try_into().unwrap();

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
            c.bench_function(&bench_name, |b| {
                b.iter(|| black_box(aead.encrypt(&nonce, &[], black_box(plaintext)).unwrap()))
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
            let ciphertext = aead.encrypt(&nonce, &[], plaintext.as_bytes()).unwrap();
            c.bench_function(&bench_name, |b| {
                b.iter(|| black_box(aead.decrypt(&nonce, &[], black_box(&ciphertext)).unwrap()));
            });
        }
    }
}

criterion_group!(benches, encrypt_bytes);
criterion_main!(benches);

// use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};
// use gramophone::stream::AudioProcessor;
// use symphonia::core::audio::{Channels, SampleBuffer, SignalSpec};

// fn resample_frame(c: &mut Criterion) {
//     let stereo = Channels::SIDE_LEFT | Channels::SIDE_RIGHT;
//     let parameters: &[(Channels, u32, &str)] = &[
//         (stereo, 44_100, "Stereo (44.1kHz)"),
//         (stereo, 48_000, "Stereo (48kHz)"),
//         (stereo, 24_000, "Stereo (24kHz)"),
//         (stereo, 16_000, "Stereo (16kHz)"),
//         (Channels::FRONT_CENTRE, 44_100, "Mono (44.1kHz)"),
//         (Channels::FRONT_CENTRE, 48_000, "Mono (48kHz)"),
//         (Channels::FRONT_CENTRE, 24_000, "Mono (24kHz)"),
//         (Channels::FRONT_CENTRE, 16_000, "Mono (16kHz)"),
//     ];

//     for (channels, khz, name) in parameters {
//         c.bench_function(name, |b| {
//             b.iter_batched_ref(
//                 || {
//                     let processor = AudioProcessor::new(&channels, *khz);
//                     let packets = make_sine(channels.count() * 960, true);
//                     (processor, packets)
//                 },
//                 |(processor, packets)| {
//                     processor.push(&packets);
//                     while processor.next_stereo_frame().is_some() {}
//                 },
//                 BatchSize::SmallInput,
//             )
//         });
//     }
// }

// #[must_use]
// fn make_sine(float_len: usize, stereo: bool) -> Vec<f32> {
//     // set period to 100 samples == 480Hz sine.

//     let mut out = vec![0f32; float_len];
//     for i in 0..float_len {
//         let x_val = (i as f32) * 50.0 / std::f32::consts::PI;
//         out.push(x_val.sin());
//     }

//     if stereo {
//         let mut new_out = vec![0f32; float_len * 2];

//         for (mono_chunk, stereo_chunk) in out[..]
//             .chunks(float_len)
//             .zip(new_out[..].chunks_mut(2 * float_len))
//         {
//             stereo_chunk[..float_len].copy_from_slice(mono_chunk);
//             stereo_chunk[float_len..].copy_from_slice(mono_chunk);
//         }

//         new_out
//     } else {
//         out
//     }
// }

// criterion_group!(benches, resample_frame);
// criterion_main!(benches);
