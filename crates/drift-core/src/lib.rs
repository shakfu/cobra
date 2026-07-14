//! `drift-core` — foundational primitives shared across the simulation.
//!
//! Deliberately free of domain types (no markets, ships, or systems). It provides
//! typed ids + interning, a deterministic RNG, the discrete clock, the money type,
//! the per-tick [`Step`] contract, and the [`NamedRegistry`] plugin seam. Higher
//! crates (`drift-economy`, `drift-combat`) build domain state on top of these.

pub mod hook;
pub mod ids;
pub mod money;
pub mod rng;
pub mod step;
pub mod time;

pub use hook::{NamedRegistry, UnknownStrategy};
pub use ids::{CommodityId, Interner, RecipeId, ShipId, SystemId};
pub use money::{Money, Quantity};
pub use rng::DetRng;
pub use step::{SimContext, Step};
pub use time::Tick;
