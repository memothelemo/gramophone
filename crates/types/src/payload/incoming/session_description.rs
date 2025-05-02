pub use serde::{Deserialize, Serialize};

use crate::RTP_KEY_LEN;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Hash, Serialize)]
pub struct SessionDescription {
    pub mode: String,
    pub secret_key: [u8; RTP_KEY_LEN],
}
