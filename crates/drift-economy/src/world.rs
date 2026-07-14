//! The simulation world: markets + traders advanced by a deterministic tick.
//!
//! Tick phase order is **production -> price update -> trading**: each tick first
//! transforms stock via industries, then reprices every market from the new
//! stock, then lets traders act on those fresh prices. Trades change stock, which
//! the next tick's repricing reflects. The world owns its RNG so a run is fully
//! reproducible and a dumped [`Snapshot`] is resumable.

use std::collections::VecDeque;
use std::sync::Arc;

use drift_combat::{Combatant, Encounter, Vec2};
use drift_core::{DetRng, Money, ShipId, SystemId, Tick};
use drift_data::ScenarioDef;
use drift_mods::Registry;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::command::{Command, CommandError, Owner, PlayerId};
use crate::event::{EventCategory, SimEvent};
use crate::market::{Market, MarketGood};
use crate::patrol::{Patrol, PatrolLocation};
use crate::pricing::PricingStrategy;
use crate::production::{apply_recipe, elastic_factor, response_signal, MAX_ELASTIC_FACTOR};
use crate::trader::{choose_trade, Trader, TraderId, TraderLocation};

/// Per-tick probability a docked patrol (pirate or navy) relocates to a
/// danger-weighted neighbor.
const ROAM_CHANCE: f64 = 0.12;

/// Most recent simulation events retained for the debug log (older ones drop off).
const EVENT_CAP: usize = 2000;

#[derive(Debug, Error)]
pub enum WorldError {
    #[error("scenario references unknown ship '{0}'")]
    UnknownShip(String),

    #[error("system '{system}' runs an industry using commodity '{commodity}', which it does not trade (add it to initial_stock)")]
    MissingIndustryCommodity { system: String, commodity: String },

    #[error("the galaxy has no systems")]
    NoSystems,
}

/// Resolved piracy settings for a run (the scenario's `pirate_ship` looked up to
/// a handle plus the numeric knobs).
#[derive(Debug, Clone)]
struct PiracyRuntime {
    pirate_ship: ShipId,
    base_ambush_chance: f64,
    max_pirates: u32,
    respawn_delay: u64,
    fleet_size: u32,
    bounty: Money,
    reinforce_interval: u64,
}

/// Resolved navy settings.
#[derive(Debug, Clone)]
struct NavyRuntime {
    navy_ship: ShipId,
    fleet_size: u32,
    reinforce_interval: u64,
}

/// Resolved escort settings.
#[derive(Debug, Clone)]
struct EscortRuntime {
    escort_ship: ShipId,
    count: u32,
}

/// Cumulative piracy tallies over a run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PiracyStats {
    /// Ambushes that were triggered.
    pub ambushes: u64,
    /// Traders destroyed by pirates.
    pub traders_lost: u64,
    /// Pirates destroyed by traders (in ambushes).
    pub pirates_destroyed: u64,
    /// Total bounty credits paid out to victorious traders.
    pub bounties_paid: Money,
    /// Pirates destroyed by the navy (on patrol, outside ambushes).
    pub pirates_suppressed: u64,
    /// Navy ships lost fighting pirates.
    pub navy_lost: u64,
}

/// A serializable view of mutable world state (excludes the static registry).
/// Used for state dumps and determinism checks.
#[derive(Serialize)]
pub struct Snapshot<'a> {
    pub tick: Tick,
    pub rng: &'a DetRng,
    pub markets: &'a [Market],
    pub traders: &'a [Trader],
    /// Fractional production progress per system, per industry (elastic rates
    /// carry a remainder between ticks).
    pub progress: &'a [Vec<f64>],
    pub piracy: PiracyStats,
    pub pirates: &'a [Patrol],
    pub navy: &'a [Patrol],
    /// Next trader id to be assigned; part of state so a resumed run keeps ids
    /// unique.
    pub next_trader_id: u64,
}

/// The world. Shares the immutable [`Registry`] (static content) via `Arc` and
/// owns the mutable simulation state. Owning (rather than borrowing) the registry
/// lets the world live in a long-lived host such as a UI app or a server session.
pub struct World {
    registry: Arc<Registry>,
    tick: Tick,
    rng: DetRng,
    markets: Vec<Market>,
    traders: Vec<Trader>,
    /// `progress[system][industry]` accumulates fractional applications so that
    /// price-scaled (non-integer) throughput stays smooth and deterministic.
    progress: Vec<Vec<f64>>,
    /// Resolved piracy settings, or `None` when the scenario disables piracy.
    piracy: Option<PiracyRuntime>,
    piracy_stats: PiracyStats,
    /// The persistent, roaming pirate fleet.
    pirates: Vec<Patrol>,
    /// Resolved navy settings, or `None` when there is no navy.
    navy_runtime: Option<NavyRuntime>,
    /// The persistent, roaming navy fleet.
    navy: Vec<Patrol>,
    /// Resolved escort settings, or `None` when traders travel unescorted.
    escort: Option<EscortRuntime>,
    /// How strongly traders discount profit by destination danger (0 = neutral).
    risk_aversion: f64,
    /// Player commands queued for the next tick (drained in `command_phase`).
    commands: Vec<Command>,
    commands_applied: u64,
    commands_rejected: u64,
    /// Errors from the most recent `command_phase` (ephemeral UI feedback; not
    /// simulation state, so excluded from the snapshot).
    last_errors: Vec<CommandError>,
    /// Monotonic source of stable trader ids; never decreases, ids never reused.
    next_trader_id: u64,
    /// Bounded, deterministic log of notable happenings (debug/observability;
    /// ephemeral, not part of the snapshot).
    events: VecDeque<SimEvent>,
}

impl World {
    /// Build a world from linked content and a scenario, seeded by `seed`.
    ///
    /// Takes shared ownership of the [`Registry`] via `Arc` (clone it cheaply to
    /// keep a handle for rendering/tooling). Each system's `pricing` name was
    /// validated at link time, so it is resolved here via the same strategy set.
    /// Traders are placed on random systems using the seeded RNG.
    pub fn new(
        registry: Arc<Registry>,
        scenario: &ScenarioDef,
        seed: u64,
        pricing: &drift_core::NamedRegistry<PricingStrategy>,
    ) -> Result<Self, WorldError> {
        if registry.system_count() == 0 {
            return Err(WorldError::NoSystems);
        }

        // --- markets ---
        let mut markets = Vec::with_capacity(registry.system_count());
        for sys in registry.systems() {
            let strategy = *pricing
                .resolve(&sys.pricing)
                .expect("pricing validated at link time");

            let mut goods = std::collections::BTreeMap::new();
            for &(commodity, qty) in &sys.initial_stock {
                let def = registry.commodity(commodity);
                let price = strategy.price(def.base_price, qty, qty, def.elasticity);
                goods.insert(
                    commodity,
                    MarketGood {
                        stock: qty,
                        equilibrium: qty,
                        price,
                    },
                );
            }

            // Every commodity an industry touches must be tradeable here, or the
            // production phase would silently drop output / never run.
            for &rid in &sys.industries {
                let recipe = registry.recipe(rid);
                for (c, _) in recipe.inputs.iter().chain(recipe.outputs.iter()) {
                    if !goods.contains_key(c) {
                        return Err(WorldError::MissingIndustryCommodity {
                            system: registry.system_name(sys.id).to_string(),
                            commodity: registry.commodity_name(*c).to_string(),
                        });
                    }
                }
            }

            markets.push(Market {
                system: sys.id,
                pricing: strategy,
                goods,
            });
        }

        // --- traders ---
        let mut rng = DetRng::from_seed(seed);
        let nsys = registry.system_count();
        let mut next_trader_id = 0u64;
        let mut traders = Vec::with_capacity(scenario.traders.count as usize);
        if scenario.traders.count > 0 {
            // Only resolve the ship when traders are actually spawned, so a
            // zero-trader probe (e.g. content validation) needs no ship.
            let ship = registry
                .ship_id(&scenario.traders.ship)
                .ok_or_else(|| WorldError::UnknownShip(scenario.traders.ship.clone()))?;
            for _ in 0..scenario.traders.count {
                let at = SystemId(rng.range_usize(0, nsys) as u32);
                let id = TraderId(next_trader_id);
                next_trader_id += 1;
                traders.push(Trader::new(id, ship, scenario.traders.starting_capital, at));
            }
        }

        let progress: Vec<Vec<f64>> = registry
            .systems()
            .map(|s| vec![0.0f64; s.industries.len()])
            .collect();

        // --- piracy ---
        let mut pirates = Vec::new();
        let piracy = match &scenario.piracy {
            None => None,
            Some(cfg) => {
                let pirate_ship = registry
                    .ship_id(&cfg.pirate_ship)
                    .ok_or_else(|| WorldError::UnknownShip(cfg.pirate_ship.clone()))?;
                let runtime = PiracyRuntime {
                    pirate_ship,
                    base_ambush_chance: cfg.base_ambush_chance,
                    max_pirates: cfg.max_pirates.max(1),
                    respawn_delay: cfg.respawn_delay,
                    fleet_size: cfg.fleet_size,
                    bounty: cfg.bounty,
                    reinforce_interval: cfg.reinforce_interval.max(1),
                };
                pirates = spawn_fleet(&registry, &mut rng, pirate_ship, runtime.fleet_size);
                Some(runtime)
            }
        };

        // --- navy ---
        let mut navy = Vec::new();
        let navy_runtime = match &scenario.navy {
            None => None,
            Some(cfg) => {
                let navy_ship = registry
                    .ship_id(&cfg.ship)
                    .ok_or_else(|| WorldError::UnknownShip(cfg.ship.clone()))?;
                let runtime = NavyRuntime {
                    navy_ship,
                    fleet_size: cfg.fleet_size,
                    reinforce_interval: cfg.reinforce_interval.max(1),
                };
                navy = spawn_fleet(&registry, &mut rng, navy_ship, runtime.fleet_size);
                Some(runtime)
            }
        };

        // --- escorts ---
        let escort = match &scenario.escort {
            None => None,
            Some(cfg) => {
                let escort_ship = registry
                    .ship_id(&cfg.ship)
                    .ok_or_else(|| WorldError::UnknownShip(cfg.ship.clone()))?;
                Some(EscortRuntime {
                    escort_ship,
                    count: cfg.count,
                })
            }
        };

        Ok(World {
            registry,
            tick: Tick::ZERO,
            rng,
            markets,
            traders,
            progress,
            piracy,
            piracy_stats: PiracyStats::default(),
            pirates,
            navy_runtime,
            navy,
            escort,
            risk_aversion: scenario.risk_aversion,
            commands: Vec::new(),
            commands_applied: 0,
            commands_rejected: 0,
            last_errors: Vec::new(),
            next_trader_id,
            events: VecDeque::new(),
        })
    }

    /// Record a simulation event (trimming the oldest once the buffer is full).
    fn log_event(&mut self, category: EventCategory, message: String) {
        self.events.push_back(SimEvent {
            tick: self.tick,
            category,
            message,
        });
        if self.events.len() > EVENT_CAP {
            self.events.pop_front();
        }
    }

    pub fn tick_count(&self) -> Tick {
        self.tick
    }
    pub fn markets(&self) -> &[Market] {
        &self.markets
    }
    pub fn traders(&self) -> &[Trader] {
        &self.traders
    }
    pub fn registry(&self) -> &Registry {
        &self.registry
    }
    /// A cloned `Arc` handle to the shared registry (for a host that wants to keep
    /// its own reference alongside the world).
    pub fn registry_arc(&self) -> Arc<Registry> {
        Arc::clone(&self.registry)
    }
    pub fn piracy_stats(&self) -> PiracyStats {
        self.piracy_stats
    }
    pub fn pirates(&self) -> &[Patrol] {
        &self.pirates
    }
    pub fn navy(&self) -> &[Patrol] {
        &self.navy
    }
    pub fn commands_applied(&self) -> u64 {
        self.commands_applied
    }
    pub fn commands_rejected(&self) -> u64 {
        self.commands_rejected
    }
    /// Errors from commands rejected in the most recent tick's `command_phase`.
    /// Ephemeral UI feedback; cleared at the start of each `command_phase`.
    pub fn last_command_errors(&self) -> &[CommandError] {
        &self.last_errors
    }
    /// The recorded simulation events, oldest first (a bounded recent tail).
    pub fn events(
        &self,
    ) -> impl DoubleEndedIterator<Item = &SimEvent> + ExactSizeIterator {
        self.events.iter()
    }

    /// Enqueue a player command for the next tick. The single input entry point —
    /// local now, over the network in a multiplayer server. Commands are validated
    /// and applied in `command_phase`, not on receipt, so ordering is deterministic.
    pub fn queue_command(&mut self, command: Command) {
        self.commands.push(command);
    }

    pub fn snapshot(&self) -> Snapshot<'_> {
        Snapshot {
            tick: self.tick,
            rng: &self.rng,
            markets: &self.markets,
            traders: &self.traders,
            progress: &self.progress,
            piracy: self.piracy_stats,
            pirates: &self.pirates,
            navy: &self.navy,
            next_trader_id: self.next_trader_id,
        }
    }

    /// Advance the world by exactly one tick.
    ///
    /// Phase order: commands -> production -> price -> pirate movement -> navy
    /// (patrol + hunt) -> piracy (ambushes) -> trading. Player commands are applied
    /// first (inputs, then simulate); the navy thins the pirate presence before
    /// ambushes are rolled; and piracy runs before trading so a destroyed trader
    /// neither arrives nor trades this tick.
    pub fn tick(&mut self) {
        self.command_phase();
        self.production_phase();
        self.price_phase();
        self.pirate_phase();
        self.navy_phase();
        self.piracy_phase();
        self.trading_phase();
        self.tick = self.tick.next();
    }

    /// Run `n` ticks.
    pub fn run(&mut self, n: u64) {
        for _ in 0..n {
            self.tick();
        }
    }

    /// Drain and apply the queued player commands, in submission order. Rejected
    /// commands (invalid input) are counted, not fatal. A server would impose a
    /// canonical order across players before this runs; single-player order is the
    /// local submission order.
    fn command_phase(&mut self) {
        self.last_errors.clear();
        let commands = std::mem::take(&mut self.commands);
        for command in commands {
            match self.apply_command(command) {
                Ok(()) => self.commands_applied += 1,
                Err(e) => {
                    self.commands_rejected += 1;
                    self.last_errors.push(e);
                }
            }
        }
    }

    /// Validate and apply a single command against the world. Every precondition
    /// (ownership, reachability, funds, stock, hold capacity) is checked, because
    /// commands are untrusted input. Traders are addressed by stable [`TraderId`],
    /// resolved to the current slot at apply time.
    fn apply_command(&mut self, command: Command) -> Result<(), CommandError> {
        let reg = self.registry.clone();
        match command {
            Command::Spawn {
                player,
                ship,
                at,
                capital,
            } => {
                if ship.index() >= reg.ship_count() {
                    return Err(CommandError::UnknownShip);
                }
                if at.index() >= reg.system_count() {
                    return Err(CommandError::InvalidSystem);
                }
                let id = self.fresh_trader_id();
                self.traders.push(Trader::owned(id, ship, capital, at, player));
                Ok(())
            }

            Command::Despawn { player, trader } => {
                let idx = self.owned_trader_index(trader, player)?;
                self.traders.remove(idx); // order-preserving; ids stay valid
                Ok(())
            }

            Command::Jump {
                player,
                trader,
                dest,
            } => {
                let idx = self.owned_trader_index(trader, player)?;
                let sys = self.docked_system(idx)?;
                if dest.index() >= reg.system_count() {
                    return Err(CommandError::InvalidSystem);
                }
                if !reg.system(sys).connections.contains(&dest) {
                    return Err(CommandError::Unreachable);
                }
                let ship = self.traders[idx].ship;
                let travel = self.travel_ticks(sys, dest, ship);
                self.traders[idx].location = TraderLocation::InTransit {
                    origin: sys,
                    dest,
                    departure: self.tick,
                    arrival: Tick(self.tick.0 + travel),
                };
                Ok(())
            }

            Command::Buy {
                player,
                trader,
                commodity,
                qty,
            } => {
                if qty == 0 {
                    return Err(CommandError::ZeroQuantity);
                }
                let idx = self.owned_trader_index(trader, player)?;
                let sys = self.docked_system(idx)?;
                let market = &self.markets[sys.index()];
                let price = market.price(commodity).ok_or(CommandError::UnknownGood)?;
                if market.stock(commodity) < qty {
                    return Err(CommandError::InsufficientStock);
                }
                let cost = price * qty as i64;
                if self.traders[idx].capital < cost {
                    return Err(CommandError::InsufficientFunds);
                }
                // Check hold capacity (in mass units).
                let unit_mass = reg.commodity(commodity).unit_mass;
                let capacity = reg.ship(self.traders[idx].ship).cargo_capacity;
                let used: u32 = self.traders[idx]
                    .cargo
                    .iter()
                    .map(|(c, q)| q * reg.commodity(*c).unit_mass)
                    .sum();
                if used + qty * unit_mass > capacity {
                    return Err(CommandError::OverCapacity);
                }

                self.markets[sys.index()].try_remove(commodity, qty);
                self.traders[idx].capital -= cost;
                *self.traders[idx].cargo.entry(commodity).or_insert(0) += qty;
                Ok(())
            }

            Command::Sell {
                player,
                trader,
                commodity,
                qty,
            } => {
                if qty == 0 {
                    return Err(CommandError::ZeroQuantity);
                }
                let idx = self.owned_trader_index(trader, player)?;
                let sys = self.docked_system(idx)?;
                let held = self.traders[idx].cargo.get(&commodity).copied().unwrap_or(0);
                if held < qty {
                    return Err(CommandError::InsufficientCargo);
                }
                let price = self.markets[sys.index()]
                    .price(commodity)
                    .ok_or(CommandError::UnknownGood)?;

                self.markets[sys.index()].add(commodity, qty);
                self.traders[idx].capital += price * qty as i64;
                let remaining = held - qty;
                if remaining == 0 {
                    self.traders[idx].cargo.remove(&commodity);
                } else {
                    self.traders[idx].cargo.insert(commodity, remaining);
                }
                Ok(())
            }
        }
    }

    /// Allocate the next stable trader id.
    fn fresh_trader_id(&mut self) -> TraderId {
        let id = TraderId(self.next_trader_id);
        self.next_trader_id += 1;
        id
    }

    /// Resolve a `TraderId` to its current slot, checking it exists and is owned by
    /// `player`. A stale id (its trader removed) resolves to `UnknownTrader`.
    fn owned_trader_index(
        &self,
        id: TraderId,
        player: PlayerId,
    ) -> Result<usize, CommandError> {
        let idx = self
            .traders
            .iter()
            .position(|t| t.id == id)
            .ok_or(CommandError::UnknownTrader)?;
        if self.traders[idx].owner != Owner::Player(player) {
            return Err(CommandError::NotOwner);
        }
        Ok(idx)
    }

    /// The system a trader is docked at, or `NotDocked`.
    fn docked_system(&self, idx: usize) -> Result<SystemId, CommandError> {
        match self.traders[idx].location {
            TraderLocation::Docked(sys) => Ok(sys),
            _ => Err(CommandError::NotDocked),
        }
    }

    fn production_phase(&mut self) {
        let reg = self.registry.clone();
        for i in 0..self.markets.len() {
            let sys = reg.system(self.markets[i].system);
            for (j, &rid) in sys.industries.iter().enumerate() {
                let recipe = reg.recipe(rid);

                // Scale the nominal rate by the price-elastic response.
                let factor = match response_signal(recipe) {
                    Some((c, supply_side)) => {
                        let base = reg.commodity(c).base_price;
                        let price = self.markets[i].price(c).unwrap_or(base);
                        elastic_factor(recipe.elasticity, supply_side, base, price)
                    }
                    None => 1.0,
                };

                // Accumulate fractional throughput; apply the whole part; keep the
                // remainder. Cap the accumulator so a starved recipe cannot store
                // an unbounded burst.
                let cap = recipe.rate as f64 * MAX_ELASTIC_FACTOR + 1.0;
                let acc = (self.progress[i][j] + recipe.rate as f64 * factor).min(cap);
                let want = acc.floor();
                let applied = apply_recipe(&mut self.markets[i], recipe, want as u32);
                self.progress[i][j] = acc - applied as f64;
            }
        }
    }

    fn price_phase(&mut self) {
        let reg = self.registry.clone();
        for market in &mut self.markets {
            let strategy = market.pricing;
            for (&commodity, good) in market.goods.iter_mut() {
                let def = reg.commodity(commodity);
                let target =
                    strategy.price(def.base_price, good.stock, good.equilibrium, def.elasticity);
                // Sticky prices: ease toward the target instead of snapping, to
                // damp trade-induced oscillation.
                good.price = crate::pricing::smoothed(good.price, target);
            }
        }
    }

    /// Move the persistent pirate fleet (arrivals, shield regen, danger-weighted
    /// roaming, periodic reinforcement). No-op without a piracy config.
    fn pirate_phase(&mut self) {
        let Some(rt) = self.piracy.clone() else {
            return;
        };
        advance_fleet(
            &mut self.pirates,
            &self.registry,
            &mut self.rng,
            self.tick,
            rt.pirate_ship,
            rt.fleet_size,
            rt.reinforce_interval,
        );
    }

    /// Move the navy fleet the same way, then hunt: wherever navy and pirates are
    /// docked together, they fight and the navy thins the pirate presence. No-op
    /// without a navy config.
    fn navy_phase(&mut self) {
        let Some(rt) = self.navy_runtime.clone() else {
            return;
        };
        advance_fleet(
            &mut self.navy,
            &self.registry,
            &mut self.rng,
            self.tick,
            rt.navy_ship,
            rt.fleet_size,
            rt.reinforce_interval,
        );
        self.navy_hunt_pirates();
    }

    /// For every system where the navy is present, engage any pirates docked
    /// there in a combined encounter, applying persistent damage to both sides.
    fn navy_hunt_pirates(&mut self) {
        // Distinct systems the navy currently occupies (deterministic order).
        let mut systems: Vec<SystemId> =
            self.navy.iter().filter_map(Patrol::docked_at).collect();
        systems.sort_by_key(|s| s.0);
        systems.dedup();

        for sys in systems {
            let navy_idx: Vec<usize> = indices_docked_at(&self.navy, sys);
            let pir_idx: Vec<usize> = indices_docked_at(&self.pirates, sys);
            if navy_idx.is_empty() || pir_idx.is_empty() {
                continue;
            }

            // Navy (faction 0) versus pirates (faction 1), both with persistent state.
            let reg = self.registry.clone();
            let mut combatants = Vec::with_capacity(navy_idx.len() + pir_idx.len());
            for (k, &ni) in navy_idx.iter().enumerate() {
                combatants.push(patrol_combatant(&reg, &self.navy[ni], 0, side_pos(0.0, k)));
            }
            for (k, &pi) in pir_idx.iter().enumerate() {
                combatants.push(patrol_combatant(&reg, &self.pirates[pi], 1, side_pos(30.0, k)));
            }
            let mut enc = Encounter::new(combatants);
            enc.resolve(&mut self.rng, 500);

            // Write persistent state back and tally casualties.
            let mut navy_down = 0u32;
            for (k, &ni) in navy_idx.iter().enumerate() {
                let c = &enc.combatants()[k];
                self.navy[ni].hull = c.hull;
                self.navy[ni].shield = c.shield;
                if !c.alive {
                    self.piracy_stats.navy_lost += 1;
                    navy_down += 1;
                }
            }
            let base = navy_idx.len();
            let mut killed = 0u32;
            for (k, &pi) in pir_idx.iter().enumerate() {
                let c = &enc.combatants()[base + k];
                self.pirates[pi].hull = c.hull;
                self.pirates[pi].shield = c.shield;
                if !c.alive {
                    self.piracy_stats.pirates_suppressed += 1;
                    killed += 1;
                }
            }
            if killed > 0 || navy_down > 0 {
                let system = &reg.system(sys).name;
                let mut msg = format!("Navy engaged pirates at {system}: {killed} destroyed");
                if navy_down > 0 {
                    msg += &format!(", {navy_down} frigate(s) lost");
                }
                self.log_event(EventCategory::Navy, msg);
            }
        }

        self.navy.retain(Patrol::is_alive);
        self.pirates.retain(Patrol::is_alive);
    }

    /// Ambushes of laden, in-transit traders by pirates present at their
    /// destination. No-op without a piracy config. Dead pirates are culled at the
    /// end.
    fn piracy_phase(&mut self) {
        let Some(base) = self.piracy.as_ref().map(|p| p.base_ambush_chance) else {
            return;
        };

        for t in 0..self.traders.len() {
            let TraderLocation::InTransit { dest, .. } = self.traders[t].location else {
                continue;
            };
            // Pirates prey on cargo; an empty (deadheading) trader is ignored.
            if self.traders[t].cargo.is_empty() {
                continue;
            }
            // Ambush likelihood rises with how many live pirates lurk at the
            // destination; no pirates there means no ambush.
            let present = self
                .pirates
                .iter()
                .filter(|p| p.is_alive() && p.docked_at() == Some(dest))
                .count();
            if present == 0 {
                continue;
            }
            let chance = (base * present as f64).clamp(0.0, 1.0);
            if self.rng.unit_f64() < chance {
                self.resolve_ambush(t, dest);
            }
        }

        self.pirates.retain(Patrol::is_alive);
        self.navy.retain(Patrol::is_alive);
    }

    /// Resolve one ambush. The trader's side (faction 0) is the trader itself,
    /// plus its convoy escorts, plus any navy present at `dest`; the pirates
    /// (faction 1) are up to `max_pirates` live raiders docked there. Persistent
    /// combatants (navy, pirates) carry and keep their damage; escorts are fresh.
    /// The trader survives iff *its own* ship survives (escorts/navy winning does
    /// not save a dead trader). A surviving trader collects a bounty per pirate
    /// killed; a lost one forfeits its cargo and is destroyed.
    fn resolve_ambush(&mut self, t: usize, dest: SystemId) {
        let reg = self.registry.clone();
        let (max_pirates, respawn_delay, bounty) = {
            let p = self.piracy.as_ref().expect("piracy_phase gates on Some");
            (p.max_pirates, p.respawn_delay, p.bounty)
        };
        let escort = self.escort.clone();

        let engaged: Vec<usize> = self
            .pirates
            .iter()
            .enumerate()
            .filter(|(_, p)| p.is_alive() && p.docked_at() == Some(dest))
            .map(|(i, _)| i)
            .take(max_pirates as usize)
            .collect();
        if engaged.is_empty() {
            return;
        }
        // Navy present at the destination joins the defense.
        let navy_def: Vec<usize> = indices_docked_at(&self.navy, dest);

        // --- Faction 0: trader, then escorts, then navy defenders ---
        let mut combatants = Vec::new();
        let mut f0 = 0usize; // running index within faction 0 (for spacing)
        let trader_ship = self.traders[t].ship;
        let tdef = reg.ship(trader_ship);
        combatants.push(Combatant::new(
            trader_ship,
            0,
            tdef.combat.unwrap_or_default(),
            tdef.hull,
            tdef.max_speed,
            side_pos(0.0, f0),
        ));

        if let Some(e) = &escort {
            let edef = reg.ship(e.escort_ship);
            let estats = edef.combat.unwrap_or_default();
            for _ in 0..e.count {
                f0 += 1;
                combatants.push(Combatant::new(
                    e.escort_ship,
                    0,
                    estats,
                    edef.hull,
                    edef.max_speed,
                    side_pos(0.0, f0),
                ));
            }
        }

        let navy_offset = combatants.len();
        for &ni in &navy_def {
            f0 += 1;
            combatants.push(patrol_combatant(&reg, &self.navy[ni], 0, side_pos(0.0, f0)));
        }

        // --- Faction 1: pirates ---
        let pirate_offset = combatants.len();
        for (k, &pi) in engaged.iter().enumerate() {
            combatants.push(patrol_combatant(&reg, &self.pirates[pi], 1, side_pos(30.0, k)));
        }

        self.piracy_stats.ambushes += 1;
        let mut enc = Encounter::new(combatants);
        enc.resolve(&mut self.rng, 500);

        // Write navy defenders back.
        for (k, &ni) in navy_def.iter().enumerate() {
            let c = &enc.combatants()[navy_offset + k];
            self.navy[ni].hull = c.hull;
            self.navy[ni].shield = c.shield;
            if !c.alive {
                self.piracy_stats.navy_lost += 1;
            }
        }
        // Write pirates back; tally kills.
        let mut kills = 0u64;
        for (k, &pi) in engaged.iter().enumerate() {
            let c = &enc.combatants()[pirate_offset + k];
            self.pirates[pi].hull = c.hull;
            self.pirates[pi].shield = c.shield;
            if !c.alive {
                kills += 1;
            }
        }
        self.piracy_stats.pirates_destroyed += kills;

        // The trader lives or dies on its own hull (index 0), not the faction.
        let system = &reg.system(dest).name;
        let tid = self.traders[t].id.0;
        if enc.combatants()[0].alive {
            let reward = bounty * kills as i64;
            self.traders[t].capital += reward;
            self.piracy_stats.bounties_paid += reward;
            let msg = format!(
                "Ambush near {system}: trader #{tid} beat {} pirate(s), killed {kills} (+{reward}cr)",
                engaged.len()
            );
            self.log_event(EventCategory::Combat, msg);
        } else {
            self.piracy_stats.traders_lost += 1;
            self.traders[t].cargo.clear(); // shipment lost to the void
            self.traders[t].location = TraderLocation::Destroyed {
                respawn: Tick(self.tick.0 + respawn_delay),
            };
            let msg = format!("Ambush near {system}: trader #{tid} destroyed, cargo lost");
            self.log_event(EventCategory::Piracy, msg);
        }
    }

    fn trading_phase(&mut self) {
        let reg = self.registry.clone();
        let now = self.tick;
        let nsys = reg.system_count();

        for t in 0..self.traders.len() {
            // Respawn destroyed traders when their downtime elapses; otherwise
            // they take no action.
            if let TraderLocation::Destroyed { respawn } = self.traders[t].location {
                if now >= respawn {
                    let at = SystemId(self.rng.range_usize(0, nsys) as u32);
                    self.traders[t].location = TraderLocation::Docked(at);
                    let tid = self.traders[t].id.0;
                    let msg = format!("Trader #{tid} respawned at {}", reg.system(at).name);
                    self.log_event(EventCategory::System, msg);
                }
                continue;
            }

            // Resolve arrivals.
            if let TraderLocation::InTransit { dest, arrival, .. } = self.traders[t].location {
                if now >= arrival {
                    self.traders[t].location = TraderLocation::Docked(dest);
                } else {
                    continue; // still travelling
                }
            }

            let TraderLocation::Docked(sys) = self.traders[t].location else {
                continue;
            };

            // Player-owned traders have their arrivals/respawns resolved above but
            // take no autonomous action — they move and trade only via commands.
            if self.traders[t].is_player() {
                continue;
            }
            let sys_idx = sys.index();

            // If carrying cargo, sell it all here, then wait for next tick to buy.
            if !self.traders[t].cargo.is_empty() {
                self.sell_all(t, sys_idx);
                continue;
            }

            // Otherwise, look for a profitable outbound trade.
            let capital = self.traders[t].capital;
            let ship = self.traders[t].ship;
            let capacity = reg.ship(ship).cargo_capacity;

            let risk_aversion = self.risk_aversion;
            let plan = {
                let neighbor_ids = &reg.system(sys).connections;
                let neighbors: Vec<&Market> =
                    neighbor_ids.iter().map(|id| &self.markets[id.index()]).collect();
                let here = &self.markets[sys_idx];
                choose_trade(
                    here,
                    &neighbors,
                    capital,
                    capacity,
                    |c| reg.commodity(c).unit_mass,
                    |s| reg.system(s).danger,
                    risk_aversion,
                )
            };

            if let Some(plan) = plan {
                // Buy: draw stock from here (raising its price next repricing).
                let ok = self.markets[sys_idx].try_remove(plan.commodity, plan.qty);
                debug_assert!(ok, "choose_trade bounded qty by available stock");
                let cost: Money = plan.qty as i64 * plan.unit_cost;
                self.traders[t].capital -= cost;
                *self.traders[t].cargo.entry(plan.commodity).or_insert(0) += plan.qty;

                // Depart toward the destination.
                let travel = self.travel_ticks(sys, plan.dest, ship);
                self.traders[t].location = TraderLocation::InTransit {
                    origin: sys,
                    dest: plan.dest,
                    departure: now,
                    arrival: Tick(now.0 + travel),
                };
            } else if let Some(dest) = self.reposition_target(sys, capital, capacity) {
                // No trade available here (e.g. docked at a starved consumer with
                // nothing to buy). Deadhead empty toward opportunity instead of
                // stranding — otherwise the fleet piles up at consumers, transport
                // collapses, and producers glut while consumers starve.
                let travel = self.travel_ticks(sys, dest, ship);
                self.traders[t].location = TraderLocation::InTransit {
                    origin: sys,
                    dest,
                    departure: now,
                    arrival: Tick(now.0 + travel),
                };
            }
        }
    }

    /// Choose a neighbor to deadhead to when no trade is available here. Ranks
    /// neighbors by the best trade obtainable *from* that neighbor (one-hop
    /// lookahead), so traders drift toward producers/opportunity. Falls back to a
    /// seeded-random neighbor to escape dead pockets where no neighbor offers a
    /// trade. Returns `None` only if the system has no connections.
    fn reposition_target(
        &mut self,
        sys: SystemId,
        capital: Money,
        capacity: u32,
    ) -> Option<SystemId> {
        let reg = self.registry.clone();
        let risk_aversion = self.risk_aversion;
        let connections = &reg.system(sys).connections;
        if connections.is_empty() {
            return None;
        }

        // Deadheading itself is empty (unladen traders are never ambushed), so the
        // immediate jump to `n` carries no risk. We rank neighbors by the
        // risk-adjusted value of the onward trade available *from* each `n`.
        let mut best: Option<(SystemId, Money)> = None;
        for &n in connections {
            let onward = &reg.system(n).connections;
            let onward_markets: Vec<&Market> =
                onward.iter().map(|id| &self.markets[id.index()]).collect();
            let here = &self.markets[n.index()];
            if let Some(plan) = choose_trade(
                here,
                &onward_markets,
                capital,
                capacity,
                |c| reg.commodity(c).unit_mass,
                |s| reg.system(s).danger,
                risk_aversion,
            ) {
                let better = match best {
                    None => true,
                    Some((bn, bp)) => plan.score > bp || (plan.score == bp && n < bn),
                };
                if better {
                    best = Some((n, plan.score));
                }
            }
        }

        Some(match best {
            Some((n, _)) => n,
            None => connections[self.rng.range_usize(0, connections.len())],
        })
    }

    /// Sell every unit of a trader's cargo into the market it is docked at,
    /// crediting the trader. Goods the market does not trade are retained (should
    /// not happen: a trader only buys goods to sell at a neighbor that trades them).
    fn sell_all(&mut self, trader: usize, sys_idx: usize) {
        let cargo = std::mem::take(&mut self.traders[trader].cargo);
        let mut leftover = std::collections::BTreeMap::new();
        for (commodity, qty) in cargo {
            if let Some(price) = self.markets[sys_idx].price(commodity) {
                self.markets[sys_idx].add(commodity, qty);
                self.traders[trader].capital += qty as i64 * price;
            } else {
                leftover.insert(commodity, qty);
            }
        }
        self.traders[trader].cargo = leftover;
    }

    /// Whole-tick travel time between two systems for a given ship.
    fn travel_ticks(&self, from: SystemId, to: SystemId, ship: ShipId) -> u64 {
        travel_between(&self.registry, from, to, ship)
    }
}

/// Euclidean jump distance divided by the ship's jump speed, rounded up, at
/// least 1.
fn travel_between(reg: &Registry, from: SystemId, to: SystemId, ship: ShipId) -> u64 {
    let a = reg.system(from).position;
    let b = reg.system(to).position;
    let dist = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
    let speed = reg.ship(ship).jump_speed.max(1e-6);
    ((dist / speed).ceil() as u64).max(1)
}

/// Spawn `size` fresh patrols at danger-weighted lawless systems (fewer if the
/// galaxy has few/no dangerous systems).
fn spawn_fleet(reg: &Registry, rng: &mut DetRng, ship: ShipId, size: u32) -> Vec<Patrol> {
    let def = reg.ship(ship);
    let stats = def.combat.unwrap_or_default();
    let mut fleet = Vec::new();
    for _ in 0..size {
        if let Some(sys) = pick_danger_system(reg, rng) {
            fleet.push(Patrol::new(ship, &stats, def.hull, sys));
        }
    }
    fleet
}

/// Advance a patrol fleet by one tick: resolve arrivals, regenerate shields, let
/// docked patrols roam toward lawless space, and periodically reinforce up to
/// `fleet_size`. Shared by pirates and navy.
fn advance_fleet(
    fleet: &mut Vec<Patrol>,
    reg: &Registry,
    rng: &mut DetRng,
    now: Tick,
    ship: ShipId,
    fleet_size: u32,
    reinforce_interval: u64,
) {
    let def = reg.ship(ship);
    let stats = def.combat.unwrap_or_default();

    for patrol in fleet.iter_mut() {
        if let PatrolLocation::InTransit { dest, arrival, .. } = patrol.location {
            if now >= arrival {
                patrol.location = PatrolLocation::Docked(dest);
            }
        }
        patrol.regen_shield(&stats);
        if let PatrolLocation::Docked(sys) = patrol.location {
            if rng.unit_f64() < ROAM_CHANCE {
                if let Some(dest) = pick_roam_neighbor(reg, sys, rng) {
                    let travel = travel_between(reg, sys, dest, ship);
                    patrol.location = PatrolLocation::InTransit {
                        origin: sys,
                        dest,
                        departure: now,
                        arrival: Tick(now.0 + travel),
                    };
                }
            }
        }
    }

    if now.0.is_multiple_of(reinforce_interval) {
        while fleet.len() < fleet_size as usize {
            let Some(sys) = pick_danger_system(reg, rng) else {
                break;
            };
            fleet.push(Patrol::new(ship, &stats, def.hull, sys));
        }
    }
}

/// Indices of live patrols docked at `sys` (deterministic order).
fn indices_docked_at(fleet: &[Patrol], sys: SystemId) -> Vec<usize> {
    fleet
        .iter()
        .enumerate()
        .filter(|(_, p)| p.is_alive() && p.docked_at() == Some(sys))
        .map(|(i, _)| i)
        .collect()
}

/// Build a combatant from a patrol, carrying its persistent hull/shield.
fn patrol_combatant(reg: &Registry, p: &Patrol, faction: u8, pos: Vec2) -> Combatant {
    let def = reg.ship(p.ship);
    let mut c = Combatant::new(
        p.ship,
        faction,
        def.combat.unwrap_or_default(),
        def.hull,
        def.max_speed,
        pos,
    );
    c.hull = p.hull;
    c.shield = p.shield;
    c
}

/// A spawn position for combatant `k` on the side at x-offset `x`, spread along y
/// so both sides start within weapon range.
fn side_pos(x: f64, k: usize) -> Vec2 {
    Vec2::new(x, k as f64 * 8.0)
}

/// Deterministically pick a system id from `(id, weight)` pairs in proportion to
/// weight. Returns `None` if the total weight is non-positive.
fn weighted_pick(items: &[(SystemId, f64)], rng: &mut DetRng) -> Option<SystemId> {
    let total: f64 = items.iter().map(|(_, w)| w.max(0.0)).sum();
    if total <= 0.0 {
        return None;
    }
    let mut r = rng.unit_f64() * total;
    for (id, w) in items {
        r -= w.max(0.0);
        if r < 0.0 {
            return Some(*id);
        }
    }
    items.last().map(|(id, _)| *id) // floating-point guard
}

/// A lawless (`danger > 0`) system, weighted by danger. `None` if the galaxy has
/// no dangerous systems — so those galaxies never get pirates.
fn pick_danger_system(reg: &Registry, rng: &mut DetRng) -> Option<SystemId> {
    let candidates: Vec<(SystemId, f64)> = reg
        .systems()
        .filter(|s| s.danger > 0.0)
        .map(|s| (s.id, s.danger))
        .collect();
    weighted_pick(&candidates, rng)
}

/// A danger-weighted neighbor to roam to. Restricted to `danger > 0` neighbors so
/// pirates stay confined to lawless space (safe systems remain genuinely safe).
/// `None` means "stay put".
fn pick_roam_neighbor(reg: &Registry, sys: SystemId, rng: &mut DetRng) -> Option<SystemId> {
    let candidates: Vec<(SystemId, f64)> = reg
        .system(sys)
        .connections
        .iter()
        .map(|&n| (n, reg.system(n).danger))
        .filter(|(_, d)| *d > 0.0)
        .collect();
    weighted_pick(&candidates, rng)
}
