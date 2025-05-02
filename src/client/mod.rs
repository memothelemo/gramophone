pub mod channel;
pub mod error;
pub mod options;

pub use self::options::ConnectionInfo;

use futures::{Stream, TryFutureExt, ready};
use gramophone_types::OpCode;
use gramophone_types::payload::SpeakingFlags;
use gramophone_types::payload::outgoing::{Identify, Resume, SelectProtocol, SelectProtocolData};
use serde_json::json;
use std::net::IpAddr;
use std::pin::Pin;
use std::str::FromStr;
use std::task::{Context, Poll};
use tracing::{debug, warn};
use twilight_gateway::CloseFrame;
use twilight_model::id::Id;
use twilight_model::id::marker::UserMarker;

use self::channel::{CommandMessage, MessageChannel, VoiceClientSender};
use self::error::{
    ReceiveEventError, ReceiveEventErrorType, SetSpeakingError, SetSpeakingErrorType,
};

use crate::crypto::{Aead, EncryptMode};
use crate::net::udp::error::VoiceUdpError;
use crate::net::{VoiceUdp, VoiceWebSocket, VoiceWebSocketEvent};

/// This struct encapsulates the complexity of components with [`VoiceWebSocket`]
/// and [`VoiceUdp`] that provides a unified interface for interacting with the
/// voice system. It bridges between them to simplify the process of working with
/// Discord's Voice API.
///
/// It fully implements with the [Voice API specification] (excluding DAVE encryption for E2EE),
/// making it easier for beginners and advanced users who need a quick and easy way
/// to manage the voice client.
///
/// To create this struct with [`VoiceClient::new`], it must pass a configuration ([`ConnectionInfo`])
/// where it has connection parameters to connect to Discord's voice gateway and UDP. You
/// can get some of these parameters like `session_id`, `token` and `endpoint` from
/// [`Voice State Update`] and [`Voice Server Update`] events but you have to send a shard
/// with [`Update Voice State`] event first.
///
/// [`twilight-rs`]: https://github.com/twilight-rs/twilight
/// [`Voice State Update`]: https://discord.com/developers/docs/events/gateway-events#voice-state-update
/// [`Voice Server Update`]: https://discord.com/developers/docs/events/gateway-events#voice-server-update
/// [`Update Voice State`]: https://discord.com/developers/docs/events/gateway-events#update-voice-state
/// [Voice API specification]: https://discord.com/developers/docs/topics/voice-connections
#[derive(Debug)]
pub struct VoiceClient {
    /// Channels that can be sent to the client.
    channel: MessageChannel,

    /// Current AEAD encryptor to encrypt packets later on.
    encryptor: Option<Box<dyn Aead>>,

    /// Connection parameters to connect to the voice gateway and UDP.
    info: ConnectionInfo,

    /// The current session of the voice client. Most of these values are assigned
    /// by Discord from the Ready voice gateway event (except the `mode` field).
    ///
    /// This is useful when resuming voice connection.
    session: Option<VoiceClientSession>,

    /// Determines whether or not we're speaking currently.
    speaking: bool,

    /// Current state of this struct here.
    state: VoiceClientState,

    /// The WebSocket connection used to communicate
    /// with the voice gateway.
    websocket: VoiceWebSocket,

    /// UDP connection used to transmit/receive voice data.
    udp: Option<VoiceUdp>,

    /// Future to establish a UDP connection.
    udp_future: Option<UdpConnectFuture>,
}

impl VoiceClient {
    #[must_use]
    pub fn new(info: ConnectionInfo) -> Self {
        let websocket = VoiceWebSocket::new(info.endpoint.clone());
        Self {
            encryptor: None,
            info,
            channel: MessageChannel::new(),
            session: None,
            speaking: false,
            state: VoiceClientState::Disconnected,
            websocket,
            udp: None,
            udp_future: None,
        }
    }

    /// Queues to close the voice connection.
    pub fn close(&mut self, frame: CloseFrame<'static>) {
        self.websocket.close(frame);
    }

    /// Queues to send current speaking status to the voice connection.
    ///
    /// # Errors
    ///
    /// It will return `Err(...)` if the client has not connected to Discord yet.
    pub fn speaking(&mut self, speaking: bool) -> Result<(), SetSpeakingError> {
        if !self.state.is_connected() {
            return Err(SetSpeakingError {
                kind: SetSpeakingErrorType::NotConnected,
                source: None,
            });
        }
        self.channel.sender().speaking(speaking)
    }

    /// Gets a channel to send (maybe valid) [Opus packets] over to the voice client.
    ///
    /// [Opus packets]: https://en.wikipedia.org/wiki/Opus_(audio_format)
    #[must_use]
    pub fn sender(&self) -> VoiceClientSender {
        self.channel.sender()
    }

    /// Queues to send a/an (maybe valid) Opus packet to the client.
    pub fn send(&self, opus: &[u8]) {
        self.channel
            .opus_tx
            .send(opus.to_vec())
            .expect("VoiceClient owns PacketChannel receivers");
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
    fn dispatch_packet(&mut self, maybe_opus_packet: &[u8]) -> Result<Vec<u8>, ReceiveEventError> {
        // Expecting that no one is crazy enough doing this...
        //
        // 960 = 48,000 (from 48kHz sample rate) / 100 * 2 (2 audio channels / stereo)
        const TIMESTAMP_INC: u32 = 960;

        let Some((session, encryptor)) = self.session.as_mut().zip(self.encryptor.as_ref()) else {
            panic!(
                "invalid use of `dispatch_packet(...)` while its state is {:?}",
                self.state
            );
        };

        let mut rtp_header = Vec::with_capacity(24);
        rtp_header.push(0x80);
        rtp_header.push(0x78);

        rtp_header.extend(session.sequence.to_be_bytes());
        rtp_header.extend(session.timestamp.to_be_bytes());
        rtp_header.extend(session.ssrc.to_be_bytes());

        let mut nonce_buffer = vec![0u8; encryptor.mode().nonce_size()];
        nonce_buffer
            .iter_mut()
            .zip(session.nonce.to_be_bytes().into_iter())
            .for_each(|(a, b)| *a = b);

        let mut encrypted = rtp_header.clone();
        let ciphertext = encryptor
            .encrypt(&nonce_buffer, &rtp_header, maybe_opus_packet)
            .map_err(|source| ReceiveEventError {
                kind: ReceiveEventErrorType::SendingPacket,
                source: Some(Box::new(source)),
            })?;

        encrypted.extend(ciphertext);
        encrypted.extend(session.nonce.to_be_bytes());

        session.nonce = session.nonce.wrapping_add(1);
        session.timestamp = session.timestamp.wrapping_add(TIMESTAMP_INC);
        session.sequence = session.sequence.wrapping_add(1);

        Ok(encrypted)
    }

    fn process_ws_event(
        &mut self,
        vws_event: VoiceWebSocketEvent,
    ) -> Result<Option<VoiceClientEvent>, ReceiveEventError> {
        use gramophone_types::payload::Event;

        let event = match vws_event {
            VoiceWebSocketEvent::Event(event) => event,
            VoiceWebSocketEvent::Reconnected => {
                self.speaking = false;
                self.state = VoiceClientState::Resuming;
                self.websocket.send(&json!({
                    "op": OpCode::Resume,
                    "d": Resume {
                        guild_id: self.info.guild_id,
                        session_id: self.info.session_id.clone(),
                        token: self.info.token.expose().into(),
                    }
                }));
                return Ok(None);
            }
            VoiceWebSocketEvent::Disconnected(..) => {
                debug!("voice connection got disconnected");

                self.speaking = false;
                if self.websocket.state().is_closed() {
                    self.state = VoiceClientState::FatallyDisconnected;
                    self.session = None;
                    self.encryptor = None;
                    return Ok(None);
                } else {
                    // Attempt to reconnect again I guess?
                    self.state = VoiceClientState::Disconnected;
                    return Ok(Some(VoiceClientEvent::Disconnected));
                }
            }
        };

        match event {
            Event::ClientConnect(data) => {
                if data.user_ids.is_empty() {
                    return Ok(None);
                }

                // Discord sometimes gave the voice client, an initial list of connected users
                // in a voice channel after the client is successfully connected into it.
                let session = self.session.as_mut().unwrap();
                if session.given_initial_users {
                    debug_assert_eq!(data.user_ids.len(), 1);
                    Ok(Some(VoiceClientEvent::Joined {
                        user_id: data.user_ids.first().copied().unwrap(),
                    }))
                } else {
                    session.given_initial_users = true;
                    Ok(Some(VoiceClientEvent::Users {
                        user_ids: data.user_ids,
                    }))
                }
            }
            Event::ClientDisconnect(data) => Ok(Some(VoiceClientEvent::Left {
                user_id: data.user_id,
            })),
            Event::HeartbeatAck => Ok(None),
            // heartbeat interval stuff is handled in VoiceWebSocket
            Event::Hello(..) => {
                if let Some(session) = self.session.as_mut() {
                    session.given_initial_users = false;
                }

                self.state = VoiceClientState::Identifying;
                self.websocket.send(&json!({
                    "op": OpCode::Identify,
                    "d": Identify {
                        guild_id: self.info.guild_id,
                        user_id: self.info.user_id,
                        session_id: self.info.session_id.clone(),
                        token: self.info.token.expose().into(),
                    }
                }));

                Ok(None)
            }
            Event::Ready(ready) => {
                debug!("received ready voice event");

                let udp_ip = ready.ip;
                let udp_port = ready.port;
                let ssrc = ready.ssrc;
                let mode =
                    EncryptMode::negotiate(&ready.modes).ok_or_else(|| ReceiveEventError {
                        kind: ReceiveEventErrorType::Handshaking,
                        source: Some("unavailable encryption mode".into()),
                    })?;

                self.session = Some(VoiceClientSession::new(udp_ip, udp_port, mode, ssrc));
                self.state = VoiceClientState::UdpHandshaking;
                self.udp_future = Some(UdpConnectFuture(Box::pin(async move {
                    debug!(ip = ?udp_ip, port = ?udp_port, "connecting UDP socket");

                    let mut udp = VoiceUdp::connect_inner(udp_ip, udp_port)
                        .map_ok(|v| VoiceUdp::new_from(Some(v)))
                        .await?;

                    debug!("discovering client's external IP address");
                    let (ext_addr, ext_port) = udp.discover_ext_ip(ssrc).await?;
                    Ok((udp, ext_addr, ext_port))
                })));

                Ok(None)
            }
            Event::Resumed => {
                debug!("received resumed voice event");

                let session = self.session.as_ref().unwrap();
                let udp_ip = session.ip;
                let udp_port = session.port;
                self.udp_future = Some(UdpConnectFuture(Box::pin(async move {
                    let udp = VoiceUdp::connect_inner(udp_ip, udp_port)
                        .map_ok(|v| VoiceUdp::new_from(Some(v)))
                        .await?;

                    Ok((udp, udp_ip, udp_port))
                })));

                Ok(None)
            }
            Event::SessionDescription(desc) => {
                debug!("received session description voice event");
                let session = self.session.as_ref().unwrap();

                let mut mode = session.mode;
                if session.mode.to_request_str() != desc.mode {
                    warn!(
                        session.mode = ?session.mode,
                        payload.mode = ?desc.mode,
                        "unmatched encryption mode for preferred session and session description"
                    );
                    mode = EncryptMode::from_str(&desc.mode).map_err(|s| ReceiveEventError {
                        kind: ReceiveEventErrorType::Handshaking,
                        source: Some(Box::new(s)),
                    })?;
                }

                self.session.as_mut().unwrap().mode = mode;
                self.encryptor = Some(mode.encryptor(&desc.secret_key));
                self.state = VoiceClientState::Active;

                Ok(Some(VoiceClientEvent::Connected))
            }
            Event::Speaking(..) => Ok(None),
        }
    }
}

/// The resulting value of `.next()` function in [`VoiceClient`].
#[derive(Debug)]
pub enum VoiceClientEvent {
    /// Successfully connected to a voice channel.
    Connected,

    /// This event provides the initial state of users connected to a voice channel.
    Users { user_ids: Vec<Id<UserMarker>> },

    /// Someone left the voice channel.
    Left { user_id: Id<UserMarker> },

    /// Someone joined the voice channel.
    Joined { user_id: Id<UserMarker> },

    /// The client got disconnected to a voice channel.
    ///
    /// It may be reconnected successfully to the voice channel.
    Disconnected,

    /// Successfully resumed call session.
    Resumed,
}

impl VoiceClient {
    fn poll_send_packet(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), ReceiveEventError>> {
        let queued_packet = self
            .session
            .as_mut()
            .expect("unexpected logic")
            .queued_packet
            .take();

        let maybe_opus = if let Some(incoming) = queued_packet {
            incoming
        } else if let Poll::Ready(incoming) = self.channel.opus_rx.poll_recv(cx) {
            incoming.expect("VoiceClient owns PacketChannel receivers")
        } else {
            return Poll::Ready(Ok(()));
        };

        // Discard any queued Opus packets if we're currently muted.
        if !self.speaking {
            return Poll::Ready(Ok(()));
        }

        let encrypted = self.dispatch_packet(&maybe_opus)?;
        let udp = self.udp.as_mut().expect("unexpected logic");
        udp.send(encrypted);

        Poll::Ready(Ok(()))
    }
}

impl Stream for VoiceClient {
    type Item = Result<VoiceClientEvent, ReceiveEventError>;

    #[tracing::instrument(skip_all, name = "poll", fields(session = ?self.session, state = ?self.state))]
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(udp) = self.udp.as_mut() {
                // It can return None if it is closed so we don't need to deal with it.
                match Pin::new(udp).poll_next(cx) {
                    Poll::Ready(Some(error)) => {
                        return Poll::Ready(Some(Err(ReceiveEventError {
                            kind: ReceiveEventErrorType::SendingPacket,
                            source: Some(Box::new(error)),
                        })));
                    }
                    Poll::Ready(None) => {
                        self.udp = None;
                    }
                    _ => {}
                }
            }

            match &self.state {
                VoiceClientState::Active => {
                    // Any queued (maybe valid) Opus packets will be sent over to the UDP connection
                    // as long as the client is successfully connected to that thing.
                    if let Err(error) = ready!(self.poll_send_packet(cx)) {
                        return Poll::Ready(Some(Err(error)));
                    }

                    if let Poll::Ready(message) = self.channel.command_rx.poll_recv(cx) {
                        let message = message.expect("VoiceClient owns PacketChannel receivers");
                        match message {
                            CommandMessage::Speaking(speaking) => {
                                let session = self.session.as_mut().expect("unexpected logic");
                                let ssrc = session.ssrc;
                                let flags = if speaking {
                                    SpeakingFlags::MICROPHONE
                                } else {
                                    SpeakingFlags::empty()
                                };

                                debug!("set speaking to {speaking}");

                                self.speaking = speaking;
                                self.websocket.send(&json!({
                                    "op": OpCode::Speaking,
                                    "d": json!({
                                        "speaking": flags,
                                        "delay": 0,
                                        "ssrc": ssrc,
                                    }),
                                }));
                            }
                            CommandMessage::WaitUntilFlushed(tx) => {
                                self.udp.as_ref().unwrap().connect_flushed_event(tx);
                            }
                        }
                    }
                }
                VoiceClientState::FatallyDisconnected => {
                    // flush any queued packets to be sent to the UDP connection.
                    while let Poll::Ready(packet) = self.channel.opus_rx.poll_recv(cx) {
                        let _ = packet.expect("VoiceClient owns PacketChannel receivers");
                    }

                    // cleanup anything and return nothing I guess?
                    if let Some(udp) = self.udp.as_mut() {
                        udp.close();
                        continue;
                    }

                    return Poll::Ready(None);
                }
                VoiceClientState::Resuming if self.udp_future.is_some() => {
                    let future = Pin::new(&mut self.udp_future.as_mut().unwrap().0);
                    let udp = ready!(future.poll(cx))
                        .map_err(|source| ReceiveEventError {
                            kind: ReceiveEventErrorType::Handshaking,
                            source: Some(Box::new(source)),
                        })?
                        .0;

                    debug!("reconnected to voice UDP");
                    self.udp = Some(udp);
                    self.udp_future = None;
                    self.state = VoiceClientState::Active;
                    return Poll::Ready(Some(Ok(VoiceClientEvent::Resumed)));
                }
                // maybe it is connected successfully and we need to send select protocol anyways
                // so we can get connected to somewhere in the world of fun!
                VoiceClientState::UdpHandshaking if self.udp_future.is_some() => {
                    let future = Pin::new(&mut self.udp_future.as_mut().unwrap().0);
                    let (udp, ext_addr, ext_port) =
                        ready!(future.poll(cx)).map_err(|source| ReceiveEventError {
                            kind: ReceiveEventErrorType::Handshaking,
                            source: Some(Box::new(source)),
                        })?;

                    let mode = self.session.as_ref().unwrap().mode;
                    debug!(?mode, "connected to voice UDP. selecting protocol");

                    let payload = json!({
                        "op": OpCode::SelectProtocol,
                        "d": SelectProtocol {
                            protocol: "udp".into(),
                            data: SelectProtocolData {
                                address: ext_addr,
                                port: ext_port,
                                mode: mode.to_request_str().to_string(),
                            },
                        },
                    });

                    self.udp = Some(udp);
                    self.udp_future = None;
                    self.websocket.send(&payload);
                }
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

            match self.process_ws_event(event) {
                Ok(Some(event)) => return Poll::Ready(Some(Ok(event))),
                Ok(None) => continue,
                Err(err) => return Poll::Ready(Some(Err(err))),
            }
        }
    }
}

#[derive(Debug)]
struct VoiceClientSession {
    pub ip: IpAddr,
    pub port: u16,
    pub mode: EncryptMode,

    pub nonce: u32,
    pub sequence: u16,
    pub timestamp: u32,
    pub ssrc: u32,

    /// Queued audio packet, because we need to speaking payload before
    /// we can proceed to emitting audio data.
    pub queued_packet: Option<Vec<u8>>,

    /// Whether Discord gave us the initial list of connected users to a
    /// particular voice channel we're connected to.
    pub given_initial_users: bool,
}

impl VoiceClientSession {
    #[must_use]
    pub fn new(ip: IpAddr, port: u16, mode: EncryptMode, ssrc: u32) -> Self {
        Self {
            ip,
            port,
            mode,

            // weird behavior from discord.js but we don't care anyways.
            nonce: fastrand::u32(0..u32::MAX),
            sequence: 0,
            timestamp: fastrand::u32(0..u32::MAX),
            ssrc,

            queued_packet: None,
            given_initial_users: false,
        }
    }
}

/// Wrapper struct around an `async fn` with a `Debug` implementation.
struct UdpConnectFuture(
    Pin<Box<dyn Future<Output = Result<(VoiceUdp, IpAddr, u16), VoiceUdpError>> + Send>>,
);

impl std::fmt::Debug for UdpConnectFuture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("UdpConnectFuture")
            .field(&"<async fn>")
            .finish()
    }
}

#[cfg(test)]
mod tests;
