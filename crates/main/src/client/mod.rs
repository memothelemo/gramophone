mod heartbeater;
mod state;

pub mod channel;
pub mod error;
pub mod event;
pub mod info;
pub mod message;

pub use self::heartbeater::HeartbeatInfo;
pub use self::info::VoiceConnectionInfo;
pub use self::message::Message;
pub use self::state::VoiceClientState;

use futures::Sink;
use gramophone_types::payload::speaking::SpeakingFlags;
use gramophone_types::{OpCode, RTP_KEY_LEN};
use serde_json::json;
use std::net::{IpAddr, Ipv4Addr};
use std::pin::Pin;
use std::str::FromStr;
use std::task::{Context, Poll, ready};
use tokio::net::UdpSocket;
use tokio_websockets::{CloseCode as WsCloseCode, Error as WsError, Message as WsMessage};
use tracing::{debug, trace, warn};
use twilight_model::gateway::CloseFrame;
use twilight_model::gateway::event::GatewayEventDeserializer;

use crate::crypto::EncryptMode;
use crate::udp::error::{VoiceUdpError, VoiceUdpErrorType};
use crate::udp::ext::DiscordUdpExt;
use crate::udp::{DiscoverIpResult, VoiceUdp};
use crate::util::{ConnectionFuture, WsConnection};

use self::channel::{MessageChannel, VoiceClientCommand, VoiceClientSender};
use self::error::{VoiceClientError, VoiceClientErrorType};
use self::heartbeater::Heartbeater;

#[derive(Debug)]
pub struct VoiceClient {
    /// Channel to receive commands and messages from users
    /// towards this associated client.
    channel: MessageChannel,

    /// WebSocket connection, which may be connected to the voice gateway.
    connection: Option<WsConnection>,

    /// Future to establish a WebSocket connection to the voice gateway.
    future_ws: Option<ConnectionFuture<WsConnection, WsError>>,

    /// Future to establish a UDP socket and fetching client's UDP socket
    /// external address and port to the voice server.
    future_udp: Option<ConnectionFuture<(UdpSocket, DiscoverIpResult), VoiceUdpError>>,

    /// This allows to keep track of heartbeats during the lifetime
    /// of the voice connection.
    heartbeater: Option<Heartbeater>,

    /// Connection parameters to allow to connect to the voice gateway.
    info: VoiceConnectionInfo,

    /// Pending message to be sent to the voice gateway.
    ///
    /// The inner message may be `None` if it is already sent to the voice
    /// gateway but has not processed yet.
    pending: Option<Pending>,

    /// Pending message to be sent to the client.
    pending_message: Option<Message>,

    /// Pending voice transport info to be consumed
    pending_transport: Option<AudioTransport>,

    /// Pending UDP socket waiting to be consumed until the client receives
    /// [`SessionDescription`] event assuming if it does not have any good
    /// previous session.
    pending_udp: Option<UdpSocket>,

    /// The current session of the voice client. Most of these values are set
    /// by Discord from Ready voice gateway event.
    ///
    /// This is useful when resuming voice connection later on.
    session: Option<Session>,

    /// Current state of a [`VoiceClient`].
    ///
    /// This field allows to implement special logic cases for specific
    /// variants of the [`VoiceClientState`] to this client.
    state: VoiceClientState,

    /// Internal value that tells whether connecting to the
    /// WebSocket with TLS is enabled.
    tls_enabled: bool,
}

/// Represents the essential fields you may need when transmitting or
/// receiving voice data from or to the voice server.
///
/// This type can be retrieved with [`VoiceClient::transport`] or
/// [`VoiceClient::transport_checked`] if it is connected successfully
/// both the gateway and the UDP server.
#[derive(Debug)]
pub struct AudioTransport {
    pub ssrc: u32,
    pub udp: VoiceUdp,
}

impl VoiceClient {
    #[must_use]
    pub fn new(info: VoiceConnectionInfo) -> Self {
        Self {
            channel: MessageChannel::new(),
            connection: None,
            future_ws: None,
            future_udp: None,
            heartbeater: None,
            info,
            pending: None,
            pending_message: None,
            pending_transport: None,
            pending_udp: None,
            session: None,
            state: VoiceClientState::Disconnected { attempts: 0 },
            tls_enabled: true,
        }
    }

    pub fn transport_checked(&mut self) -> Option<AudioTransport> {
        if self.state.is_active() {
            self.pending_transport.take()
        } else {
            None
        }
    }

    /// # Panics
    ///
    /// This function will panic if it has not connected to the voice gateway
    /// and UDP server successfully or already consumed the [`VoiceServerInfo`] type.
    pub fn transport(&mut self) -> AudioTransport {
        assert!(self.state.is_active(), "not connected");
        match self.pending_transport.take() {
            Some(n) => n,
            None => panic!("already consumed audio transport"),
        }
    }

    /// Queue a WebSocket close frame and sends it to the Voice gateway.
    #[allow(clippy::missing_panics_doc)]
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
    #[allow(clippy::missing_panics_doc)]
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

/// Current session of the [`VoiceClient`] object.
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
enum Initiator {
    /// Connection failed something with WebSocket.
    Transport,

    /// Voice gateway initiated the close.
    Gateway(Option<u16>),

    /// Connection closed initiated by a user.
    Client(CloseFrame<'static>),
}

impl VoiceClient {
    /// Queues the state of the connection to be disconnected or closed
    /// depending on the cause of initiating the disconnection.
    fn disconnect(&mut self, initiator: Initiator) {
        self.heartbeater = None;
        self.pending = None;
        self.pending_udp = None;
        self.state = match initiator {
            Initiator::Transport => VoiceClientState::Closed,
            Initiator::Gateway(code) => VoiceClientState::from_close_code(code),
            Initiator::Client(frame) => {
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

        if let VoiceClientState::Closed = self.state {
            self.session = None;
        }
    }
}

#[derive(Debug)]
struct Pending {
    message: Option<WsMessage>,
    is_heartbeat: bool,
}

impl Pending {
    fn json<T: serde::Serialize>(data: &T) -> Self {
        let message = serde_json::to_string(data).expect("serialization should not fail");
        Self {
            message: Some(WsMessage::text(message)),
            is_heartbeat: false,
        }
    }

    fn heartbeat() -> Self {
        let mut payload = Self::json(&json!({
            "op": OpCode::Heartbeat,
            "d": std::time::SystemTime::UNIX_EPOCH.elapsed()
                .unwrap_or_default()
                .as_millis(),
        }));
        payload.is_heartbeat = true;
        payload
    }
}

impl VoiceClient {
    /// Attempts to connect to the voice server with UDP.
    ///
    /// It returns [`VoiceServer`] if it is successfully connected
    /// to the voice UDP socket.
    fn poll_udp_connect(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<Message>, VoiceUdpError>> {
        use gramophone_types::payload::outgoing::SelectProtocol;

        match self.state {
            VoiceClientState::Resuming if self.future_udp.is_some() => {
                let future = self.future_udp.as_mut().expect("unexpected logic");
                let session = self.session.as_ref().expect("unexpected logic");
                let result = ready!(Pin::new(&mut future.0).poll(cx));
                self.future_udp = None;

                let (socket, ..) = result?;
                debug!("reconnected to voice UDP server");

                let udp = VoiceUdp::new(socket)?;
                let server = AudioTransport {
                    ssrc: session.ssrc,
                    udp,
                };

                self.pending_transport = Some(server);
                self.state = VoiceClientState::Active;

                return Poll::Ready(Ok(Some(Message::Connected(true))));
            }
            VoiceClientState::UdpHandshaking if self.future_udp.is_some() => {
                let future = self.future_udp.as_mut().expect("unexpected logic");
                let result = ready!(Pin::new(&mut future.0).poll(cx));
                self.future_udp = None;

                let (socket, result) = result?;
                self.pending_udp = Some(socket);
                debug!("connected to voice UDP server");

                let session = self.session.as_ref().expect("unexpected logic");
                let payload = SelectProtocol::builder()
                    .address(result.address)
                    .port(result.port)
                    .mode(session.mode.as_str())
                    .protocol("udp")
                    .build();

                debug!("sending select protocol payload");
                self.future_udp = None;
                self.pending = Some(Pending::json(&json!({
                    "op": OpCode::SelectProtocol,
                    "d": payload,
                })));
            }
            _ => {}
        }

        Poll::Ready(Ok(None))
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
            VoiceClientState::Disconnected { attempts } if self.connection.is_none() => {
                #[cfg(not(test))]
                assert!(
                    self.tls_enabled,
                    "TLS MUST BE ENABLED IN NON-TESTING ENVIRONMENT!!!"
                );

                if self.future_ws.is_none() {
                    let protocol = if self.tls_enabled { "wss" } else { "ws" };
                    let url = format!(
                        "{protocol}://{}/?v={}",
                        self.info.endpoint,
                        gramophone_types::API_VERSION
                    );
                    debug!(?attempts, ?url, "connecting to the voice gateway");

                    let tls = tokio_websockets::Connector::new()
                        .expect("could not establish WebSocket connection with TLS layer");

                    self.future_ws = Some(ConnectionFuture::new(async move {
                        let secs = 2u8.saturating_pow(attempts);
                        tokio::time::sleep(tokio::time::Duration::from_secs(secs.into())).await;

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

                let future = Pin::new(&mut self.future_ws.as_mut().expect("unexpected logic").0);
                let result = ready!(future.poll(cx));
                self.future_ws = None;

                match result {
                    Ok(connection) => {
                        self.connection = Some(connection);
                        self.state = VoiceClientState::Identifying;
                    }
                    Err(source) => {
                        let attempts = attempts.wrapping_add(1);
                        self.state = VoiceClientState::Disconnected { attempts };
                        return Poll::Ready(Err(source));
                    }
                }
            }
            VoiceClientState::Closed => {
                if let Some(connection) = self.connection.as_mut() {
                    _ = ready!(Pin::new(connection).poll_close(cx));
                }
                return Poll::Ready(Ok(false));
            }
            _ => {}
        }
        Poll::Ready(Ok(true))
    }

    /// Attempts to send due commands to the voice gateway.
    fn poll_send(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), WsError>> {
        loop {
            ready!(self.poll_send_pending(cx))?;

            if !self.state.is_disconnected() {
                if let Poll::Ready(frame) = self.channel.close_rx.poll_recv(cx) {
                    let frame = frame.expect("close channel opens");
                    debug!(?frame, "received close frame from user channel");
                    self.disconnect(Initiator::Client(frame));
                    continue;
                }
            }

            // A voice gateway connection must be identified before we can start
            // sending hearbeats unlike Discord gateway.
            let should_send_heartbeat = self.heartbeater.as_mut().is_some_and(|h| {
                h.interval.poll_tick(cx).is_ready() && self.state.can_send_heartbeats()
            });

            if should_send_heartbeat {
                let heartbeater = self.heartbeater.as_mut().expect("unexpected logic");
                if heartbeater.has_sent() {
                    warn!("connection is failed or \"zombied\", closing connection");
                    self.disconnect(Initiator::Client(CloseFrame::RESUME));
                } else {
                    self.pending = Some(Pending::heartbeat());
                    continue;
                }
            }

            // Sending due commands from the user channel.
            if self.state.is_active() {
                if let Poll::Ready(command) = self.channel.command_rx.poll_recv(cx) {
                    let command = command.expect("command channel opens");
                    self.handle_user_command(command);
                    continue;
                }
            }

            return Poll::Ready(Ok(()));
        }
    }

    fn handle_user_command(&mut self, command: VoiceClientCommand) {
        use gramophone_types::payload::Speaking;

        debug!(?command, "received command from user channel");
        match command {
            VoiceClientCommand::Speaking(flags) => {
                let session = self.session.as_ref().expect("unexpected logic");
                self.pending = Some(Pending::json(&json!({
                    "op": OpCode::Speaking,
                    "d": Speaking {
                        user_id: self.info.user_id,
                        ssrc: session.ssrc,
                        speaking: flags,
                    }
                })));
            }
        }
    }

    /// Attempts to send a pending message to the voice gateway.
    ///
    /// It can only be sent if the connection is active.
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
            trace!("sent heartbeat payload");
            self.heartbeater
                .as_mut()
                .expect("heartbeater should be initialized")
                .record_sent();
        }

        self.pending = None;
        Poll::Ready(Ok(()))
    }
}

/// Gateway event with only minimal required data.
#[derive(serde::Deserialize)]
struct MinimalEvent<T> {
    #[serde(rename = "d")]
    data: T,
}

impl<'de, T: serde::Deserialize<'de>> MinimalEvent<T> {
    fn deserialize(event: &'de str) -> Result<T, VoiceClientError> {
        serde_json::from_str::<Self>(event)
            .map_err(|source| VoiceClientError {
                kind: VoiceClientErrorType::Deserializing {
                    event: event.to_owned(),
                },
                source: Some(Box::new(source)),
            })
            .map(|v| v.data)
    }
}

impl VoiceClient {
    #[allow(clippy::too_many_lines)]
    fn handle_event(&mut self, event: &str) -> Result<(), VoiceClientError> {
        use gramophone_types::payload::incoming::{Hello, Ready, SessionDescription};
        use gramophone_types::payload::outgoing::{Identify, Resume};

        let (raw_opcode, ..) = GatewayEventDeserializer::from_json(event)
            .ok_or_else(|| VoiceClientError {
                kind: VoiceClientErrorType::Deserializing {
                    event: event.to_owned(),
                },
                source: Some("missing opcode".into()),
            })?
            .into_parts();

        // Let the user handle the unknown opcodes.
        match OpCode::from(raw_opcode) {
            Some(OpCode::Hello) => {
                let event = MinimalEvent::<Hello>::deserialize(event)?;
                let interval = std::time::Duration::from_secs_f64(event.heartbeat_interval / 1000.);
                debug!(?interval, "received hello event");

                self.heartbeater = Some(Heartbeater::new(interval));

                // Resuming connection if we have a session earlier.
                if self.session.is_some() {
                    debug!("resuming session");
                    self.state = VoiceClientState::Resuming;
                    self.pending = Some(Pending::json(&json!({
                        "op": OpCode::Resume,
                        "d": Resume {
                            guild_id: self.info.guild_id,
                            session_id: self.info.session_id.clone(),
                            token: self.info.token.expose().into(),
                        }
                    })));
                } else {
                    self.state = VoiceClientState::Identifying;
                    self.pending = Some(Pending::json(&json!({
                        "op": OpCode::Identify,
                        "d": Identify {
                            guild_id: self.info.guild_id,
                            user_id: self.info.user_id,
                            session_id: self.info.session_id.clone(),
                            token: self.info.token.expose().into(),
                        }
                    })));
                }
            }
            Some(OpCode::Resumed) => {
                debug!("received resumed event");

                let Some(session) = self.session.as_ref() else {
                    warn!("no previous session found; reconnecting as new session");
                    self.close(CloseFrame::RESUME);
                    return Ok(());
                };

                let ip = session.ip;
                let port = session.port;
                self.future_udp = Some(ConnectionFuture::new(async move {
                    let socket = UdpSocket::bind("0.0.0.0:0").await;
                    let socket = socket.map_err(|source| VoiceUdpError {
                        kind: VoiceUdpErrorType::Connect,
                        source: Some(Box::new(source)),
                    })?;

                    debug!(address = ?ip, port = ?port, "reconnecting to voice UDP socket");
                    socket
                        .connect((ip, port))
                        .await
                        .map_err(|source| VoiceUdpError {
                            kind: VoiceUdpErrorType::Connect,
                            source: Some(Box::new(source)),
                        })?;

                    // We already discovered our external IP and address and sent it to Discord
                    // so we will put fake credentials because why not!
                    //
                    // Don't worry, this will be handled with `poll_udp_connect` function.
                    let result = DiscoverIpResult {
                        address: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                        port: 0,
                    };

                    Ok((socket, result))
                }));
            }
            Some(OpCode::Ready) => {
                let event = MinimalEvent::<Ready>::deserialize(event)?;
                debug!(?event.ssrc, "received ready event");

                let result = EncryptMode::negotiate(&event.modes);
                let mode = result.ok_or_else(|| VoiceClientError {
                    kind: VoiceClientErrorType::Handshaking,
                    source: Some("cannot find compatible encryption modes".into()),
                })?;

                self.session = Some(Session {
                    ip: event.ip,
                    port: event.port,
                    secret_key: None,
                    ssrc: event.ssrc,
                    mode,
                });
                self.state = VoiceClientState::UdpHandshaking;
                self.future_udp = Some(ConnectionFuture::new(async move {
                    let socket = UdpSocket::bind("0.0.0.0:0").await;
                    let socket = socket.map_err(|source| VoiceUdpError {
                        kind: VoiceUdpErrorType::Connect,
                        source: Some(Box::new(source)),
                    })?;

                    debug!(address = ?event.ip, port = ?event.port, "connecting to voice UDP socket");
                    socket
                        .connect((event.ip, event.port))
                        .await
                        .map_err(|source| VoiceUdpError {
                            kind: VoiceUdpErrorType::Connect,
                            source: Some(Box::new(source)),
                        })?;

                    let result = socket.discover(event.ssrc).await?;
                    debug!(
                        ext.address = ?result.address,
                        ext.port = ?result.port,
                        "found client's external UDP socket address and port"
                    );

                    Ok((socket, result))
                }));
            }
            Some(OpCode::HeartbeatAck) => {
                let heartbeater = self
                    .heartbeater
                    .as_mut()
                    .and_then(|h| h.has_sent().then_some(h));

                if let Some(heartbeater) = heartbeater {
                    heartbeater.acknowledged();
                    trace!(latency = ?heartbeater.info().recent(), "received heartbeat ack");
                } else {
                    warn!("received unwanted heartbeat ack");
                }
            }
            Some(OpCode::SessionDescription) => {
                let event = MinimalEvent::<SessionDescription>::deserialize(event)?;
                let session = self.session.as_mut().expect("unexpected logic");
                debug!(event.mode = ?event.mode, "received session description event");

                if event.mode != session.mode.as_str() {
                    warn!(
                        event.mode = ?event.mode,
                        session.mode = ?session.mode,
                        "unmatched encryption mode in session description to the preferred"
                    );

                    let result = EncryptMode::from_str(&event.mode);
                    session.mode = result.map_err(|_| VoiceClientError {
                        kind: VoiceClientErrorType::Handshaking,
                        source: Some(
                            format!("incompatible encryption mode: {}", event.mode).into(),
                        ),
                    })?;
                }

                if let Some(udp) = self.pending_udp.take() {
                    let udp = VoiceUdp::new(udp).map_err(|source| VoiceClientError {
                        kind: VoiceClientErrorType::Handshaking,
                        source: Some(Box::new(source)),
                    })?;

                    self.state = VoiceClientState::Active;
                    self.pending_message = Some(Message::Connected(false));
                    self.pending_transport = Some(AudioTransport {
                        ssrc: session.ssrc,
                        udp,
                    });
                } else {
                    warn!("received session description event but self.pending_udp is missing");
                }
            }
            _ => {
                trace!("received opcode: {raw_opcode}");
            }
        }

        Ok(())
    }
}

impl futures::Stream for VoiceClient {
    type Item = Result<Message, VoiceClientError>;

    #[tracing::instrument(skip_all, name = "poll", fields(
        endpoint = ?self.info.endpoint,
        state = ?self.state,
    ))]
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        const ABNORMAL_CLOSE: CloseFrame = CloseFrame::new(1006, "");

        let message = loop {
            if let Some(message) = self.pending_message.take() {
                break message;
            }

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

            let maybe_message =
                ready!(self.poll_udp_connect(cx)).map_err(|source| VoiceClientError {
                    kind: VoiceClientErrorType::Handshaking,
                    source: Some(Box::new(source)),
                })?;

            if let Some(message) = maybe_message {
                return Poll::Ready(Some(Ok(message)));
            }

            if ready!(self.poll_send(cx)).is_err() {
                self.disconnect(Initiator::Transport);
                self.connection = None;
                return Poll::Ready(Some(Ok(Message::Close(Some(ABNORMAL_CLOSE)))));
            }

            let connection = self
                .connection
                .as_mut()
                .expect("should be connected to the voice gateway");

            let message = ready!(Pin::new(connection).poll_next(cx));
            match message {
                Some(Ok(message)) => {
                    if message.is_close() {
                        let (code, reason) = message.as_close().expect("unexpected logic");
                        let frame = (code != WsCloseCode::NO_STATUS_RECEIVED).then(|| CloseFrame {
                            code: code.into(),
                            reason: std::borrow::Cow::Owned(reason.to_string()),
                        });
                        break Message::Close(frame);
                    } else if message.is_text() {
                        break Message::Text(
                            message.as_text().expect("unexpected logic").to_owned(),
                        );
                    }
                }
                Some(Err(error)) if self.state.is_disconnected() => {
                    debug!(?error, "caught WebSocket error");
                }
                Some(Err(error)) => {
                    debug!(?error, "caught WebSocket error");
                    self.disconnect(Initiator::Transport);
                    return Poll::Ready(Some(Ok(Message::Close(Some(ABNORMAL_CLOSE)))));
                }
                None => {
                    let connection = self
                        .connection
                        .as_mut()
                        .expect("should be connected to the voice gateway");

                    _ = ready!(Pin::new(connection).poll_close(cx));
                    debug!("WebSocket connection closed");

                    if !self.state.is_disconnected() {
                        self.disconnect(Initiator::Transport);
                    }
                    self.connection = None;
                }
            }
        };

        match message {
            Message::Close(frame) => {
                debug!(?frame, "received WebSocket close message");
                if !self.state.is_disconnected() {
                    self.disconnect(Initiator::Gateway(frame.as_ref().map(|f| f.code)));
                }
                Poll::Ready(Some(Ok(Message::Close(frame))))
            }
            Message::Text(event) => {
                self.handle_event(&event)?;
                Poll::Ready(Some(Ok(Message::Text(event))))
            }
            Message::Connected(..) => Poll::Ready(Some(Ok(message))),
        }
    }
}
