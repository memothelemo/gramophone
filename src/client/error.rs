use std::error::Error;

/// Receiving the next [`VoiceClient`] event failed.
///
/// [`VoiceClient`]: super::VoiceClient
#[derive(Debug)]
pub struct ReceiveEventError {
    pub(crate) kind: ReceiveEventErrorType,
    pub(crate) source: Option<Box<dyn Error + Send + Sync>>,
}

impl ReceiveEventError {
    #[must_use]
    pub const fn kind(&self) -> &ReceiveEventErrorType {
        &self.kind
    }
}

impl std::fmt::Display for ReceiveEventError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            ReceiveEventErrorType::UnsupportedMode => f.write_str("unsupported encryption mode"),
            ReceiveEventErrorType::Reconnect => {
                f.write_str("could not reconnect to the voice gateway")
            }
            ReceiveEventErrorType::Handshaking => {
                f.write_str("could not handshake voice UDP connection")
            }
        }
    }
}

impl Error for ReceiveEventError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| &**source as &(dyn Error + 'static))
    }
}

#[derive(Debug)]
pub enum ReceiveEventErrorType {
    /// Found undefined encryption mode.
    UnsupportedMode,

    /// Could not handshake UDP.
    Handshaking,

    /// Could not reconnect to the voice gateway.
    Reconnect,
}
