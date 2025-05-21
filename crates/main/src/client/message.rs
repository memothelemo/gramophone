use twilight_model::gateway::CloseFrame;

use super::AudioTransport;

/// Message sent from the connection that can be received from [`VoiceClient`].
///
/// [`VoiceClient`]: super::VoiceClient
#[derive(Debug)]
pub enum Message {
    Connected(AudioTransport),
    Close(Option<CloseFrame<'static>>),
    Text(String),
}
