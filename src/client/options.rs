use twilight_model::id::Id;
use twilight_model::id::marker::{GuildMarker, UserMarker};

#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub endpoint: String,
    pub guild_id: Id<GuildMarker>,
    pub session_id: String,
    pub token: Token,
    pub user_id: Id<UserMarker>,
}

/// Wrapper for an authorization token with a debug implementation
/// that redacts the string.
#[derive(Clone, Default)]
pub struct Token {
    /// Authorization token that is redacted in the Debug implementation.
    inner: Box<str>,
}

impl Token {
    /// Create a new authorization wrapper.
    #[must_use]
    pub const fn new(token: Box<str>) -> Self {
        Self { inner: token }
    }

    /// Exposes the authorization token.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.inner
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}
