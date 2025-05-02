use serde::{Deserialize, Serialize};
use twilight_model::id::Id;
use twilight_model::id::marker::{GuildMarker, UserMarker};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Hash, Serialize)]
pub struct Identify {
    #[serde(rename = "server_id")]
    pub guild_id: Id<GuildMarker>,
    pub user_id: Id<UserMarker>,
    pub session_id: String,
    pub token: String,
}
