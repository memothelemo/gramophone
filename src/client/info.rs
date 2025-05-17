use std::marker::PhantomData;
use twilight_model::id::Id;
use twilight_model::id::marker::{GuildMarker, UserMarker};

/// This struct holds connection parameters for the [`VoiceClient`].
///
/// You can get these parameters from [`Voice State Update`] and
/// [`Voice Server Update`] events by getting events from [`twilight_gateway::Shard`]
/// but you have to send to the shard with [`Update Voice State`] event first.
///
/// [`VoiceClient`]: super::VoiceClient
/// [`Voice State Update`]: https://discord.com/developers/docs/events/gateway-events#voice-state-update
/// [`Voice Server Update`]: https://discord.com/developers/docs/events/gateway-events#voice-server-update
/// [`Update Voice State`]: https://discord.com/developers/docs/events/gateway-events#update-voice-state
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub endpoint: String,
    pub guild_id: Id<GuildMarker>,
    pub session_id: String,
    pub token: Token,
    pub user_id: Id<UserMarker>,
}

impl ConnectionInfo {
    #[must_use]
    pub const fn builder() -> ConnectionInfoBuilder {
        ConnectionInfoBuilder::new()
    }
}

pub struct ConnectionInfoBuilder<E = (), G = (), S = (), T = (), U = ()> {
    endpoint: Option<String>,
    guild_id: Option<Id<GuildMarker>>,
    session_id: Option<String>,
    token: Option<String>,
    user_id: Option<Id<UserMarker>>,
    phantom: PhantomData<(E, G, S, T, U)>,
}

impl ConnectionInfoBuilder {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            endpoint: None,
            guild_id: None,
            session_id: None,
            token: None,
            user_id: None,
            phantom: PhantomData,
        }
    }
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

mod builder_state {
    use super::{ConnectionInfo, ConnectionInfoBuilder, Token};

    use std::marker::PhantomData;
    use std::num::NonZeroU64;
    use twilight_model::id::Id;
    use twilight_model::id::marker::{GuildMarker, UserMarker};

    pub struct WithEndpoint;
    pub struct WithGuildId;
    pub struct WithSessionId;
    pub struct WithToken;
    pub struct WithUserId;
    pub struct Optional;

    impl ConnectionInfoBuilder {
        #[must_use]
        pub fn optional(
            self,
        ) -> ConnectionInfoBuilder<Optional, Optional, Optional, Optional, Optional> {
            ConnectionInfoBuilder {
                endpoint: None,
                guild_id: None,
                session_id: None,
                token: None,
                user_id: None,
                phantom: PhantomData,
            }
        }
    }

    impl ConnectionInfoBuilder<Optional, Optional, Optional, Optional, Optional> {
        #[must_use]
        pub fn clear(self) -> Self {
            ConnectionInfoBuilder {
                endpoint: None,
                guild_id: None,
                session_id: None,
                token: None,
                user_id: None,
                phantom: PhantomData,
            }
        }

        #[must_use]
        pub fn set_endpoint(self, endpoint: impl Into<String>) -> Self {
            ConnectionInfoBuilder {
                endpoint: Some(endpoint.into()),
                ..self
            }
        }

        #[must_use]
        pub fn set_guild_id(self, guild_id: Id<GuildMarker>) -> Self {
            ConnectionInfoBuilder {
                guild_id: Some(guild_id),
                ..self
            }
        }

        #[must_use]
        pub fn set_session_id(self, session_id: impl Into<String>) -> Self {
            ConnectionInfoBuilder {
                session_id: Some(session_id.into()),
                ..self
            }
        }

        #[must_use]
        pub fn set_token(self, token: impl Into<String>) -> Self {
            ConnectionInfoBuilder {
                token: Some(token.into()),
                ..self
            }
        }

        #[must_use]
        pub fn set_user_id(self, user_id: Id<UserMarker>) -> Self {
            ConnectionInfoBuilder {
                user_id: Some(user_id),
                ..self
            }
        }

        #[must_use]
        pub fn build_optional(&self) -> Option<ConnectionInfo> {
            Some(ConnectionInfo {
                endpoint: self.endpoint.clone()?,
                guild_id: self.guild_id.clone()?,
                session_id: self.session_id.clone().unwrap(),
                token: Token::new(self.token.clone().unwrap().into_boxed_str()),
                user_id: self.user_id.clone().unwrap(),
            })
        }
    }

    impl ConnectionInfoBuilder<WithEndpoint, WithGuildId, WithSessionId, WithToken, WithUserId> {
        #[must_use]
        pub fn build(self) -> ConnectionInfo {
            ConnectionInfo {
                endpoint: self.endpoint.unwrap(),
                guild_id: self.guild_id.unwrap(),
                session_id: self.session_id.unwrap(),
                token: Token::new(self.token.unwrap().into_boxed_str()),
                user_id: self.user_id.unwrap(),
            }
        }
    }

    impl<G, S, T, U> ConnectionInfoBuilder<(), G, S, T, U> {
        #[must_use]
        pub fn endpoint(
            self,
            endpoint: impl Into<String>,
        ) -> ConnectionInfoBuilder<WithEndpoint, S, T, U> {
            ConnectionInfoBuilder {
                endpoint: Some(endpoint.into()),
                guild_id: self.guild_id,
                session_id: self.session_id,
                token: self.token,
                user_id: self.user_id,
                phantom: PhantomData,
            }
        }
    }

    impl<E, S, T, U> ConnectionInfoBuilder<E, (), S, T, U> {
        #[must_use]
        pub fn guild_id(
            self,
            guild_id: Id<GuildMarker>,
        ) -> ConnectionInfoBuilder<E, WithGuildId, S, T, U> {
            ConnectionInfoBuilder {
                endpoint: self.endpoint,
                guild_id: Some(guild_id),
                session_id: self.session_id,
                token: self.token,
                user_id: self.user_id,
                phantom: PhantomData,
            }
        }

        #[must_use]
        pub fn try_guild_id(
            self,
            guild_id: u64,
        ) -> Option<ConnectionInfoBuilder<E, WithGuildId, S, T, U>> {
            let guild_id = NonZeroU64::new(guild_id)?;
            Some(ConnectionInfoBuilder {
                endpoint: self.endpoint,
                guild_id: Some(guild_id.into()),
                session_id: self.session_id,
                token: self.token,
                user_id: self.user_id,
                phantom: PhantomData,
            })
        }
    }

    impl<E, G, T, U> ConnectionInfoBuilder<E, G, (), T, U> {
        #[must_use]
        pub fn session_id(
            self,
            session: impl Into<String>,
        ) -> ConnectionInfoBuilder<E, G, WithSessionId, T, U> {
            ConnectionInfoBuilder {
                endpoint: self.endpoint,
                guild_id: self.guild_id,
                session_id: Some(session.into()),
                token: self.token,
                user_id: self.user_id,
                phantom: PhantomData,
            }
        }
    }

    impl<E, G, S, U> ConnectionInfoBuilder<E, G, S, (), U> {
        #[must_use]
        pub fn token(
            self,
            token: impl Into<String>,
        ) -> ConnectionInfoBuilder<E, G, S, WithToken, U> {
            ConnectionInfoBuilder {
                endpoint: self.endpoint,
                guild_id: self.guild_id,
                session_id: self.session_id,
                token: Some(token.into()),
                user_id: self.user_id,
                phantom: PhantomData,
            }
        }
    }

    impl<E, G, S, T> ConnectionInfoBuilder<E, G, S, T, ()> {
        #[must_use]
        pub fn user_id(
            self,
            user_id: Id<GuildMarker>,
        ) -> ConnectionInfoBuilder<E, G, S, T, WithUserId> {
            ConnectionInfoBuilder {
                endpoint: self.endpoint,
                guild_id: Some(user_id),
                session_id: self.session_id,
                token: self.token,
                user_id: self.user_id,
                phantom: PhantomData,
            }
        }

        #[must_use]
        pub fn try_user_id(
            self,
            user_id: u64,
        ) -> Option<ConnectionInfoBuilder<E, G, S, T, WithUserId>> {
            let user_id = NonZeroU64::new(user_id)?;
            Some(ConnectionInfoBuilder {
                endpoint: self.endpoint,
                guild_id: self.guild_id,
                session_id: self.session_id,
                token: self.token,
                user_id: Some(user_id.into()),
                phantom: PhantomData,
            })
        }
    }
}
