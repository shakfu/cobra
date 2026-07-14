//! NPC traders — the agents whose arbitrage self-corrects the economy.
//!
//! A trader runs a buy-low / travel / sell-high cycle over single jumps. Buying
//! draws stock from the source (raising its price) and selling adds stock to the
//! destination (lowering its price), so every executed trade narrows the very
//! differential it exploited. That negative feedback is what drives the whole
//! galaxy toward a price equilibrium — and it is the test oracle for convergence.
//!
//! The route decision is the pure [`choose_trade`]; movement and execution live
//! in the world, which has the galaxy graph and ship data.

use std::collections::BTreeMap;

use drift_core::{CommodityId, Money, Quantity, ShipId, SystemId, Tick};
use serde::{Deserialize, Serialize};

use crate::command::{Owner, PlayerId};
use crate::market::Market;

/// Where a trader is right now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TraderLocation {
    /// Parked at a system, able to trade.
    Docked(SystemId),
    /// Between systems; departed `origin` at tick `departure` and becomes
    /// `Docked(dest)` at tick `arrival`. `origin`/`departure` let a client
    /// interpolate the ship's position along the jump edge.
    InTransit {
        origin: SystemId,
        dest: SystemId,
        departure: Tick,
        arrival: Tick,
    },
    /// Destroyed by pirates; respawns (empty, at a random system) at `respawn`.
    Destroyed { respawn: Tick },
}

/// A stable, never-reused handle for a trader. Unlike a vector index, a
/// `TraderId` remains valid as traders are added and removed: it is assigned once,
/// monotonically, and a stale id simply fails to resolve. This is what lets
/// commands (and a future server echoing ids to clients) address a specific trader
/// safely across ticks.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct TraderId(pub u64);

/// A trading ship — NPC by default, or player-owned when created by a command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trader {
    /// Stable handle (see [`TraderId`]); distinct from the trader's position in
    /// the world's vector, which may change as traders are removed.
    pub id: TraderId,
    pub ship: ShipId,
    /// Who controls this trader. NPC traders run the AI; player traders act only
    /// on commands.
    pub owner: Owner,
    pub capital: Money,
    pub location: TraderLocation,
    /// Held goods, by commodity. `BTreeMap` for deterministic iteration.
    pub cargo: BTreeMap<CommodityId, Quantity>,
}

impl Trader {
    /// A new NPC trader docked at `at`.
    pub fn new(id: TraderId, ship: ShipId, capital: Money, at: SystemId) -> Self {
        Self {
            id,
            ship,
            owner: Owner::Npc,
            capital,
            location: TraderLocation::Docked(at),
            cargo: BTreeMap::new(),
        }
    }

    /// A new player-owned trader docked at `at`.
    pub fn owned(id: TraderId, ship: ShipId, capital: Money, at: SystemId, player: PlayerId) -> Self {
        Self {
            owner: Owner::Player(player),
            ..Self::new(id, ship, capital, at)
        }
    }

    pub fn is_player(&self) -> bool {
        matches!(self.owner, Owner::Player(_))
    }

    pub fn cargo_units(&self) -> u32 {
        self.cargo.values().copied().sum()
    }
}

/// A chosen single-commodity trade: buy `qty` of `commodity` here at `unit_cost`,
/// carry it to `dest`. `profit` is the raw total spread; `score` is the
/// risk-adjusted expected value that trades are actually ranked by (equal to
/// `profit` when the trader is risk-neutral).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TradePlan {
    pub commodity: CommodityId,
    pub dest: SystemId,
    pub qty: Quantity,
    pub unit_cost: Money,
    pub profit: Money,
    pub score: Money,
}

/// Pick the best buy-here/sell-at-a-neighbor plan by risk-adjusted expected
/// value, or `None` if no worthwhile trade is feasible.
///
/// For each candidate the raw profit is the per-unit spread times the quantity the
/// trader can fill (given capital, hold space, and stock). That profit is then
/// discounted for the danger of the destination: with `p_loss = clamp(danger *
/// risk_aversion, 0, 1)`, the expected value is
/// `EV = (1 - p_loss) * profit - p_loss * cargo_cost`, i.e. the trader keeps the
/// profit if it arrives but eats the cargo cost if pirates take it. Only
/// `EV > 0` trades are considered, and the best `EV` wins (ties broken by
/// commodity then destination id for determinism).
///
/// `risk_aversion == 0` makes `EV == profit`, i.e. pure profit-maximising.
/// `neighbors` must be provided in a deterministic order (sorted by system id).
pub fn choose_trade(
    here: &Market,
    neighbors: &[&Market],
    capital: Money,
    cargo_capacity: u32,
    unit_mass: impl Fn(CommodityId) -> u32,
    danger_of: impl Fn(SystemId) -> f64,
    risk_aversion: f64,
) -> Option<TradePlan> {
    let mut best: Option<(f64, TradePlan)> = None; // (expected value, plan)

    for (&commodity, good) in &here.goods {
        let buy_price = good.price;
        let mass = unit_mass(commodity).max(1);
        let by_mass = (cargo_capacity / mass) as i64;
        let affordable = capital / buy_price.max(1);
        let by_stock = here.stock(commodity) as i64;
        let qty = affordable.min(by_mass).min(by_stock);
        if qty <= 0 {
            continue;
        }

        for neighbor in neighbors {
            let Some(sell_price) = neighbor.price(commodity) else {
                continue; // neighbor does not trade this good
            };
            let per_unit = sell_price - buy_price;
            if per_unit <= 0 {
                continue;
            }
            let profit = per_unit * qty;
            let cargo_cost = buy_price * qty;

            // Discount for the danger of the destination.
            let p_loss = (danger_of(neighbor.system) * risk_aversion).clamp(0.0, 1.0);
            let ev = (1.0 - p_loss) * profit as f64 - p_loss * cargo_cost as f64;
            if ev <= 0.0 {
                continue; // not worth the risk
            }

            let plan = TradePlan {
                commodity,
                dest: neighbor.system,
                qty: qty as Quantity,
                unit_cost: buy_price,
                profit,
                score: ev.round() as Money,
            };
            let better = match best {
                None => true,
                Some((best_ev, best_plan)) => {
                    ev > best_ev
                        || (ev == best_ev
                            && (commodity, neighbor.system) < (best_plan.commodity, best_plan.dest))
                }
            };
            if better {
                best = Some((ev, plan));
            }
        }
    }

    best.map(|(_, plan)| plan)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::market::{Market, MarketGood};
    use crate::pricing::PricingStrategy;

    fn market_with(system: u32, goods: &[(u32, u32, i64)]) -> Market {
        // goods: (commodity, stock, price)
        let mut m = BTreeMap::new();
        for &(c, stock, price) in goods {
            m.insert(
                CommodityId(c),
                MarketGood {
                    stock,
                    equilibrium: 100,
                    price,
                },
            );
        }
        Market {
            system: SystemId(system),
            pricing: PricingStrategy::SupplyDemandV1,
            goods: m,
        }
    }

    #[test]
    fn picks_positive_spread_route() {
        // commodity 0 cheap here (50), dear at neighbor (90).
        let here = market_with(0, &[(0, 100, 50)]);
        let there = market_with(1, &[(0, 100, 90)]);
        let plan = choose_trade(&here, &[&there], 10_000, 100, |_| 1, |_| 0.0, 0.0).unwrap();
        assert_eq!(plan.commodity, CommodityId(0));
        assert_eq!(plan.dest, SystemId(1));
        assert_eq!(plan.unit_cost, 50);
        assert!(plan.qty > 0);
    }

    #[test]
    fn no_trade_when_no_positive_spread() {
        let here = market_with(0, &[(0, 100, 90)]);
        let there = market_with(1, &[(0, 100, 50)]); // cheaper there: no profit
        assert!(choose_trade(&here, &[&there], 10_000, 100, |_| 1, |_| 0.0, 0.0).is_none());
    }

    #[test]
    fn quantity_bounded_by_capital_hold_and_stock() {
        let here = market_with(0, &[(0, 5, 50)]); // only 5 in stock
        let there = market_with(1, &[(0, 100, 200)]);
        // Capital allows 20, hold allows 100, stock allows 5 -> qty capped at 5.
        let plan = choose_trade(&here, &[&there], 1000, 100, |_| 1, |_| 0.0, 0.0).unwrap();
        assert_eq!(plan.qty, 5);
    }

    #[test]
    fn prefers_higher_total_profit() {
        // Good 0: spread 10 over up-to-100 units = 1000. Good 1: spread 40 over
        // only 5 units = 200. Higher total profit wins (good 0), not higher margin.
        let here = market_with(0, &[(0, 100, 50), (1, 5, 10)]);
        let there = market_with(1, &[(0, 100, 60), (1, 100, 50)]);
        let plan = choose_trade(&here, &[&there], 100_000, 1000, |_| 1, |_| 0.0, 0.0).unwrap();
        assert_eq!(plan.commodity, CommodityId(0));
    }

    #[test]
    fn risk_aversion_prefers_the_safe_route() {
        // Two destinations for commodity 0 (bought here at 50):
        //   safe (system 1, danger 0): sells at 65  -> spread 15
        //   rich (system 2, danger 1): sells at 120 -> spread 70
        let here = market_with(0, &[(0, 100, 50)]);
        let safe = market_with(1, &[(0, 100, 65)]);
        let rich = market_with(2, &[(0, 100, 120)]);
        let danger = |s: SystemId| if s == SystemId(2) { 1.0 } else { 0.0 };

        // Risk-neutral: the rich but dangerous route wins on raw profit.
        let neutral = choose_trade(&here, &[&safe, &rich], 100_000, 1000, |_| 1, danger, 0.0).unwrap();
        assert_eq!(neutral.dest, SystemId(2));

        // Risk-averse: the dangerous route's expected value collapses, so the
        // trader takes the safe, lower-margin route instead.
        let cautious =
            choose_trade(&here, &[&safe, &rich], 100_000, 1000, |_| 1, danger, 1.0).unwrap();
        assert_eq!(cautious.dest, SystemId(1));
    }
}
