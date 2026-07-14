//! `drift-combat` — the 2-D space combat model (M2).
//!
//! Builds on the economy's ship defs: an [`Encounter`] holds [`Combatant`]s drawn
//! from [`drift_data::ShipDef`]s across factions and resolves a battle one
//! deterministic tick at a time. Ships target the nearest enemy, steer to
//! engagement range, and fire hitscan weapons whose accuracy falls off with
//! distance and is rolled against the shared seeded RNG. Shields absorb damage and
//! regenerate; hull depletion destroys a ship.
//!
//! The model is intentionally 2-D (matching the galaxy's coordinates) and
//! self-contained: [`Encounter`] implements [`drift_core::Step`], so it advances
//! over the same per-tick seam as the rest of the simulation.

pub mod combatant;
pub mod encounter;
pub mod math;

pub use combatant::Combatant;
pub use encounter::{Encounter, Outcome, DT};
pub use math::Vec2;
