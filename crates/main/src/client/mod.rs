use std::borrow::Cow;
use std::net::IpAddr;
use std::pin::Pin;
use std::task::{Context, Poll, ready};
use std::time::{Duration, SystemTime};

use futures::{FutureExt, Sink};
use gramophone_types::payload::incoming;
use gramophone_types::payload::outgoing::SelectProtocol;
use gramophone_types::payload::speaking::SpeakingFlags;
use gramophone_types::payload::{Speaking, outgoing};
use gramophone_types::{OpCode, RTP_KEY_LEN};
use serde::Deserialize;
use serde_json::json;
use tokio::net::UdpSocket;
use tokio_websockets::{CloseCode as WsCloseCode, Error as WsError, Message as WsMessage};
use tracing::{debug, trace, warn};
use twilight_gateway::Message as TwilightMessage;
use twilight_model::gateway::CloseFrame;
use twilight_model::gateway::event::GatewayEventDeserializer;

pub mod channel;
pub mod error;
pub mod heartbeater;
pub mod info;
pub mod state;

pub use self::error::VoiceClientError;
pub use self::info::VoiceConnectionInfo;
pub use self::state::VoiceClientState;

use self::channel::{MessageChannel, VoiceClientCommand, VoiceClientSender};
use self::error::VoiceClientErrorType;
use self::heartbeater::{HeartbeatInfo, Heartbeater};

use crate::crypto::EncryptMode;
use crate::udp::VoiceUdp;
use crate::udp::error::VoiceUdpError;
use crate::udp::ext::{DiscoverIpResult, VoiceUdpExt};
use crate::util::{ConnectionFuture, WsConnection};

#[derive(Debug)]
pub struct VoiceClient {
    channel: MessageChannel,

    /// WebSocket connection, which may be connected to Discord's voice gateway.
    connection: Option<WsConnection>,

    /// Future to establish a WebSocket connection with the voice gateway.
    future: Option<ConnectionFuture<WsConnection, WsError>>,

    /// Future to connects to a UDP socket assigned by Discord and fetch the
    /// client's UDP external socket address and port.
    future_udp: Option<ConnectionFuture<(VoiceUdp, DiscoverIpResult), VoiceUdpError>>,

    /// This allows to keep track of heartbeats during the lifetime
    /// of the voice gateway connection.
    heartbeater: Option<Heartbeater>,

    /// Connection info to connect to the voice gateway.
    info: VoiceConnectionInfo,

    /// Pending message to be sent to the voice gateway.
    pending: Option<Pending>,

    /// The current session of the voice client. Most of these values are assigned
    /// by Discord from the Ready voice gateway event.
    ///
    /// This is useful when resuming voice connection.
    session: Option<Session>,

    /// Current state of a [`VoiceClient`].
    ///
    /// This is to allow to implement some logic for some variants
    /// of the [`VoiceClientState`].
    state: VoiceClientState,

    /// Internal value that tells whether connecting to the
    /// WebSocket with TLS is enabled.
    tls_enabled: bool,

    /// Pending UDP socket to be released once it is successfully connected
    /// both with the gateway and the voice socket.
    udp: Option<VoiceUdp>,
}

impl VoiceClient {
    #[must_use]
    pub fn new(info: VoiceConnectionInfo) -> Self {
        Self {
            channel: MessageChannel::new(),
            connection: None,
            future: None,
            future_udp: None,
            heartbeater: None,
            info,
            pending: None,
            session: None,
            state: VoiceClientState::Disconnected { attempts: 0 },
            tls_enabled: true,
            udp: None,
        }
    }

    /// Queue a WebSocket close frame and sends it to the Voice gateway.
    pub fn close(&self, frame: CloseFrame<'static>) {
        self.channel
            .close_tx
            .send(frame)
            .expect("close channel opens");
    }

    /// Gets the heartbeat information of the [connection].
    ///
    /// It returns `None` if it has not connected to the voice server.
    ///
    /// [connection]: VoiceClient
    #[must_use]
    pub fn heartbeat(&self) -> Option<HeartbeatInfo<'_>> {
        self.heartbeater.as_ref().map(|v| v.info())
    }

    /// Gets a channel to send messages over the client to the Voice gateway.
    ///
    /// This is useful for sending messages to the client if it is not
    /// feasible to communicate with the client from other tasks or threads.
    #[must_use]
    pub fn sender(&self) -> VoiceClientSender {
        self.channel.sender()
    }

    /// Queues [`Speaking`] payload to be sent to the gateway.
    ///
    /// [`Speaking`]: https://discord.com/developers/docs/topics/voice-connections#speaking
    pub fn speaking(&self, flags: SpeakingFlags) {
        self.channel
            .command_tx
            .send(VoiceClientCommand::Speaking(flags))
            .expect("command channel opens");
    }

    /// Gets the current state of [`VoiceClient`].
    #[must_use]
    pub fn state(&self) -> VoiceClientState {
        self.state
    }
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

#[derive(Debug)]
struct Pending {
    message: Option<WsMessage>,
    is_heartbeat: bool,
}

impl Pending {
    fn json<T: serde::Serialize>(data: &T) -> Self {
        Pending {
            message: Some(WsMessage::text(
                serde_json::to_string(data).expect("serialization should not fail"),
            )),
            is_heartbeat: false,
        }
    }
}

#[derive(Debug)]
enum CloseInitiator {
    /// Connection failed something with WebSocket.
    Transport,

    /// Voice gateway initiated the close.
    Gateway(Option<u16>),

    /// Connection closed initiated by a user.
    Client(CloseFrame<'static>),
}

impl VoiceClient {
    fn close_inner(&mut self, initiator: CloseInitiator) {
        self.heartbeater = None;
        self.state = match initiator {
            CloseInitiator::Transport => VoiceClientState::Closed,
            CloseInitiator::Gateway(code) => VoiceClientState::from_close_code(code),
            CloseInitiator::Client(frame) => {
                let code = frame.code;
                self.pending = Some(Pending {
                    message: Some(WsMessage::close(
                        WsCloseCode::try_from(code).ok(),
                        &frame.reason,
                    )),
                    is_heartbeat: false,
                });
                match code {
                    1000 | 1001 => VoiceClientState::Closed,
                    _ => VoiceClientState::from_close_code(Some(code)),
                }
            }
        };
    }
}

impl VoiceClient {
    /// Attempts to connect to the voice UDP socket.
    fn poll_udp_connect(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), VoiceUdpError>> {
        match self.state {
            VoiceClientState::UdpHandshaking if self.future_udp.is_some() => {
                let fut = self.future_udp.as_mut().expect("unexpected logic");
                let result = ready!(Pin::new(&mut fut.0).poll(cx));
                self.future_udp = None;

                let (udp, discover) = result?;
                let session = self.session.as_ref().expect("unexpected logic");
                let data = SelectProtocol::builder()
                    .address(discover.address)
                    .port(discover.port)
                    .mode(session.mode.as_str())
                    .protocol("udp")
                    .build();

                debug!(
                    external.ip = ?discover.address,
                    external.port = ?discover.port,
                    "connected to a voice UDP socket"
                );
                self.future_udp = None;
                self.pending = Some(Pending::json(&json!({
                    "op": OpCode::SelectProtocol,
                    "d": data,
                })));
                self.udp = Some(udp);
            }
            _ => {}
        }
        Poll::Ready(Ok(()))
    }

    /// Attempts to connect to the voice gateway.
    ///
    /// # Returns
    ///
    /// * `Poll::Pending` if connection is in progress
    /// * `Poll::Ready(Ok(true))` if the WebSocket connection has been successfully connected.
    /// * `Poll::Ready(Ok(false))` if the connection cannot be reconnected.
    /// * `Poll::Ready(Err)` if connecting to the voice gateway is failed.
    fn poll_gateway_connect(&mut self, cx: &mut Context<'_>) -> Poll<Result<bool, WsError>> {
        match self.state {
            VoiceClientState::Closed => {
                ready!(self.poll_send_pending(cx))?;
                if let Some(connection) = self.connection.as_mut() {
                    _ = ready!(Pin::new(connection).poll_close(cx));
                }
                return Poll::Ready(Ok(false));
            }
            VoiceClientState::Disconnected { attempts } if self.connection.is_none() => {
                #[cfg(not(test))]
                assert!(
                    self.tls_enabled,
                    "TLS MUST BE ENABLED IN NON-TESTING ENVIRONMENT!!!"
                );

                if self.future.is_none() {
                    let protocol = if self.tls_enabled { "wss" } else { "ws" };
                    let url = format!(
                        "{protocol}://{}/?v={}",
                        self.info.endpoint,
                        gramophone_types::API_VERSION
                    );
                    debug!(?attempts, ?url, "connecting to the voice gateway");

                    let tls = tokio_websockets::Connector::new()
                        .expect("could not establish WebSocket connection with TLS layer");

                    self.future = Some(ConnectionFuture::new(async move {
                        let secs = 2u8.saturating_pow(attempts);
                        tokio::time::sleep(Duration::from_secs(secs.into())).await;

                        let (client, ..) = tokio_websockets::ClientBuilder::new()
                            .uri(&url)
                            .expect("URL should be valid")
                            .limits(tokio_websockets::Limits::unlimited())
                            .connector(&tls)
                            .connect()
                            .await?;

                        Ok(client)
                    }));
                }

                let result = ready!(
                    Pin::new(&mut self.future.as_mut().expect("self.future must be defined").0)
                        .poll(cx)
                );

                self.future = None;
                match result {
                    Ok(connection) => {
                        self.connection = Some(connection);
                        self.state = VoiceClientState::Identifying;
                    }
                    Err(source) => {
                        // do not go back to 0 if it does that.
                        let mut attempts = attempts.wrapping_add(1);
                        if attempts == 0 {
                            attempts = 1;
                        }
                        self.state = VoiceClientState::Disconnected { attempts };
                        return Poll::Ready(Err(source));
                    }
                }
            }
            _ => {}
        };
        Poll::Ready(Ok(true))
    }

    /// Attempts to send a pending message to the voice gateway if
    /// self.connection is `Some`.
    fn poll_send_pending(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), WsError>> {
        let Some((mut ws, pending)) = self.connection.as_mut().zip(self.pending.as_mut()) else {
            return Poll::Ready(Ok(()));
        };
        ready!(Pin::new(&mut ws).poll_ready(cx))?;

        if let Some(message) = pending.message.take() {
            Pin::new(&mut ws).start_send(message)?;
        }
        ready!(Pin::new(&mut ws).poll_flush(cx))?;

        if pending.is_heartbeat {
            self.heartbeater
                .as_mut()
                .expect("heartbeater should be defined after it established connection")
                .record_sent();
        }

        self.pending = None;
        Poll::Ready(Ok(()))
    }

    /// Attempts to send due commands to the voice gateway.
    fn poll_send(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), WsError>> {
        loop {
            ready!(self.poll_send_pending(cx))?;

            if !self.state.is_disconnected() {
                if let Poll::Ready(frame) = self.channel.close_rx.poll_recv(cx) {
                    let frame = frame.expect("close channel opens");
                    debug!(?frame, "sending close frame from user channel");

                    self.close_inner(CloseInitiator::Client(frame));
                    continue;
                }
            }

            // You can only send heartbeats to Discord voice gateway if you identified the
            // connection to them unlike the normal Discord gateway.
            let send_heartbeat = self
                .heartbeater
                .as_mut()
                .is_some_and(|h| h.interval.poll_tick(cx).is_ready());

            if send_heartbeat && self.state.can_send_heartbeats() {
                let heartbeater = self
                    .heartbeater
                    .as_mut()
                    .expect("should be defined from send_heartbeat");

                if heartbeater.has_sent() {
                    warn!("connection is failed or \"zombied\", closing connection");
                    self.close_inner(CloseInitiator::Client(CloseFrame::RESUME));
                } else {
                    trace!("sending heartbeat payload");

                    let mut pending = Pending::json(&json!({
                        "op": OpCode::Heartbeat,
                        "d": SystemTime::UNIX_EPOCH.elapsed()
                            .unwrap_or_default()
                            .as_millis(),
                    }));
                    pending.is_heartbeat = true;

                    self.pending = Some(pending);
                    continue;
                }
            }

            // Sending any pending commands from the channel (only if the connection is identified)
            if self.state.is_active() {
                if let Poll::Ready(command) = self.channel.command_rx.poll_recv(cx) {
                    let command = command.expect("command channel opens");
                    let session = self.session.as_ref().expect("unexpected logic");

                    debug!(?command, "sending command from message channel");
                    self.pending = Some(match command {
                        VoiceClientCommand::Speaking(flags) => Pending::json(&json!({
                            "op": OpCode::Speaking,
                            "d": Speaking {
                                ssrc: session.ssrc,
                                user_id: self.info.user_id,
                                speaking: flags,
                            },
                        })),
                    });

                    continue;
                }
            }

            return Poll::Ready(Ok(()));
        }
    }
}

/// Gateway event with only minimal required data.
#[derive(Deserialize)]
struct MinimalEvent<T> {
    #[serde(rename = "d")]
    pub data: T,
}

impl VoiceClient {
    fn process_event(&mut self, event: &str) -> Result<(), VoiceClientError> {
        let (raw_opcode, ..) = GatewayEventDeserializer::from_json(event)
            .ok_or_else(|| VoiceClientError {
                kind: VoiceClientErrorType::Deserializing {
                    event: event.to_owned(),
                },
                source: Some("missing opcode".into()),
            })?
            .into_parts();

        // Let the user handle the unknown opcodes.
        let Some(opcode) = OpCode::from(raw_opcode) else {
            return Ok(());
        };

        match opcode {
            OpCode::Hello => {
                let result = serde_json::from_str::<MinimalEvent<self::incoming::Hello>>(event);
                let event = result.map_err(|source| VoiceClientError {
                    kind: VoiceClientErrorType::Deserializing {
                        event: event.to_owned(),
                    },
                    source: Some(Box::new(source)),
                })?;

                let interval = Duration::from_secs_f64(event.data.heartbeat_interval);
                if self.session.is_some() {
                    debug!(heartbeat_interval = ?interval, "received hello event, resuming session...");
                    self.state = VoiceClientState::Resuming;
                    self.heartbeater = Some(Heartbeater::new(interval));
                    self.pending = Some(Pending::json(&json!({
                        "op": OpCode::Resume,
                        "d": self::outgoing::Resume {
                            guild_id: self.info.guild_id,
                            session_id: self.info.session_id.clone(),
                            token: self.info.token.expose().into(),
                        },
                    })));
                } else {
                    debug!(heartbeat_interval = ?interval, "received hello event");
                    self.state = VoiceClientState::Identifying;
                    self.heartbeater = Some(Heartbeater::new(interval));
                    self.pending = Some(Pending::json(&json!({
                        "op": OpCode::Identify,
                        "d": self::outgoing::Identify {
                            guild_id: self.info.guild_id,
                            user_id: self.info.user_id,
                            session_id: self.info.session_id.clone(),
                            token: self.info.token.expose().into(),
                        },
                    })));
                }
            }
            OpCode::Ready => {
                let result = serde_json::from_str::<MinimalEvent<self::incoming::Ready>>(event);
                let event = result.map_err(|source| VoiceClientError {
                    kind: VoiceClientErrorType::Deserializing {
                        event: event.to_owned(),
                    },
                    source: Some(Box::new(source)),
                })?;

                let ip = event.data.ip;
                let port = event.data.port;
                let ssrc = event.data.ssrc;
                debug!(?ssrc, "received ready event");

                let result = EncryptMode::negotiate(&event.data.modes);
                let mode = result.ok_or_else(|| VoiceClientError {
                    kind: VoiceClientErrorType::UnsupportedMode,
                    source: None,
                })?;

                let heartbeater = self.heartbeater.as_mut().expect("unexpected logic");
                heartbeater
                    .interval
                    .reset_after(heartbeater.interval.period());

                self.future_udp = Some(ConnectionFuture::new(async move {
                    debug!(?ip, ?port, "connecting to UDP socket...");

                    let socket = <UdpSocket as VoiceUdpExt>::connect(ip, port).await?;
                    let result = socket.discover(ssrc).await;
                    result.and_then(|result| VoiceUdp::new(socket).map(|socket| (socket, result)))
                }));

                self.state = VoiceClientState::UdpHandshaking;
                self.session = Some(Session {
                    ip,
                    port,
                    secret_key: None,
                    ssrc,
                    mode,
                });
            }
            OpCode::Resumed => {
                debug!("received resumed event");

                self.session.as_ref().expect("unexpected logic");
                todo!("connect");
            }
            OpCode::HeartbeatAck => {
                let mut unwanted = true;
                if let Some(hbr) = self.heartbeater.as_mut() {
                    if hbr.has_sent() {
                        unwanted = false;
                        hbr.acknowledged();
                        trace!(latency = ?hbr.info().recent(), "received heartbeat ack");
                    }
                }
                if unwanted {
                    warn!("received unwanted heartbeat ack");
                }
            }
            OpCode::SessionDescription => {
                let result =
                    serde_json::from_str::<MinimalEvent<self::incoming::SessionDescription>>(event)
                        .map_err(|source| VoiceClientError {
                            kind: VoiceClientErrorType::Deserializing {
                                event: event.to_owned(),
                            },
                            source: Some(Box::new(source)),
                        })?;

                println!("{:?}", result.data);
            }
            _ => {
                trace!("received opcode: {opcode:?}");
            }
        }

        Ok(())
    }
}

const ABNORMAL_CLOSE: CloseFrame = CloseFrame::new(1006, "");

impl futures::Stream for VoiceClient {
    type Item = Result<TwilightMessage, VoiceClientError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let message = loop {
            trace!("self.poll_gateway_connect - start");
            match ready!(self.poll_gateway_connect(cx)) {
                Ok(false) => return Poll::Ready(None),
                Ok(true) => {}
                Err(source) => {
                    return Poll::Ready(Some(Err(VoiceClientError {
                        kind: VoiceClientErrorType::Reconnect,
                        source: Some(Box::new(source)),
                    })));
                }
            }

            trace!("self.poll_udp_connect - start");
            ready!(self.poll_udp_connect(cx)).map_err(|source| VoiceClientError {
                kind: VoiceClientErrorType::Handshaking,
                source: Some(Box::new(source)),
            })?;

            trace!("self.poll_send - start");
            if ready!(self.poll_send(cx)).is_err() {
                self.close_inner(CloseInitiator::Transport);
                self.connection = None;

                return Poll::Ready(Some(Ok(TwilightMessage::Close(Some(ABNORMAL_CLOSE)))));
            }

            let connection = self
                .connection
                .as_mut()
                .expect("should be connected to the gateway");

            trace!("self.connection.poll_next - start");
            let message = ready!(Pin::new(connection).poll_next(cx));
            match message {
                Some(Ok(message)) => {
                    if message.is_close() {
                        let (code, reason) = message.as_close().expect("unexpected logic");
                        let frame = (code != WsCloseCode::NO_STATUS_RECEIVED).then(|| CloseFrame {
                            code: code.into(),
                            reason: Cow::Owned(reason.to_string()),
                        });
                        break TwilightMessage::Close(frame);
                    } else if message.is_text() {
                        break TwilightMessage::Text(
                            message.as_text().expect("unexpected logic").to_owned(),
                        );
                    }
                }
                Some(Err(_)) if self.state.is_disconnected() => {}
                Some(Err(_)) => {
                    self.close_inner(CloseInitiator::Transport);
                    return Poll::Ready(Some(Ok(TwilightMessage::Close(Some(ABNORMAL_CLOSE)))));
                }
                None => {
                    let connection = self.connection.as_mut().expect("unexpected logic");
                    _ = ready!(Pin::new(connection).poll_close(cx));
                    debug!("WebSocket connection closed");

                    if !self.state.is_disconnected() {
                        self.close_inner(CloseInitiator::Transport);
                    }
                    self.connection = None;
                }
            }
        };

        match message {
            TwilightMessage::Close(frame) => {
                debug!(?frame, "received WebSocket close message");
                if !self.state.is_disconnected() {
                    self.close_inner(CloseInitiator::Gateway(frame.as_ref().map(|f| f.code)));
                }
                Poll::Ready(Some(Ok(TwilightMessage::Close(frame))))
            }
            TwilightMessage::Text(event) => {
                self.process_event(&event)?;
                Poll::Ready(Some(Ok(TwilightMessage::Text(event))))
            }
        }
    }
}
