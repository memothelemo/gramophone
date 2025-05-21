use std::error::Error;
use std::fmt::Display;

#[derive(Debug)]
pub struct VoiceUdpError {
    pub(crate) kind: VoiceUdpErrorType,
    pub(crate) source: Option<Box<dyn Error + Send + Sync>>,
}

impl VoiceUdpError {
    #[must_use]
    pub fn kind(&self) -> &VoiceUdpErrorType {
        &self.kind
    }
}

impl Display for VoiceUdpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            VoiceUdpErrorType::Connect => f.write_str("could not connect to the Voice UDP"),
            VoiceUdpErrorType::Sending => f.write_str("could not send UDP packet"),
            VoiceUdpErrorType::DiscoveringIp => f.write_str("could not discover local IP address"),
        }
    }
}

impl Error for VoiceUdpError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| &**source as &(dyn Error + 'static))
    }
}

#[derive(Debug)]
pub enum VoiceUdpErrorType {
    /// Could not send audio packet to Discord.
    Sending,

    /// Could not connect to the specific address or port voice UDP socket.
    Connect,

    /// Could not perform IP discovery.
    DiscoveringIp,
}
