pub mod error;
pub mod ext;

use std::net::{IpAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use self::error::{VoiceUdpError, VoiceUdpErrorType};

/// The `VoiceUdp` struct provides primitive methods to manage a UDP socket connection,
/// including connecting to a remote server, discovering external IP and port,
/// sending keepalive packets and general data transmission.
///
/// This type is synchronous in design because [Symphonia] and some audio decoding
/// libraries do not support asynchronous operations. Secondly, audio packets must
/// be sent to Discord in a timed manner.
///
/// # Examples
/// ```rust
/// # use gramophone::net::VoiceUdp;
/// udp.send(b"Hello, World!")?;
///
/// // You need to send keepalive packets in 5 second intervals.
/// udp.send_keepalive()?;
/// ```
///
/// [Symphonia]: symphonia
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

impl VoiceUdp {
    /// Sends a raw data packet to the server.
    ///
    /// # Examples
    /// ```rust,no_run
    /// # let udp = VoiceUdp::new();
    /// // Assuming you have a properly formatted RTP packet
    /// let rtp_packet = prepare_rtp_packet();
    /// udp.send(&rtp_packet)?;
    /// ```
    ///
    /// # Note
    /// This function assumes the contents of the packet is a valid RTP packet with
    /// Opus format described from Discord's Voice API documentation. Invalid packets
    /// may be rejected by Discord's servers or cause unexpected behavior.
    pub fn send(&self, buffer: &[u8]) -> Result<(), VoiceUdpError> {
        self.socket
            .send(buffer)
            .map_err(|source| VoiceUdpError {
                kind: VoiceUdpErrorType::Sending,
                source: Some(Box::new(source)),
            })
            .map(|_| ())
    }

    /// Sends a keepalive packet to the server.
    pub fn send_keepalive(&self) -> Result<(), VoiceUdpError> {
        let counter = self.keep_alive.load(Ordering::SeqCst);
        let new_counter = counter.wrapping_add(1);

        self.keep_alive.store(new_counter, Ordering::SeqCst);
        self.send(&counter.to_be_bytes())
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
