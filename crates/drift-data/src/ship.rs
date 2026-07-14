//! Ship schema.
//!
//! The trading sim consumes `cargo_capacity` and `jump_speed`; the combat model
//! consumes `hull`, `max_speed`, and the optional [`CombatStats`]. A ship with no
//! `combat` block is an unarmed civilian in an encounter.

use serde::{Deserialize, Serialize};

/// Combat loadout for a ship. Optional on [`ShipDef`]; absent means unarmed.
/// `Default` is all-zero, i.e. an inert, unarmed ship.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CombatStats {
    /// Shield hit points; absorbs damage before the hull and regenerates.
    pub shield: u32,
    /// Shield points regenerated per tick.
    pub shield_regen: f64,
    /// Damage dealt per successful shot.
    pub weapon_damage: u32,
    /// Maximum engagement distance for the weapon.
    pub weapon_range: f64,
    /// Ticks between shots.
    pub weapon_cooldown: u32,
    /// Point-blank hit probability in `[0, 1]`; falls off linearly to zero at
    /// `weapon_range`.
    pub accuracy: f64,
    /// Steering acceleration (velocity change per tick) when manoeuvring.
    pub acceleration: f64,
}

/// A ship variant. NPC traders and (later) the player fly these.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShipDef {
    /// Namespaced unique id (e.g. `"core:cobra_mk3"`).
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Cargo hold capacity in mass units.
    pub cargo_capacity: u32,
    /// Jump speed: distance units traversable per tick when travelling between
    /// systems. Higher = fewer ticks in transit.
    pub jump_speed: f64,
    /// Structural hull points.
    pub hull: u32,
    /// Maximum in-system flight speed.
    pub max_speed: f64,
    /// Combat loadout. `None` = unarmed civilian.
    #[serde(default)]
    pub combat: Option<CombatStats>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ship_ron_roundtrips() {
        let def = ShipDef {
            id: "core:cobra_mk3".into(),
            name: "Cobra Mk III".into(),
            cargo_capacity: 35,
            jump_speed: 7.0,
            hull: 100,
            max_speed: 350.0,
            combat: Some(CombatStats {
                shield: 50,
                shield_regen: 1.0,
                weapon_damage: 8,
                weapon_range: 40.0,
                weapon_cooldown: 2,
                accuracy: 0.9,
                acceleration: 30.0,
            }),
        };
        let text = ron::to_string(&def).unwrap();
        let back: ShipDef = ron::from_str(&text).unwrap();
        assert_eq!(def, back);
    }

    #[test]
    fn combat_defaults_to_none_when_omitted() {
        let text = r#"(id: "x", name: "X", cargo_capacity: 1, jump_speed: 1.0, hull: 1, max_speed: 1.0)"#;
        let s: ShipDef = ron::from_str(text).unwrap();
        assert_eq!(s.combat, None);
    }
}
