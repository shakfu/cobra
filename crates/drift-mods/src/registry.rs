//! Linking: string-id content -> handle-based, cross-checked runtime data.
//!
//! This is where the "link" half of load-and-link happens. Every id is interned
//! to a handle, then every reference (recipe -> commodities, system -> recipes /
//! systems / commodities, system -> pricing strategy) is resolved. A reference
//! that does not resolve aborts the whole load: the resulting [`Registry`] is by
//! construction fully connected, so the simulation never has to handle a dangling
//! id.

use std::collections::HashSet;

use drift_core::{CommodityId, Interner, Quantity, RecipeId, ShipId, SystemId};
use drift_data::{CommodityDef, ShipDef};
use tracing::warn;

use crate::error::LoadError;
use crate::loader::MergedContent;

/// A recipe with all commodity references resolved to handles.
#[derive(Debug, Clone)]
pub struct ResolvedRecipe {
    pub id: RecipeId,
    pub inputs: Vec<(CommodityId, Quantity)>,
    pub outputs: Vec<(CommodityId, Quantity)>,
    pub rate: u32,
    pub elasticity: f64,
}

/// A system with all references resolved to handles.
#[derive(Debug, Clone)]
pub struct ResolvedSystem {
    pub id: SystemId,
    pub name: String,
    pub position: [f64; 2],
    pub industries: Vec<RecipeId>,
    pub connections: Vec<SystemId>,
    pub initial_stock: Vec<(CommodityId, Quantity)>,
    /// Validated pricing strategy name (guaranteed registered by the caller).
    pub pricing: String,
    /// Route lawlessness in `[0, 1]`; scales pirate ambush chance.
    pub danger: f64,
}

/// Immutable, fully-linked game data. Vectors are indexed by the corresponding
/// handle's `.index()`; the interners provide id<->handle and handle->name.
#[derive(Debug)]
pub struct Registry {
    commodity_ids: Interner,
    commodities: Vec<CommodityDef>,
    recipe_ids: Interner,
    recipes: Vec<ResolvedRecipe>,
    system_ids: Interner,
    systems: Vec<ResolvedSystem>,
    ship_ids: Interner,
    ships: Vec<ShipDef>,
}

impl Registry {
    // --- commodities ---
    pub fn commodity_count(&self) -> usize {
        self.commodities.len()
    }
    pub fn commodity(&self, id: CommodityId) -> &CommodityDef {
        &self.commodities[id.index()]
    }
    pub fn commodity_id(&self, name: &str) -> Option<CommodityId> {
        self.commodity_ids.get(name).map(CommodityId)
    }
    pub fn commodity_name(&self, id: CommodityId) -> &str {
        self.commodity_ids.name(id.0).unwrap_or("?")
    }
    pub fn commodities(&self) -> impl Iterator<Item = (CommodityId, &CommodityDef)> {
        self.commodities
            .iter()
            .enumerate()
            .map(|(i, c)| (CommodityId(i as u32), c))
    }

    // --- recipes ---
    pub fn recipe(&self, id: RecipeId) -> &ResolvedRecipe {
        &self.recipes[id.index()]
    }
    pub fn recipe_id(&self, name: &str) -> Option<RecipeId> {
        self.recipe_ids.get(name).map(RecipeId)
    }
    pub fn recipe_name(&self, id: RecipeId) -> &str {
        self.recipe_ids.name(id.0).unwrap_or("?")
    }
    pub fn recipe_count(&self) -> usize {
        self.recipes.len()
    }

    // --- systems ---
    pub fn system_count(&self) -> usize {
        self.systems.len()
    }
    pub fn system(&self, id: SystemId) -> &ResolvedSystem {
        &self.systems[id.index()]
    }
    pub fn system_id(&self, name: &str) -> Option<SystemId> {
        self.system_ids.get(name).map(SystemId)
    }
    pub fn system_name(&self, id: SystemId) -> &str {
        self.system_ids.name(id.0).unwrap_or("?")
    }
    pub fn systems(&self) -> impl Iterator<Item = &ResolvedSystem> {
        self.systems.iter()
    }

    // --- ships ---
    pub fn ship_id(&self, name: &str) -> Option<ShipId> {
        self.ship_ids.get(name).map(ShipId)
    }
    pub fn ship(&self, id: ShipId) -> &ShipDef {
        &self.ships[id.index()]
    }
    pub fn ship_count(&self) -> usize {
        self.ships.len()
    }
}

/// Link merged content into a [`Registry`], validating every reference.
///
/// `known_pricing` is the set of pricing strategy names the caller has registered
/// (e.g. the economy's built-in strategies). Passing it in keeps this crate
/// ignorant of the economy while still failing fast on a bad `pricing` name.
pub fn link(
    merged: MergedContent,
    known_pricing: &HashSet<String>,
) -> Result<Registry, LoadError> {
    // Intern all ids first so references may point forward or backward freely.
    // Interning in vector order makes handle.index() == vector index.
    let mut commodity_ids = Interner::new();
    for c in &merged.commodities {
        commodity_ids.intern(&c.id);
    }
    let mut recipe_ids = Interner::new();
    for r in &merged.recipes {
        recipe_ids.intern(&r.id);
    }
    let mut system_ids = Interner::new();
    for s in &merged.systems {
        system_ids.intern(&s.id);
    }
    let mut ship_ids = Interner::new();
    for s in &merged.ships {
        ship_ids.intern(&s.id);
    }

    let commodity = |referrer: &str, name: &str| -> Result<CommodityId, LoadError> {
        commodity_ids
            .get(name)
            .map(CommodityId)
            .ok_or_else(|| LoadError::DanglingRef {
                kind: "recipe/system",
                referrer: referrer.to_string(),
                target_kind: "commodity",
                target: name.to_string(),
            })
    };

    // Resolve recipes.
    let mut recipes = Vec::with_capacity(merged.recipes.len());
    for r in &merged.recipes {
        let id = RecipeId(recipe_ids.get(&r.id).expect("interned above"));
        let inputs = r
            .inputs
            .iter()
            .map(|a| Ok((commodity(&r.id, &a.commodity)?, a.qty)))
            .collect::<Result<Vec<_>, LoadError>>()?;
        let outputs = r
            .outputs
            .iter()
            .map(|a| Ok((commodity(&r.id, &a.commodity)?, a.qty)))
            .collect::<Result<Vec<_>, LoadError>>()?;
        recipes.push(ResolvedRecipe {
            id,
            inputs,
            outputs,
            rate: r.rate,
            elasticity: r.elasticity,
        });
    }

    // Resolve systems.
    let mut systems = Vec::with_capacity(merged.systems.len());
    for s in &merged.systems {
        let id = SystemId(system_ids.get(&s.id).expect("interned above"));

        let industries = s
            .industries
            .iter()
            .map(|rid| {
                recipe_ids
                    .get(rid)
                    .map(RecipeId)
                    .ok_or_else(|| LoadError::DanglingRef {
                        kind: "system",
                        referrer: s.id.clone(),
                        target_kind: "recipe",
                        target: rid.clone(),
                    })
            })
            .collect::<Result<Vec<_>, LoadError>>()?;

        let connections = s
            .connections
            .iter()
            .map(|sid| {
                system_ids
                    .get(sid)
                    .map(SystemId)
                    .ok_or_else(|| LoadError::DanglingRef {
                        kind: "system",
                        referrer: s.id.clone(),
                        target_kind: "system",
                        target: sid.clone(),
                    })
            })
            .collect::<Result<Vec<_>, LoadError>>()?;

        let initial_stock = s
            .initial_stock
            .iter()
            .map(|a| Ok((commodity(&s.id, &a.commodity)?, a.qty)))
            .collect::<Result<Vec<_>, LoadError>>()?;

        if !known_pricing.contains(&s.pricing) {
            return Err(LoadError::UnknownPricing {
                system: s.id.clone(),
                strategy: s.pricing.clone(),
            });
        }

        systems.push(ResolvedSystem {
            id,
            name: s.name.clone(),
            position: s.position,
            industries,
            connections,
            initial_stock,
            pricing: s.pricing.clone(),
            danger: s.danger,
        });
    }

    // Non-fatal hygiene check: warn on one-way jump connections.
    warn_asymmetric_connections(&systems);

    Ok(Registry {
        commodity_ids,
        commodities: merged.commodities,
        recipe_ids,
        recipes,
        system_ids,
        systems,
        ship_ids,
        ships: merged.ships,
    })
}

/// Warn (not error) if a jump connection is not mirrored. A one-way jump is
/// usually an authoring mistake, but not fatal.
fn warn_asymmetric_connections(systems: &[ResolvedSystem]) {
    let has: HashSet<(u32, u32)> = systems
        .iter()
        .flat_map(|s| s.connections.iter().map(move |c| (s.id.0, c.0)))
        .collect();
    for s in systems {
        for c in &s.connections {
            if !has.contains(&(c.0, s.id.0)) {
                warn!(
                    from = s.id.0,
                    to = c.0,
                    "asymmetric jump connection (not mirrored)"
                );
            }
        }
    }
}
