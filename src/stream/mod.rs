pub mod resampler;
pub use self::resampler::AudioProcessor;

// use audiopus::coder::Encoder as OpusEncoderInner;

// use crate::client::channel::VoiceClientSender;

// pub struct OpusEncoder {
//     encoder: OpusEncoderInner,
// }

// impl OpusEncoder {
//     #[must_use]
//     pub fn new(sender: VoiceClientSender) -> Self {
//         let encoder = OpusEncoderInner::new(
//             audiopus::SampleRate::Hz48000,
//             audiopus::Channels::Stereo,
//             audiopus::Application::Audio,
//         )
//         .expect("should create Opus encoder");

//         Self { encoder }
//     }
// }

// impl OpusEncoder {
//     pub fn play(&self) {}
// }
