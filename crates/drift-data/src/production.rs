//! Production recipe schema — the edges of the production graph.

use serde::{Deserialize, Serialize};

use crate::commodity::CommodityAmount;

/// A transformation of input commodities into output commodities. A system that
/// runs this recipe consumes `inputs` and yields `outputs` up to `rate` times per
/// tick, limited by available input stock. Recipes with empty `inputs` are raw
/// producers (e.g. mining ore from nothing).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionRecipe {
    /// Namespaced unique id.
    pub id: String,
    /// Consumed per application. Empty for raw extraction.
    pub inputs: Vec<CommodityAmount>,
    /// Produced per application.
    pub outputs: Vec<CommodityAmount>,
    /// Nominal applications per tick at equilibrium price.
    pub rate: u32,
    /// Price responsiveness of throughput. `0.0` (the default) is inelastic
    /// (fixed-rate). For a recipe with outputs it is supply elasticity (produce
    /// more when the product is dear); for a pure consumer it is demand
    /// elasticity (consume less when the input is dear). Higher = more responsive.
    #[serde(default)]
    pub elasticity: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipe_ron_roundtrips() {
        let def = ProductionRecipe {
            id: "core:smelting".into(),
            inputs: vec![CommodityAmount {
                commodity: "core:ore".into(),
                qty: 2,
            }],
            outputs: vec![CommodityAmount {
                commodity: "core:alloys".into(),
                qty: 1,
            }],
            rate: 5,
            elasticity: 0.6,
        };
        let text = ron::to_string(&def).unwrap();
        let back: ProductionRecipe = ron::from_str(&text).unwrap();
        assert_eq!(def, back);
    }

    #[test]
    fn elasticity_defaults_to_zero_when_omitted() {
        // Content authored before elasticity existed must still parse.
        let text = r#"(id: "x", inputs: [], outputs: [], rate: 1)"#;
        let r: ProductionRecipe = ron::from_str(text).unwrap();
        assert_eq!(r.elasticity, 0.0);
    }
}
