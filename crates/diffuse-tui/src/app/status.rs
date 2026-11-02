use std::time::{Duration, Instant};

use super::AppState;

impl AppState {
    pub fn record_stream_activity(&mut self) {
        self.last_stream_activity = Some(Instant::now());
    }

    pub fn record_stream_activity_at(&mut self, now: Instant) {
        self.last_stream_activity = Some(now);
    }

    pub fn streaming_dot(&self) -> &'static str {
        self.streaming_dot_at(Instant::now())
    }

    pub fn streaming_dot_at(&self, now: Instant) -> &'static str {
        if !self.is_streaming {
            return "●";
        }
        let Some(last_activity) = self.last_stream_activity else {
            return "●";
        };
        const DELAY_THRESHOLD: Duration = Duration::from_millis(500);
        const BLINK_INTERVAL_MS: u128 = 200;

        if now.duration_since(last_activity) < DELAY_THRESHOLD {
            let phase = (now.duration_since(self.start_time).as_millis() / BLINK_INTERVAL_MS) % 2;
            if phase == 0 { "●" } else { " " }
        } else {
            "●"
        }
    }

    /// Time-to-live for status-bar feedback: ok confirmations vanish
    /// quickly, errors linger longer. Both expire.
    pub const STATUS_OK_TTL: Duration = Duration::from_secs(4);
    pub const STATUS_ERR_TTL: Duration = Duration::from_secs(10);

    pub fn status_ttl(is_ok: bool) -> Duration {
        if is_ok {
            Self::STATUS_OK_TTL
        } else {
            Self::STATUS_ERR_TTL
        }
    }

    /// Record ephemeral feedback for the secondary status bar.
    pub fn set_status_message(&mut self, msg: String, is_ok: bool) {
        self.set_status_message_at(msg, is_ok, Instant::now());
    }

    pub fn set_status_message_at(&mut self, msg: String, is_ok: bool, at: Instant) {
        self.status_message = Some((msg, is_ok, at));
    }

    /// The message as of `now`, or `None` when absent/expired (pure).
    pub fn status_message_at(&self, now: Instant) -> Option<(&str, bool)> {
        let (msg, is_ok, at) = self.status_message.as_ref()?;
        if now.duration_since(*at) >= Self::status_ttl(*is_ok) {
            return None;
        }
        Some((msg.as_str(), *is_ok))
    }

    /// The currently visible message, if any.
    pub fn visible_status_message(&self) -> Option<(&str, bool)> {
        self.status_message_at(Instant::now())
    }

    /// Drop the message when past its TTL. Returns true when cleared.
    pub fn poll_status_expiry(&mut self, now: Instant) -> bool {
        let expired = match self.status_message.as_ref() {
            Some((_, is_ok, at)) => now.duration_since(*at) >= Self::status_ttl(*is_ok),
            None => false,
        };
        if expired {
            self.status_message = None;
            return true;
        }
        false
    }
}
