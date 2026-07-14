//! Per-system market: stock, equilibrium anchor, and current price per good.
//!
//! A market only trades the commodities listed in its system's `initial_stock`.
//! Each such good carries its equilibrium anchor (the authored "normal" level)
//! against which pricing judges surplus or shortage.

use std::collections::BTreeMap;

use drift_core::{CommodityId, Money, Quantity, SystemId};
use serde::{Deserialize, Serialize};

use crate::pricing::PricingStrategy;

/// One tradeable good in a market.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketGood {
    /// Current inventory.
    pub stock: Quantity,
    /// Equilibrium anchor (authored baseline) used by pricing.
    pub equilibrium: Quantity,
    /// Current unit price, recomputed each tick.
    pub price: Money,
}

/// A system's marketplace. `goods` is a `BTreeMap` so iteration order is
/// deterministic (important for reproducible trader scans).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Market {
    pub system: SystemId,
    pub pricing: PricingStrategy,
    pub goods: BTreeMap<CommodityId, MarketGood>,
}

impl Market {
    pub fn has(&self, c: CommodityId) -> bool {
        self.goods.contains_key(&c)
    }

    /// Current stock, or 0 if this market does not trade the commodity.
    pub fn stock(&self, c: CommodityId) -> Quantity {
        self.goods.get(&c).map(|g| g.stock).unwrap_or(0)
    }

    /// Current price, or `None` if this market does not trade the commodity.
    pub fn price(&self, c: CommodityId) -> Option<Money> {
        self.goods.get(&c).map(|g| g.price)
    }

    /// Add `qty` to an existing good's stock. No-op if the good is not traded
    /// here (callers only add goods the market already holds).
    pub fn add(&mut self, c: CommodityId, qty: Quantity) {
        if let Some(g) = self.goods.get_mut(&c) {
            g.stock = g.stock.saturating_add(qty);
        }
    }

    /// Remove `qty` if at least that much is in stock. Returns whether it
    /// succeeded (all-or-nothing).
    pub fn try_remove(&mut self, c: CommodityId, qty: Quantity) -> bool {
        match self.goods.get_mut(&c) {
            Some(g) if g.stock >= qty => {
                g.stock -= qty;
                true
            }
            _ => false,
        }
    }
}
