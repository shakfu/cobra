# TODO

Outstanding work for Drift. For rationale and sequencing see
[docs/dev/roadmap.md](docs/dev/roadmap.md); for the multiplayer plan see
[docs/dev/multiplayer.md](docs/dev/multiplayer.md). For what is already built, see
[CHANGELOG.md](CHANGELOG.md).

## Near-term (highest leverage)

- [ ] Verify `drift-client` renders on a real display — it is currently
      compile-checked and unit-tested only (no GUI in the dev sandbox).
- [ ] Graphical client: per-node **market panels** (click a system to show its
      prices, stock vs. equilibrium, and production chains).
- [ ] Graphical client: on-map **combat flashes** / ambush markers, so fights are
      visible where they happen (not only in the log).
- [ ] Factor a `drift-sim` session/driver type that owns the `World`, applies
      commands, and emits snapshots — reused by the CLI, a future server, and
      in-process single-player.

## Graphical client polish

- [ ] Pan/zoom the galaxy map.
- [ ] Ship visuals as data (a `sprite`/`color` field on `ShipDef`, or a client-side
      asset manifest keyed by ship id).
- [ ] A graphical **player** client (input + HUD) over the command pipeline.

## Gameplay depth

- [ ] Missions and contracts (cargo runs, bounty contracts, courier jobs) on top of
      the existing bounty/economy plumbing.
- [ ] Financial instruments (futures, loans, insurance) — the "sophisticated
      trading" differentiator.
- [ ] Escort fees / navy funding as real economic costs (protection is currently
      free).
- [ ] Multi-tick running battles (encounters currently resolve instantly within a
      single economy tick).

## Multiplayer

- [ ] Server-authoritative networking transport (build on the command pipeline).
- [ ] Snapshot delta encoding + interest management (needed only at scale).
- [ ] Client prediction / rollback (needed only for a real-time flight layer).
- [ ] Optional hardening: generational `TraderId` (the current monotonic,
      never-reused id is already ABA-safe; revisit only if it becomes a bottleneck).

## Modding / scripting

- [ ] WASM (extism) or Lua scripting runtime plugging into the `NamedRegistry` seam
      (data model is already ready; no schema change required).

## Content and balance

- [ ] Larger galaxy: more commodities, ships, and systems.
- [ ] Deepen the economy further (the core differentiator).

## Observability and tooling

- [ ] Richer event set (large NPC trades, reinforcements) and optional event-to-file.
- [ ] CI running `make test` and `make lint`.
