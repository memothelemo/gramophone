use std::error::Error;
use std::fmt::Display;

#[derive(Debug)]
pub struct VoiceClientError {
    pub(crate) kind: VoiceClientErrorType,
    pub(crate) source: Option<Box<dyn Error + Send + Sync>>,
}

impl VoiceClientError {
    #[must_use]
    pub fn kind(&self) -> &VoiceClientErrorType {
        &self.kind
    }
}

impl Display for VoiceClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            VoiceClientErrorType::Handshaking => {
                f.write_str("could not handshake voice UDP connection")
            }
            VoiceClientErrorType::Deserializing { event } => {
                f.write_str("voice gateway event could not be deserialized: event=")?;
                f.write_str(event)
            }
            VoiceClientErrorType::Reconnect => {
                let source = self
                    .source
                    .as_ref()
                    .expect("websocket error should have a source");

                Display::fmt(source, f)
            } // VoiceClientErrorType::UnsupportedMode => f.write_str("unsupported encryption mode"),
        }
    }
}

impl Error for VoiceClientError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| &**source as &(dyn Error + 'static))
    }
}

#[derive(Debug)]
pub enum VoiceClientErrorType {
    /// Could not handshake a voice server.
    Handshaking,

    /// Could not reconnect to the voice gateway.
    Reconnect,

    /// Voice gateway event could not be deserialized.
    Deserializing { event: String },
}
