use serde::{Deserialize, Serialize};

// We need to redefine it because Hello payload in the Discord voice
// documentation is different from the same as the gateway one with
// `heartbeat_interval` being a floating value.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct Hello {
    #[serde(with = "crate::deserializers::u64_from_f64")]
    pub heartbeat_interval: u64,
}
