#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HeartbeatAck {
    /// Previously sent nonce from Heartbeat payload.
    pub t: u128,
}
