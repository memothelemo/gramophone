use futures::{Sink, Stream, ready};
use gramophone_types::payload::{Event, VoiceGatewayEventDeserializer};
use gramophone_types::{CloseCode, OpCode, payload};
use serde::{Deserialize, Serialize, de::DeserializeSeed};
use serde_json::json;
use std::fmt::Debug;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime};
use tokio::net::TcpStream;
use tokio_websockets::{Error as WsError, MaybeTlsStream, Message as WsMessage};
use tracing::{debug, trace, warn};
use twilight_gateway::{CloseFrame, Message};
use twilight_model::gateway::event::GatewayEventDeserializer;

pub mod error;
pub mod heartbeater;

use self::error::{VoiceWebSocketError, VoiceWebSocketErrorType};
use self::heartbeater::Heartbeater;

#[derive(Debug)]
struct Pending {
    message: Option<Message>,
    is_heartbeat: bool,
}

/// Determines the current state of [`VoiceWebSocket`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceWebSocketState {
    /// Successfully connected to the voice gateway.
    Connected,

    /// [`VoiceWebSocket`] is disconnected to the voice gateway.
    ///
    /// It may reconnect to the voice gateway if needed.
    Disconnected { attempts: u32 },

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
            _ => Self::Disconnected { attempts: 0 },
        }
    }

    #[must_use]
    const fn is_disconnected(&self) -> bool {
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
/// use gramophone::net::VoiceWebSocketEvent;
/// use futures::StreamExt;
///
/// let mut ws = VoiceWebSocket::new("discord.gg".to_string());
/// while let Some(event) = ws.next().await {
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

    /// This value determines whether it has gracefully disconnected before. This is useful
    /// to determine the socket whether it has reconnected or not.
    gracefully_disconnected: bool,

    /// WebSocket endpoint to connect to Discord's voice gateway.
    endpoint: String,

    /// Future to establish a WebSocket connection with the voice gateway.
    future: Option<ConnectionFuture>,

    /// This allows to keep track of heartbeats during the lifetime
    /// of its voice gateway connection.
    heartbeater: Option<Heartbeater>,

    /// Pending event, waiting to be sent to the user via `.next()` function.
    pending_event: Option<VoiceWebSocketEvent>,

    /// Pending message to be sent to the voice gateway.
    pending: Option<Pending>,

    /// Inner value when reconnection to other endpoint is needed.
    reconnect: Option<String>,

    /// Current state of a [`VoiceWebSocket`]
    state: VoiceWebSocketState,
}

impl VoiceWebSocket {
    #[must_use]
    pub fn new(endpoint: String) -> Self {
        Self {
            connection: None,
            gracefully_disconnected: true,
            endpoint,
            future: None,
            heartbeater: None,
            pending_event: None,
            pending: None,
            reconnect: None,
            state: VoiceWebSocketState::Disconnected { attempts: 0 },
        }
    }

    /// Gets the heartbeat information of the [socket].
    ///
    /// It returns `None` if it has not connected to the voice gateway.
    ///
    /// [socket]: VoiceWebSocket
    #[must_use]
    pub fn heartbeat(&self) -> Option<&Heartbeater> {
        self.heartbeater.as_ref()
    }

    /// Queues to restart the WebSocket connection to the voice gateway with
    /// a new endpoint if it is `Some`.
    pub fn reconnect(&mut self, endpoint: String) {
        self.reconnect = Some(endpoint);

        // otherwise, it will be closed and we will not receive any reconnections something
        self.close_inner(DisconnectCause::Transport);
        self.gracefully_disconnected = true;
        self.pending = Some(Pending {
            message: Some(Message::Close(Some(CloseFrame::NORMAL))),
            is_heartbeat: false,
        });
    }

    /// Gets the current state of [`VoiceWebSocket`].
    #[must_use]
    pub fn state(&self) -> VoiceWebSocketState {
        self.state
    }

    /// Queues to send a message to the voice gateway.
    pub fn send<T: Serialize>(&mut self, payload: &T) {
        let event = serde_json::to_string(payload).expect("should serialize");
        self.pending = Some(Pending {
            message: Some(Message::Text(event)),
            is_heartbeat: false,
        });
    }

    /// Queues to close the WebSocket connection with the voice gateway.
    pub fn close(&mut self, frame: CloseFrame<'static>) {
        self.close_inner(DisconnectCause::User(frame));
    }
}

impl VoiceWebSocket {
    fn close_inner(&mut self, cause: DisconnectCause) {
        self.heartbeater = None;
        self.state = match cause {
            DisconnectCause::Transport => VoiceWebSocketState::Disconnected { attempts: 0 },
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

    fn parse_opcode(&self, event: &str) -> Result<Option<OpCode>, VoiceWebSocketError> {
        let (raw_opcode, ..) = GatewayEventDeserializer::from_json(event)
            .ok_or_else(|| VoiceWebSocketError {
                kind: VoiceWebSocketErrorType::Deserializing {
                    event: event.to_owned(),
                },
                source: Some("missing opcode".into()),
            })?
            .into_parts();

        match OpCode::from(raw_opcode) {
            Some(opcode) => Ok(Some(opcode)),
            None => {
                warn!(?event, "unknown voice gateway opcode: {raw_opcode}");
                Ok(None)
            }
        }
    }

    /// Updates the shard's internal state from a voice gateway event and returns the [`Event`].
    fn process_event(&mut self, opcode: OpCode, event: &str) -> Result<Event, VoiceWebSocketError> {
        match opcode {
            OpCode::Hello => {
                let result = serde_json::from_str::<MinimalEvent<payload::incoming::Hello>>(event);
                let event = result.map_err(|source| VoiceWebSocketError {
                    kind: VoiceWebSocketErrorType::Deserializing {
                        event: event.to_owned(),
                    },
                    source: Some(Box::new(source)),
                })?;

                let interval = Duration::from_millis(event.data.heartbeat_interval);
                debug!(heartbeat_interval = ?interval, "received hello event");

                self.heartbeater = Some(Heartbeater::new(interval));
            }
            OpCode::HeartbeatAck => {
                if let Some(hbr) = self.heartbeater.as_mut() {
                    if hbr.has_sent() {
                        hbr.acknowledged();
                        trace!(latency = ?hbr.recent_latency(), "received heartbeat ack");
                    } else {
                        warn!("received unwanted heartbeat ack");
                    }
                }
            }
            _ => {}
        };

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
}

impl VoiceWebSocket {
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
    fn poll_send(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), WsError>> {
        loop {
            trace!("poll_send - poll_send_pending");
            ready!(self.poll_send_pending(cx))?;

            let should_send_heartbeat = self
                .heartbeater
                .as_mut()
                .is_some_and(|h| h.interval().poll_tick(cx).is_ready());

            trace!("poll_send - checking_heartbeat");

            if should_send_heartbeat {
                let hbr = self.heartbeater.as_mut().unwrap();
                if hbr.is_zombied() {
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

                    trace!("sending heartbeat");
                    self.pending = Some(Pending {
                        message: Some(Message::Text(payload)),
                        is_heartbeat: true,
                    });
                    continue;
                }
            }

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
    fn poll_ws_connect(&mut self, cx: &mut Context<'_>) -> Poll<Result<bool, WsError>> {
        match self.state {
            VoiceWebSocketState::Closed => {
                ready!(self.poll_send_pending(cx))?;
                if let Some(connection) = self.connection.as_mut() {
                    _ = ready!(Pin::new(connection).poll_close(cx));
                }
                return Poll::Ready(Ok(false));
            }
            VoiceWebSocketState::Disconnected { attempts } if self.connection.is_none() => {
                // Replace the old endpoint with new if needed
                if let Some(new_endpoint) = self.reconnect.take() {
                    self.endpoint = new_endpoint;
                }

                if self.future.is_none() {
                    let url = format!(
                        "wss://{}/?v={}",
                        self.endpoint,
                        gramophone_types::API_VERSION
                    );
                    debug!(?url, "connecting to the voice gateway");

                    let tls = tokio_websockets::Connector::new().unwrap();
                    self.future = Some(ConnectionFuture(Box::pin(async move {
                        let secs = 2u8.saturating_pow(attempts.into());
                        tokio::time::sleep(tokio::time::Duration::from_secs(secs.into())).await;

                        Ok(tokio_websockets::ClientBuilder::new()
                            .uri(&url)
                            .expect("URL should be valid")
                            .limits(tokio_websockets::Limits::unlimited())
                            .connector(&tls)
                            .connect()
                            .await?
                            .0)
                    })));
                }

                trace!("poll_ws_connect - self.future");

                let result = ready!(Pin::new(&mut self.future.as_mut().unwrap().0).poll(cx));
                self.future = None;
                match result {
                    Ok(connection) => {
                        self.connection = Some(connection);
                        self.state = VoiceWebSocketState::Connected;
                        if self.gracefully_disconnected {
                            self.gracefully_disconnected = false;
                        } else {
                            // this is to inform the user that this has been reconnected and
                            // their session needs to be resumed.
                            self.pending_event = Some(VoiceWebSocketEvent::Reconnected);
                        }
                    }
                    Err(source) => {
                        self.state = VoiceWebSocketState::Disconnected {
                            attempts: attempts + 1,
                        };
                        return Poll::Ready(Err(source));
                    }
                }
            }
            _ => {}
        }
        Poll::Ready(Ok(true))
    }
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

/// The resulting value of `.next()` function in [`VoiceWebSocket`].
///
/// It determines the current state of the [voice web socket] which will
/// allow the user to add additional functionality and can be used to
/// observe actions made from this socket.
///
/// [voice web socket]: VoiceWebSocket
#[derive(Debug)]
pub enum VoiceWebSocketEvent {
    /// Received an event from the voice gateway.
    Event(Event),

    /// Successfully reconnected to the voice gateway.
    Reconnected,

    /// Got disconnected to the voice gateway.
    Disconnected(Option<CloseFrame<'static>>),
}

const ABNORMAL_CLOSE: CloseFrame = CloseFrame::new(1006, "");

impl Stream for VoiceWebSocket {
    type Item = Result<VoiceWebSocketEvent, VoiceWebSocketError>;

    #[tracing::instrument(skip_all, name = "poll", fields(
        endpoint = ?self.endpoint,
        latency = ?self.heartbeater.as_ref().and_then(|v| v.recent_latency()),
        state = ?self.state,
    ))]
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(event) = self.pending_event.take() {
            return Poll::Ready(Some(Ok(event)));
        }

        let message = loop {
            match ready!(self.poll_ws_connect(cx)) {
                Ok(false) => return Poll::Ready(None),
                Ok(true) => {}
                Err(error) => {
                    return Poll::Ready(Some(Err(VoiceWebSocketError {
                        kind: VoiceWebSocketErrorType::WebSocket,
                        source: Some(Box::new(error)),
                    })));
                }
            };

            if ready!(self.poll_send(cx)).is_err() {
                self.close_inner(DisconnectCause::Transport);
                self.connection = None;
                return Poll::Ready(Some(Ok(VoiceWebSocketEvent::Disconnected(Some(
                    ABNORMAL_CLOSE,
                )))));
            }

            match ready!(Pin::new(self.connection.as_mut().unwrap()).poll_next(cx)) {
                Some(Ok(message)) => {
                    use tokio_websockets::CloseCode as WsCloseCode;
                    if message.is_close() {
                        let (code, reason) = message.as_close().unwrap();
                        let frame = (code != WsCloseCode::NO_STATUS_RECEIVED).then(|| CloseFrame {
                            code: code.into(),
                            reason: std::borrow::Cow::Owned(reason.to_string()),
                        });
                        break Message::Close(frame);
                    } else if message.is_text() {
                        break Message::Text(message.as_text().unwrap().to_owned());
                    }
                }
                Some(Err(_)) if self.state.is_disconnected() => {}
                Some(Err(_)) => {
                    self.close_inner(DisconnectCause::Transport);
                    return Poll::Ready(Some(Ok(VoiceWebSocketEvent::Disconnected(Some(
                        ABNORMAL_CLOSE,
                    )))));
                }
                None => {
                    _ = ready!(Pin::new(self.connection.as_mut().unwrap()).poll_close(cx));
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
                Poll::Ready(Some(Ok(VoiceWebSocketEvent::Disconnected(frame))))
            }
            Message::Text(event) => {
                let Some(opcode) = self.parse_opcode(&event)? else {
                    return Poll::Pending;
                };

                let event = VoiceWebSocketEvent::Event(self.process_event(opcode, &event)?);
                if let Some(pending_event) = self.pending_event.take() {
                    self.pending_event = Some(event);
                    return Poll::Ready(Some(Ok(pending_event)));
                }

                Poll::Ready(Some(Ok(event)))
            }
        }
    }
}

/// [`tokio_websockets`] library Websocket connection.
type Connection = tokio_websockets::WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Wrapper struct around an `async fn` with a `Debug` implementation.
struct ConnectionFuture(Pin<Box<dyn Future<Output = Result<Connection, WsError>> + Send>>);

impl Debug for ConnectionFuture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ConnectionFuture")
            .field(&"<async fn>")
            .finish()
    }
}

/// Gateway event with only minimal required data.
#[derive(Deserialize)]
struct MinimalEvent<T> {
    #[serde(rename = "d")]
    pub data: T,
}
