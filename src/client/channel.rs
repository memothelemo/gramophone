use std::error::Error;
use std::fmt::Display;
use tokio::sync::{mpsc, oneshot};

use super::error::{SetSpeakingError, SetSpeakingErrorType};
use crate::net::internal::MpscWrapper;

/// A channel between the [voice client] and the user for sending
/// outcoming (maybe valid) [Opus packets].
///
/// [voice client]: super::VoiceClient
/// [Opus packets]: https://en.wikipedia.org/wiki/Opus_(audio_format)
#[derive(Debug)]
pub struct MessageChannel {
    pub(crate) opus_rx: MpscWrapper<mpsc::UnboundedReceiver<Vec<u8>>>,
    pub(crate) opus_tx: MpscWrapper<mpsc::UnboundedSender<Vec<u8>>>,

    pub(crate) command_tx: MpscWrapper<mpsc::UnboundedSender<CommandMessage>>,
    pub(crate) command_rx: MpscWrapper<mpsc::UnboundedReceiver<CommandMessage>>,
}

#[derive(Debug)]
pub(crate) enum CommandMessage {
    /// Waits until all packets are properly.
    WaitUntilFlushed(oneshot::Sender<()>),
    Speaking(bool),
}

impl MessageChannel {
    /// Initialize a new [packet channel].
    ///
    /// [packet channel]: PacketChannel
    #[must_use]
    pub fn new() -> Self {
        let (opus_tx, opus_rx) = mpsc::unbounded_channel();
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        Self {
            opus_tx: MpscWrapper(opus_tx),
            opus_rx: MpscWrapper(opus_rx),
            command_rx: MpscWrapper(command_rx),
            command_tx: MpscWrapper(command_tx),
        }
    }

    #[must_use]
    pub fn sender(&self) -> VoiceClientSender {
        VoiceClientSender {
            opus_tx: self.opus_tx.clone(),
            command_tx: self.command_tx.clone(),
        }
    }
}

/// A channel to send commands or (maybe valid) [Opus packets] to the [voice client].
///
/// [voice client]: super::VoiceClient
/// [Opus packets]: https://en.wikipedia.org/wiki/Opus_(audio_format)
#[derive(Debug, Clone)]
pub struct VoiceClientSender {
    opus_tx: mpsc::UnboundedSender<Vec<u8>>,
    command_tx: mpsc::UnboundedSender<CommandMessage>,
}

impl VoiceClientSender {
    /// Whether the channel is closed.
    ///
    /// The channel will be closed if the associated [voice client] has been dropped.
    ///
    /// [voice client]: super::VoiceClient
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.opus_tx.is_closed()
    }

    /// Waits for all packets to be successfully sent to the associated
    /// [voice client] but it locks the current thread.
    ///
    /// This is useful when using this function while running
    /// on a non-async environment.
    ///
    /// [voice client]: super::VoiceClient
    pub fn wait_until_flushed_blocking(&self) -> Result<(), ChannelError> {
        let (tx, rx) = oneshot::channel::<()>();
        self.command_tx
            .send(CommandMessage::WaitUntilFlushed(tx))
            .map_err(|source| ChannelError {
                kind: ChannelErrorType::Closed,
                source: Some(Box::new(source)),
            })?;

        rx.blocking_recv().map_err(|source| ChannelError {
            kind: ChannelErrorType::Closed,
            source: Some(Box::new(source)),
        })
    }

    /// Waits for all packets to be successfully sent to the associated [voice client].
    ///
    /// [voice client]: super::VoiceClient
    pub async fn wait_until_flushed(&self) -> Result<(), ChannelError> {
        let (tx, rx) = oneshot::channel::<()>();
        self.command_tx
            .send(CommandMessage::WaitUntilFlushed(tx))
            .map_err(|source| ChannelError {
                kind: ChannelErrorType::Closed,
                source: Some(Box::new(source)),
            })?;

        rx.await.map_err(|source| ChannelError {
            kind: ChannelErrorType::Closed,
            source: Some(Box::new(source)),
        })
    }

    /// Queues to send current speaking status to the associated [voice client].
    ///
    /// [voice client]: super::VoiceClient
    pub fn speaking(&self, value: bool) -> Result<(), SetSpeakingError> {
        self.command_tx
            .send(CommandMessage::Speaking(value))
            .map_err(|source| SetSpeakingError {
                kind: SetSpeakingErrorType::Closed,
                source: Some(Box::new(source)),
            })
    }

    /// Sends a (maybe valid) [Opus packet] to the associated [voice client].
    ///
    /// [voice client]: super::VoiceClient
    /// [Opus packet]: https://en.wikipedia.org/wiki/Opus_(audio_format)
    pub fn send(&self, packet: Vec<u8>) -> Result<(), ChannelError> {
        self.opus_tx.send(packet).map_err(|source| ChannelError {
            kind: ChannelErrorType::Closed,
            source: Some(Box::new(source)),
        })
    }
}

#[derive(Debug)]
pub struct ChannelError {
    pub(crate) kind: ChannelErrorType,
    pub(crate) source: Option<Box<dyn Error + Send + Sync>>,
}

impl Display for ChannelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            ChannelErrorType::Closed => f.write_str("tried sending over a closed channel"),
        }
    }
}

impl Error for ChannelError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| &**source as &(dyn Error + 'static))
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum ChannelErrorType {
    /// Tried sending over a closed channel.
    Closed,
}
