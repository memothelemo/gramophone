use std::net::UdpSocket;
use std::sync::{Arc, atomic::AtomicU32};
use tokio::net::UdpSocket as AsyncUdpSocket;

pub mod error;
pub use self::error::VoiceUdpError;

/// Internal extensions for additional functionality.
pub(crate) mod ext;

use self::error::VoiceUdpErrorType;

#[derive(Debug)]
pub struct VoiceUdp {
    keep_alive: Arc<AtomicU32>,
    socket: Arc<UdpSocket>,
}

impl VoiceUdp {
    #[must_use]
    pub(crate) fn new(socket: AsyncUdpSocket) -> Result<Self, VoiceUdpError> {
        let socket = socket.into_std().map_err(|source| VoiceUdpError {
            kind: VoiceUdpErrorType::Connect,
            source: Some(Box::new(source)),
        })?;

        socket
            .set_nonblocking(false)
            .map_err(|source| VoiceUdpError {
                kind: VoiceUdpErrorType::Connect,
                source: Some(Box::new(source)),
            })?;

        Ok(Self {
            keep_alive: Arc::new(AtomicU32::new(0)),
            socket: Arc::new(socket),
        })
    }
}
