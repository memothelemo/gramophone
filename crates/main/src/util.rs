use std::ops::Deref;
use std::pin::Pin;
use std::{fmt::Debug, ops::DerefMut};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
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

#[derive(Clone)]
pub struct FlumeSender<T>(flume::Sender<T>);

impl<T> Debug for FlumeSender<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlumeSender")
            .field("closed", &self.is_disconnected())
            .finish_non_exhaustive()
    }
}

impl<T> Deref for FlumeSender<T> {
    type Target = flume::Sender<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct FlumeReceiver<T>(flume::Receiver<T>);

impl<T> FlumeReceiver<T> {
    #[must_use]
    pub fn new() -> (FlumeSender<T>, FlumeReceiver<T>) {
        let (tx, rx) = flume::unbounded();
        (FlumeSender(tx), FlumeReceiver(rx))
    }
}

impl<T> Debug for FlumeReceiver<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlumeReceiver")
            .field("closed", &self.is_disconnected())
            .finish_non_exhaustive()
    }
}

impl<T> Deref for FlumeReceiver<T> {
    type Target = flume::Receiver<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for FlumeReceiver<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Clone)]
pub struct Sender<T>(mpsc::UnboundedSender<T>);

impl<T> Debug for Sender<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sender")
            .field("closed", &self.is_closed())
            .finish_non_exhaustive()
    }
}

pub struct Receiver<T>(mpsc::UnboundedReceiver<T>);

impl<T> Debug for Receiver<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Receiver")
            .field("closed", &self.is_closed())
            .finish_non_exhaustive()
    }
}

impl<T> Receiver<T> {
    #[must_use]
    pub fn new() -> (Sender<T>, Receiver<T>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Sender(tx), Receiver(rx))
    }
}

impl<T> Deref for Sender<T> {
    type Target = mpsc::UnboundedSender<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for Receiver<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> Deref for Receiver<T> {
    type Target = mpsc::UnboundedReceiver<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
