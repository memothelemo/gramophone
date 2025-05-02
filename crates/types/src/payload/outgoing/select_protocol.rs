use serde::{Deserialize, Serialize};
use std::net::IpAddr;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Hash, Serialize)]
pub struct SelectProtocol {
    pub protocol: String,
    pub data: SelectProtocolData,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Hash, Serialize)]
pub struct SelectProtocolData {
    #[serde(with = "crate::deserializers::ip_string")]
    pub address: IpAddr,
    pub port: u16,
    pub mode: String,
}
