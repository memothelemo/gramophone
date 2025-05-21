use discortp::discord::{IpDiscoveryPacket, IpDiscoveryType, MutableIpDiscoveryPacket};
use std::net::IpAddr;
use std::str::FromStr;

use super::{
    DiscoverIpResult,
    error::{VoiceUdpError, VoiceUdpErrorType},
};

/// Additional extensions that allow for users to perform Discord Voice UDP
/// related functions as described under the hood.
#[allow(async_fn_in_trait)]
pub trait DiscordUdpExt: Sealed {
    /// Attempts to discover the client's external binded UDP socket
    /// address and port by sending the IP discovery packet along with
    /// the SSRC value assigned by the voice gateway.
    async fn discover(&self, ssrc: u32) -> Result<DiscoverIpResult, VoiceUdpError>;
}

mod sealed {
    pub trait Sealed {}
}
use self::sealed::Sealed;

impl Sealed for tokio::net::UdpSocket {}
impl DiscordUdpExt for tokio::net::UdpSocket {
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
