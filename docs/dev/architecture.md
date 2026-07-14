# Architecture

The current architecture of `drift`, a Rust Elite/Oolite variant. This describes
what exists today: a headless, deterministic, data-driven simulation with no
renderer. For where it is going (including the graphical client), see
[roadmap.md](./roadmap.md).

## Crate dependency graph

The workspace is a strict dependency DAG. Nothing depends on a renderer, and the
two simulation crates (`drift-economy`, `drift-combat`) contain no I/O. That
renderer-agnostic property is what makes a graphical client cheap to add later.

```
drift-core        primitives: typed ids + interning, DetRng, Tick, Money,
   ^              the Step/SimContext seam, NamedRegistry (plugin seam)
   |
drift-data        moddable schema (pure serde): CommodityDef, ShipDef+CombatStats,
   ^              SystemDef(+danger), ProductionRecipe(+elasticity), ScenarioDef
   |              (+piracy / navy / escort / risk_aversion)
   +---------------+
drift-mods      drift-combat     mods: discover -> toposort -> merge -> link -> Registry
   ^              ^              combat: Vec2, Combatant, Encounter (impl Step)
   |              |
drift-economy ---+   the World: markets, pricing, production, traders, pirates,
   ^                 navy, escorts; owns the RNG and the tick pipeline
   |
drift-cli            headless driver: validate / run / inspect / battle
```

| Crate | Responsibility |
|---|---|
| `drift-core` | Primitives: typed ids + interning, deterministic RNG, `Tick`, `Money`, the `Step`/`SimContext` seam, and the `NamedRegistry` plugin seam. |
| `drift-data` | The moddable content schema (pure serde definitions). |
| `drift-mods` | The mod-loader: discover, dependency-order, merge, and link content into an immutable `Registry`. |
| `drift-economy` | The simulation: markets, pricing, production, NPC traders, pirates, navy, escorts, and the `World` that owns the RNG and drives the tick. |
| `drift-combat` | The 2-D combat model: factions, targeting AI, hitscan weapons, shields, encounter resolution. |
| `drift-sim` | The session/driver layer: `Session` owns a `World` and centralizes loading, command application, ticking, per-tick event draining, and snapshots for hosts. |
| `drift-proto` | The client/server wire contract (no I/O): the `ClientMessage`/`ServerMessage` types, length-prefixed JSON framing, and `WorldView` (the owned mirror a client deserializes a broadcast snapshot into). |
| `drift-server` | The authoritative networked server: a `Session` plus a TCP socket. Accepts serialized `Command`s from clients, ticks at a fixed low rate, and broadcasts state (events every tick, full snapshots periodically). |
| `drift-cli` | Driver exposing `validate`, `run`, `inspect`, `battle`, and `play` (over `Session`). |
| `drift-client` | Graphical client (egui/eframe): renders from a read-model fed by either an in-process `Session` or a networked `drift-server` (`--connect`), and drives player commands (launch / buy / sell / jump / retire) from a Pilot panel through the same command sink in both modes. |

## The three load-bearing patterns

### Headless and deterministic

The whole simulation is a seeded, discrete-tick state machine. `World::tick()`
runs a fixed phase pipeline:

```
production -> price -> pirate movement -> navy (patrol + hunt) -> piracy (ambushes) -> trading
```

The world owns its `DetRng`, so the same seed produces a byte-identical run. This
is enforced by a determinism test and by a serializable `snapshot()` (content
uses RON; state dumps use JSON, which supports the RNG's 128-bit counter).

### Data-driven content and a name-keyed plugin seam

All content is authored as RON and linked into an immutable `Registry`. Behavior
that mods may vary (pricing today) is referenced from content by name
(`pricing: "supply_demand_v1"`) and resolved through a `NamedRegistry` of built-in
strategies. The loader validates those names against what the engine can execute,
so a typo fails fast at load. Because the data model is already fully
data-addressable and versioned, a future WASM/Lua scripting runtime plugs into the
same seam with no schema or caller changes.

### Agents over an abstract galaxy graph

This is the most important property to internalize. Traders, pirates, and navy
ships are **graph agents**, not spatial ships. An agent's location is either
`Docked(system)` or `InTransit { dest, arrival_tick }`. The only real 2-D
coordinates in the simulation are:

- **System positions** (galaxy-map coordinates, `[f64; 2]` per `SystemDef`), and
- **Combat positions** inside an `Encounter`, which are ephemeral: created and
  discarded within the single tick in which a fight resolves.

There is no continuous, in-system ship kinematics between systems. The economy is
an abstract graph simulation. This shapes what a client can render (see the
roadmap).

## Feature summary (what the simulation models today)

- **Dynamic supply/demand pricing** with sticky (eased) prices to damp
  trade-induced boom/bust cycles.
- **Price-elastic demand**: consumers and refiners buy less of a good when it is
  locally dear, capping scarcity prices at an interior equilibrium.
- **Production chains** (`ore -> alloys -> machinery -> luxuries`, plus raw
  production and population consumption).
- **NPC trader economy**: greedy buy-low/sell-high agents that self-correct
  shortages and gluts; idle traders deadhead toward opportunity.
- **Risk-aware routing**: traders discount a run's profit by destination danger
  (`risk_aversion`), trading fewer losses for underserving the frontier.
- **2-D combat**: nearest-enemy targeting, steering to engagement range, hitscan
  weapons with distance-based accuracy against regenerating shields and hull.
- **Persistent pirate fleets and bounties**: roaming agents that congregate in
  lawless (`danger > 0`) systems, carry persistent battle damage, are reinforced
  to a target fleet size, and ambush laden traders. Danger is emergent (clear a
  route and it stays safe until pirates return); a danger-free galaxy spawns none.
- **Escorts and navy patrols**: convoy escorts join a trader's side in an ambush;
  a persistent navy fleet hunts pirates and defends traders. Both make ambushes
  survivable, grind the pirate fleet down, and keep the frontier supplied, which
  visibly lowers manufactured-goods prices.

## The client-facing read model

A graphical client (or any observer) reads the world through the `World`
accessors, which already provide a complete read model. `World` also exposes a
serializable `Snapshot`.

| Accessor | Data | Natural visual mapping |
|---|---|---|
| `registry().systems()` | id, name, `position[2]`, `danger`, `connections` | galaxy nodes and jump edges |
| `markets()` | per-good `{ stock, equilibrium, price }` | market panels / price heatmap |
| `traders()` | `{ ship, capital, location, cargo }` | dots moving along edges; wealth |
| `pirates()` / `navy()` | `Patrol { ship, location, hull, shield }` | hostile / friendly dots in lawless space |
| `piracy_stats()` | ambushes, losses, bounties, suppressions | HUD counters |
| `tick_count()` | current `Tick` | clock |
| `snapshot()` | serializable view of all mutable state | persistence / external tooling |

Drive API: `World::tick()` advances one tick; `World::run(n)` advances `n`. Both
are deterministic given the seed.

**Event log.** `World::events()` returns a bounded, deterministic stream of
`SimEvent { tick, category, message }` recorded as the sim runs (ambush win/loss,
navy suppression battles, respawns). It is ephemeral debug/observability output —
excluded from the snapshot, never fed back into the sim — read by the client's log
panel and `drift-cli run --log`.

### Interpolation support

The `InTransit { origin, dest, departure, arrival }` variants (trader and patrol)
carry the origin system and departure tick, so a client can compute the fraction
travelled — `progress = (now - departure) / (arrival - departure)` — and lerp an
agent's position along its jump edge. `drift-client` uses this (with a sub-tick
fraction from its fixed-timestep accumulator) to animate ships smoothly between
systems.
