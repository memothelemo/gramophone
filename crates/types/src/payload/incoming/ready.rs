use serde::{Deserialize, Serialize};
use std::net::IpAddr;

// `heartbeat_interval` is an erroneous field and should be ignored.
//
// https://discord.com/developers/docs/topics/voice-connections#establishing-a-voice-websocket-connection-example-voice-ready-payload
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Hash, Serialize)]
pub struct Ready {
    // u32 is used because webrtc-rs uses u32 for SSRC-related fields
    pub ssrc: u32,
    #[serde(with = "crate::deserializers::ip_string")]
    pub ip: IpAddr,
    pub port: u16,
    pub modes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_test::{Configure, Token};
    use std::net::Ipv4Addr;

    #[test]
    fn structure() {
        let payload = Ready {
            ssrc: 1,
            ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            port: 1234,
            modes: vec!["xsalsa20_poly1305".to_string()],
        };
        serde_test::assert_tokens(
            &payload.compact(),
            &[
                Token::Struct {
                    name: "Ready",
                    len: 4,
                },
                Token::Str("ssrc"),
                Token::U32(1),
                Token::Str("ip"),
                Token::Str("127.0.0.1"),
                Token::Str("port"),
                Token::U16(1234),
                Token::Str("modes"),
                Token::Seq { len: Some(1) },
                Token::Str("xsalsa20_poly1305"),
                Token::SeqEnd,
                Token::StructEnd,
            ],
        );
    }
}
