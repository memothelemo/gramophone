use bitflags::bitflags;
use serde::{Deserialize, Serialize};
use twilight_model::id::{Id, marker::UserMarker};

// seq field will be handled in the main crate
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Hash, Serialize)]
pub struct Speaking {
    pub user_id: Id<UserMarker>,
    pub ssrc: u32,
    pub speaking: SpeakingFlags,
}

bitflags! {
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct SpeakingFlags: u8 {
        const MICROPHONE = 1 << 0;
        const SOUNDSHARE = 1 << 1;
        const PRIORITY = 1 << 2;
    }
}

impl<'de> Deserialize<'de> for SpeakingFlags {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self::from_bits_truncate(u8::deserialize(deserializer)?))
    }
}

impl Serialize for SpeakingFlags {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u8(self.bits())
    }
}

#[cfg(test)]
mod tests {
    use super::SpeakingFlags;
    use serde::{Serialize, de::DeserializeOwned};
    use static_assertions::assert_impl_all;
    use std::{fmt::Debug, hash::Hash};

    assert_impl_all!(
        SpeakingFlags: Copy,
        Clone,
        Debug,
        DeserializeOwned,
        Eq,
        Hash,
        PartialEq,
        Send,
        Serialize,
        Sync,
    );
}
