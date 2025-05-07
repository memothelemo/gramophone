pub mod ip_string {
    use serde::{Deserializer, Serializer};
    use std::{net::IpAddr, str::FromStr};

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<IpAddr, D::Error> {
        struct Visitor;

        impl serde::de::Visitor<'_> for Visitor {
            type Value = IpAddr;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("ip address")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                IpAddr::from_str(v).map_err(serde::de::Error::custom)
            }
        }

        deserializer.deserialize_str(Visitor)
    }

    pub fn serialize<S: Serializer>(addr: &IpAddr, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(addr)
    }
}
