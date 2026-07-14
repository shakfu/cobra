# Changelog

All notable changes to Drift are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
the project aims to follow [Semantic Versioning](https://semver.org/). Drift is
pre-1.0 and unreleased; everything below is development toward the first release.

## [Unreleased]

### Added

Core simulation

- Deterministic, headless economy core: a single seeded RNG drives a discrete tick
  loop, so the same seed produces a byte-identical run. Serializable `Snapshot` for
  state dumps and resume.
- Cargo workspace: `drift-core` (typed ids + interning, `DetRng`, tick, money, the
  `Step`/`SimContext` seam, the `NamedRegistry` plugin seam), `drift-data` (moddable
  serde schema), `drift-mods` (mod-loader), `drift-economy` (the `World`),
  `drift-combat`, `drift-sim` (the session/driver layer), `drift-proto` (the
  client/server wire contract), `drift-server` (the authoritative networked host),
  `drift-cli`, `drift-client`.
- `drift-sim::Session`: a session/driver façade that owns a `World` and centralizes
  loading, command application, ticking, per-tick event draining, and snapshots.
  Both the CLI and the graphical client drive it; a server would use the same façade.

Data-driven mods

- RON content (commodities, recipes, systems, ships) loaded through a mod-loader
  with dependency ordering, explicit `overrides` rules, and fail-fast link-time
  validation of every cross-reference into an immutable `Registry`.
- Name-keyed plugin seam: content references behavior by name (e.g. a pricing
  strategy), validated against what the engine can execute — ready for a future
  WASM/Lua runtime with no schema change.

Economy

- Dynamic supply/demand pricing with sticky (eased) prices to damp trade-induced
  boom/bust cycles.
- Price-elastic demand: consumers and refiners buy less of a good when it is
  locally dear, capping scarcity prices at an interior equilibrium.
- Production chains (`ore -> alloys -> machinery -> luxuries`) plus raw production
  and population consumption.
- NPC trader economy: greedy buy-low/sell-high agents that self-correct shortages
  and gluts; idle traders deadhead toward opportunity.
- Risk-aware routing: traders discount a run's profit by destination danger
  (`risk_aversion`), losing fewer ships at the cost of underserving the frontier.

Combat and factions

- 2-D combat model (`drift-combat`): faction targeting AI, steering to engagement
  range, hitscan weapons with distance-based accuracy, regenerating shields and
  hull, and deterministic encounter resolution.
- Persistent roaming pirate fleets with bounties. Danger is emergent — clearing a
  route of pirates keeps it safe until they return, and a danger-free galaxy spawns
  none.
- Trader escorts (convoy protection) and a persistent navy fleet that hunts pirates
  on patrol and defends traders under ambush.

Player and clients

- Command pipeline: player actions are validated `Command`s applied at a tick
  boundary; agent ownership (`Owner`); stable, never-reused `TraderId` handles. The
  multiplayer-ready input path (single-player is the N=1 case).
- Interactive player CLI (`drift play`): fly a trader through the living galaxy
  (buy/sell/jump/wait/status/map), with pirate ambushes and bounties narrated in
  transit.
- Graphical observer client (`drift-client`, egui/eframe): a live galaxy-map view
  (systems coloured by danger, jump edges, agents animated along their routes) over
  a fixed-timestep sim loop, with pause/speed controls, a piracy HUD, and a
  colour-coded, per-category-filterable event log.
- CLI subcommands: `validate`, `run` (with `--dump`, `--log`, `--log-stream`),
  `inspect`, `battle`, `play`.
- Authoritative networked server (`drift-server`): a `Session` plus a TCP socket.
  Clients connect, send serialized `Command`s, and receive state; the server ticks
  the one canonical world at a fixed low rate and broadcasts each tick's events,
  with a full snapshot on connect and every `snapshot_every` ticks. Length-prefixed
  JSON framing (reusing the existing serde `Command`/`SimEvent`), `std`-threads only
  (no async runtime), one thread mutating the world so determinism holds.
- Shared wire-contract crate (`drift-proto`): the `ClientMessage`/`ServerMessage`
  types, the length-prefixed JSON framing, and `WorldView` — the owned mirror a
  client deserializes a broadcast snapshot into (the server sends a borrowed,
  serialize-only `Snapshot`).
- Networked client mode (`drift-client --connect <addr>`): the graphical client
  now renders from a read-model fed by either an in-process `Session` or a remote
  server. A background thread receives broadcasts into an owned `WorldView` and a
  bounded event log; the client interpolates agent motion between the server's
  ticks.
- Player controls (Pilot panel): launch a ship, buy/sell against the docked
  market, jump to a connected system, or retire — issued through one command sink
  that queues on the in-process `Session` (single-player) or sends to the server
  (networked). `WorldView` now carries per-system markets so the client can price
  buys and sells. The player finds its own trader by owner in the received state.

Observability

- Simulation event log: a deterministic `SimEvent` stream (ambush win/loss, navy
  suppression battles, respawns) read via `World::events()`, shown in the client
  log panel, printed as a tail by `run --log`, and streamed live tick-by-tick to
  stdout by `run --log-stream`.

Content and docs

- Core mod: an 8-system production-chain galaxy with lawless frontier systems.
- Scenarios: `equilibrium` (law-enforced, with navy and escorts) and `frontier`
  (lawless hard mode: heavy piracy, no navy or escorts).
- Developer docs under `docs/dev/`: `architecture.md`, `roadmap.md`,
  `multiplayer.md`.

### Changed

- Renamed the project from `cobra` to `drift` (crates `drift-*`, binary `drift`).
  The "Cobra Mk III" ship keeps its name — that is Elite content, not the project
  name.
- `World` now owns `Arc<Registry>` (no `'r` lifetime), so it can be held in a
  long-lived client app or server session.
- The `InTransit` variants of `TraderLocation`/`PatrolLocation` now carry `origin`
  and `departure`, so a client can interpolate an agent's position along its jump
  edge.
- Trade route selection ranks candidates by risk-adjusted expected value rather
  than raw profit.

### Fixed

- Mod dependency toposort produced a reversed load order.
- Discrete, lumpy trading induced a price limit cycle pinned at the clamp; resolved
  with sticky prices.
- Traders could strand at starved systems (they could only depart by buying, and a
  starved system has nothing to buy); they now deadhead toward opportunity.
- Supply elasticity keyed on a producer's own (glutted, cheap) local price
  perversely throttled exports; demand-side elasticity on the consumed good is used
  instead, which also gives intermediate goods a price-restoring force.
