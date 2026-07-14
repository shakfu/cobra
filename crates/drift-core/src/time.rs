//! Simulation clock.
//!
//! Time is discrete: the world advances one [`Tick`] at a time. There is no
//! wall-clock anywhere in the simulation — a tick is the only notion of "when".

use serde::{Deserialize, Serialize};

/// Monotonic simulation tick counter.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Tick(pub u64);

impl Tick {
    pub const ZERO: Tick = Tick(0);

    #[inline]
    pub fn next(self) -> Tick {
        Tick(self.0 + 1)
    }

    #[inline]
    pub fn get(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_advance() {
        let t = Tick::ZERO;
        assert_eq!(t.get(), 0);
        assert_eq!(t.next().get(), 1);
        assert_eq!(t.next().next(), Tick(2));
    }
}
