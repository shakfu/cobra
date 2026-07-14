//! `drift-sim` — the session/driver layer.
//!
//! A [`Session`] owns a running [`World`] and is the single façade a host (the CLI,
//! a future server, in-process single-player) drives: it centralizes building a
//! world from content + a scenario, applying commands, advancing ticks, draining
//! per-tick events, and taking snapshots — so hosts don't repeat that wiring or
//! reach into `World` internals. Single-player is the N=1 case of the same façade
//! a networked server would use.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use drift_data::ScenarioDef;
use drift_economy::{
    builtin_pricing, Command, SimEvent, Snapshot, World, WorldError,
};
use drift_mods::{load_and_link, LoadError, Registry};
use thiserror::Error;

/// Errors from loading content/scenario or building a session.
#[derive(Debug, Error)]
pub enum SessionError {
    #[error("i/o error reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse scenario {path}: {source}")]
    Scenario {
        path: String,
        #[source]
        source: ron::error::SpannedError,
    },
    #[error(transparent)]
    Load(#[from] LoadError),
    #[error(transparent)]
    World(#[from] WorldError),
}

/// The set of pricing strategy names the engine can execute (for content
/// validation). Kept in one place so hosts don't rebuild it.
fn pricing_names() -> HashSet<String> {
    builtin_pricing().names().map(String::from).collect()
}

/// Load and link a registry from a mods directory.
pub fn load_registry(mods: &Path) -> Result<Arc<Registry>, SessionError> {
    Ok(Arc::new(load_and_link(mods, &pricing_names())?))
}

/// Read and parse a scenario file.
pub fn load_scenario(path: &Path) -> Result<ScenarioDef, SessionError> {
    let text = std::fs::read_to_string(path).map_err(|source| SessionError::Io {
        path: path.display().to_string(),
        source,
    })?;
    ron::from_str(&text).map_err(|source| SessionError::Scenario {
        path: path.display().to_string(),
        source,
    })
}

/// A running simulation and the façade for driving it.
pub struct Session {
    world: World,
}

impl Session {
    /// Build a session from an already-loaded registry, a scenario, and a seed.
    /// The pricing registry is resolved internally (hosts never pass it).
    pub fn new(
        registry: Arc<Registry>,
        scenario: &ScenarioDef,
        seed: u64,
    ) -> Result<Self, SessionError> {
        let world = World::new(registry, scenario, seed, &builtin_pricing())?;
        Ok(Self { world })
    }

    /// Convenience: load a registry and scenario from disk and build a session.
    /// `seed` overrides the scenario's seed when `Some`.
    pub fn load(
        mods: &Path,
        scenario: &Path,
        seed: Option<u64>,
    ) -> Result<Self, SessionError> {
        let registry = load_registry(mods)?;
        let scn = load_scenario(scenario)?;
        let seed = seed.unwrap_or(scn.seed);
        Self::new(registry, &scn, seed)
    }

    /// Advance exactly one tick and return the events emitted during it (in
    /// order). This is the per-tick event primitive hosts stream or broadcast.
    pub fn step(&mut self) -> Vec<SimEvent> {
        let now = self.world.tick_count();
        self.world.tick();
        // This tick's events are the trailing entries (logged with `tick == now`).
        let mut fresh: Vec<SimEvent> = self
            .world
            .events()
            .rev()
            .take_while(|e| e.tick == now)
            .cloned()
            .collect();
        fresh.reverse();
        fresh
    }

    /// Advance `n` ticks. Events remain queryable via [`world`](Self::world).
    pub fn run(&mut self, n: u64) {
        self.world.run(n);
    }

    /// Queue a player command for the next tick.
    pub fn queue_command(&mut self, command: Command) {
        self.world.queue_command(command);
    }

    /// A serializable snapshot of the mutable world state.
    pub fn snapshot(&self) -> Snapshot<'_> {
        self.world.snapshot()
    }

    pub fn world(&self) -> &World {
        &self.world
    }
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }
    pub fn registry(&self) -> &Registry {
        self.world.registry()
    }
    /// A cloned `Arc` handle to the shared registry (independent of `self`, so a
    /// host can read the registry while mutating the world).
    pub fn registry_arc(&self) -> Arc<Registry> {
        self.world.registry_arc()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn mods_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../mods")
    }

    fn scenario_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scenarios/frontier.ron")
    }

    #[test]
    fn load_builds_a_runnable_session() {
        let mut s = Session::load(&mods_path(), &scenario_path(), Some(1)).unwrap();
        assert_eq!(s.world().tick_count().get(), 0);
        s.run(100);
        assert_eq!(s.world().tick_count().get(), 100);
        assert!(s.registry().system_count() > 0);
    }

    #[test]
    fn step_returns_that_ticks_events_and_matches_the_log() {
        let mut s = Session::load(&mods_path(), &scenario_path(), Some(7)).unwrap();
        // Reconstruct the full log from per-tick step() results.
        let mut streamed: Vec<(u64, String)> = Vec::new();
        for _ in 0..150 {
            for e in s.step() {
                streamed.push((e.tick.get(), e.message));
            }
        }
        let full: Vec<(u64, String)> = s
            .world()
            .events()
            .map(|e| (e.tick.get(), e.message.clone()))
            .collect();
        assert!(!streamed.is_empty(), "the frontier run should emit events");
        assert_eq!(streamed, full, "step() reconstructs the full event log in order");
    }

    #[test]
    fn sessions_are_deterministic() {
        let dump = |seed| {
            let reg = load_registry(&mods_path()).unwrap();
            let scn = load_scenario(&scenario_path()).unwrap();
            let mut s = Session::new(reg, &scn, seed).unwrap();
            s.run(500);
            serde_json::to_string(&s.snapshot()).unwrap()
        };
        assert_eq!(dump(42), dump(42));
    }
}
