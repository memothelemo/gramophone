#![forbid(unsafe_code)]
#![warn(
    clippy::pedantic,
    clippy::must_use_candidate,
    clippy::empty_enum,
    clippy::unwrap_used
)]
#![allow(
    clippy::new_without_default,
    clippy::missing_errors_doc,
    clippy::module_name_repetitions
)]

mod deserializers;

pub mod close_code;
pub mod opcode;
pub mod payload;

pub use self::close_code::CloseCode;
pub use self::opcode::OpCode;

/// Discord voice API version that Gramophone currently supports.
pub const API_VERSION: u8 = 4;
pub const RTP_KEY_LEN: usize = 32;
