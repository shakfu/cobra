//! The shared per-tick stepping contract.
//!
//! Both the economy world and the (stubbed) combat model advance in discrete
//! ticks against the same [`SimContext`], which carries the current tick and the
//! deterministic RNG. Keeping this trait in `core` lets higher layers drive any
//! number of subsystems uniformly without depending on their internals.

use crate::rng::DetRng;
use crate::time::Tick;

/// Per-tick context threaded to every subsystem. The RNG is borrowed mutably so
/// all randomness flows from the single seeded stream (preserving determinism).
pub struct SimContext<'a> {
    pub tick: Tick,
    pub rng: &'a mut DetRng,
}

impl<'a> SimContext<'a> {
    pub fn new(tick: Tick, rng: &'a mut DetRng) -> Self {
        Self { tick, rng }
    }
}

/// Something that advances by exactly one simulation tick.
pub trait Step {
    fn step(&mut self, ctx: &mut SimContext);
}
