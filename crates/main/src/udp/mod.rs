pub mod error;
pub mod ext;

use std::net::{IpAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use self::error::{VoiceUdpError, VoiceUdpErrorType};

#[derive(Debug)]
pub struct VoiceUdp {
    keep_alive: Arc<AtomicU32>,
    socket: Arc<UdpSocket>,
}

impl VoiceUdp {
    pub(crate) fn new(socket: tokio::net::UdpSocket) -> Result<Self, VoiceUdpError> {
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

/// It contains details that can be retrieved with [`DiscordUdpExt::discover`].
#[derive(Debug)]
pub struct DiscoverIpResult {
    /// External IP address of the client.
    pub address: IpAddr,

    /// External UDP port binded by the client.
    pub port: u16,
}
