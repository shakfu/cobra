//! Pricing strategies — the first behavior routed through the plugin seam.
//!
//! Content names a strategy per system (`pricing: "supply_demand_v1"`). Here we
//! register the built-in strategies in a [`NamedRegistry`] and provide the price
//! math. When WASM/Lua lands, a new registry entry (or a new [`PricingStrategy`]
//! variant) is all that changes — markets already hold a resolved strategy, not a
//! hard-coded formula.

use drift_core::{Money, NamedRegistry, Quantity};
use serde::{Deserialize, Serialize};

/// Lower/upper bounds on price as a multiple of base, so scarcity/glut cannot
/// drive prices to absurd extremes (or to zero, which would break trading).
pub const MIN_FACTOR: f64 = 0.25;
pub const MAX_FACTOR: f64 = 4.0;

/// Fraction of the gap between the current price and the freshly-computed target
/// that a market closes each tick. Sticky prices (< 1.0) damp the boom/bust limit
/// cycles that discrete, lumpy trading otherwise induces, so the economy settles
/// into a stable regime rather than oscillating at the clamp bounds.
pub const PRICE_SMOOTHING: f64 = 0.2;

/// Move `current` a `PRICE_SMOOTHING` fraction toward `target`, floored at 1.
pub fn smoothed(current: Money, target: Money) -> Money {
    let next = current as f64 + PRICE_SMOOTHING * (target - current) as f64;
    (next.round() as i64).max(1)
}

/// A resolved pricing strategy. Currently one built-in; the enum exists so the
/// seam has a concrete, serializable handler type to store per market.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PricingStrategy {
    SupplyDemandV1,
}

impl PricingStrategy {
    /// Compute the unit price given the market's current `stock`, its
    /// `equilibrium` anchor, and the commodity's `base_price` and `elasticity`.
    pub fn price(
        self,
        base_price: Money,
        stock: Quantity,
        equilibrium: Quantity,
        elasticity: f64,
    ) -> Money {
        match self {
            PricingStrategy::SupplyDemandV1 => {
                supply_demand_v1(base_price, stock, equilibrium, elasticity)
            }
        }
    }
}

/// `price = base * clamp((equilibrium / stock)^elasticity)`.
///
/// Scarcity (stock below equilibrium) pushes price up; glut pushes it down; at
/// exactly equilibrium the price is `base`. Clamped to `[MIN_FACTOR, MAX_FACTOR]`
/// and floored at 1 credit.
pub fn supply_demand_v1(
    base_price: Money,
    stock: Quantity,
    equilibrium: Quantity,
    elasticity: f64,
) -> Money {
    let eq = equilibrium.max(1) as f64;
    let st = stock.max(1) as f64;
    let factor = (eq / st).powf(elasticity).clamp(MIN_FACTOR, MAX_FACTOR);
    let price = (base_price as f64 * factor).round() as i64;
    price.max(1)
}

/// Build the registry of built-in pricing strategies. The CLI uses this both to
/// resolve names when constructing the world and to hand the set of valid names
/// to the loader for content validation.
pub fn builtin_pricing() -> NamedRegistry<PricingStrategy> {
    let mut reg = NamedRegistry::new();
    reg.register("supply_demand_v1", PricingStrategy::SupplyDemandV1);
    reg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_equilibrium_price_is_base() {
        assert_eq!(supply_demand_v1(100, 500, 500, 0.8), 100);
    }

    #[test]
    fn monotonic_decreasing_in_stock() {
        let mut prev = i64::MAX;
        for stock in [1u32, 50, 100, 250, 500, 1000, 5000] {
            let p = supply_demand_v1(100, stock, 500, 0.8);
            assert!(p <= prev, "price must not rise as stock rises (stock={stock}, p={p}, prev={prev})");
            prev = p;
        }
    }

    #[test]
    fn scarcity_raises_and_glut_lowers() {
        let base = supply_demand_v1(100, 500, 500, 0.8);
        let scarce = supply_demand_v1(100, 50, 500, 0.8);
        let glut = supply_demand_v1(100, 5000, 500, 0.8);
        assert!(scarce > base);
        assert!(glut < base);
    }

    #[test]
    fn clamps_at_bounds() {
        // Extreme scarcity clamps to MAX_FACTOR; extreme glut to MIN_FACTOR.
        let scarce = supply_demand_v1(100, 1, 100_000, 1.0);
        let glut = supply_demand_v1(100, 100_000, 1, 1.0);
        assert_eq!(scarce, (100.0 * MAX_FACTOR) as i64);
        assert_eq!(glut, (100.0 * MIN_FACTOR) as i64);
    }

    #[test]
    fn never_below_one_credit() {
        assert!(supply_demand_v1(1, 100_000, 1, 2.0) >= 1);
    }

    #[test]
    fn registry_resolves_builtin() {
        let reg = builtin_pricing();
        assert!(reg.contains("supply_demand_v1"));
        assert_eq!(reg.resolve("supply_demand_v1"), Ok(&PricingStrategy::SupplyDemandV1));
    }
}
