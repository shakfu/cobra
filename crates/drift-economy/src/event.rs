//! The simulation event log — a stream of notable happenings for observing and
//! debugging a run.
//!
//! Events are recorded deterministically by the [`World`](crate::World) as it
//! ticks (same seed => same events), kept in a bounded ring buffer, and read back
//! via `World::events()`. They are ephemeral debug output, not simulation state,
//! so they are excluded from the snapshot and never feed back into the sim.

use drift_core::Tick;
use serde::{Deserialize, Serialize};

/// Broad kind of a [`SimEvent`], for filtering and colouring in a viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventCategory {
    /// A trader won or lost a fight.
    Combat,
    /// A trader was destroyed by pirates.
    Piracy,
    /// The navy engaged pirates on patrol.
    Navy,
    /// Fleet/agent lifecycle (respawns, etc.).
    System,
}

/// One recorded happening: when, what kind, and a human-readable description.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimEvent {
    pub tick: Tick,
    pub category: EventCategory,
    pub message: String,
}
