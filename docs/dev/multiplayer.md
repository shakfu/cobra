# Multiplayer readiness

How `drift` is designed to scale to multiplayer, and what has been provisioned for
it so far. For the current architecture see [architecture.md](./architecture.md);
for the broader plan see [roadmap.md](./roadmap.md).

The intent is **not** to build networking now. It is to make a small number of
design decisions that keep multiplayer a *layering* project rather than a rewrite.

## Where we already are

The single most valuable property for networked simulation is **determinism**, and
it already exists, tested and guarded:

- No wall-clock, no `HashMap` iteration, no `thread_rng` in simulation logic; a
  single seeded `DetRng`. Same seed produces a byte-identical run.
- A **headless core** (the simulation runs with no renderer) — exactly what a
  dedicated server needs.
- A **discrete tick** simulation — networking keys naturally off ticks (input
  scheduling, ordering, replay all reference ticks).
- A serializable **`snapshot()`** — the basis for state sync, save/resume, and
  late-join.

## The difficulty is bimodal

- **The abstract galaxy/economy layer** (everything today) is *easy* to make
  multiplayer. It is turn-like: trades and combat resolve on ticks, not
  milliseconds. A server-authoritative model at a low tick rate fits, and most of
  the machinery already exists.
- **A real-time cockpit/flight layer** (does not exist) is where hard netcode lives
  — client prediction, interpolation, lag compensation, rollback. It is
  independent and only needed for co-located real-time flight.

Multiplayer for the game *as shaped today* is therefore very achievable; the scary
parts of a "space MMO" all live in a layer that has not been built.

## Recommended model: server-authoritative, low tick rate

The server runs the one canonical `World`; clients send **commands** (intents) and
receive **state** (snapshots/deltas). This prevents cheating, tolerates client-side
non-determinism (clients are observers), and scales to many players. Determinism
remains valuable server-side (reproducibility, debugging, save/restore, optional
prediction) but correctness does not depend on clients simulating identically.

### Alternative: deterministic lockstep (send inputs only)

Every client runs the full simulation; the server only orders inputs.
Bandwidth-light and elegant for small-N co-op, but fragile: one non-determinism
bug or one cheater desyncs everyone, and it demands **bit-identical cross-platform
floating point**. The simulation is `f64`-heavy; `f64` is deterministic on one
platform but not guaranteed identical across compilers/architectures. Prefer
server-authoritative. Keep lockstep only as an option for a trusted co-op mode, and
if it is ever pursued, isolate determinism-critical math behind fixed-point first.

## The structural decision: single-player as an in-process client/server

Structure single-player as the N=1 case of the multiplayer loop:

```
              commands                       snapshot/deltas
  Client  ---------------->  Server(World)  ---------------->  Client
 (render, input)            tick pipeline                    (render)
```

If single-player runs client and server in one process over a loopback channel,
multiplayer becomes a **transport swap, not a rewrite**. The trap to avoid is the
opposite: letting UI mutate the world directly (e.g. `world.markets[x].buy(...)`),
which is unorderable and un-networkable.

## Provisions

### Made now (this repo)

- **Commands applied at a tick boundary** (scaffolded — see below). Every player
  action is a serializable `Command` drained and validated in a `command_phase`
  that runs first in `tick()`. Single-player enqueues locally; multiplayer enqueues
  from the network. This is the load-bearing provision.
- **Agent ownership.** `Trader` carries an `Owner` (`Npc` or `Player(PlayerId)`).
  NPC-owned traders run the greedy AI; player-owned traders act only on commands.
  This is the "who controls what / whose command is valid" model.
- **Player-as-agent.** The player's ship is a `Trader` in the same collection as
  NPCs, so the world already handles N players uniformly — there is no
  `world.the_player` singleton.
- **Stable agent handles.** Commands address a trader by a `TraderId` — a
  monotonic, never-reused id assigned by the world and observed in state — not by a
  vector index. A stale id (its trader removed) simply fails to resolve, so traders
  can be added and removed safely and a server can echo ids to clients without an
  ABA hazard. `Command::Despawn` exercises removal.
- **Determinism discipline** kept intact (no new wall-clock / unordered iteration;
  the single seeded RNG advances the whole world; the id counter is part of state).

### To make when the work reaches them

- **Factor a session/driver type** (`drift-sim` or similar) that owns the `World`,
  applies commands, and emits snapshots, reused by the CLI, an eventual server, and
  in-process single-player. The CLI is already a headless driver; a server is that
  plus a socket.
- **`World<'r>` -> `Arc<Registry>`.** The world borrows the registry, which is
  awkward for a long-lived session/resource and for snapshotting. Owning the
  registry via `Arc` makes the world `'static`-friendly. (Also flagged for the
  graphical client.)

### Deferred (premature now)

Networking transport; delta encoding and interest management (needed only at
scale); client prediction / rollback / lag compensation (needed only for real-time
flight); accounts / auth / persistence backend.

## The command pipeline (scaffold)

Implemented in `drift-economy`:

- **`command.rs`** — `PlayerId`, `Owner { Npc, Player(PlayerId) }`, `TraderId`, the
  `Command` enum (`Spawn`, `Despawn`, `Jump`, `Buy`, `Sell`), and `CommandError`.
  `Command` and its operands are serde-serializable, i.e. already wire-ready.
  Traders are addressed by stable `TraderId`, resolved to the current slot at apply
  time.
- **`World::queue_command(cmd)`** — the single input entry point (local now,
  network later).
- **`World::command_phase()`** — runs first each tick; drains the queue and applies
  each command through `apply_command`, which **validates** ownership, reachability,
  funds, stock, and hold capacity. Invalid commands are rejected (counted, not
  fatal) — essential because multiplayer input is untrusted. Applying at the tick
  boundary (not on receipt) is what makes ordering deterministic.
- Player-owned traders are skipped by the NPC trading AI; their arrivals and
  respawns are still processed, but their buy/sell/jump decisions come only from
  commands.

Observability: `World::commands_applied()` / `commands_rejected()`.

Because no scenario spawns player traders and no commands are queued in the
existing runs, `command_phase` is a no-op there and the simulation is byte-identical
to before — so determinism and all existing tests are unaffected.

## Failure modes to guard against

- **Direct-mutation UI** bypassing commands (the biggest one).
- **A `the_player` singleton** baked into `World`.
- **Non-snapshotable state** creeping into `World` (breaks save/sync/late-join).
- **Choosing lockstep, then discovering cross-platform `f64` drift.**
- **Wall-clock or unordered iteration** sneaking into simulation logic later.
