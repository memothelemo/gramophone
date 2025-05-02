pub mod u64_from_f64 {
    use serde::{Deserializer, Serializer};

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = u64;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("u64 or f64")
            }

            fn visit_f32<E>(self, v: f32) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_f64(v as f64)
            }

            fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                (v as u64)
                    .try_into()
                    .map_err(|_| E::custom("f32 value is out of range for u64"))
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(v)
            }
        }

        deserializer.deserialize_any(Visitor)
    }

    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(value)
    }
}

pub mod ip_string {
    use serde::{Deserializer, Serializer};
    use std::{net::IpAddr, str::FromStr};

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<IpAddr, D::Error> {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
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
