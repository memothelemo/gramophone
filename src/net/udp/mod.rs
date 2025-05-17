pub mod error;

use self::error::{VoiceUdpError, VoiceUdpErrorType};
use discortp::discord::{IpDiscoveryPacket, IpDiscoveryType, MutableIpDiscoveryPacket};
use std::net::{IpAddr, UdpSocket};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// The `VoiceUdp` struct provides primitive methods to manage a UDP socket connection,
/// including connecting to a remote server, discovering external IP and port,
/// sending keepalive packets and general data transmission.
///
/// Unlike [`VoiceWebSocket`] type where it supports asynchronous functions, this type
/// is synchronous in design because [Symphonia] or some audio decoding libraries do not
/// support asynchronous operations. Secondly, audio packets must be sent to Discord
/// in a timed manner.
///
/// # Examples
/// ```rust
/// # use gramophone::net::VoiceUdp;
/// let mut udp = VoiceUdp::new();
///
/// // By default, VoiceUdp is not connected to any UDP socket so you have
/// // to connect to a specific IP address and port first.
/// udp.connect("127.0.0.1".parse().unwrap(), 12345).unwrap();
/// if udp.is_connected() {
///     println!("Connected!");
/// }
///
/// udp.send(b"Hello, World!")?;
///
/// // You need to send keepalive packets in 5 second intervals.
/// udp.send_keepalive()?;
///
/// // You may want to use VoiceUdp but in async manner, here's our answer:
/// //
/// // We support only tokio unfortunately.
/// let udp = udp.into_tokio()?;
/// ```
///
/// [Symphonia]: symphonia
/// [`VoiceWebSocket`]: crate::net::VoiceWebSocket
#[derive(Debug, Clone)]
pub struct VoiceUdp {
    keep_alive_counter: Arc<AtomicU32>,
    socket: Option<Arc<UdpSocket>>,
}

impl VoiceUdp {
    /// Creates a new instance of `VoiceUdp` that is uninitialized by default.
    #[must_use]
    pub fn new() -> Self {
        Self {
            keep_alive_counter: Arc::new(AtomicU32::new(0)),
            socket: None,
        }
    }

    /// Checks if whether this socket has established a connection
    /// with the remote server.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.socket.is_some()
    }

    /// Connects to a remote server using the specified IP address and port.
    pub fn connect(&mut self, ip: IpAddr, port: u16) -> Result<(), VoiceUdpError> {
        let socket = UdpSocket::bind("0.0.0.0:0").map_err(|source| VoiceUdpError {
            kind: VoiceUdpErrorType::Connect,
            source: Some(Box::new(source)),
        })?;

        socket.connect((ip, port)).map_err(|source| VoiceUdpError {
            kind: VoiceUdpErrorType::Connect,
            source: Some(Box::new(source)),
        })?;

        self.socket = Some(Arc::new(socket));
        Ok(())
    }

    /// Connects to a remote server using the specified IP address and port.
    ///
    /// Unlike [`VoideUdp::connect`], this function can be `.await`'ed.
    pub async fn connect_async(&mut self, ip: IpAddr, port: u16) -> Result<(), VoiceUdpError> {
        use tokio::net::UdpSocket as TokioUdpSocket;

        let socket = TokioUdpSocket::bind("0.0.0.0:0")
            .await
            .map_err(|source| VoiceUdpError {
                kind: VoiceUdpErrorType::Connect,
                source: Some(Box::new(source)),
            })?;

        socket
            .connect((ip, port))
            .await
            .map_err(|source| VoiceUdpError {
                kind: VoiceUdpErrorType::Connect,
                source: Some(Box::new(source)),
            })?;

        let socket = socket.into_std().map_err(|source| VoiceUdpError {
            kind: VoiceUdpErrorType::Connect,
            source: Some(Box::new(source)),
        })?;
        socket.set_nonblocking(false).unwrap();

        self.socket = Some(Arc::new(socket));
        Ok(())
    }

    /// As prescribed from the Discord's Voice API documentation, the client has to
    /// discover its own external IP address and port of the client before transmitting
    /// voice packet data to Discord.
    ///
    /// This method implements the [Discover IP] procedure to make it easier to find
    /// the client's external connection info as long as they have a valid `ssrc`
    /// passed to this method.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use gramophone::net::VoiceUdp;
    /// # use gramophone_types::payload::Event;
    /// # let event: Event = Event::GatewayClosed(None);
    /// match event {
    ///     Event::GatewayReady(info) => {
    ///         let udp = VoiceUdp::new();
    ///         udp.connect_async(info.ip, info.port).await?;
    ///
    ///         let result = udp.discover(info.ssrc).await?;
    ///         println!("Discovered IP: {}, Port: {}", result.address, result.port);
    ///         
    ///     },
    ///     ...
    /// }
    /// ```
    ///
    /// [Discover IP]: https://discord.com/developers/docs/topics/voice-connections#ip-discovery
    #[allow(clippy::missing_panics_doc)]
    pub async fn discover(&self, ssrc: u32) -> Result<DiscoverIpResult, VoiceUdpError> {
        let socket = self.into_tokio()?;

        let mut bytes = [0u8; IpDiscoveryPacket::const_packet_size()];
        let mut view = MutableIpDiscoveryPacket::new(&mut bytes[..]).expect("unexpected behavior");
        view.set_pkt_type(IpDiscoveryType::Request);
        view.set_length(70);
        view.set_ssrc(ssrc);

        socket.send(&bytes).await.map_err(|source| VoiceUdpError {
            kind: VoiceUdpErrorType::DiscoveringIp,
            source: Some(Box::new(source)),
        })?;

        let (len, _addr) = socket
            .recv_from(&mut bytes)
            .await
            .map_err(|source| VoiceUdpError {
                kind: VoiceUdpErrorType::DiscoveringIp,
                source: Some(Box::new(source)),
            })?;

        let view = IpDiscoveryPacket::new(&bytes[..len]).ok_or_else(|| VoiceUdpError {
            kind: VoiceUdpErrorType::DiscoveringIp,
            source: Some("invalid ip discovery response".into()),
        })?;

        if view.get_pkt_type() != IpDiscoveryType::Response {
            return Err(VoiceUdpError {
                kind: VoiceUdpErrorType::DiscoveringIp,
                source: Some("invalid ip discovery response".into()),
            });
        }

        let nul_byte_index = view
            .get_address_raw()
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| VoiceUdpError {
                kind: VoiceUdpErrorType::DiscoveringIp,
                source: Some("invalid ip discovery response".into()),
            })?;

        let address_raw = &view.get_address_raw()[..nul_byte_index];
        let address_str = std::str::from_utf8(address_raw).map_err(|_| VoiceUdpError {
            kind: VoiceUdpErrorType::DiscoveringIp,
            source: Some("invalid ip discovery response".into()),
        })?;

        let address = IpAddr::from_str(address_str).map_err(|_| VoiceUdpError {
            kind: VoiceUdpErrorType::DiscoveringIp,
            source: Some("invalid ip discovery response".into()),
        })?;

        Ok(DiscoverIpResult {
            address,
            port: view.get_port(),
        })
    }

    /// Converts the internal [`UdpSocket`] into a [`tokio::net::UdpSocket`].
    ///
    /// This function is useful for handling asynchronous operations that are not
    /// possible with synchronous operations.
    pub fn into_tokio(&self) -> Result<tokio::net::UdpSocket, VoiceUdpError> {
        let socket = self.socket.as_ref().ok_or_else(|| VoiceUdpError {
            kind: VoiceUdpErrorType::Connect,
            source: Some("socket is not connected".into()),
        })?;

        let tokio_socket = socket.as_ref().try_clone().unwrap();
        tokio_socket.set_nonblocking(true).unwrap();

        Ok(tokio::net::UdpSocket::from_std(tokio_socket).unwrap())
    }

    /// Sends a keepalive packet to the server.
    pub fn send_keepalive(&self) -> Result<(), VoiceUdpError> {
        let counter = self.keep_alive_counter.load(Ordering::SeqCst);
        let new_counter = counter.wrapping_add(1);

        self.keep_alive_counter.store(new_counter, Ordering::SeqCst);
        self.send(&counter.to_be_bytes())
    }

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
        let socket = self.socket.as_ref().ok_or_else(|| VoiceUdpError {
            kind: VoiceUdpErrorType::Sending,
            source: Some("not connected".into()),
        })?;

        socket
            .send(buffer)
            .map_err(|source| VoiceUdpError {
                kind: VoiceUdpErrorType::Sending,
                source: Some(Box::new(source)),
            })
            .map(|_| ())
    }
}

/// It contains the details that can be retrieved with [`VoiceUdp::discover`].
#[derive(Debug)]
pub struct DiscoverIpResult {
    /// External IP address of the client.
    pub address: IpAddr,

    /// External UDP port binded by the client.
    pub port: u16,
}
