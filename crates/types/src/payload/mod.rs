pub mod incoming;
pub mod outgoing;
pub mod speaking;

pub use self::speaking::Speaking;

use serde::Deserialize;
use serde::de::value::U8Deserializer;
use serde::de::{DeserializeSeed, IgnoredAny, IntoDeserializer, MapAccess, Unexpected};

use twilight_model::gateway::CloseFrame;
use twilight_model::gateway::event::GatewayEventDeserializer;

#[allow(clippy::wildcard_imports)]
use self::incoming::*;
use crate::OpCode;

/// Any type of event that a voice connection emits.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Successfully connected to the voice server.
    ///
    /// The inner value shows whether it is reconnected.
    Connected(bool),
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

pub struct VoiceGatewayEventDeserializer<'a>(GatewayEventDeserializer<'a>);

impl<'a> VoiceGatewayEventDeserializer<'a> {
    /// Create a new voice gateway deserializer from the gateway deserializer.
    #[must_use]
    pub fn new(deserializer: GatewayEventDeserializer<'a>) -> Self {
        Self(deserializer)
    }

    /// Create a deserializer with an owned event type.
    #[must_use]
    pub fn into_owned(self) -> VoiceGatewayEventDeserializer<'static> {
        VoiceGatewayEventDeserializer(self.0.into_owned())
    }
}

impl<'a> std::ops::Deref for VoiceGatewayEventDeserializer<'a> {
    type Target = GatewayEventDeserializer<'a>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(field_identifier, rename_all = "lowercase")]
enum Field {
    D,
    Op,
    Seq,
}

struct VoiceGatewayEventVisitor(u8);

impl VoiceGatewayEventVisitor {
    fn field<'de, T: Deserialize<'de>, V: MapAccess<'de>>(
        map: &mut V,
        field: Field,
    ) -> Result<T, V::Error> {
        let mut found = None;

        loop {
            match map.next_key::<Field>() {
                Ok(Some(key)) if key == field => found = Some(map.next_value()?),
                Ok(Some(_)) | Err(_) => {
                    map.next_value::<IgnoredAny>()?;
                }
                Ok(Option::None) => {
                    break;
                }
            }
        }

        found.ok_or_else(|| {
            serde::de::Error::missing_field(match field {
                Field::D => "d",
                Field::Op => "op",
                Field::Seq => "s",
            })
        })
    }

    fn ignore_all<'de, V: MapAccess<'de>>(map: &mut V) -> Result<(), V::Error> {
        while let Ok(Some(_)) | Err(_) = map.next_key::<Field>() {
            map.next_value::<IgnoredAny>()?;
        }

        Ok(())
    }
}

impl<'de> serde::de::Visitor<'de> for VoiceGatewayEventVisitor {
    type Value = Event;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("struct VoiceGatewayEvent")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        static VALID_OPCODES: &[&str] = &[
            "CLIENT_CONNECT",
            "CLIENT_DISCONNECT",
            "HEARTBEAT_ACK",
            "HELLO",
            "READY",
            "RESUMED",
            "SESSION_DESCRIPTION",
            "SPEAKING",
        ];

        let op_deser: U8Deserializer<A::Error> = self.0.into_deserializer();
        let op = OpCode::deserialize(op_deser).ok().ok_or_else(|| {
            let unexpected = Unexpected::Unsigned(u64::from(self.0));
            serde::de::Error::invalid_value(unexpected, &"an opcode")
        })?;

        Ok(match op {
            OpCode::Identify => {
                return Err(serde::de::Error::unknown_field("Identify", VALID_OPCODES));
            }
            OpCode::SelectProtocol => {
                return Err(serde::de::Error::unknown_field(
                    "SelectProtocol",
                    VALID_OPCODES,
                ));
            }
            OpCode::Ready => {
                let data = Self::field::<Ready, _>(&mut map, Field::D)?;
                Self::ignore_all(&mut map)?;
                Event::GatewayReady(data)
            }
            OpCode::Heartbeat => {
                return Err(serde::de::Error::unknown_field("Heartbeat", VALID_OPCODES));
            }
            OpCode::SessionDescription => {
                let data = Self::field::<SessionDescription, _>(&mut map, Field::D)?;
                Self::ignore_all(&mut map)?;
                Event::SessionDescription(data)
            }
            OpCode::Speaking => {
                let data = Self::field::<Speaking, _>(&mut map, Field::D)?;
                Self::ignore_all(&mut map)?;
                Event::Speaking(data)
            }
            OpCode::HeartbeatAck => {
                Self::ignore_all(&mut map)?;
                Event::GatewayHeartbeatAck
            }
            OpCode::Resume => return Err(serde::de::Error::unknown_field("Resume", VALID_OPCODES)),
            OpCode::Hello => {
                let hello = Self::field::<Hello, _>(&mut map, Field::D)?;
                Self::ignore_all(&mut map)?;
                Event::GatewayHello(hello)
            }
            OpCode::Resumed => {
                Self::ignore_all(&mut map)?;
                Event::GatewayResumed
            }
            OpCode::ClientConnect => {
                let data = Self::field::<ClientConnect, _>(&mut map, Field::D)?;
                Self::ignore_all(&mut map)?;
                Event::ClientConnect(data)
            }
            OpCode::ClientDisconnect => {
                let data = Self::field::<ClientDisconnect, _>(&mut map, Field::D)?;
                Self::ignore_all(&mut map)?;
                Event::ClientDisconnect(data)
            }
        })
    }
}

impl<'de> DeserializeSeed<'de> for VoiceGatewayEventDeserializer<'_> {
    type Value = Event;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &["op", "d", "s"];

        deserializer.deserialize_struct(
            "VoiceGatewayEvent",
            FIELDS,
            VoiceGatewayEventVisitor(self.op()),
        )
    }
}
