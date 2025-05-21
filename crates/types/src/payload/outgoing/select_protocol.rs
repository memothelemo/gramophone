use serde::{Deserialize, Serialize};
use std::{marker::PhantomData, net::IpAddr};

// Non-dependency approach making a builder similar to `bon`
pub struct SelectProtocolBuilder<Address = (), Port = (), Protocol = (), Mode = ()> {
    phantom: PhantomData<(Address, Port, Protocol, Mode)>,
    address: Option<IpAddr>,
    port: Option<u16>,
    protocol: Option<String>,
    mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Hash, Serialize)]
pub struct SelectProtocol {
    pub protocol: String,
    pub data: SelectProtocolData,
}

impl SelectProtocol {
    #[must_use]
    pub const fn builder() -> SelectProtocolBuilder {
        SelectProtocolBuilder::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Hash, Serialize)]
pub struct SelectProtocolData {
    #[serde(with = "crate::deserializers::ip_string")]
    pub address: IpAddr,
    pub port: u16,
    pub mode: String,
}

#[allow(private_interfaces)]
mod builder {
    use super::{SelectProtocol, SelectProtocolBuilder};
    use std::{marker::PhantomData, net::IpAddr, str::FromStr};

    pub struct WithAddr;
    pub struct WithPort;
    pub struct WithProto;
    pub struct WithMode;

    impl SelectProtocolBuilder {
        #[must_use]
        pub const fn new() -> Self {
            SelectProtocolBuilder {
                phantom: PhantomData,
                address: None,
                port: None,
                protocol: None,
                mode: None,
            }
        }
    }

    impl<Port, Proto, Mode> SelectProtocolBuilder<(), Port, Proto, Mode> {
        pub fn parse_address(
            self,
            address: impl Into<String>,
        ) -> Result<SelectProtocolBuilder<WithAddr, (), (), ()>, std::net::AddrParseError> {
            let address = IpAddr::from_str(&address.into())?;
            Ok(SelectProtocolBuilder {
                phantom: PhantomData,
                address: Some(address),
                port: self.port,
                protocol: self.protocol,
                mode: self.mode,
            })
        }

        #[must_use]
        pub fn address(self, address: IpAddr) -> SelectProtocolBuilder<WithAddr, (), (), ()> {
            SelectProtocolBuilder {
                phantom: PhantomData,
                address: Some(address),
                port: self.port,
                protocol: self.protocol,
                mode: self.mode,
            }
        }
    }

    impl<Addr, Port, Proto> SelectProtocolBuilder<Addr, Port, Proto, ()> {
        #[must_use]
        pub fn mode(
            self,
            mode: impl Into<String>,
        ) -> SelectProtocolBuilder<Addr, Port, Proto, WithMode> {
            SelectProtocolBuilder {
                phantom: PhantomData,
                address: self.address,
                port: self.port,
                protocol: self.protocol,
                mode: Some(mode.into()),
            }
        }
    }

    impl<Addr, Proto, Mode> SelectProtocolBuilder<Addr, (), Proto, Mode> {
        #[must_use]
        pub fn port(self, port: u16) -> SelectProtocolBuilder<Addr, WithPort, Proto, Mode> {
            SelectProtocolBuilder {
                phantom: PhantomData,
                address: self.address,
                port: Some(port),
                protocol: self.protocol,
                mode: self.mode,
            }
        }
    }

    impl<Addr, Port, Mode> SelectProtocolBuilder<Addr, Port, (), Mode> {
        #[must_use]
        pub fn protocol(
            self,
            protocol: impl Into<String>,
        ) -> SelectProtocolBuilder<Addr, Port, WithProto, Mode> {
            SelectProtocolBuilder {
                phantom: PhantomData,
                address: self.address,
                port: self.port,
                protocol: Some(protocol.into()),
                mode: self.mode,
            }
        }
    }

    impl SelectProtocolBuilder<WithAddr, WithPort, WithProto, WithMode> {
        #[allow(clippy::missing_panics_doc)]
        #[must_use]
        pub fn build(self) -> SelectProtocol {
            SelectProtocol {
                protocol: self.protocol.expect("protocol is required"),
                data: super::SelectProtocolData {
                    address: self.address.expect("address is required"),
                    port: self.port.expect("port is required"),
                    mode: self.mode.expect("mode is required"),
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {}
