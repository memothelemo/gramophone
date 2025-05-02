use serde::{Deserialize, Serialize};
use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Hash, Serialize)]
pub struct Resume {
    #[serde(rename = "server_id")]
    pub guild_id: Id<GuildMarker>,
    pub session_id: String,
    pub token: String,
}
