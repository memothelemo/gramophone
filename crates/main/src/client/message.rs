use twilight_model::gateway::CloseFrame;

/// Message sent from the connection that can be received with [`VoiceClient`].
///
/// [`VoiceClient`]: super::VoiceClient
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Connected(bool),
    Close(Option<CloseFrame<'static>>),
    Text(String),
}
