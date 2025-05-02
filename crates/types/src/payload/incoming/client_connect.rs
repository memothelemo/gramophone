use serde::{Deserialize, Serialize};
use twilight_model::id::{Id, marker::UserMarker};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct ClientConnect {
    /// A list of users connected to a particular voice channel.
    ///
    /// This should not be used as a notification as it may contain all of
    /// the users connected to the voice channel when the bot successfully
    /// connects to the voice gateway.
    pub user_ids: Vec<Id<UserMarker>>,
}
