use std::fmt::Debug;
use std::time::Duration;
use tokio::time::{Instant, Interval, MissedTickBehavior, interval_at};

pub struct Heartbeater {
    /// Whether the heartbeat sent by the client has been
    /// acknowledged by Discord
    acknowledged: bool,

    /// A list of latencies from the voice gateway connection.
    latencies: Vec<Duration>,

    /// Interval of how often the client must send heartbeats.
    interval: Interval,

    /// Indicates when heartbeat was send to Discord
    sent: Option<Instant>,
}

impl Debug for Heartbeater {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Heartbeater")
            .field("acknowledged", &self.acknowledged)
            .field("interval", &self.interval.period())
            .field("latencies", &self.latencies.len())
            .field("sent", &self.sent)
            .finish_non_exhaustive()
    }
}

impl Heartbeater {
    #[must_use]
    pub fn new(interval: Duration) -> Self {
        let mut interval = interval_at(Instant::now() + interval, interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        Self {
            acknowledged: false,
            latencies: Vec::new(),
            interval,
            sent: None,
        }
    }

    /// Gets the internal [interval object].
    ///
    /// [interval object]: Interval
    pub(crate) fn interval(&mut self) -> &mut Interval {
        &mut self.interval
    }

    /// Checks whether it is requesting for acknowledgement.
    #[must_use]
    pub(crate) fn has_sent(&self) -> bool {
        self.sent.is_some()
    }

    /// Acknowledges the heartbeat payload sent by the client.
    ///
    /// This function must be used whenever Discord acknowledges
    /// the heartbeat payload.
    pub(crate) fn acknowledged(&mut self) {
        debug_assert!(self.sent.is_some());
        self.acknowledged = true;
        self.latencies.push(self.sent.unwrap().elapsed());
        self.sent = None;
    }

    /// Gets the average latency from a list of latencies
    /// observed by the heartbeater.
    ///
    /// It will return `None` if the connection has already started.
    #[must_use]
    pub fn average_latency(&self) -> Option<Duration> {
        let (latencies, total): (&[Duration], u32) = match self.latencies.len().try_into() {
            Ok(total) => (&self.latencies, total),
            Err(..) => {
                let upper = self.latencies.len() - (u32::MAX as usize) - 1;
                (&self.latencies[upper..], u32::MAX)
            }
        };

        latencies
            .iter()
            .fold(Duration::ZERO, |acc, entry| acc + *entry)
            .checked_div(total)
    }

    /// Gets the recent latency as of calling this function.
    ///
    /// It will return `None` if the connection has already started.
    #[must_use]
    pub fn recent_latency(&self) -> Option<Duration> {
        self.latencies.last().copied()
    }

    /// Gets the list of latencies.
    ///
    /// It will return an empty list if the connection has already started.
    #[must_use]
    pub fn latencies(&self) -> &[Duration] {
        &self.latencies
    }

    /// Resets the sent and acknowledged metadata.
    ///
    /// This is used when the heartbeat payload has been sent to Discord.
    pub(crate) fn record_sent(&mut self) {
        self.acknowledged = false;
        self.sent = Some(Instant::now());
    }

    /// Whether the connection is failed or "zombied".
    #[must_use]
    pub fn is_zombied(&self) -> bool {
        self.sent.is_some() && !self.acknowledged
    }
}
