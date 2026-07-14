# TODO

Outstanding work for Drift. For rationale and sequencing see
[docs/dev/roadmap.md](docs/dev/roadmap.md); for the multiplayer plan see
[docs/dev/multiplayer.md](docs/dev/multiplayer.md). For what is already built, see
[CHANGELOG.md](CHANGELOG.md).

## Near-term (highest leverage)

- [ ] Verify `drift-client` renders on a real display — it is currently
      compile-checked and unit-tested only (no GUI in the dev sandbox).
- [ ] Graphical client: per-node **market panels** (click *any* system to show its
      prices, stock vs. equilibrium, and production chains). Market data is already
      on the wire (`WorldView.markets`) and shown for the docked system in the Pilot
      panel; this is the click-any-node generalization.
- [ ] Graphical client: on-map **combat flashes** / ambush markers, so fights are
      visible where they happen (not only in the log).

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

- [x] Server-authoritative networking transport (`drift-server`: TCP +
      length-prefixed JSON over the command pipeline; std threads, no async).
- [x] Networked **client** (`drift-client --connect`): an owned `WorldView` mirror
      that applies server broadcasts and renders (shared wire contract in
      `drift-proto`).
- [x] Graphical **player** client: a Pilot panel drives launch / buy / sell / jump
      / retire from the UI, through one command sink (local `Session` or server),
      round-trip tested in both modes.
- [ ] Content-version handshake: the client loads mods locally and assumes they
      match the server; send a content hash on connect to detect a mismatch.
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
