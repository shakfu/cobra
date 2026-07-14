//! Scenario schema — parameters for a simulation run.
//!
//! A scenario is *not* content (it does not define the galaxy); it configures a
//! run over already-loaded content: the RNG seed, how long to run, and which NPC
//! traders populate the economy. Kept separate so the same galaxy can be exercised
//! by many scenarios (calm vs. shock, few vs. many traders).

use drift_core::Money;
use serde::{Deserialize, Serialize};

/// How to populate the galaxy with NPC traders at the start of a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraderSpawn {
    /// Number of NPC traders to create.
    pub count: u32,
    /// Ship id every spawned trader flies.
    pub ship: String,
    /// Starting capital, in credits, per trader.
    pub starting_capital: Money,
}

/// Piracy settings for a run. Absent means no piracy (traders are never ambushed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PiracyConfig {
    /// Ship id pirates fly.
    pub pirate_ship: String,
    /// Per-tick, per-pirate probability that a pirate present at a laden trader's
    /// destination intercepts it. More pirates at a system => more likely ambush.
    pub base_ambush_chance: f64,
    /// Maximum number of pirates that join a single ambush.
    pub max_pirates: u32,
    /// Ticks a destroyed trader is out of action before respawning.
    pub respawn_delay: u64,
    /// Target size of the persistent, roaming pirate fleet. Pirates are spawned at
    /// (and roam toward) systems with `danger > 0`; a galaxy with no dangerous
    /// systems has no pirates.
    pub fleet_size: u32,
    /// Credits paid to a trader for each pirate it destroys in a won fight.
    pub bounty: Money,
    /// Ticks between reinforcement checks that top the fleet back up to
    /// `fleet_size`.
    pub reinforce_interval: u64,
}

/// Convoy escorts: armed ships that fight alongside every trader when it is
/// ambushed. Absent means traders travel unescorted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EscortConfig {
    /// Ship id the escorts fly.
    pub ship: String,
    /// Number of escorts accompanying each trader.
    pub count: u32,
}

/// A persistent navy fleet that patrols lawless space, hunts pirates, and
/// defends traders under ambush. Absent means no navy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NavyConfig {
    /// Ship id navy patrols fly.
    pub ship: String,
    /// Target size of the persistent navy fleet.
    pub fleet_size: u32,
    /// Ticks between reinforcement checks that top the fleet back up.
    pub reinforce_interval: u64,
}

/// A complete run configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioDef {
    /// Human-readable name.
    pub name: String,
    /// RNG seed. Fixed here so a scenario is reproducible by default; the CLI may
    /// override it.
    pub seed: u64,
    /// Default number of ticks to run (CLI may override).
    pub ticks: u64,
    /// NPC trader population.
    pub traders: TraderSpawn,
    /// Optional piracy settings. `None` (the default) disables piracy entirely.
    #[serde(default)]
    pub piracy: Option<PiracyConfig>,
    /// How strongly traders avoid dangerous routes. A trade's profit is discounted
    /// by `clamp(danger * risk_aversion, 0, 1)` as the perceived chance of losing
    /// the cargo. `0` (the default) is risk-neutral: danger is ignored in routing.
    #[serde(default)]
    pub risk_aversion: f64,
    /// Optional convoy escorts. `None` (the default) = traders travel unescorted.
    #[serde(default)]
    pub escort: Option<EscortConfig>,
    /// Optional navy patrol fleet. `None` (the default) = no navy.
    #[serde(default)]
    pub navy: Option<NavyConfig>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenario_ron_roundtrips() {
        let def = ScenarioDef {
            name: "equilibrium".into(),
            seed: 42,
            ticks: 2000,
            traders: TraderSpawn {
                count: 24,
                ship: "core:cobra_mk3".into(),
                starting_capital: 1000,
            },
            piracy: Some(PiracyConfig {
                pirate_ship: "core:pirate".into(),
                base_ambush_chance: 0.05,
                max_pirates: 2,
                respawn_delay: 50,
                fleet_size: 12,
                bounty: 400,
                reinforce_interval: 20,
            }),
            risk_aversion: 1.5,
            escort: Some(EscortConfig {
                ship: "core:escort".into(),
                count: 1,
            }),
            navy: Some(NavyConfig {
                ship: "core:navy".into(),
                fleet_size: 6,
                reinforce_interval: 30,
            }),
        };
        let text = ron::to_string(&def).unwrap();
        let back: ScenarioDef = ron::from_str(&text).unwrap();
        assert_eq!(def, back);
    }

    #[test]
    fn optional_fields_default_when_omitted() {
        let text = r#"(name: "s", seed: 1, ticks: 10, traders: (count: 0, ship: "", starting_capital: 0))"#;
        let s: ScenarioDef = ron::from_str(text).unwrap();
        assert_eq!(s.piracy, None);
        assert_eq!(s.risk_aversion, 0.0);
        assert_eq!(s.escort, None);
        assert_eq!(s.navy, None);
    }
}
