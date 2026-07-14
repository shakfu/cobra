//! Star system schema — a node in the galaxy graph and a marketplace.

use serde::{Deserialize, Serialize};

use crate::commodity::CommodityAmount;

/// A star system: a market, a set of industries, and jump connections to other
/// systems.
///
/// `initial_stock` does double duty: it is both the market's starting inventory
/// *and* its equilibrium anchor. Pricing reads the anchor to decide whether the
/// current stock is a surplus (cheap) or a shortage (dear), so authoring a
/// system's "normal" stock levels is how you tune its baseline economy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemDef {
    /// Namespaced unique id.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// 2-D galactic coordinates; jump distance is Euclidean over these.
    pub position: [f64; 2],
    /// Recipe ids this system runs each tick (its industries).
    pub industries: Vec<String>,
    /// System ids reachable by a single jump. Expected to be symmetric; the
    /// loader warns if a connection is not mirrored.
    pub connections: Vec<String>,
    /// Starting inventory and equilibrium anchor for each commodity.
    pub initial_stock: Vec<CommodityAmount>,
    /// Name of the pricing strategy this market uses (resolved via the plugin
    /// seam, e.g. `"supply_demand_v1"`).
    pub pricing: String,
    /// Lawlessness of routes into this system, in `[0, 1]`. Scales the chance a
    /// laden trader travelling here is ambushed by pirates. `0` (the default) is
    /// perfectly safe.
    #[serde(default)]
    pub danger: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_ron_roundtrips() {
        let def = SystemDef {
            id: "core:lave".into(),
            name: "Lave".into(),
            position: [0.0, 0.0],
            industries: vec!["core:agriculture".into()],
            connections: vec!["core:diso".into()],
            initial_stock: vec![CommodityAmount {
                commodity: "core:food".into(),
                qty: 500,
            }],
            pricing: "supply_demand_v1".into(),
            danger: 0.3,
        };
        let text = ron::to_string(&def).unwrap();
        let back: SystemDef = ron::from_str(&text).unwrap();
        assert_eq!(def, back);
    }

    #[test]
    fn danger_defaults_to_zero() {
        let text = r#"(id: "s", name: "S", position: (0.0, 0.0), industries: [], connections: [], initial_stock: [], pricing: "supply_demand_v1")"#;
        let s: SystemDef = ron::from_str(text).unwrap();
        assert_eq!(s.danger, 0.0);
    }
}
