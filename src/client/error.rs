use std::error::Error;

/// Setting speaking value failed.
#[derive(Debug)]
pub struct SetSpeakingError {
    pub(crate) kind: SetSpeakingErrorType,
    pub(crate) source: Option<Box<dyn Error + Send + Sync>>,
}

impl SetSpeakingError {
    #[must_use]
    pub const fn kind(&self) -> &SetSpeakingErrorType {
        &self.kind
    }
}

impl std::fmt::Display for SetSpeakingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            SetSpeakingErrorType::Closed => f.write_str("tried sending over a closed channel"),
            SetSpeakingErrorType::NotConnected => f.write_str("not connected to Discord"),
        }
    }
}

impl Error for SetSpeakingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| &**source as &(dyn Error + 'static))
    }
}

#[derive(Debug)]
pub enum SetSpeakingErrorType {
    /// Tried sending over a closed channel.
    Closed,

    /// Not connected to Discord.
    NotConnected,
}

/// Receiving the next [`Connection`] event failed.
///
/// [`Connection`]: super::Connection
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
            ReceiveEventErrorType::Reconnect => {
                f.write_str("could not reconnect to the voice gateway")
            }
            ReceiveEventErrorType::Handshaking => {
                f.write_str("could not handshake voice UDP connection")
            }
            ReceiveEventErrorType::SendingPacket => f.write_str("could not send voice data"),
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
    /// Could not handshake UDP.
    Handshaking,

    /// Could not reconnect to the voice gateway.
    Reconnect,

    /// Could not send voice data to Discord.
    SendingPacket,
}
