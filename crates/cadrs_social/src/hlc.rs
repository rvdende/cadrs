//! A hybrid logical clock: wall time in milliseconds plus a counter.
//!
//! No clock can be proven across peers, so a timestamp is a claim. What the clock does give: a
//! peer's stamps only ever increase, a stamp is always later than every stamp the peer has seen,
//! and it stays close to real time. Stamps further ahead of our own wall clock than
//! [`Clock::max_ahead`] are refused, so nobody can stamp an entry into the far future to sort it
//! last.

use serde::{Deserialize, Serialize};

/// A moment: milliseconds since the Unix epoch, then a counter for stamps within one millisecond.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct Timestamp {
    pub millis: u64,
    pub counter: u32,
}

impl Timestamp {
    pub const fn new(millis: u64, counter: u32) -> Self {
        Self { millis, counter }
    }

    /// The very next stamp.
    pub fn next(self) -> Self {
        match self.counter.checked_add(1) {
            Some(counter) => Self::new(self.millis, counter),
            None => Self::new(self.millis + 1, 0),
        }
    }
}

/// A stamp too far ahead of our wall clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooFarAhead {
    pub stamp: Timestamp,
    pub wall: u64,
}

/// One peer's clock.
#[derive(Clone, Debug)]
pub struct Clock {
    last: Timestamp,
    /// How far ahead of our wall clock a stamp we accept may be, in milliseconds.
    pub max_ahead: u64,
}

impl Default for Clock {
    fn default() -> Self {
        Self {
            last: Timestamp::default(),
            max_ahead: 60_000,
        }
    }
}

impl Clock {
    /// The last stamp this clock made or saw.
    pub fn last(&self) -> Timestamp {
        self.last
    }

    /// A new stamp at wall time `wall` (milliseconds): later than every stamp before it.
    pub fn stamp(&mut self, wall: u64) -> Timestamp {
        self.last = if wall > self.last.millis {
            Timestamp::new(wall, 0)
        } else {
            self.last.next()
        };
        self.last
    }

    /// Takes in a stamp from another peer, so our next stamp comes after it.
    pub fn observe(&mut self, stamp: Timestamp, wall: u64) -> Result<(), TooFarAhead> {
        self.check(stamp, wall)?;
        self.last = self.last.max(stamp);
        Ok(())
    }

    /// Whether a stamp is near enough to our wall time to accept.
    pub fn check(&self, stamp: Timestamp, wall: u64) -> Result<(), TooFarAhead> {
        if stamp.millis > wall.saturating_add(self.max_ahead) {
            Err(TooFarAhead { stamp, wall })
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_increase_even_when_the_wall_clock_stands_still_or_goes_back() {
        let mut clock = Clock::default();
        let a = clock.stamp(1_000);
        let b = clock.stamp(1_000);
        let c = clock.stamp(900);
        let d = clock.stamp(2_000);
        assert!(a < b && b < c && c < d);
        assert_eq!(d, Timestamp::new(2_000, 0));
    }

    #[test]
    fn a_stamp_comes_after_what_was_observed() {
        let mut clock = Clock::default();
        clock.observe(Timestamp::new(5_000, 3), 4_990).unwrap();
        assert_eq!(clock.stamp(4_990), Timestamp::new(5_000, 4));
    }

    #[test]
    fn stamps_from_the_far_future_are_refused() {
        let mut clock = Clock::default();
        assert!(clock.observe(Timestamp::new(200_000, 0), 100_000).is_err());
        assert_eq!(clock.last(), Timestamp::default());
        assert!(clock.observe(Timestamp::new(160_000, 0), 100_000).is_ok());
    }
}
