use serde_repr::{Deserialize_repr, Serialize_repr};
use std::{error::Error, fmt::Display};

/// Voice gateway close event codes.
#[derive(Clone, Copy, Debug, Deserialize_repr, Eq, Hash, PartialEq, Serialize_repr)]
#[non_exhaustive]
#[repr(u16)]
pub enum CloseCode {
    /// An invalid opcode was sent.
    UnknownOpcode = 4001,
    /// An invalid payload was sent.
    DecodeError = 4002,
    /// A payload was sent prior to identifying.
    NotAuthenticated = 4003,
    /// An invalid token was sent when identifying.
    AuthenticationFailed = 4004,
    /// Multiple identify payloads were sent.
    AlreadyAuthenticated = 4005,
    /// The session was invalidated.
    SessionNoLongerValid = 4006,
    /// The session timed out.
    SessionTimedOut = 4009,
    /// The specified voice server was not found.
    ServerNotFound = 4011,
    /// An unknown protocol was sent.
    UnknownProtocol = 4012,
    /// Disconnected from the voice channel.
    Disconnected = 4014,
    /// The voice server crashed.
    VoiceServerCrashed = 4015,
    /// The encryption could not be recognized.
    UnknownEncryptionMode = 4016,
}

impl CloseCode {
    /// Whether this close code is one that allows to reconnect the voice connection.
    #[must_use]
    pub const fn can_reconnect(&self) -> bool {
        matches!(
            self,
            Self::UnknownOpcode
                | Self::DecodeError
                | Self::NotAuthenticated
                | Self::AlreadyAuthenticated
                | Self::VoiceServerCrashed
        )
    }
}

impl From<CloseCode> for u16 {
    fn from(val: CloseCode) -> Self {
        val as u16
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct CloseCodeConversionError {
    code: u16,
}

impl CloseCodeConversionError {
    #[must_use]
    const fn new(code: u16) -> Self {
        Self { code }
    }

    #[must_use]
    pub const fn code(&self) -> u16 {
        self.code
    }
}

impl Display for CloseCodeConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.code, f)?;
        f.write_str(" is not a valid close code")
    }
}

impl Error for CloseCodeConversionError {}

impl TryFrom<u16> for CloseCode {
    type Error = CloseCodeConversionError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        let close_code = match value {
            4001 => Self::UnknownOpcode,
            4002 => Self::DecodeError,
            4003 => Self::NotAuthenticated,
            4004 => Self::AuthenticationFailed,
            4005 => Self::AlreadyAuthenticated,
            4006 => Self::SessionNoLongerValid,
            4009 => Self::SessionTimedOut,
            4011 => Self::ServerNotFound,
            4012 => Self::UnknownProtocol,
            4014 => Self::Disconnected,
            4015 => Self::VoiceServerCrashed,
            4016 => Self::UnknownEncryptionMode,
            _ => return Err(CloseCodeConversionError::new(value)),
        };

        Ok(close_code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use static_assertions::assert_impl_all;
    use std::fmt::Debug;

    assert_impl_all!(
        CloseCode: Clone,
        Copy,
        Debug,
        Deserialize<'static>,
        Eq,
        PartialEq,
        Send,
        Serialize,
        Sync,
    );
    assert_impl_all!(CloseCodeConversionError: Debug, PartialEq, Eq, Send, Sync, Error);
}
