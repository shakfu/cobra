# drift

A Rust take on the Elite/Oolite space sim, differentiated by a deeper trading economy and a mod system rather than by reimplementing 3D combat. This repository is the **Milestone 1: headless economy core** — a deterministic, testable simulation with no renderer. A graphical client (Bevy) is a later milestone.

## What runs today

A galaxy of star systems, each a specialized marketplace, is loaded entirely from mods. NPC traders arbitrage price differences between systems; their buying and selling is exactly what pulls prices back toward equilibrium. Left to run with no player, the economy settles into a stable, plausible price structure.

- **Dynamic supply/demand pricing** — prices follow stock relative to an equilibrium anchor, not fixed tables. Prices are sticky (eased toward target) to keep discrete trading from inducing boom/bust cycles.

- **Price-elastic demand** — consumers (population and refiners alike) buy less of a good when it is locally dear. This negative feedback caps scarcity prices at an interior equilibrium instead of pinning a chronically short good at its clamp.

- **Production chains** — `ore -> alloys -> machinery -> luxuries`, plus raw farming/mining and population consumption. Specialization creates the trade.

- **NPC trader economy** — greedy buy-low/sell-high agents whose flows self-correct shortages and gluts; idle traders deadhead toward opportunity rather than stranding at starved systems.

- **Risk-aware routing** — traders value a run by its risk-adjusted expected value, discounting profit by the destination's danger (`risk_aversion`). Cautious traders shun valuable cargo runs into lawless space: they lose far fewer ships (and end up richer), at the cost of underserving the frontier.

- **2-D combat** — squadrons of ships across factions target the nearest enemy, steer to engagement range, and fire hitscan weapons with distance-based accuracy against regenerating shields and hull. Encounters resolve deterministically to a victor, drawing on the same seeded RNG.

- **Persistent pirate fleets & bounties** — pirates are first-class roaming agents that congregate in lawless (`danger > 0`) systems, carry persistent battle damage between fights, and are periodically reinforced toward a target fleet size. A laden trader arriving at a pirate-held system may be ambushed in a real combat encounter; a victorious trader collects a **bounty** per kill. Danger is now emergent — clear a route of pirates and it stays safe until they return — and a fully-safe galaxy never spawns any. Combat and economy are one system: losses choke frontier supply and push manufactured-goods prices up.

- **Escorts & navy patrols** — convoys can hire escort fighters that join the trader's side in an ambush, and a persistent navy fleet patrols the frontier: it hunts pirates where it finds them (thinning their numbers) and defends traders under attack. With both, ambushes become survivable, pirates are ground down, and the frontier stays supplied — visibly lowering manufactured-goods prices. Law enforcement pays for itself in the economy.

- **Data-driven mods** — all content (commodities, recipes, systems, ships) is authored as RON and loaded through a mod-loader with dependency ordering, explicit override rules, and fail-fast link-time validation.

- **Determinism** — a seeded RNG drives the whole simulation; the same seed produces a byte-identical run.

## Workspace

| Crate           | Responsibility                                                        |
|-----------------|-----------------------------------------------------------------------|
| `drift-core`    | Primitives: typed ids + interning, deterministic RNG, tick, money, the `Step` seam, and the `NamedRegistry` plugin seam. |
| `drift-data`    | The moddable content schema (pure serde defs).                        |
| `drift-mods`    | The mod-loader: discover, order, merge, and link content into an immutable `Registry`. |
| `drift-economy` | The simulation: markets, pricing, production, NPC traders, the `World`. |
| `drift-combat`  | 2-D combat model: factions, targeting AI, hitscan weapons, shields, encounter resolution. |
| `drift-cli`     | Headless driver: `validate`, `run`, `inspect`, `battle`.             |

## The plugin seam

Behavior that mods may vary (currently pricing) is referenced from content by *name* (`pricing: "supply_demand_v1"`) and resolved through a registry of built-in strategies. The loader validates those names against what the engine can execute, so content fails fast on a typo. The data model is already fully data-addressable and versioned, so a future WASM/Lua scripting runtime plugs into the same seam with no schema or caller changes.

## Usage

```sh
make test                 # full workspace test suite (must stay green)
make validate             # load + link the bundled mods, report errors
make run                  # run the equilibrium scenario (override: make run TICKS=5000 SEED=7)

# Watch prices converge:
cargo run -p drift-cli -- inspect --mods mods/ --scenario scenarios/equilibrium.ron --ticks 2000 --every 200

# Deterministic state dump (identical for a fixed seed):
cargo run -p drift-cli -- run --mods mods/ --scenario scenarios/equilibrium.ron --seed 42 --dump state.json

# Stage a standalone combat encounter (squadron vs squadron):
cargo run -p drift-cli -- battle --mods mods/ --ship core:python --vs core:cobra_mk3 --per-side 3 --seed 1

# Watch piracy disrupt the frontier trade (the equilibrium scenario enables it):
cargo run -p drift-cli -- run --mods mods/ --scenario scenarios/equilibrium.ron --ticks 3000
```

## Content and formats

Mods live under `mods/<id>/` with a `manifest.toml` and RON content in `commodities/`, `production/`, `systems/`, `ships/`. Human-authored content uses RON; machine state dumps use JSON (which supports the RNG's 128-bit counter).

## Deferred (not cancelled)

3D/Bevy client; WASM/Lua scripting; the player agent, missions, and contracts; financial instruments (futures, loans, insurance); escort fees / navy funding as an economic cost; and multi-tick running battles (encounters currently resolve instantly within a single economy tick).
