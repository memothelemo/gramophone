pub mod speaking;
pub use self::speaking::Speaking;

pub trait Command: DeserializeOwned + Sealed {}

mod sealed {
    pub trait Sealed {}
}
use serde::de::DeserializeOwned;

pub(crate) use self::sealed::Sealed;
