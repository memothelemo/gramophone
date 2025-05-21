use std::time::Duration;
use std::{borrow::Cow, collections::VecDeque};
use tokio::time::{Instant, Interval, MissedTickBehavior, interval_at};

pub(crate) struct Heartbeater {
    /// Interval of how often the client must send heartbeats.
    pub(crate) interval: Interval,

    /// A list of latencies observed during the heartbeat process.
    //
    // VecDeque is faster than Vec with this case here.
    latencies: VecDeque<Duration>,

    /// Indicates when heartbeat payload was sent to Discord
    sent: Option<Instant>,
}

// Maximum length of array of latencies because we don't like to keep it
// around 10,000+ entries (it will slow down the entire program).
const LATENCIES_MAX_LEN: usize = 1000;

impl Heartbeater {
    #[must_use]
    pub fn new(interval: Duration) -> Self {
        let mut interval = interval_at(Instant::now(), interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        Self {
            interval,
            latencies: VecDeque::new(),
            sent: None,
        }
    }

    /// Gets a referenced simplified context of [`Heartbeater`]
    /// with [`HeartbeatInfo`] type.
    #[must_use]
    pub fn info(&self) -> HeartbeatInfo<'_> {
        HeartbeatInfo {
            latencies: Cow::Borrowed(&self.latencies),
        }
    }

    /// Checks whether the [heartbeater] is requesting for acknowledgement.
    ///
    /// [heartbeater]: Heartbeater
    #[must_use]
    pub const fn has_sent(&self) -> bool {
        self.sent.is_some()
    }
}

impl Heartbeater {
    /// Acknowledges the heartbeat payload sent by the client.
    ///
    /// This function must be used whenever Discord acknowledges
    /// the heartbeat payload.
    ///
    /// # Panics
    ///
    /// This function will panic if it is not called with [`record_sent(...)`] function first.
    ///
    /// [`record_sent(...)`]: Self::record_sent
    #[doc(hidden)]
    pub fn acknowledged(&mut self) {
        // Clippy: debug_assert!(self.sent.is_some())
        debug_assert!(self.sent.is_some());

        // Drain the first index of the vector if its length reaches
        // the LATENCIES_MAX_LEN value
        if self.latencies.len() == LATENCIES_MAX_LEN {
            self.latencies.pop_front();
        }

        #[allow(clippy::unwrap_used)]
        self.latencies
            .push_back(self.sent.take().unwrap().elapsed());
    }

    /// Resets the sent and acknowledged metadata.
    ///
    /// This is used when the heartbeat payload has been sent to Discord.
    #[doc(hidden)]
    pub fn record_sent(&mut self) {
        debug_assert!(self.sent.is_none());
        self.sent = Some(Instant::now());
    }
}

impl std::fmt::Debug for Heartbeater {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Heartbeater")
            .field("interval", &self.interval.period())
            .field("latencies", &self.latencies.len())
            .field("sent", &self.sent.is_some())
            .finish_non_exhaustive()
    }
}

/// Heartbeat information of the connection as of the time when
/// [`Heartbeater::info`] is being called.
pub struct HeartbeatInfo<'a> {
    /// A list of latencies observed during the heartbeat process.
    latencies: Cow<'a, VecDeque<Duration>>,
}

impl std::fmt::Debug for HeartbeatInfo<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeartbeatInfo")
            .field("samples", &self.latencies.len())
            .finish_non_exhaustive()
    }
}

impl HeartbeatInfo<'_> {
    /// It clones the entire list of latencies and returns a static reference of [`HeartbeatInfo`].
    #[must_use]
    pub fn to_owned(&self) -> HeartbeatInfo<'static> {
        HeartbeatInfo {
            latencies: Cow::Owned(self.latencies.as_ref().clone()),
        }
    }

    /// Gets the average latency from a list of latencies observed
    /// by the heartbeater.
    ///
    /// It will return `None` if the connection has already started.
    #[must_use]
    pub fn average(&self) -> Option<Duration> {
        let max_len = self.latencies.len().min(LATENCIES_MAX_LEN);
        debug_assert!(max_len <= LATENCIES_MAX_LEN);

        // Clippy:
        // The value of LATENCIES_MAX_LEN is less than a million so we're
        // not concerned about the truncation or something.
        #[allow(clippy::cast_possible_truncation)]
        self.latencies
            .iter()
            .fold(Duration::ZERO, |acc, entry| acc + *entry)
            .checked_div(max_len as u32)
    }

    /// Gets an iterator of latencies observed by the heartbeater.
    pub fn latencies(&self) -> impl Iterator<Item = &Duration> {
        self.latencies.iter()
    }

    /// Gets the recent latency as of calling this function.
    ///
    /// It will return `None` if the connection has already started.
    #[must_use]
    pub fn recent(&self) -> Option<Duration> {
        self.latencies.back().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::{Duration, Heartbeater, LATENCIES_MAX_LEN};

    #[tokio::test]
    async fn should_stay_exactly_in_latencies_max_length() {
        let mut hbr = Heartbeater::new(Duration::from_secs(1));
        hbr.latencies.push_back(Duration::from_secs(1));

        (1..LATENCIES_MAX_LEN).for_each(|_| hbr.latencies.push_back(Duration::ZERO));
        hbr.record_sent();
        hbr.acknowledged();

        assert_ne!(hbr.latencies[0], Duration::from_secs(1));
        assert_ne!(hbr.latencies[LATENCIES_MAX_LEN - 1], Duration::ZERO);
    }
}
