use discortp::discord::{IpDiscoveryPacket, IpDiscoveryType, MutableIpDiscoveryPacket};
use futures::{Stream, ready};
use std::net::IpAddr;
use std::pin::Pin;
use std::str::FromStr;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, Interval, MissedTickBehavior};
use tracing::trace;

pub mod error;

use self::error::{VoiceUdpError, VoiceUdpErrorType};
use super::internal::MpscWrapper;

#[derive(Debug)]
pub struct VoiceUdp {
    /// Keep alive counter.
    keep_alive_counter: u32,

    /// Keep alive interval.
    keep_alive_interval: Interval,

    /// Sending flushed packets event channel.
    flushed_tx: MpscWrapper<mpsc::UnboundedSender<oneshot::Sender<()>>>,

    /// Receiving flushed packets event channel.
    flushed_rx: MpscWrapper<mpsc::UnboundedReceiver<oneshot::Sender<()>>>,

    /// Opus send interval.
    opus_send_interval: Interval,

    /// Sending outcoming Opus packets channel.
    outcoming_tx: MpscWrapper<mpsc::UnboundedSender<Vec<u8>>>,

    /// Receiving outcoming Opus packets channel.
    outcoming_rx: MpscWrapper<mpsc::UnboundedReceiver<Vec<u8>>>,

    /// Current outcoming keepalive packet to be sent.
    pending_keep_alive: Option<Vec<u8>>,

    /// Current outcoming Opus packets to be sent.
    pending_opus: Option<Vec<u8>>,

    /// Discord's voice UDP socket acquired from ready voice gateway event.
    socket: Option<UdpSocket>,
}

const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(5);
const OPUS_SEND_INTERVAL: Duration = Duration::from_millis(20);

impl VoiceUdp {
    /// This function makes an uninitialized voice UDP socket.
    #[must_use]
    pub fn new() -> Self {
        Self::new_from(None)
    }

    pub async fn connect(&mut self, ip: IpAddr, port: u16) -> Result<(), VoiceUdpError> {
        let socket = Self::connect_inner(ip, port).await?;
        self.socket = Some(socket);

        Ok(())
    }

    /// Attempts to find the external IP address and port from the client.
    ///
    /// [Documentation](https://discord.com/developers/docs/topics/voice-connections#ip-discovery)
    pub async fn discover_ext_ip(&mut self, ssrc: u32) -> Result<(IpAddr, u16), VoiceUdpError> {
        let mut bytes = [0; IpDiscoveryPacket::const_packet_size()];
        {
            let mut view = MutableIpDiscoveryPacket::new(&mut bytes[..]).unwrap();
            view.set_pkt_type(IpDiscoveryType::Request);
            view.set_length(70);
            view.set_ssrc(ssrc);
        }

        let Some(socket) = self.socket.as_mut() else {
            return Err(VoiceUdpError {
                kind: VoiceUdpErrorType::DiscoveringIp,
                source: Some("socket is not connected".into()),
            });
        };

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

        let address_str =
            std::str::from_utf8(&view.get_address_raw()[..nul_byte_index]).map_err(|_| {
                VoiceUdpError {
                    kind: VoiceUdpErrorType::DiscoveringIp,
                    source: Some("invalid ip discovery response".into()),
                }
            })?;

        let address = IpAddr::from_str(address_str).map_err(|_| VoiceUdpError {
            kind: VoiceUdpErrorType::DiscoveringIp,
            source: Some("invalid ip discovery response".into()),
        })?;

        Ok((address, view.get_port()))
    }

    /// Checks whether the socket is closed.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.socket.is_none()
    }

    /// Consumes the socket and ignores any pending buffers
    pub fn close(&mut self) {
        self.socket = None;
    }

    /// Queues to send a UDP packet to the socket.
    pub fn send(&self, packet: Vec<u8>) {
        self.outcoming_tx
            .send(packet)
            .expect("VoiceUdp owns outcoming channel");
    }
}

impl VoiceUdp {
    pub(crate) fn connect_flushed_event(&self, tx: oneshot::Sender<()>) {
        self.flushed_tx
            .send(tx)
            .expect("VoiceUdp owns self.flushed_rx");
    }

    #[must_use]
    pub(crate) fn new_from(socket: Option<UdpSocket>) -> Self {
        let mut keep_alive_interval = tokio::time::interval_at(Instant::now(), KEEP_ALIVE_INTERVAL);
        let mut opus_send_interval =
            tokio::time::interval_at(Instant::now() + OPUS_SEND_INTERVAL, OPUS_SEND_INTERVAL);

        let (outcoming_tx, outcoming_rx) = mpsc::unbounded_channel();
        opus_send_interval.set_missed_tick_behavior(MissedTickBehavior::Burst);
        keep_alive_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        let (flushed_tx, flushed_rx) = mpsc::unbounded_channel();
        Self {
            keep_alive_counter: 0,
            keep_alive_interval,
            flushed_tx: MpscWrapper(flushed_tx),
            flushed_rx: MpscWrapper(flushed_rx),
            opus_send_interval,
            outcoming_tx: MpscWrapper(outcoming_tx),
            outcoming_rx: MpscWrapper(outcoming_rx),
            pending_keep_alive: None,
            pending_opus: None,
            socket,
        }
    }

    /// Connects to a specific IP address and port and creates a new [UDP socket].
    ///
    /// [UDP socket]: UdpSocket
    pub(crate) async fn connect_inner(ip: IpAddr, port: u16) -> Result<UdpSocket, VoiceUdpError> {
        let socket = UdpSocket::bind("0.0.0.0:0")
            .await
            .map_err(|source| VoiceUdpError {
                kind: VoiceUdpErrorType::Bind,
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

    fn poll_send(&self, packet: &[u8], cx: &mut Context<'_>) -> Poll<tokio::io::Result<()>> {
        ready!(self.socket.as_ref().unwrap().poll_send_ready(cx))?;
        ready!(self.socket.as_ref().unwrap().poll_send(cx, packet))?;
        Poll::Ready(Ok(()))
    }
}
impl Stream for VoiceUdp {
    type Item = VoiceUdpError;

    #[tracing::instrument(skip_all, name = "poll")]
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.socket.is_none() {
            // flush any remaining pending packets to make it seem empty
            while let Poll::Ready(..) = self.outcoming_rx.poll_recv(cx) {}

            // invoke all pending flushed events
            while let Ok(item) = self.flushed_rx.try_recv() {
                _ = item.send(());
            }

            return Poll::Ready(None);
        }

        loop {
            if self.pending_opus.is_none() {
                match self.outcoming_rx.poll_recv(cx) {
                    Poll::Ready(entry) => {
                        let pending = entry.expect("VoiceUdp owns channel");
                        self.pending_opus = Some(pending);
                    }
                    Poll::Pending => {
                        while let Ok(tx) = self.flushed_rx.try_recv() {
                            _ = tx.send(());
                        }
                    }
                }
            }

            let keep_alive_ready = self.keep_alive_interval.poll_tick(cx).is_ready();
            let opus_ready = self.opus_send_interval.poll_tick(cx).is_ready();

            if opus_ready {
                if let Some(packet) = self.pending_opus.as_ref() {
                    if let Err(error) = ready!(self.poll_send(packet, cx)) {
                        return Poll::Ready(Some(VoiceUdpError {
                            kind: VoiceUdpErrorType::Sending,
                            source: Some(Box::new(error)),
                        }));
                    }

                    let sequence = ((packet[2] as u16) << 8) | (packet[3] as u16);
                    trace!(?sequence, packet.len = ?packet.len(), "sent opus packet");
                    self.pending_opus = None;
                }
                continue;
            }

            if keep_alive_ready {
                if self.pending_keep_alive.is_none() {
                    let buffer = self.keep_alive_counter.to_be_bytes();
                    self.keep_alive_counter = self.keep_alive_counter.wrapping_add(1);
                    self.pending_keep_alive = Some(buffer.to_vec());
                }

                if let Some(packet) = self.pending_keep_alive.as_ref() {
                    if let Err(error) = ready!(self.poll_send(packet, cx)) {
                        return Poll::Ready(Some(VoiceUdpError {
                            kind: VoiceUdpErrorType::Sending,
                            source: Some(Box::new(error)),
                        }));
                    }
                    self.pending_keep_alive = None;
                }
            }

            // If neither interval is ready, exit the loop
            if !keep_alive_ready && !opus_ready {
                break;
            }
        }

        Poll::Pending
    }
}
