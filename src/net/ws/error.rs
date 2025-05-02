use std::error::Error;
use std::fmt::Display;

#[derive(Debug)]
pub struct VoiceWebSocketError {
    pub(crate) kind: VoiceWebSocketErrorType,
    pub(crate) source: Option<Box<dyn Error + Send + Sync>>,
}

impl VoiceWebSocketError {
    #[must_use]
    pub fn kind(&self) -> &VoiceWebSocketErrorType {
        &self.kind
    }
}

impl Display for VoiceWebSocketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            VoiceWebSocketErrorType::Deserializing { event } => {
                f.write_str("voice gateway event could not be deserialized: event=")?;
                f.write_str(&event)
            }
            VoiceWebSocketErrorType::WebSocket => {
                let source = self
                    .source
                    .as_ref()
                    .expect("websocket error should have a source");

                Display::fmt(source, f)
            }
        }
    }
}

impl Error for VoiceWebSocketError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| &**source as &(dyn Error + 'static))
    }
}

#[derive(Debug)]
pub enum VoiceWebSocketErrorType {
    /// WebSocket error.
    WebSocket,

    /// Voice gateway event could not be deserialized.
    Deserializing { event: String },
}
