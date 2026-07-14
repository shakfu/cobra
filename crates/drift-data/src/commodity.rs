//! Commodity schema — a tradeable good.

use drift_core::{Money, Quantity};
use serde::{Deserialize, Serialize};

/// A tradeable good. Authored with a namespaced string `id` (e.g. `"core:food"`)
/// which the loader interns to a `CommodityId`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommodityDef {
    /// Namespaced unique id.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Reference price in credits at equilibrium stock.
    pub base_price: Money,
    /// Mass per unit; constrains how much fits in a ship's hold.
    pub unit_mass: u32,
    /// Price responsiveness to scarcity. Higher = prices swing harder as stock
    /// deviates from equilibrium. Typically in `[0.3, 1.5]`.
    pub elasticity: f64,
    /// Free-form grouping (e.g. "food", "minerals", "tech"). A string, not an
    /// enum, so mods can introduce new categories.
    pub category: String,
}

/// A quantity of a specific commodity, referenced by string id at authoring time.
/// Used in recipes and initial stock lists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommodityAmount {
    pub commodity: String,
    pub qty: Quantity,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commodity_ron_roundtrips() {
        let def = CommodityDef {
            id: "core:food".into(),
            name: "Food".into(),
            base_price: 100,
            unit_mass: 1,
            elasticity: 0.8,
            category: "food".into(),
        };
        let text = ron::to_string(&def).unwrap();
        let back: CommodityDef = ron::from_str(&text).unwrap();
        assert_eq!(def, back);
    }

    #[test]
    fn unknown_field_is_rejected() {
        // A typo'd field must fail loudly, not be silently dropped.
        let text = r#"(id: "core:food", name: "Food", base_price: 100, unit_mass: 1, elasticity: 0.8, category: "food", typo: 1)"#;
        assert!(ron::from_str::<CommodityDef>(text).is_err());
    }
}
