use discortp::discord::{IpDiscoveryPacket, IpDiscoveryType, MutableIpDiscoveryPacket};
use std::{net::IpAddr, str::FromStr};
use tokio::net::UdpSocket;

use super::error::{VoiceUdpError, VoiceUdpErrorType};

/// It contains the details that can be retrieved with
/// [`VoiceUdpExt::discover`].
#[derive(Debug)]
pub struct DiscoverIpResult {
    /// External IP address of the client.
    pub address: IpAddr,

    /// External UDP port binded by the client.
    pub port: u16,
}

/// Additional extensions that allow for developers of Gramophone
/// to call functions as described under the hood.
pub trait VoiceUdpExt: Sized {
    /// Shortcut function from this code below:
    /// ```rs,no_run
    /// use tokio::net::UdpSocket;
    ///
    /// let socket = UdpSocket::bind("0.0.0.0:0").await?;
    /// socket.connect("1.2.3.4:1234").await?;
    /// socket
    /// ```
    async fn connect(ip: IpAddr, port: u16) -> Result<Self, VoiceUdpError>;

    /// Attempts to discover the client's external binded UDP socket
    /// address and port by sending the IP discovery packet along with
    /// the SSRC value assigned by the voice gateway to the
    /// Discord's voice server.
    async fn discover(&self, ssrc: u32) -> Result<DiscoverIpResult, VoiceUdpError>;
}

impl VoiceUdpExt for UdpSocket {
    async fn connect(ip: IpAddr, port: u16) -> Result<Self, VoiceUdpError> {
        let socket = UdpSocket::bind("0.0.0.0:0")
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

        Ok(socket)
    }

    async fn discover(&self, ssrc: u32) -> Result<DiscoverIpResult, VoiceUdpError> {
        let mut bytes = [0u8; IpDiscoveryPacket::const_packet_size()];
        let mut view = MutableIpDiscoveryPacket::new(&mut bytes)
            .expect("unexpected behavior from unequal size");

        view.set_pkt_type(IpDiscoveryType::Request);
        view.set_length(70);
        view.set_ssrc(ssrc);

        self.send(&bytes).await.map_err(|source| VoiceUdpError {
            kind: VoiceUdpErrorType::DiscoveringIp,
            source: Some(Box::new(source)),
        })?;

        let (len, _addr) = self
            .recv_from(&mut bytes)
            .await
            .map_err(|source| VoiceUdpError {
                kind: VoiceUdpErrorType::DiscoveringIp,
                source: Some(Box::new(source)),
            })?;

        IpDiscoveryPacket::new(&bytes[..len])
            .and_then(|view| (view.get_pkt_type() == IpDiscoveryType::Response).then_some(view))
            .and_then(|view| {
                let nul_byte_index = view.get_address_raw().iter().position(|&b| b == 0)?;
                Some((view, nul_byte_index))
            })
            .and_then(|(view, nul_byte_index)| {
                std::str::from_utf8(&view.get_address_raw()[..nul_byte_index])
                    .ok()
                    .and_then(|s| IpAddr::from_str(s).ok())
                    .map(|ip| (ip, view.get_port()))
            })
            .map(|(address, port)| DiscoverIpResult { address, port })
            .ok_or_else(|| VoiceUdpError {
                kind: VoiceUdpErrorType::DiscoveringIp,
                source: Some("invalid ip discovery response".into()),
            })
    }
}

// use std::net::IpAddr;
// use tokio::net::UdpSocket as AsyncUdpSocket;

// use super::error::{VoiceUdpError, VoiceUdpErrorType};

// pub trait VoiceUdpExt {
//     async fn connect(ip: IpAddr, port: u16) -> Result<AsyncUdpSocket, VoiceUdpError>;
//     async fn discover_ip(&self) -> Result<DiscoverIpResult, VoiceUdpError>;
// }

// impl VoiceUdpExt for AsyncUdpSocket {
//     async fn connect(ip: IpAddr, port: u16) -> Result<AsyncUdpSocket, VoiceUdpError> {
//         AsyncUdpSocket::bind((ip, port))
//             .await
//             .map_err(|source| VoiceUdpError {
//                 kind: VoiceUdpErrorType::Connect,
//                 source: Some(Box::new(source)),
//             })
//     }

//     async fn discover_ip(&self) -> Result<DiscoverIpResult, VoiceUdpError> {
//         let mut bytes = [0u8; IpDiscoveryPacket::const_packet_size()];
//         let mut view = MutableIpDiscoveryPacket::new(&mut bytes[..]).expect("unexpected behavior");
//         view.set_pkt_type(IpDiscoveryType::Request);
//         view.set_length(70);
//         view.set_ssrc(ssrc);

//         socket.send(&bytes).await.map_err(|source| VoiceUdpError {
//             kind: VoiceUdpErrorType::DiscoveringIp,
//             source: Some(Box::new(source)),
//         })?;

//         let (len, _addr) = socket
//             .recv_from(&mut bytes)
//             .await
//             .map_err(|source| VoiceUdpError {
//                 kind: VoiceUdpErrorType::DiscoveringIp,
//                 source: Some(Box::new(source)),
//             })?;

//         let view = IpDiscoveryPacket::new(&bytes[..len]).ok_or_else(|| VoiceUdpError {
//             kind: VoiceUdpErrorType::DiscoveringIp,
//             source: Some("invalid ip discovery response".into()),
//         })?;

//         if view.get_pkt_type() != IpDiscoveryType::Response {
//             return Err(VoiceUdpError {
//                 kind: VoiceUdpErrorType::DiscoveringIp,
//                 source: Some("invalid ip discovery response".into()),
//             });
//         }

//         let nul_byte_index = view
//             .get_address_raw()
//             .iter()
//             .position(|&b| b == 0)
//             .ok_or_else(|| VoiceUdpError {
//                 kind: VoiceUdpErrorType::DiscoveringIp,
//                 source: Some("invalid ip discovery response".into()),
//             })?;

//         let address_raw = &view.get_address_raw()[..nul_byte_index];
//         let address_str = std::str::from_utf8(address_raw).map_err(|_| VoiceUdpError {
//             kind: VoiceUdpErrorType::DiscoveringIp,
//             source: Some("invalid ip discovery response".into()),
//         })?;

//         let address = IpAddr::from_str(address_str).map_err(|_| VoiceUdpError {
//             kind: VoiceUdpErrorType::DiscoveringIp,
//             source: Some("invalid ip discovery response".into()),
//         })?;

//         Ok(DiscoverIpResult {
//             address,
//             port: view.get_port(),
//         })
//     }
// }
