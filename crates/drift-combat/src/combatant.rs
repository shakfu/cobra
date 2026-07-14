//! A ship participating in an encounter: physical state plus its combat loadout.

use drift_core::ShipId;
use drift_data::CombatStats;
use serde::{Deserialize, Serialize};

use crate::math::Vec2;

/// A ship in a battle. It carries a copy of its [`CombatStats`] so the encounter
/// is self-contained (no registry lookups mid-fight). An unarmed ship has
/// `stats.weapon_damage == 0`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Combatant {
    pub ship: ShipId,
    /// Which side this ship fights for. Enemies have a different faction.
    pub faction: u8,
    pub stats: CombatStats,
    /// Maximum flight speed (from the ship def), used when manoeuvring.
    pub max_speed: f64,
    pub pos: Vec2,
    pub vel: Vec2,
    pub hull: i32,
    /// Current shield points (fractional to allow smooth regen).
    pub shield: f64,
    /// Ticks remaining before this ship can fire again.
    pub cooldown: u32,
    pub alive: bool,
}

impl Combatant {
    /// Spawn a combatant at `pos` with full hull and shield from its stats.
    pub fn new(
        ship: ShipId,
        faction: u8,
        stats: CombatStats,
        hull: u32,
        max_speed: f64,
        pos: Vec2,
    ) -> Self {
        Self {
            ship,
            faction,
            stats,
            max_speed,
            pos,
            vel: Vec2::default(),
            hull: hull as i32,
            shield: stats.shield as f64,
            cooldown: 0,
            alive: true,
        }
    }

    pub fn is_armed(&self) -> bool {
        self.stats.weapon_damage > 0 && self.stats.weapon_range > 0.0
    }

    /// Apply `amount` damage: shields absorb first, the remainder cuts hull.
    /// Destroys the ship when hull reaches zero.
    pub fn take_damage(&mut self, amount: u32) {
        if !self.alive {
            return;
        }
        let mut remaining = amount as f64;
        if self.shield > 0.0 {
            let absorbed = self.shield.min(remaining);
            self.shield -= absorbed;
            remaining -= absorbed;
        }
        if remaining > 0.0 {
            self.hull -= remaining.ceil() as i32;
            if self.hull <= 0 {
                self.hull = 0;
                self.alive = false;
            }
        }
    }

    /// Regenerate shields up to their maximum.
    pub fn regen_shield(&mut self, dt: f64) {
        if self.alive {
            let max = self.stats.shield as f64;
            self.shield = (self.shield + self.stats.shield_regen * dt).min(max);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats() -> CombatStats {
        CombatStats {
            shield: 20,
            shield_regen: 2.0,
            weapon_damage: 10,
            weapon_range: 30.0,
            weapon_cooldown: 2,
            accuracy: 1.0,
            acceleration: 10.0,
        }
    }

    #[test]
    fn shields_absorb_before_hull() {
        let mut c = Combatant::new(ShipId(0), 0, stats(), 100, 200.0, Vec2::default());
        c.take_damage(15); // all absorbed by 20 shield
        assert_eq!(c.shield, 5.0);
        assert_eq!(c.hull, 100);
        c.take_damage(15); // 5 to shield, 10 to hull
        assert_eq!(c.shield, 0.0);
        assert_eq!(c.hull, 90);
        assert!(c.alive);
    }

    #[test]
    fn hull_depletion_destroys() {
        let mut c = Combatant::new(ShipId(0), 0, stats(), 10, 200.0, Vec2::default());
        c.shield = 0.0;
        c.take_damage(100);
        assert_eq!(c.hull, 0);
        assert!(!c.alive);
        // Further damage to a wreck is a no-op.
        c.take_damage(100);
        assert_eq!(c.hull, 0);
    }

    #[test]
    fn shield_regen_is_capped() {
        let mut c = Combatant::new(ShipId(0), 0, stats(), 100, 200.0, Vec2::default());
        c.shield = 0.0;
        c.regen_shield(1.0);
        assert_eq!(c.shield, 2.0);
        for _ in 0..100 {
            c.regen_shield(1.0);
        }
        assert_eq!(c.shield, 20.0, "regen cannot exceed max shield");
    }

    #[test]
    fn unarmed_when_no_weapon() {
        let mut s = stats();
        s.weapon_damage = 0;
        let c = Combatant::new(ShipId(0), 0, s, 10, 200.0, Vec2::default());
        assert!(!c.is_armed());
    }
}
