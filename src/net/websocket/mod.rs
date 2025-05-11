use futures::{Sink, ready};
use gramophone_types::payload::{Event, VoiceGatewayEventDeserializer};
use gramophone_types::{CloseCode, OpCode};
use serde::Deserialize;
use serde_json::json;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime};
use tokio::sync::mpsc;
use tokio_websockets::{Error as WsError, Message as WsMessage};
use tracing::{debug, trace, warn};
use twilight_gateway::{CloseFrame, Message};
use twilight_model::gateway::event::GatewayEventDeserializer;

use self::error::VoiceWebSocketErrorType;
use super::internal::{ConnectionFuture, Wrapper, WsConnection as Connection};

pub mod error;
pub mod heartbeater;

pub use self::error::VoiceWebSocketError;
pub use self::heartbeater::Heartbeater;

#[derive(Debug)]
struct Pending {
    message: Option<Message>,
    is_heartbeat: bool,
}

/// This type allows to determine the cause of closure of connection.
#[derive(Debug)]
enum DisconnectCause {
    /// Gateway initiated the close.
    #[doc(hidden)]
    Gateway { code: Option<u16> },

    /// The user initiated the close.
    User(CloseFrame<'static>),

    /// Transport error initiated the close.
    #[doc(hidden)]
    Transport,
}

/// Determines the current state of [`VoiceWebSocket`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceWebSocketState {
    /// Successfully connected to the voice gateway.
    Connected,

    /// [`VoiceWebSocket`] is disconnected to the voice gateway.
    ///
    /// It may reconnect to the voice gateway if needed.
    Disconnected,

    /// [`VoiceWebSocket`] is permanently closed and should not attempt
    /// to reconnect to the voice gateway.
    Closed,
}

impl VoiceWebSocketState {
    #[must_use]
    #[inline]
    pub const fn is_closed(&self) -> bool {
        matches!(self, Self::Closed)
    }

    #[must_use]
    #[inline]
    pub const fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }
}

impl VoiceWebSocketState {
    #[must_use]
    fn from_close_code(code: Option<u16>) -> Self {
        match code.map(CloseCode::try_from) {
            Some(Ok(code)) if !code.can_reconnect() => Self::Closed,
            _ => Self::Disconnected,
        }
    }

    #[must_use]
    const fn is_disconnected(self) -> bool {
        matches!(self, Self::Disconnected { .. })
    }
}

/// This struct provides to meet the primitive requirements of handling Discord's
/// WebSocket voice gateway such as heartbeats. It can also receive incoming events
/// through [`event streaming`] inspired from [`twilight-rs`] and outgoing messages
/// with [`send(...)`] method.
///
/// However, this struct does not provide any implementation of authentication, session
/// resumption nor holding any additional useful states other than retaining connection
/// itself as this is foundation for the [`VoiceConnection`] struct and this allows
/// users to customize the behavior of this struct based on their specific requirements
/// but it has to be identified to the Discord before sending any events.
///
/// Discord will close the connection if it tries to sent events without being
/// identified to them.
///
/// # Usage
///
/// ```rs,no-run
/// # use gramophone::net::VoiceWebSocket;
/// use futures::StreamExt;
/// use gramophone::net::VoiceWebSocketEvent;
/// use tracing::warn;
///
/// let mut ws = VoiceWebSocket::new("discord.gg".to_string());
/// while let Some(event) = ws.next().await {
///     let event = match event {
///         Ok(event) => event,
///         Err(error) => {
///             warn!(?error, "got error");
///             continue;
///         },
///     };
///     ...
/// }
/// ```
///
/// [`send(...)`]: VoiceWebSocket::send
/// [`event streaming`]: futures::StreamExt
/// [`twilight-rs`]: https://github.com/twilight-rs/twilight
#[derive(Debug)]
pub struct VoiceWebSocket {
    /// WebSocket connection, which may be connected to Discord's voice gateway.
    connection: Option<Connection>,

    /// How many connections have attempted without successfully
    /// connected to the voice gateway.
    connect_attempts: u32,

    /// This value determines whether it has gracefully disconnected before. This is useful
    /// to determine the socket whether it has reconnected or not.
    gracefully_disconnected: bool,

    /// Endpoint to connect to Discord's voice gateway.
    endpoint: String,

    /// Future to establish a WebSocket connection with the voice gateway.
    future: Option<ConnectionFuture<Connection, WsError>>,

    /// This allows to keep track of heartbeats during the lifetime
    /// of the voice gateway connection.
    heartbeater: Option<Heartbeater>,

    /// Determines whether the message channels are created automatically
    /// by the [`VoiceWebSocket`] from stream polling function.
    message_channels_native: bool,

    /// Channel to send incoming messages that need to be
    /// sent to the voice gateway.
    ///
    /// It is uninitialized on purpose to plug with it created
    /// from the voice client object.
    message_tx: Option<Wrapper<mpsc::UnboundedSender<String>>>,

    /// Channel to receive incoming messages that need to be
    /// sent to the voice gateway.
    ///
    /// It is uninitialized on purpose to plug with it created
    /// from the voice client object.
    message_rx: Option<Wrapper<mpsc::UnboundedReceiver<String>>>,

    /// Pending message to be sent to the voice gateway.
    pending: Option<Pending>,

    /// Whether it is reconnected or not.
    reconnected: Option<()>,

    /// Inner value when reconnection to other endpoint is needed.
    reconnect: Option<String>,

    /// Current state of a [`VoiceWebSocket`]
    state: VoiceWebSocketState,

    /// Internal value that tells whether connecting to the
    /// WebSocket with TLS is enabled.
    use_tls: bool,
}

impl VoiceWebSocket {
    #[must_use]
    pub fn new(endpoint: String) -> Self {
        Self {
            connection: None,
            connect_attempts: 0,
            endpoint,
            gracefully_disconnected: true,
            future: None,
            heartbeater: None,
            message_channels_native: true,
            message_tx: None,
            message_rx: None,
            pending: None,
            reconnect: None,
            reconnected: None,
            state: VoiceWebSocketState::Disconnected,
            use_tls: true,
        }
    }

    /// Queues to close the WebSocket connection with the voice gateway.
    pub fn close(&mut self, frame: CloseFrame<'static>) {
        self.close_inner(DisconnectCause::User(frame));
    }

    /// Returns whether the connection is reconnected or not.
    #[must_use]
    pub const fn is_reconnected(&self) -> bool {
        self.reconnected.is_some()
    }

    /// Gets the heartbeat information of the [connection].
    ///
    /// It returns `None` if it has not connected to the voice gateway.
    ///
    /// [connection]: VoiceWebSocket
    #[must_use]
    pub fn heartbeater(&self) -> Option<&Heartbeater> {
        self.heartbeater.as_ref()
    }

    /// Initializes an [MPSC sender channel] for sending messages to the voice gateway, useful
    /// when implementing custom logic for dealing with Discord's Voice API, especially when
    /// sending WebSocket messages from a separate thread is not feasible.
    ///
    /// ```no_run
    /// # use gramophone::net::VoiceWebSocket;
    ///
    /// let mut ws = VoiceWebSocket::new("wss://example.endpoint".to_string());
    /// let sender = ws.init_message_channels();
    /// tokio::spawn(async move {
    ///     // ...
    ///     sender.send("sample txt".to_string()).unwrap();
    /// });
    /// ```
    ///
    /// # Panics
    ///
    /// This function will panic if the message channel is already initialized either cases:
    /// - When `.next(...)` from [`StreamExt::next`] is called from [`VoiceWebSocket`]
    /// - [`VoiceWebSocket::init_message_channels`] is called before.
    ///
    /// [MPSC sender channel]: mpsc::UnboundedSender
    pub fn init_message_channels(&mut self) -> mpsc::UnboundedSender<String> {
        assert!(
            self.message_tx.is_none(),
            "Already initialized message channel"
        );
        let (tx, rx) = mpsc::unbounded_channel();
        self.message_channels_native = false;
        self.message_rx = Some(Wrapper(rx));
        self.message_tx = Some(Wrapper(tx.clone()));
        tx
    }

    /// Queues to restart the WebSocket connection to the voice gateway with
    /// a new endpoint if it is `Some`.
    pub fn reconnect(&mut self, endpoint: String) {
        self.reconnect = Some(endpoint);
        self.close_inner(DisconnectCause::Transport);

        // otherwise, it will be closed and we will not receive any reconnections something
        self.gracefully_disconnected = true;
        self.pending = Some(Pending {
            message: Some(Message::Close(Some(CloseFrame::NORMAL))),
            is_heartbeat: false,
        });
    }

    // TODO: Add a Command trait or something like that.
    /// Queues to send a message to the voice gateway.
    #[allow(clippy::missing_panics_doc)]
    pub fn send<T: serde::Serialize>(&mut self, payload: &T) {
        let event = serde_json::to_string(payload).expect("should serialize");
        self.load_message_channels();
        self.message_tx
            .as_ref()
            .expect("message_tx should be initialized")
            .send(event)
            .expect("VoiceWebSocket owns message_rx");
    }

    /// Gets the current state of [`VoiceWebSocket`].
    #[must_use]
    pub fn state(&self) -> VoiceWebSocketState {
        self.state
    }
}

impl VoiceWebSocket {
    /// Initializes `message_tx` and `message_rx` if it is not defined yet.
    fn load_message_channels(&mut self) {
        if self.message_tx.is_none() || self.message_rx.is_none() {
            let (tx, rx) = mpsc::unbounded_channel();
            self.message_rx = Some(Wrapper(rx));
            self.message_tx = Some(Wrapper(tx));
        }
    }

    fn close_inner(&mut self, cause: DisconnectCause) {
        self.heartbeater = None;
        self.state = match cause {
            DisconnectCause::Transport => VoiceWebSocketState::Disconnected,
            DisconnectCause::Gateway { code } => VoiceWebSocketState::from_close_code(code),
            DisconnectCause::User(frame) => {
                let code = frame.code;
                self.pending = Some(Pending {
                    message: Some(Message::Close(Some(frame))),
                    is_heartbeat: false,
                });

                if matches!(code, 1000 | 1001) {
                    self.gracefully_disconnected = true;
                    VoiceWebSocketState::Closed
                } else {
                    VoiceWebSocketState::from_close_code(Some(code))
                }
            }
        };
    }
}

impl VoiceWebSocket {
    /// Updates the shard's internal state from a voice gateway event and returns the [`Event`].
    fn process_event(&mut self, opcode: OpCode, event: &str) -> Result<Event, VoiceWebSocketError> {
        use gramophone_types::payload::incoming::Hello;
        use serde::de::DeserializeSeed;

        match opcode {
            OpCode::Hello => {
                let result = serde_json::from_str::<MinimalEvent<Hello>>(event);
                let event = result.map_err(|source| VoiceWebSocketError {
                    kind: VoiceWebSocketErrorType::Deserializing {
                        event: event.to_owned(),
                    },
                    source: Some(Box::new(source)),
                })?;

                let interval = Duration::from_secs_f64(event.data.heartbeat_interval / 1000.);
                debug!(heartbeat_interval = ?interval, "received hello event");

                self.heartbeater = Some(Heartbeater::new(interval));
            }
            OpCode::Ready | OpCode::Resumed => {
                self.reconnected = None;
            }
            OpCode::HeartbeatAck => {
                if let Some(hbr) = self.heartbeater.as_mut() {
                    if hbr.has_sent() {
                        hbr.acknowledged();
                        trace!(latency = ?hbr.recent(), "received heartbeat ack");
                    } else {
                        warn!("received unwanted heartbeat ack");
                    }
                }
            }
            _ => {}
        }

        let Some(deserializer) = GatewayEventDeserializer::from_json(event) else {
            return Err(VoiceWebSocketError {
                kind: VoiceWebSocketErrorType::Deserializing {
                    event: event.to_owned(),
                },
                source: None,
            });
        };

        let mut json = serde_json::Deserializer::from_str(event);
        let deserializer = VoiceGatewayEventDeserializer::new(deserializer);
        deserializer
            .deserialize(&mut json)
            .map_err(|source| VoiceWebSocketError {
                kind: VoiceWebSocketErrorType::Deserializing {
                    event: event.to_owned(),
                },
                source: Some(Box::new(source)),
            })
    }

    #[allow(clippy::unused_self)]
    fn parse_opcode(&self, event: &str) -> Result<Option<OpCode>, VoiceWebSocketError> {
        let (raw_opcode, ..) = GatewayEventDeserializer::from_json(event)
            .ok_or_else(|| VoiceWebSocketError {
                kind: VoiceWebSocketErrorType::Deserializing {
                    event: event.to_owned(),
                },
                source: Some("missing opcode".into()),
            })?
            .into_parts();

        if let Some(opcode) = OpCode::from(raw_opcode) {
            Ok(Some(opcode))
        } else {
            warn!(?event, "unknown voice gateway opcode: {raw_opcode}");
            Ok(None)
        }
    }

    /// Attempts to send a pending message to the voice gateway if
    /// self.connection is `Some`.
    fn poll_send_pending(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), WsError>> {
        let Some(mut ws) = self.connection.as_mut() else {
            return Poll::Ready(Ok(()));
        };

        let Some(pending) = self.pending.as_mut() else {
            return Poll::Ready(Ok(()));
        };

        ready!(Pin::new(&mut ws).poll_ready(cx))?;

        if let Some(message) = pending.message.take() {
            use tokio_websockets::CloseCode as WsCloseCode;
            Pin::new(&mut ws).start_send(match message {
                Message::Close(frame) => WsMessage::close(
                    frame
                        .as_ref()
                        .and_then(|f| WsCloseCode::try_from(f.code).ok()),
                    frame.map(|f| f.reason).as_deref().unwrap_or_default(),
                ),
                Message::Text(str) => WsMessage::text(str),
            })?;
        }
        ready!(Pin::new(&mut ws).poll_flush(cx))?;

        if pending.is_heartbeat {
            self.heartbeater
                .as_mut()
                .expect("should be defined")
                .record_sent();
        }

        self.pending = None;
        Poll::Ready(Ok(()))
    }

    /// Attempts to send due commands to the voice gateway.
    ///
    /// # Returns
    ///
    /// * `Poll::Pending` if sending is in progress.
    /// * `Poll::Ready(Ok)` if no more pending commands remain
    /// * `Poll::Ready(Err)` if sending a command failed.
    #[tracing::instrument(skip_all, name = "ws.poll_send", level = "trace")]
    fn poll_send(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), WsError>> {
        loop {
            trace!("loop iteration started");
            ready!(self.poll_send_pending(cx))?;
            trace!("done sending pending messages");

            let should_send_heartbeat = self.heartbeater.as_mut().is_some_and(|h| {
                h.interval
                    .as_mut()
                    .expect("VoiceWebSocket initializes heartbeater")
                    .poll_tick(cx)
                    .is_ready()
            });
            trace!("done checking heartbeat status");

            // you can only send heartbeats to Discord if you identified the connection to them.
            if should_send_heartbeat {
                let heartbeater = self.heartbeater.as_mut().expect("unexpected logic");
                if heartbeater.has_sent() {
                    warn!("connection is failed or \"zombied\", closing connection");
                    self.close_inner(DisconnectCause::User(CloseFrame::RESUME));
                } else {
                    let payload = serde_json::to_string(&json!({
                        "op": OpCode::Heartbeat,
                        "d": SystemTime::UNIX_EPOCH.elapsed()
                            .unwrap_or_default()
                            .as_millis(),
                    }))
                    .expect("should not fail");

                    debug!("sending heartbeat payload");
                    self.pending = Some(Pending {
                        message: Some(Message::Text(payload)),
                        is_heartbeat: true,
                    });
                    continue;
                }
            }

            // sending any pending messages from channel.
            trace!("polling self.message_rx.recv");

            if let Poll::Ready(message) = self
                .message_rx
                .as_mut()
                .expect("unexpected logic")
                .poll_recv(cx)
            {
                let message = message.expect("VoiceWebSocket owns self.message_tx channel");
                self.pending = Some(Pending {
                    message: Some(Message::Text(message)),
                    is_heartbeat: false,
                });
                continue;
            }

            trace!("loop iteration end");
            return Poll::Ready(Ok(()));
        }
    }

    /// Attempts to connect to the voice gateway with WebSocket protocol.
    ///
    /// # Returns
    ///
    /// * `Poll::Pending` if connection is in progress
    /// * `Poll::Ready(Ok(true))` if the WebSocket connection has been successfully connected.
    /// * `Poll::Ready(Ok(false))` if the connection cannot be reconnected.
    /// * `Poll::Ready(Err)` if connecting to the voice gateway is failed.
    #[tracing::instrument(skip_all, name = "ws.poll_ws_connect", level = "trace")]
    fn poll_ws_connect(&mut self, cx: &mut Context<'_>) -> Poll<Result<bool, WsError>> {
        match self.state {
            VoiceWebSocketState::Closed => {
                ready!(self.poll_send_pending(cx))?;
                if let Some(connection) = self.connection.as_mut() {
                    _ = ready!(Pin::new(connection).poll_close(cx));
                }
                return Poll::Ready(Ok(false));
            }
            VoiceWebSocketState::Disconnected if self.connection.is_none() => {
                // Replace the old endpoint with new if needed
                if let Some(new_endpoint) = self.reconnect.take() {
                    self.endpoint = new_endpoint;
                }

                if self.future.is_none() {
                    let attempts = self.connect_attempts;
                    let protocol = if self.use_tls { "wss" } else { "ws" };
                    let url = format!(
                        "{protocol}://{}/?v={}",
                        self.endpoint,
                        gramophone_types::API_VERSION
                    );
                    debug!(?attempts, ?url, "connecting to the voice gateway");

                    let tls = tokio_websockets::Connector::new()
                        .expect("could not establish WebSocket connection with TLS layer");

                    self.future = Some(ConnectionFuture::new(async move {
                        let secs = 2u8.saturating_pow(attempts);
                        tokio::time::sleep(tokio::time::Duration::from_secs(secs.into())).await;

                        Ok(tokio_websockets::ClientBuilder::new()
                            .uri(&url)
                            .expect("URL should be valid")
                            .limits(tokio_websockets::Limits::unlimited())
                            .connector(&tls)
                            .connect()
                            .await?
                            .0)
                    }));
                }

                trace!("polling self.future");
                let result = ready!(
                    Pin::new(&mut self.future.as_mut().expect("unexpected logic").0).poll(cx)
                );

                self.future = None;
                match result {
                    Ok(connection) => {
                        self.connection = Some(connection);
                        self.connect_attempts = 0;
                        self.state = VoiceWebSocketState::Connected;
                        if self.gracefully_disconnected {
                            self.gracefully_disconnected = true;
                        } else {
                            self.reconnected = Some(());
                        }
                    }
                    Err(source) => {
                        self.state = VoiceWebSocketState::Disconnected;
                        self.connect_attempts += 1;
                        return Poll::Ready(Err(source));
                    }
                }
            }
            _ => {}
        }
        Poll::Ready(Ok(true))
    }
}

const ABNORMAL_CLOSE: CloseFrame = CloseFrame::new(1006, "");

impl futures::Stream for VoiceWebSocket {
    type Item = Result<Event, VoiceWebSocketError>;

    #[tracing::instrument(skip_all, name = "ws.poll", fields(
        endpoint = ?self.endpoint,
        latency = ?self.heartbeater.as_ref().and_then(Heartbeater::recent),
        state = ?self.state,
    ))]
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        trace!("poll next start");
        if self.message_rx.is_none() {
            self.load_message_channels();
        }

        let message = loop {
            trace!("polling self.poll_ws_connect");
            match ready!(self.poll_ws_connect(cx)) {
                Ok(false) => return Poll::Ready(None),
                Ok(true) => {}
                Err(error) => {
                    return Poll::Ready(Some(Err(VoiceWebSocketError {
                        kind: VoiceWebSocketErrorType::WebSocket,
                        source: Some(Box::new(error)),
                    })));
                }
            }

            trace!("polling self.poll_send");
            if ready!(self.poll_send(cx)).is_err() {
                self.close_inner(DisconnectCause::Transport);
                self.connection = None;
                return Poll::Ready(Some(Ok(Event::GatewayClosed(Some(ABNORMAL_CLOSE)))));
            }

            trace!("receiving websocket messages");
            match ready!(
                Pin::new(self.connection.as_mut().expect("unexpected logic")).poll_next(cx)
            ) {
                Some(Ok(message)) => {
                    use tokio_websockets::CloseCode as WsCloseCode;
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
                Some(Err(_)) if self.state.is_disconnected() => {}
                Some(Err(_)) => {
                    self.close_inner(DisconnectCause::Transport);
                    return Poll::Ready(Some(Ok(Event::GatewayClosed(Some(ABNORMAL_CLOSE)))));
                }
                None => {
                    _ = ready!(
                        Pin::new(self.connection.as_mut().expect("unexpected logic"))
                            .poll_close(cx)
                    );
                    debug!("WebSocket connection closed");

                    if !self.state.is_disconnected() {
                        self.close_inner(DisconnectCause::Transport);
                    }
                    self.connection = None;
                }
            }
        };

        match message {
            Message::Close(frame) => {
                debug!(?frame, "received WebSocket close message");
                if !self.state.is_disconnected() {
                    self.close_inner(DisconnectCause::Gateway {
                        code: frame.as_ref().map(|f| f.code),
                    });
                }
                Poll::Ready(Some(Ok(Event::GatewayClosed(frame))))
            }
            Message::Text(event) => {
                let Some(opcode) = self.parse_opcode(&event)? else {
                    return Poll::Pending;
                };
                debug!("received voice gateway event");

                let event = self.process_event(opcode, &event)?;
                Poll::Ready(Some(Ok(event)))
            }
        }
    }
}

/// Gateway event with only minimal required data.
#[derive(Deserialize)]
struct MinimalEvent<T> {
    #[serde(rename = "d")]
    pub data: T,
}
