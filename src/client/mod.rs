mod event;
mod info;

pub mod error;
pub use self::event::*;
pub use self::info::*;

use self::error::{ReceiveEventError, ReceiveEventErrorType};
use crate::crypto::EncryptMode;
use crate::net::VoiceUdp;
use crate::net::VoiceWebSocket;
use crate::net::internal::ConnectionFuture;
use crate::net::udp::DiscoverIpResult;
use crate::net::udp::error::VoiceUdpError;

use futures::Stream;
use futures::ready;
use gramophone_types::OpCode;
use gramophone_types::RTP_KEY_LEN;
use gramophone_types::payload::Event;
use gramophone_types::payload::outgoing::Identify;
use gramophone_types::payload::outgoing::Resume;
use gramophone_types::payload::outgoing::SelectProtocol;
use gramophone_types::payload::outgoing::SelectProtocolData;
use serde_json::json;
use std::net::IpAddr;
use std::pin::Pin;
use std::str::FromStr;
use std::task::Context;
use std::task::Poll;
use tracing::debug;
use tracing::warn;
use twilight_gateway::CloseFrame;

/// This struct encapsulates the complexity of [`VoiceWebSocket`] that provides a
/// unified interface for interacting with the voice system. It bridges between them
/// to simplify the process of working with Discord's Voice API.
///
/// It fully implements with the [Voice API specification] (excluding DAVE encryption for E2EE
/// and the UDP part), making it easier for beginners and advanced users who need a quick and
/// easy way to manage the voice client.
///
/// To create this struct with [`VoiceClient::new`], it must pass a configuration ([`ConnectionInfo`])
/// where it has connection parameters to connect to Discord's voice gateway. You can get some of
/// these parameters like `session_id`, `token` and `endpoint` from [`Voice State Update`] and
/// [`Voice Server Update`] events but you have to send a shard with [`Update Voice State`] event first.
///
/// [`twilight-rs`]: https://github.com/twilight-rs/twilight
/// [`Voice State Update`]: https://discord.com/developers/docs/events/gateway-events#voice-state-update
/// [`Voice Server Update`]: https://discord.com/developers/docs/events/gateway-events#voice-server-update
/// [`Update Voice State`]: https://discord.com/developers/docs/events/gateway-events#update-voice-state
/// [Voice API specification]: https://discord.com/developers/docs/topics/voice-connections
#[derive(Debug)]
pub struct VoiceClient {
    /// Connection parameters to connect to the voice gateway and UDP.
    info: ConnectionInfo,

    /// The current session of the voice client. Most of these values are assigned
    /// by Discord from the Ready voice gateway event.
    ///
    /// This is useful when resuming voice connection.
    session: Option<Session>,

    /// Current state of this mere struct here.
    state: VoiceClientState,

    /// UDP connection used to transmit/receive voice data but it will be
    /// consumed if it is successfully handshaked.
    udp: Option<VoiceUdp>,

    /// Future to establish a UDP connection.
    udp_future: Option<ConnectionFuture<(VoiceUdp, DiscoverIpResult), VoiceUdpError>>,

    /// The WebSocket connection used to communicate
    /// with the voice gateway.
    websocket: VoiceWebSocket,
}

/// Current session for the [`VoiceClient`] object.
#[derive(Debug)]
struct Session {
    pub ip: IpAddr,
    pub port: u16,
    pub secret_key: Option<[u8; RTP_KEY_LEN]>,
    // We need to keep the SSRC value.
    pub ssrc: u32,
    pub mode: EncryptMode,
}

impl VoiceClient {
    #[must_use]
    pub fn new(info: ConnectionInfo) -> Self {
        Self {
            websocket: VoiceWebSocket::new(info.endpoint.clone()),
            session: None,
            state: VoiceClientState::Disconnected,
            udp: None,
            udp_future: None,
            info,
        }
    }

    /// Queues to close the voice connection.
    pub fn close(&mut self, frame: CloseFrame<'static>) {
        self.websocket.close(frame);
    }
}

// TODO: Explain these states in detail.
/// The current state of a [`VoiceClient`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VoiceClientState {
    /// The client is disconnected to Discord but it can
    /// reconnect in the future when needed.
    Disconnected,

    /// The client is fatally disconnected to Discord and it should not reconnected
    /// at any circumstances in the future.
    FatallyDisconnected,

    /// The client is connected to the gateway but it is in the
    /// process of resuming its session.
    Resuming,

    /// The client is connected to the gateway but it is on the
    /// process of identifying.
    Identifying,

    /// The client is connected to the gateway but it is not ready to
    /// transmitted any voice data yet.
    UdpHandshaking,

    /// The client is now ready to transmit voice data.
    Active,
}

impl VoiceClientState {
    #[must_use]
    pub const fn is_connected(&self) -> bool {
        matches!(self, Self::Active)
    }
}

impl VoiceClient {
    fn process_gateway_event(
        &mut self,
        event: Event,
    ) -> Result<Option<VoiceClientEvent>, ReceiveEventError> {
        match &event {
            Event::GatewayClosed(frame) => {
                debug!(?frame, "voice client got disconnected");
                return Ok(Some(if self.websocket.state().is_closed() {
                    self.session = None;
                    VoiceClientEvent::Event(Event::GatewayClosed(frame.clone()))
                } else {
                    self.state = VoiceClientState::Disconnected;
                    VoiceClientEvent::Reconnecting
                }));
            }
            Event::GatewayHeartbeatAck => {}
            Event::GatewayHello(..) if self.websocket.is_reconnected() => {
                debug!("received hello event, resuming session...");
                self.state = VoiceClientState::Resuming;
                self.websocket.send(&json!({
                    "op": OpCode::Resume,
                    "d": Resume {
                        guild_id: self.info.guild_id,
                        session_id: self.info.session_id.clone(),
                        token: self.info.token.expose().into(),
                    },
                }));
            }
            Event::GatewayHello(hello) => {
                debug!(interval = ?hello.heartbeat_interval, "received hello event, identifying session...");
                self.state = VoiceClientState::Identifying;
                self.websocket.send(&json!({
                    "op": OpCode::Identify,
                    "d": Identify {
                        guild_id: self.info.guild_id,
                        user_id: self.info.user_id,
                        session_id: self.info.session_id.clone(),
                        token: self.info.token.expose().into(),
                    },
                }));
            }
            Event::GatewayReady(ready) => {
                debug!(ssrc = ?ready.ssrc, "received ready voice event");

                let ready_ip = ready.ip;
                let ready_port = ready.port;
                let ssrc = ready.ssrc;
                let mode =
                    EncryptMode::negotiate(&ready.modes).ok_or_else(|| ReceiveEventError {
                        kind: ReceiveEventErrorType::Handshaking,
                        source: Some("unavailable encryption mode".into()),
                    })?;

                self.session = Some(Session {
                    ip: ready_ip,
                    port: ready_port,
                    mode,
                    secret_key: None,
                    ssrc,
                });
                self.state = VoiceClientState::UdpHandshaking;
                self.udp_future = Some(ConnectionFuture::new(async move {
                    debug!(ip = ?ready_ip, port = ?ready_port, "connecting to the Voice UDP server");

                    let mut udp = VoiceUdp::new();
                    udp.connect_async(ready_ip, ready_port).await?;

                    let result = udp.discover(ssrc).await?;
                    debug!(?result, "discovered client's external IP address");

                    Ok((udp, result))
                }));
            }
            Event::GatewayResumed => {
                debug!("received resumed voice event");

                let session = self.session.as_ref().expect("unexpected logic");
                let ready_ip = session.ip;
                let ready_port = session.port;
                self.udp_future = Some(ConnectionFuture::new(async move {
                    let mut udp = VoiceUdp::new();
                    udp.connect_async(ready_ip, ready_port).await?;

                    let result = DiscoverIpResult {
                        address: ready_ip,
                        port: ready_port,
                    };
                    Ok((udp, result))
                }));
            }
            Event::SessionDescription(info) => {
                let session = self.session.as_mut().expect("unexpected logic");
                debug!("received session description voice event");

                let mut mode = session.mode;
                if mode.to_request_str() != info.mode {
                    let new_mode =
                        EncryptMode::from_str(&info.mode).map_err(|source| ReceiveEventError {
                            kind: ReceiveEventErrorType::UnsupportedMode,
                            source: Some(Box::new(source)),
                        })?;

                    warn!(preferred = ?mode, responded = ?info.mode, "unmatched encryption mode\
                        for preferred session and session description");

                    mode = new_mode;
                }
                session.mode = mode;

                let aead = mode.aead(&info.secret_key);
                let info = VoiceServerInfo {
                    aead,
                    ssrc: session.ssrc,
                    udp: self.udp.take().expect("unexpected logic"),
                };

                self.state = VoiceClientState::Active;
                return Ok(Some(VoiceClientEvent::Connected(info)));
            }
            _ => {}
        };
        Ok(Some(VoiceClientEvent::Event(event)))
    }
}

impl Stream for VoiceClient {
    type Item = Result<VoiceClientEvent, ReceiveEventError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match self.state {
                VoiceClientState::UdpHandshaking if self.udp_future.is_some() => {
                    let future = Pin::new(
                        &mut self
                            .udp_future
                            .as_mut()
                            .expect("already called `.is_some()`")
                            .0,
                    );

                    let (udp, info) =
                        ready!(future.poll(cx)).map_err(|source| ReceiveEventError {
                            kind: ReceiveEventErrorType::Handshaking,
                            source: Some(Box::new(source)),
                        })?;

                    let session = self.session.as_mut().expect("unexpected logic");
                    debug!(mode = ?session.mode, "connected to Voice UDP server. selecting protocol");

                    let payload = json!({
                        "op": OpCode::SelectProtocol,
                        "d": SelectProtocol {
                            protocol: "udp".into(),
                            data: SelectProtocolData {
                                address: info.address,
                                port: info.port,
                                mode: session.mode.to_request_str().to_string(),
                            },
                        },
                    });

                    self.udp = Some(udp);
                    self.udp_future = None;
                    self.websocket.send(&payload);
                }
                VoiceClientState::Resuming if self.udp_future.is_some() => {
                    let future = Pin::new(
                        &mut self
                            .udp_future
                            .as_mut()
                            .expect("already called `.is_some()`")
                            .0,
                    );

                    let udp = ready!(future.poll(cx))
                        .map_err(|source| ReceiveEventError {
                            kind: ReceiveEventErrorType::Handshaking,
                            source: Some(Box::new(source)),
                        })?
                        .0;

                    self.udp = Some(udp);
                    self.state = VoiceClientState::Active;

                    let session = self.session.as_mut().expect("unexpected logic");
                    let key = session.secret_key.as_ref().expect("unexpected logic");

                    let encryptor = session.mode.aead(key);
                    let info = VoiceServerInfo {
                        aead: encryptor,
                        ssrc: session.ssrc,
                        udp: self.udp.take().expect("unexpected logic"),
                    };

                    return Poll::Ready(Some(Ok(VoiceClientEvent::Connected(info))));
                }
                VoiceClientState::FatallyDisconnected => return Poll::Ready(None),
                _ => {}
            }

            let event = ready!(Pin::new(&mut self.websocket).poll_next(cx));
            let event = match event {
                Some(Ok(event)) => event,
                Some(Err(source)) => {
                    return Poll::Ready(Some(Err(ReceiveEventError {
                        kind: ReceiveEventErrorType::Reconnect,
                        source: Some(Box::new(source)),
                    })));
                }
                None => {
                    self.state = VoiceClientState::FatallyDisconnected;
                    continue;
                }
            };

            match self.process_gateway_event(event) {
                Ok(None) => continue,
                result => return Poll::Ready(result.transpose()),
            }
        }
    }
}
