pub mod udp;
pub mod websocket;

pub use self::udp::VoiceUdp;
pub use self::websocket::VoiceWebSocket;

pub(crate) mod internal {
    use std::pin::Pin;
    use tokio::net::TcpStream;
    use tokio_websockets::{MaybeTlsStream, WebSocketStream};

    /// [`tokio_websockets`] library Websocket connection.
    pub type WsConnection = WebSocketStream<MaybeTlsStream<TcpStream>>;

    /// Wrapper struct around an `async fn` with a `Debug` implementation.
    pub struct ConnectionFuture<T, E>(pub Pin<Box<dyn Future<Output = Result<T, E>> + Send>>);

    impl<T, E> ConnectionFuture<T, E> {
        #[must_use]
        pub fn new<F: Future<Output = Result<T, E>> + Send + 'static>(future: F) -> Self {
            Self(Box::pin(future))
        }
    }

    impl<T, E> std::fmt::Debug for ConnectionFuture<T, E> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_tuple("ConnectionFuture")
                .field(&"<async fn>")
                .finish()
        }
    }

    /// Wrapper struct around a channel with a `Debug` implementation to
    /// get rid of `Debug` clutter made from [`tokio`].
    pub(crate) struct Wrapper<T>(pub T);

    impl<T> std::fmt::Debug for Wrapper<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_tuple("Channel").finish_non_exhaustive()
        }
    }

    impl<T> std::ops::Deref for Wrapper<T> {
        type Target = T;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl<T> std::ops::DerefMut for Wrapper<T> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.0
        }
    }
}
