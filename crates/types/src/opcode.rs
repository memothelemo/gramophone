use serde_repr::{Deserialize_repr, Serialize_repr};

/// Voice gateway opcodes.
#[derive(Clone, Copy, Debug, Deserialize_repr, Eq, Hash, PartialEq, Serialize_repr)]
#[non_exhaustive]
#[repr(u8)]
pub enum OpCode {
    /// Start a voice websocket connection.
    Identify = 0,
    /// Select the protocol to use.
    SelectProtocol = 1,
    /// Received to indicate completion of handshake.
    Ready = 2,
    /// Fired periodically to keep connection alive.
    Heartbeat = 3,
    /// Received to indicate session description.
    SessionDescription = 4,
    /// Sent and received to indicate speaking status.
    Speaking = 5,
    /// Received in response to a heartbeat.
    HeartbeatAck = 6,
    /// Resume a previously disconnected session.
    Resume = 7,
    /// Received after connecting, contains heartbeat interval.
    Hello = 8,
    /// Received to indicate a successful resume.
    Resumed = 9,
    /// Received to indicate one or more clients have connected
    /// to the voice channel.
    ClientConnect = 11,
    /// Received to indicate someone was disconnected.
    ClientDisconnect = 13,
}

impl OpCode {
    /// Tries to match an integer value to an opcode.
    ///
    /// Returns [`None`] if no match is found.
    #[must_use]
    pub const fn from(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Identify),
            1 => Some(Self::SelectProtocol),
            2 => Some(Self::Ready),
            3 => Some(Self::Heartbeat),
            4 => Some(Self::SessionDescription),
            5 => Some(Self::Speaking),
            6 => Some(Self::HeartbeatAck),
            7 => Some(Self::Resume),
            8 => Some(Self::Hello),
            9 => Some(Self::Resumed),
            11 => Some(Self::ClientConnect),
            13 => Some(Self::ClientDisconnect),
            _ => None,
        }
    }
}

impl From<OpCode> for u8 {
    fn from(val: OpCode) -> Self {
        val as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use static_assertions::assert_impl_all;
    use std::fmt::Debug;

    assert_impl_all!(
        OpCode: Clone,
        Copy,
        Debug,
        Deserialize<'static>,
        Eq,
        PartialEq,
        Send,
        Serialize,
        Sync,
    );
}
