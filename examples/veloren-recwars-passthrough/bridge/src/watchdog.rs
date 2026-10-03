//! Peer liveness (docs/PROTOCOL.md §4).
//!
//! The host freezes the guest's last frame when the guest goes stale, and shows `guest offline` when
//! it goes dead; the guest releases input focus and stops taking host actions. Neither side ever kills
//! the other's process: that is the player's decision (`um win kill <pid>`).

use std::time::{Duration, Instant};

/// How the peer looks right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchdogState {
    /// Traffic within the stale window.
    Ok,
    /// No traffic for a while: keep the last known state, stop trusting new events.
    Stale,
    /// No traffic for long enough that the peer should be considered gone.
    Dead,
}

/// Tracks the last time anything at all arrived from the peer.
#[derive(Debug, Clone)]
pub struct Watchdog {
    last: Instant,
    stale_after: Duration,
    dead_after: Duration,
}

impl Watchdog {
    /// New watchdog with the protocol defaults (600 ms stale, 2 s dead).
    pub fn new(stale_after_ms: f64, dead_after_ms: f64) -> Self {
        Self {
            last: Instant::now(),
            stale_after: Duration::from_secs_f64((stale_after_ms / 1000.0).max(0.001)),
            dead_after: Duration::from_secs_f64((dead_after_ms / 1000.0).max(0.001)),
        }
    }

    /// Record that the peer said something (any control line or region commit).
    pub fn beat(&mut self) {
        self.last = Instant::now();
    }

    /// Milliseconds since the last activity.
    pub fn age_ms(&self) -> f64 {
        self.last.elapsed().as_secs_f64() * 1000.0
    }

    /// The state right now.
    pub fn state(&self) -> WatchdogState {
        let age = self.last.elapsed();
        if age >= self.dead_after {
            WatchdogState::Dead
        } else if age >= self.stale_after {
            WatchdogState::Stale
        } else {
            WatchdogState::Ok
        }
    }

    /// True when the peer should be treated as gone.
    pub fn is_dead(&self) -> bool {
        self.state() == WatchdogState::Dead
    }

    /// Test helper: pretend the last activity was `ms` milliseconds ago.
    #[doc(hidden)]
    pub fn backdate_ms(&mut self, ms: u64) {
        self.last = Instant::now() - Duration::from_millis(ms);
    }
}

impl Default for Watchdog {
    fn default() -> Self {
        Self::new(600.0, 2000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_follow_the_clock() {
        let mut w = Watchdog::new(50.0, 200.0);
        assert_eq!(w.state(), WatchdogState::Ok);
        w.backdate_ms(80);
        assert_eq!(w.state(), WatchdogState::Stale);
        w.backdate_ms(500);
        assert_eq!(w.state(), WatchdogState::Dead);
        w.beat();
        assert_eq!(w.state(), WatchdogState::Ok);
    }
}
