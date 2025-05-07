use serde::{Deserialize, Serialize};

// We need to redefine it because Hello payload in the Discord voice
// documentation is different from the same as the gateway one with
// `heartbeat_interval` being a floating value.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Hello {
    pub heartbeat_interval: f64,
}
