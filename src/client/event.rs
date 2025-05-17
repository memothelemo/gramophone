use gramophone_types::payload::Event;

use crate::crypto::Aead;
use crate::net::VoiceUdp;

/// The resulting value of `.next()` function in [`VoiceClient`].
#[derive(Debug)]
pub enum VoiceClientEvent {
    /// Received a gateway event from Discord.
    ///
    /// [`Event::SessionDescription`] will not always be included when called
    /// from [`VoiceClient::next`] due to security reasons but [AEAD encryptor]
    /// is already provided in the [`Connected`] variant.
    ///
    /// [`VoiceClient::next`]: super::VoiceClient::next
    /// [`Connected`]: VoiceClientEvent::Connected
    /// [AEAD encryptor]: Aead
    Event(Event),

    /// The client got disconnected. It is restarting its connection in a moment.
    Reconnecting,

    /// Successfully to the voice server. You may now use this to transmit
    /// audio packets as you may please.
    Connected(VoiceServerInfo),
}

/// Information of a voice server, its preferred encryptor and
/// the SSRC dedicated for the client assigned by Discord.
#[derive(Debug)]
pub struct VoiceServerInfo {
    pub aead: Box<dyn Aead>,
    pub ssrc: u32,
    pub udp: VoiceUdp,
}
