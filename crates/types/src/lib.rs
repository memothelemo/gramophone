pub mod close_code;
pub mod opcode;
pub mod payload;

pub use self::close_code::CloseCode;
pub use self::opcode::OpCode;

/// Discord voice API version that [`gramophone-types`] currently supports.
///
/// [`gramophone-types`]: crate
pub const API_VERSION: u8 = 4;

/// RTP key length used for AEAD encryption on Discord.
pub const RTP_KEY_LEN: usize = 32;

mod deserializers;
