use std::pin::Pin;
use std::task::{Context, Poll, ready};

use gramophone_types::payload::Event as ModelEvent;
use gramophone_types::payload::incoming::{
    ClientConnect, ClientDisconnect, Hello, Ready, SessionDescription,
};
use gramophone_types::payload::{Speaking, VoiceGatewayEventDeserializer};

use futures::Stream;
use serde::de::DeserializeSeed;
use twilight_model::gateway::CloseFrame;
use twilight_model::gateway::event::GatewayEventDeserializer;

use self::private::NextEvent;
use super::error::{VoiceClientError, VoiceClientErrorType};
use super::{AudioTransport, Message, VoiceClient};

/// Unlike the [`Event`] type in [`gramophone_types`], this type is
/// gateway-specific and it includes the `Connected` variant that receives
/// an [`AudioTransport`] type.
///
/// [`Event`]: gramophone_types::payload::Event
#[derive(Debug)]
pub enum Event {
    Connected(AudioTransport),
    ClientConnect(ClientConnect),
    ClientDisconnect(ClientDisconnect),
    GatewayClosed(Option<CloseFrame<'static>>),
    GatewayHeartbeatAck,
    GatewayHello(Hello),
    GatewayReady(Ready),
    GatewayResumed,
    SessionDescription(SessionDescription),
    Speaking(Speaking),
}

impl VoiceClient {
    /// Consumes and returns the next [`Event`] in the stream or `None`
    /// if the client is closed.
    #[must_use]
    pub fn next_event(&mut self) -> NextEvent<'_> {
        NextEvent::new(self)
    }
}

impl Future for NextEvent<'_> {
    type Output = Option<Result<Event, VoiceClientError>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match ready!(Pin::new(&mut self.client).poll_next(cx)) {
            Some(Ok(item)) => match item {
                Message::Connected(reconnected) => {
                    Poll::Ready(Some(Ok(Event::Connected(reconnected))))
                }
                Message::Close(frame) => Poll::Ready(Some(Ok(Event::GatewayClosed(frame)))),
                Message::Text(event) => {
                    let deserializer = GatewayEventDeserializer::from_json(&event);
                    let deserializer = deserializer.ok_or_else(|| VoiceClientError {
                        kind: VoiceClientErrorType::Deserializing {
                            event: event.clone(),
                        },
                        source: None,
                    })?;

                    let mut json = serde_json::Deserializer::from_str(&event);
                    let deserializer = VoiceGatewayEventDeserializer::new(deserializer);
                    let result = deserializer.deserialize(&mut json);
                    let event = result.map_err(|source| VoiceClientError {
                        kind: VoiceClientErrorType::Deserializing {
                            event: event.clone(),
                        },
                        source: Some(Box::new(source)),
                    })?;

                    Poll::Ready(Some(Ok(match event {
                        ModelEvent::ClientConnect(n) => Event::ClientConnect(n),
                        ModelEvent::ClientDisconnect(n) => Event::ClientDisconnect(n),
                        ModelEvent::GatewayClosed(n) => Event::GatewayClosed(n),
                        ModelEvent::GatewayHeartbeatAck => Event::GatewayHeartbeatAck,
                        ModelEvent::GatewayHello(n) => Event::GatewayHello(n),
                        ModelEvent::GatewayReady(n) => Event::GatewayReady(n),
                        ModelEvent::GatewayResumed => Event::GatewayResumed,
                        ModelEvent::SessionDescription(n) => Event::SessionDescription(n),
                        ModelEvent::Speaking(n) => Event::Speaking(n),
                    })))
                }
            },
            Some(Err(error)) => Poll::Ready(Some(Err(error))),
            None => Poll::Ready(None),
        }
    }
}

mod private {
    use crate::client::VoiceClient;

    pub struct NextEvent<'a> {
        pub(crate) client: &'a mut VoiceClient,
    }

    impl<'a> NextEvent<'a> {
        pub fn new(client: &'a mut VoiceClient) -> Self {
            Self { client }
        }
    }
}
