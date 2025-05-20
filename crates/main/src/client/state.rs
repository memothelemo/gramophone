use gramophone_types::CloseCode;

/// The current state of [`VoiceClient`].
///
/// [`VoiceClient`]: super::VoiceClient
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceClientState {
    /// The client is disconnected to Discord voice gateway but it can
    /// reconnect in the future when needed.
    Disconnected { attempts: u32 },

    /// The client is fatally disconnected to Discord voice gateway and it
    /// should not reconnected at any circumstances in the future.
    Closed,

    /// The client is connected to the voice gateway but it is in the
    /// process of resuming its session.
    Resuming,

    /// The client is connected to the voice gateway but it is on the
    /// process of identifying.
    Identifying,

    /// The client is connected to the voice gateway but it is not ready to
    /// transmitted any voice data yet.
    UdpHandshaking,

    /// The client is now ready to transmit voice data.
    Active,
}

impl VoiceClientState {
    #[must_use]
    pub(crate) fn from_close_code(code: Option<u16>) -> Self {
        match code.map(CloseCode::try_from) {
            Some(Ok(code)) if !code.can_reconnect() => Self::Closed,
            _ => Self::Disconnected { attempts: 0 },
        }
    }

    #[must_use]
    pub(crate) const fn is_active(self) -> bool {
        matches!(self, Self::Active { .. })
    }

    #[must_use]
    pub(crate) const fn can_send_heartbeats(self) -> bool {
        !matches!(
            self,
            Self::Identifying | Self::Disconnected { .. } | Self::Closed
        )
    }

    #[must_use]
    pub(crate) const fn is_disconnected(self) -> bool {
        matches!(self, Self::Disconnected { .. })
    }
}
