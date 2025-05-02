use std::fmt::Debug;
use symphonia::core::audio::Channels;
use tracing::warn;

pub struct AudioProcessor {
    channels: usize,
    buffer: Vec<f32>,
    sample_rate: u32,
}

// We need to hit around 20ms, which is the Opus standard duration time
// otherwise, we'll let them buffer in a special place.
const OPUS_TICK_DURATION: f64 = 0.02;

impl AudioProcessor {
    pub const TARGET_SAMPLES: usize = 960;

    #[must_use]
    pub fn new(channels: &Channels, sample_rate: u32) -> Self {
        let channels = channels.count();
        let sample_rate = sample_rate;

        Self {
            channels,
            buffer: Vec::new(),
            sample_rate,
        }
    }

    #[must_use]
    fn samples_per_opus_tick(&self) -> usize {
        (self.sample_rate as f64 * OPUS_TICK_DURATION).ceil() as usize * self.channels
    }
}

impl AudioProcessor {
    pub fn push(&mut self, samples: &[f32]) {
        self.buffer.extend_from_slice(samples);
    }

    #[must_use]
    pub fn has_remaining_samples(&self) -> bool {
        !self.buffer.is_empty()
    }

    #[must_use]
    pub fn next_stereo_frame(&mut self) -> Option<Vec<Vec<f32>>> {
        let samples_per_opus_tick = self.samples_per_opus_tick();
        let per_tick_frame_interleaved = if self.buffer.len() >= samples_per_opus_tick {
            self.buffer
                .drain(..samples_per_opus_tick)
                .collect::<Vec<_>>()
        } else if !self.buffer.is_empty() {
            let mut buffer = self.buffer.drain(..).collect::<Vec<_>>();
            buffer.resize(samples_per_opus_tick, 0.0);
            buffer
        } else {
            return None;
        };

        // Then, we can deinterleave it.
        let mut stereo_interleaved = vec![Vec::<f32>::with_capacity(samples_per_opus_tick / 2); 2];
        match self.channels {
            2 => {
                for (i, sample) in per_tick_frame_interleaved.into_iter().enumerate() {
                    stereo_interleaved[i % 2].push(sample);
                }
            }
            1 => {
                for sample in per_tick_frame_interleaved {
                    stereo_interleaved[0].push(sample);
                    stereo_interleaved[1].push(sample);
                }
            }
            _ => warn!(
                "resampling with {} channels is not supported!",
                self.channels
            ),
        }

        Some(stereo_interleaved)
    }

    // #[must_use]
    // pub fn next_frame(&mut self, buffer: &SampleBuffer<f32>) -> Option<Cow<'_, [f32]>> {
    //     let samples_per_opus_tick = self.samples_per_opus_tick();
    //     let (tick_samples, overflow) = if self.buffer.is_empty() {
    //         (
    //             Cow::Borrowed(&buffer.samples()[..samples_per_opus_tick]),
    //             &buffer.samples()[samples_per_opus_tick..],
    //         )
    //     } else {
    //         // this is getting a bit complicated here. derive from the overflow buffer
    //         let overflown_max_idx = self.buffer.len().min(samples_per_opus_tick);

    //         let mut combined_samples = Vec::with_capacity(samples_per_opus_tick);
    //         let filler_max_idx = samples_per_opus_tick - overflown_max_idx;
    //         combined_samples.extend_from_slice(&self.buffer[..overflown_max_idx]);
    //         combined_samples.extend_from_slice(&buffer.samples()[..filler_max_idx]);
    //         self.buffer.drain(..overflown_max_idx);

    //         (
    //             Cow::Owned(combined_samples),
    //             &buffer.samples()[filler_max_idx..],
    //         )
    //     };

    //     if !overflow.is_empty() {
    //         overflow_buffer.extend_from_slice(overflow);
    //     }
    // }
}

impl Debug for AudioProcessor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpusEncoder")
            .field("channels", &self.channels)
            .field("buffer", &self.buffer.len())
            .field("sample_rate", &self.sample_rate)
            .finish_non_exhaustive()
    }
}

// use audiopus::coder::Encoder;
// use symphonia::core::{audio::SampleBuffer, codecs::CodecParameters};

// pub struct Resampler {
//     channels: usize,
//     buffer: Vec<f32>,
//     inner: Encoder,
//     sample_rate: u32,
// }

// #[cfg(feature = "primitive")]
// impl Resampler {
//     #[must_use]
//     pub fn new_from_codec(codec: &CodecParameters) -> Option<Self> {
//         Self::new_from_codec_inner(codec)
//     }
// }

// impl Resampler {
//     #[must_use]
//     pub(crate) fn new_from_codec_inner(codec: &CodecParameters) -> Option<Self> {
//         let channels = codec.channels?.count();
//         let sample_rate = codec.sample_rate?;

//         let inner = Encoder::new(
//             audiopus::SampleRate::Hz48000,
//             audiopus::Channels::Stereo,
//             audiopus::Application::Audio,
//         )
//         .expect("should create Opus encoder");

//         Some(Self {
//             channels,
//             buffer: Vec::new(),
//             inner,
//             sample_rate,
//         })
//     }

//     pub(crate) fn push(&mut self, buffer: &SampleBuffer<f32>) {
//         self.buffer.extend_from_slice(buffer.samples());
//     }

//     #[must_use]
//     pub fn next_frame(&self) -> Option<Vec<u8>> {
//         let samples_per_twenty_ms =
//             (sample_rate as f64 * OPUS_TARGET_PACKET_DUR).ceil() as usize * channels;

//         let (twenty_ms_samples, overflow) = if overflow_buffer.is_empty() {
//             (
//                 Cow::Borrowed(&buf.samples()[..samples_per_twenty_ms]),
//                 &buf.samples()[samples_per_twenty_ms..],
//             )
//         } else {
//             // this is getting a bit complicated here. derive from the overflow buffer
//             let overflown_max_idx = overflow_buffer.len().min(samples_per_twenty_ms);

//             let mut combined_samples = Vec::with_capacity(samples_per_twenty_ms);
//             let filler_max_idx = samples_per_twenty_ms - overflown_max_idx;
//             combined_samples.extend_from_slice(&overflow_buffer[..overflown_max_idx]);
//             combined_samples.extend_from_slice(&buf.samples()[..filler_max_idx]);
//             overflow_buffer.drain(..overflown_max_idx);

//             (
//                 Cow::Owned(combined_samples),
//                 &buf.samples()[filler_max_idx..],
//             )
//         };

//         if !overflow.is_empty() {
//             overflow_buffer.extend_from_slice(overflow);
//         }
//     }
// }
