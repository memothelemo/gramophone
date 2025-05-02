use serde::{Deserialize, Serialize};
use twilight_model::id::{Id, marker::UserMarker};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct ClientDisconnect {
    pub user_id: Id<UserMarker>,
}
