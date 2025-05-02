use anyhow::{Context, Result};
use console_subscriber::ConsoleLayer;
use dotenvy::dotenv;
use futures::StreamExt as _;
use gramophone::client::{ConnectionInfo, VoiceClient, VoiceClientEvent, options::Token};
use pin_project_lite::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::task::Poll;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Layer};
use twilight_gateway::{
    CloseFrame, Event as GatewayEvent, EventTypeFlags, Intents, Shard, ShardId, StreamExt,
};
use twilight_model::gateway::payload::outgoing::UpdateVoiceState;
use twilight_model::id::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};

mod audio {
    use anyhow::{Context, Result, anyhow};
    use gramophone::client::channel::VoiceClientSender;
    use rubato::{FftFixedOut, Resampler};
    use std::{borrow::Cow, fs::File};
    use symphonia::{
        core::{
            audio::SampleBuffer, codecs::DecoderOptions, formats::FormatOptions,
            io::MediaSourceStream, meta::MetadataOptions,
        },
        default::{get_codecs, get_probe},
    };

    // We need to hit around 20ms, which is the Opus standard duration time
    // otherwise, we'll let them buffer in a special place.
    const TARGET_SAMPLES: usize = 960;
    const OPUS_TARGET_PACKET_DUR: f64 = 0.02;

    pub fn do_stuff(sender: VoiceClientSender) -> Result<()> {
        const AUDIO_PATH: &str = concat!(env!("CARGO_WORKSPACE_DIR"), "/assets/audio/Clap.mp3");

        let mss = MediaSourceStream::new(
            Box::new(
                File::open(AUDIO_PATH)
                    .context(format!("Failed to open audio file at {}", AUDIO_PATH))?,
            ),
            Default::default(),
        );
        let mut probed = get_probe().format(
            &Default::default(),
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )?;

        // TODO: Dealing with multiple audio tracks
        let track = probed
            .format
            .tracks()
            .iter()
            .next()
            .ok_or_else(|| anyhow!("No audio tracks available"))?;

        let mut decoder = get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
        let channels = track
            .codec_params
            .channels
            .ok_or_else(|| anyhow!("No channel available"))?;

        let sample_rate = track
            .codec_params
            .sample_rate
            .ok_or_else(|| anyhow!("Unknown sample rate"))?;

        // 20ms equivalent for that particular sample rate
        let channels = channels.count();
        let default_track_id = probed.format.default_track().unwrap().id;
        let samples_per_twenty_ms =
            (sample_rate as f64 * OPUS_TARGET_PACKET_DUR).ceil() as usize * channels;

        let mut opus_buf = vec![0; TARGET_SAMPLES * 6];
        let mut resampler = FftFixedOut::<f32>::new(
            sample_rate as usize,
            48_000,
            TARGET_SAMPLES,
            samples_per_twenty_ms / 2,
            channels,
        )
        .context("Could not make resampler with this audio")?;

        let encoder = audiopus::coder::Encoder::new(
            audiopus::SampleRate::Hz48000,
            audiopus::Channels::Stereo,
            audiopus::Application::Audio,
        )?;

        let mut sample_buffer = None;

        // we'll going to combine them together which is pretty sophisticated
        let mut overflow_buffer = Vec::<f32>::new();
        sender.speaking(true)?;

        while let Ok(packet) = probed.format.next_packet() {
            let decoded = decoder.decode(&packet)?;
            if packet.track_id() != default_track_id {
                continue;
            }

            if sample_buffer.is_none() {
                let spec = *decoded.spec();
                let duration = decoded.capacity() as u64;
                sample_buffer = Some(SampleBuffer::<f32>::new(duration, spec));
            }

            if let Some(buf) = &mut sample_buffer {
                buf.copy_interleaved_ref(decoded);

                let (twenty_ms_samples, overflow) = if overflow_buffer.is_empty() {
                    (
                        Cow::Borrowed(&buf.samples()[..samples_per_twenty_ms]),
                        &buf.samples()[samples_per_twenty_ms..],
                    )
                } else {
                    // this is getting a bit complicated here. derive from the overflow buffer
                    let overflown_max_idx = overflow_buffer.len().min(samples_per_twenty_ms);

                    let mut combined_samples = Vec::with_capacity(samples_per_twenty_ms);
                    let filler_max_idx = samples_per_twenty_ms - overflown_max_idx;
                    combined_samples.extend_from_slice(&overflow_buffer[..overflown_max_idx]);
                    combined_samples.extend_from_slice(&buf.samples()[..filler_max_idx]);
                    overflow_buffer.drain(..overflown_max_idx);

                    (
                        Cow::Owned(combined_samples),
                        &buf.samples()[filler_max_idx..],
                    )
                };

                if !overflow.is_empty() {
                    overflow_buffer.extend_from_slice(overflow);
                }

                let mut deinterleaved = vec![vec![]; channels];
                for (i, sample) in twenty_ms_samples.iter().enumerate() {
                    deinterleaved[i % channels].push(*sample);
                }

                let resampled = resampler
                    .process(&deinterleaved, None)
                    .context("could not resample samples to 48kHz")?;

                let mut preopus = Vec::with_capacity(resampled[0].len() * channels);
                for i in 0..resampled[0].len() {
                    for ch in 0..channels {
                        preopus.push(resampled[ch][i]);
                    }
                }

                // Ensure the length of `preopus` matches the Opus frame size
                if preopus.len() != TARGET_SAMPLES * channels {
                    tracing::warn!(
                        "Resampled data length mismatch: expected {}, got {}",
                        TARGET_SAMPLES * channels,
                        preopus.len()
                    );
                    continue;
                }

                let len = encoder
                    .encode_float(&preopus, &mut opus_buf)
                    .context("could not encode to Opus")?;

                let frame = &opus_buf[..len];
                sender.send(frame.to_vec())?;
            }
        }

        _ = sender.wait_until_flushed_blocking();
        sender.speaking(false)?;

        Ok(())
    }
}

pin_project! {
    /// This type runs the future if it exists and returns the value
    /// as expected but it will yield indefinitely if this struct
    /// has no value inside.
    #[derive(Debug)]
    pub struct Optional<F> {
        #[pin]
        future: Option<F>,
    }
}

/// This trait allows to conveniently utilize [`Optional`] without
/// having to initialize it using the type itself by calling
/// `optional(...)` method.
pub trait OptionalExt {
    type Future: Future;

    fn optional(self) -> Optional<Self::Future>;
}

impl<F: Future> OptionalExt for Option<F> {
    type Future = F;

    fn optional(self) -> Optional<Self::Future> {
        Optional { future: self }
    }
}

impl<F: Future> Future for Optional<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        match self.project().future.as_pin_mut() {
            Some(f) => f.poll(cx),
            None => Poll::Pending,
        }
    }
}

pub struct NopFuture;

impl Future for NopFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenv().ok();
    let token = dotenvy::var("TOKEN").context("Missing `TOKEN` environment variable")?;

    let console = ConsoleLayer::builder().with_default_env().spawn();
    let fmt = tracing_subscriber::fmt::layer().with_filter(EnvFilter::from_default_env());
    tracing::subscriber::set_global_default(
        tracing_subscriber::registry().with(console).with(fmt),
    )?;

    let mut shard = Shard::new(
        ShardId::ONE,
        token,
        Intents::GUILDS | Intents::GUILD_VOICE_STATES,
    );

    let guild_id = dotenvy::var("GUILD_ID").unwrap();
    let guild_id = Id::<GuildMarker>::from_str(&guild_id).unwrap();

    let channel_id = dotenvy::var("CHANNEL_ID").unwrap();
    let channel_id = Id::<ChannelMarker>::from_str(&channel_id).unwrap();

    let user_id = dotenvy::var("USER_ID").unwrap();
    let user_id = Id::<UserMarker>::from_str(&user_id).unwrap();

    let mut endpoint: Option<String> = None;
    let mut session_id: Option<String> = None;
    let mut token: Option<String> = None;
    let mut voice: Option<VoiceClient> = None;

    let mut audio_handle = None;
    let mut should_connect_voice = true;
    let (close_tx, mut close_rx) = mpsc::channel(1);

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.unwrap();
        close_tx.send(()).await.unwrap();
    });

    loop {
        tokio::select! {
            biased;
            _ = NopFuture => {}
            _ = close_rx.recv() => {
                info!("ctrl+c detected, closing all connections...");
                if let Some(voice) = voice.as_mut() {
                    voice.close(CloseFrame::NORMAL);
                }
                shard.close(CloseFrame::NORMAL);
                break;
            }
            event = voice.as_mut().map(|v| v.next()).optional() => {
                let event = match event {
                    Some(Ok(event)) => event,
                    Some(Err(error)) => {
                        warn!("voice client got an error: {error}");
                        continue;
                    },
                    None => {
                        warn!("voice client closed");
                        shard.close(CloseFrame::NORMAL);
                        break;
                    }
                };
                info!(?event, "shard: received voice event");

                match event {
                    VoiceClientEvent::Connected => {
                        info!("starting audio session");

                        let sender = voice.as_ref().unwrap().sender();
                        audio_handle = Some(tokio::task::spawn_blocking(|| if let Err(error) = audio::do_stuff(sender) {
                            warn!("audio::do_stuff failed: {error:?}");
                        }));
                    },
                    _ => {}
                }
            }
            entry = shard.next_event(EventTypeFlags::all()) => {
                let event = match entry {
                    Some(Ok(event)) => event,
                    Some(Err(error)) => {
                        warn!("Shard #0 got an error: {error}");
                        continue;
                    },
                    None => break,
                };
                debug!("shard: received event {:?}", event.kind());

                match &event {
                    GatewayEvent::Ready(..) => {
                        info!("bot is ready. connecting to the voice channel...");
                        shard.command(&UpdateVoiceState::new(guild_id, channel_id, true, false));
                    },
                    GatewayEvent::VoiceStateUpdate(data) => {
                        if data.guild_id != Some(guild_id)
                            || data.member.as_ref().map(|v| v.user.id) != Some(user_id)
                            || data.channel_id.is_none()
                        {
                            continue;
                        }

                        info!("received voice state update");
                        session_id = Some(data.session_id.to_string());
                    }
                    GatewayEvent::VoiceServerUpdate(data) if data.guild_id == guild_id => {
                        info!("received voice server update");

                        endpoint = data.endpoint.clone();
                        token = Some(data.token.to_string());
                    }
                    _ => {}
                }

                if should_connect_voice && session_id.is_some() && endpoint.is_some() && token.is_some() {
                    should_connect_voice = false;
                    voice = Some(VoiceClient::new(ConnectionInfo {
                        endpoint: endpoint.clone().unwrap(),
                        guild_id,
                        session_id: session_id.clone().unwrap(),
                        token: Token::new(token.clone().unwrap().to_string().into_boxed_str()),
                        user_id,
                    }));
                }
            }
        }
    }

    // so twilight can process closing the connection.
    _ = shard.next().await;

    info!("closed");
    if audio_handle.is_some() {
        std::process::exit(0);
    }

    Ok(())
}
