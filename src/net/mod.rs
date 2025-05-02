pub mod udp;
pub mod ws;

pub use self::udp::VoiceUdp;
pub use self::ws::{VoiceWebSocket, VoiceWebSocketEvent};

pub(crate) mod internal {
    /// Wrapper struct around a channel with a `Debug` implementation to
    /// get rid of `Debug` clutter made from [`tokio`].
    pub(crate) struct MpscWrapper<T>(pub T);

    impl<T> std::fmt::Debug for MpscWrapper<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("MpscChannel").finish()
        }
    }

    impl<T> std::ops::Deref for MpscWrapper<T> {
        type Target = T;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl<T> std::ops::DerefMut for MpscWrapper<T> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.0
        }
    }
}
