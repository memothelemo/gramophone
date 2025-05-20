use crate::util::{Receiver, Sender};

use gramophone_types::payload::speaking::SpeakingFlags;
use twilight_model::gateway::CloseFrame;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum VoiceClientCommand {
    Speaking(SpeakingFlags),
}

#[derive(Debug)]
pub struct MessageChannel {
    pub(crate) close_tx: Sender<CloseFrame<'static>>,
    pub(crate) close_rx: Receiver<CloseFrame<'static>>,

    pub(crate) command_tx: Sender<VoiceClientCommand>,
    pub(crate) command_rx: Receiver<VoiceClientCommand>,
}

impl MessageChannel {
    /// Initialize a new [message channel].
    ///
    /// [message channel]: MessageChannel
    #[must_use]
    pub fn new() -> Self {
        let (close_tx, close_rx) = Receiver::new();
        let (command_tx, command_rx) = Receiver::new();

        Self {
            close_tx,
            close_rx,

            command_tx,
            command_rx,
        }
    }

    #[must_use]
    pub fn sender(&self) -> VoiceClientSender {
        VoiceClientSender {
            close_tx: self.close_tx.clone(),
            command_tx: self.command_tx.clone(),
        }
    }
}

/// A channel to send messages to the [`VoiceClient`] to the Discord Voice gateway.
///
/// [`VoiceClient`]: super::VoiceClient
#[derive(Debug, Clone)]
pub struct VoiceClientSender {
    close_tx: Sender<CloseFrame<'static>>,
    command_tx: Sender<VoiceClientCommand>,
}

impl VoiceClientSender {
    /// Whether the channel is closed.
    ///
    /// The channel will be closed if the associated [voice client] has been dropped.
    ///
    /// [voice client]: super::VoiceClient
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.command_tx.is_closed()
    }

    /// Sends a WebSocket close frame to the associated pvoice client].
    ///
    /// [voice client]: super::VoiceClient
    pub fn close(&self, frame: CloseFrame<'static>) -> Result<(), ChannelError> {
        self.close_tx.send(frame).map_err(|source| ChannelError {
            kind: ChannelErrorType::Closed,
            source: Some(Box::new(source)),
        })
    }

    /// Sets speaking mode to the associated [voice client].
    pub fn speaking(&self, flags: SpeakingFlags) -> Result<(), ChannelError> {
        self.send(VoiceClientCommand::Speaking(flags))
    }

    /// Sends to a command to the associated [voice client].
    ///
    /// [voice client]: super::VoiceClient
    pub(crate) fn send(&self, command: VoiceClientCommand) -> Result<(), ChannelError> {
        self.command_tx
            .send(command)
            .map_err(|source| ChannelError {
                kind: ChannelErrorType::Closed,
                source: Some(Box::new(source)),
            })
    }
}

#[derive(Debug)]
pub struct ChannelError {
    pub(crate) kind: ChannelErrorType,
    pub(crate) source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl std::fmt::Display for ChannelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            ChannelErrorType::Closed => f.write_str("tried sending over a closed channel"),
            ChannelErrorType::Serialize => f.write_str("could not serialize specified command"),
        }
    }
}

impl std::error::Error for ChannelError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| &**source as &(dyn std::error::Error + 'static))
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum ChannelErrorType {
    /// Tried sending over a closed channel.
    Closed,

    /// Cannot serialize command.
    Serialize,
}
