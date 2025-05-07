use std::collections::VecDeque;
use std::time::Duration;
use tokio::time::{Instant, Interval, MissedTickBehavior, interval_at};

// VecDeque is faster than Vec with this case here.
pub struct Heartbeater {
    /// Interval of how often the client must send heartbeats.
    pub(crate) interval: Option<Interval>,

    /// A list of latencies observed during the heartbeat process.
    latencies: VecDeque<Duration>,

    /// Indicates when heartbeat payload was sent to Discord
    sent: Option<Instant>,
}

// Maximum length of array of latencies because we don't like to keep it
// around 10,000+ entries (it will slow down the entire program).
const LATENCIES_MAX_LEN: usize = 1000;

impl std::fmt::Debug for Heartbeater {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Heartbeater")
            .field(
                "interval",
                &self.interval.as_ref().map(tokio::time::Interval::period),
            )
            .field("latencies", &self.latencies.len())
            .field("sent", &self.sent.is_some())
            .finish_non_exhaustive()
    }
}

impl Heartbeater {
    #[must_use]
    pub fn new(interval: Duration) -> Self {
        let mut interval = interval_at(Instant::now(), interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        Self {
            interval: Some(interval),
            latencies: VecDeque::new(),
            sent: None,
        }
    }

    /// Clones the entire object but the interval ticker will not be functional.
    ///
    /// This function creates a new `Heartbeater` object with the same latencies as the original,
    /// but without the interval ticker. The cloned object can be used for statistical analysis.
    #[must_use]
    pub fn into_stats(&self) -> Heartbeater {
        Heartbeater {
            interval: None,
            latencies: self.latencies.clone(),
            sent: None,
        }
    }

    /// Checks whether the [heartbeater] is requesting for acknowledgement.
    ///
    /// [heartbeater]: Heartbeater
    #[must_use]
    pub const fn has_sent(&self) -> bool {
        self.sent.is_some()
    }

    /// Gets the list of latencies observed by the heartbeater.
    ///
    /// Due to limitations of [`VecDeque`], it returns an owned [`Vec`]
    /// type so expect there will be a performance regression when
    /// processing this function as it has to iterate all of the
    /// elements and returning into [`Vec`] type.
    #[must_use]
    pub fn latencies(&self) -> Vec<Duration> {
        self.latencies.iter().copied().collect::<Vec<_>>()
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

    /// Gets the recent latency as of calling this function.
    ///
    /// It will return `None` if the connection has already started.
    #[must_use]
    pub fn recent(&self) -> Option<Duration> {
        self.latencies.back().copied()
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
