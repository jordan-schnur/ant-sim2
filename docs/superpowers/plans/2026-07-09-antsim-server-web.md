# antsim2 Server and Web — Implementation Plan (Plan 2 of 2)

**Goal:** Stream the running simulation to a browser that renders it, charts it, and lets the operator pause, single-step, inspect one ant's neural network live, and retune the economy without recompiling.

**Architecture:** A `server` crate owns the clock and a WebSocket. The `World` lives on a dedicated OS thread — not a tokio task, because `World::tick` is CPU-bound and drives its own rayon pool. Commands flow in on an `mpsc`; frames flow out on `watch` channels, one per cadence, so a slow client drops frames instead of stalling the simulation. The `web` app holds no simulation state: it decodes bytes and draws them.

**Tech Stack:** `axum` + `tokio` (WebSocket, static files), `clap` (CLI). TypeScript + Vite + WebGL2 + `vitest`. No JSON in the hot path.

**Spec:** `docs/superpowers/specs/2026-07-09-antsim-design.md`
**Prior plan:** `docs/superpowers/plans/2026-07-09-antsim-sim-core.md` (complete)
**Findings that shaped this plan:** `docs/superpowers/notes/2026-07-09-first-500k-tick-run.md`

## Global Constraints

- **`sim` stays pure.** No I/O, no sockets, no printing. Everything this plan adds to `sim` is read-only accessors, plus one observation field.
- **Determinism is preserved.** Nothing in `server` may change what `World::tick` computes. Tick rate and frame rate are independent; ticking faster must not alter the trajectory.
- **The wire format is little-endian, tagged, and mirrored twice** — once in `crates/server/src/protocol.rs`, once in `web/src/protocol.ts`. A cross-language fixture test guards the mirror. This is the single highest-risk surface in the plan: a layout mismatch is silent and presents as garbled rendering, not an error.
- **A malformed client command is logged and dropped.** It never panics the sim thread. (Spec §9.)
- **A dead WebSocket does not stop the simulation.** The sim keeps ticking headless; reconnecting resumes the view.
- **Colony colors are a client concern.** The server ships colony IDs; the shader maps them through a lookup.
- **The grid is 512 wide**, so positions encode as u16 fixed-point 9.7 (9 integer bits, 7 fractional) exactly.
- Rust edition 2021. `#![forbid(unsafe_code)]` in `server/src/main.rs`.
- Every task ends with a green test run and a commit to `main`.

## File Structure

```
crates/server/
├── Cargo.toml
└── src/
    ├── main.rs        # clap CLI, tokio runtime, spawns sim thread + axum
    ├── protocol.rs    # encode frames, decode commands, Config field table (pure)
    ├── clock.rs       # Speed, pause, single-step, ticks_due (pure)
    ├── sim_thread.rs  # owns World; command loop; cadenced frame publication
    └── ws.rs          # axum WebSocket route + per-connection fan-out
web/
├── package.json, tsconfig.json, vite.config.ts, index.html
├── src/
│   ├── main.ts        # bootstrap and layout wiring
│   ├── protocol.ts    # mirrors protocol.rs
│   ├── net.ts         # WebSocket, reconnect
│   ├── state.ts       # latest frames, selection, playback
│   ├── colors.ts      # colony palette
│   ├── render/
│   │   ├── camera.ts  # pan/zoom -> view matrix
│   │   ├── shaders.ts
│   │   └── world.ts   # pheromone quad + instanced ants
│   └── ui/
│       ├── controls.ts   # left rail
│       ├── colony.ts     # right rail stats + charts
│       ├── inspector.ts  # ant detail
│       └── nnview.ts     # canvas2d network
└── tests/protocol.test.ts
```

---

## Wire Format

Little-endian. Every message begins with a `u8` tag. Sizes below are exact.

### Server → client

**`0x01` Hello** — sent once on connect, before any frame.

| offset | type | field |
| --- | --- | --- |
| 0 | u8 | tag = 0x01 |
| 1 | u16 | width |
| 3 | u16 | height |
| 5 | u8 | num_colonies |
| 6 | u8 | phero_resolution_log2 (8 = 256, 9 = 512) |
| 7 | u64 | tick |

**`0x02` Ants** — ~20 fps. Header 13 bytes, then 8 bytes per ant.

| offset | type | field |
| --- | --- | --- |
| 0 | u8 | tag = 0x02 |
| 1 | u64 | tick |
| 9 | u32 | count |
| 13 + 8i | u16 | x, fixed-point 9.7: `min(x * 128, 65535)` |
| +2 | u16 | y, same |
| +4 | u8 | colony |
| +5 | u8 | size: `min(size / 3.0 * 255, 255)` |
| +6 | u8 | flags: bit0 carrying, bit1 attacking |
| +7 | u8 | pad (zero) |

`max_size`'s trait range tops out at 3.0, so the size byte's divisor is 3.0 and cannot clip in practice.

**`0x03` Pheromones** — ~10 fps. Header 14 bytes, then `w * h` RGBA8 texels, row-major from y=0.

| offset | type | field |
| --- | --- | --- |
| 0 | u8 | tag = 0x03 |
| 1 | u64 | tick |
| 9 | u16 | w |
| 11 | u16 | h |
| 13 | u8 | downsample factor (1 or 2) |
| 14 + 4i | u8 x4 | R = food, G = alarm, B = scent strength, A = owning colony (255 = none) |

R, G, B are `squash_phero(v, cfg.phero_log_div) * 255`, reusing `sim::sense::squash_phero` verbatim so the brightness on screen is the number the ant senses.

**Downsampling is 2×2 max, not mean**, and A is taken from the sub-cell that won the scent max. A trail is often one cell wide; averaging it with three empty neighbours quarters its brightness and it vanishes at the default 256×256, which would read as "no trails formed". Max over-shows sparse pheromone. That is the correct failure direction for an instrument whose job is to reveal whether trails emerge.

**`0x08` Terrain** — ~4 fps. Same header shape as Pheromones.

*Added during implementation; the plan as written had no way to send the map.*
The pheromone frame carries *trails*, not the food they lead to, and knows
nothing about the rock the ants walk around, so the client would have drawn an
empty void with smears on it.

| offset | type | field |
| --- | --- | --- |
| 0 | u8 | tag = 0x08 |
| 1 | u64 | tick |
| 9 | u16 | w |
| 11 | u16 | h |
| 13 | u8 | downsample factor |
| 14 + 4i | u8 x4 | R = standing food / `food_patch_max`, G = stone coverage, B = nest colony (255 = none), A = 255 |

Stone downsamples by **coverage fraction**, not max. A max would paint a whole super-cell solid for one stone corner and draw walls the ants can walk straight through. The pheromone layer deliberately over-shows thin trails; terrain must be honest.

**`0x04` Stats** — ~4 fps. Header 10 bytes, then 46 bytes per colony.

| offset | type | field |
| --- | --- | --- |
| 0 | u8 | tag = 0x04 |
| 1 | u64 | tick |
| 9 | u8 | count |
| 10 + 46i | u8 | id |
| +1 | u8 | pad |
| +2 | u32 | population |
| +6 | f32 | store |
| +10 | u64 | births |
| +18 | u64 | deaths |
| +26 | u64 | floor_spawns |
| +34 | f32 | mean_size |
| +38 | f32 | mean_lineage (this is "generation") |
| +42 | f32 | delivered_total |

`ColonyStats::food_delivered` (living ants only) is deliberately **not** on the wire. It falls when a good forager dies of old age, and every operator who saw it would read a dying colony. `delivered_total` is monotonic and is the curve that answers "is it evolving".

**`0x05` AntDetail** — ~4 fps, only while an ant is selected. 421 bytes.

| offset | type | field |
| --- | --- | --- |
| 0 | u8 | tag = 0x05 |
| 1 | u64 | id |
| 9 | u8 | colony |
| 10 | u8 | alive (0/1) |
| 11 | u8 x2 | pad |
| 13 | f32 x8 | x, y, heading, energy, max_energy, size, carrying, food_delivered |
| 45 | u32 | age |
| 49 | u32 | lineage |
| 53 | f32 x8 | traits, in `Traits::as_array` order |
| 85 | f32 x44 | input activations |
| 261 | f32 x16 | h1 |
| 325 | f32 x16 | h2 |
| 389 | f32 x8 | outputs |

Total 421 bytes. The `alive` byte exists because a selected ant can die between frames, and the inspector must say so rather than freezing on stale numbers.

**`0x06` AntGenome** — sent once when the selection changes. `1 + 8 + 1128*4 = 4521` bytes.

| offset | type | field |
| --- | --- | --- |
| 0 | u8 | tag = 0x06 |
| 1 | u64 | id |
| 9 | f32 x1128 | params, in `Brain for Genome` layout order |

Split from AntDetail because the weights do not change while an ant lives; resending 4.5 KB at 4 fps to redraw static edges is waste.

**`0x07` Config** — sent on connect and after any successful `SetConfig`. Tag, then `u8 count`, then `count` × (`u8 field_id`, `f32 value`). Lets the client render slider positions from the server's truth rather than guessing.

### Client → server

| tag | payload | meaning |
| --- | --- | --- |
| 0x01 | u8 paused | pause / resume |
| 0x02 | u8 speed (0=1x, 1=10x, 2=100x) | set speed |
| 0x03 | — | single step (implies pause) |
| 0x04 | f32 x, f32 y | select nearest ant to a world coordinate |
| 0x05 | — | clear selection |
| 0x06 | u8 field_id, f32 value | set a tunable Config field |
| 0x07 | u8 log2 (8 or 9) | pheromone resolution |
| 0x08 | — | save snapshot to the server's `--save` path |
| 0x09 | — | load snapshot from the server's `--save` path |
| 0x0A | u64 seed | reset the world with a seed |

Any unknown tag, or a short payload, is logged at `warn` and dropped.

### Tunable `Config` field table

`field_id` → field. Only scalars that are safe to change mid-run appear here; `width`, `height`, and `num_colonies` are structural and are absent by construction.

| id | field | id | field |
| --- | --- | --- | --- |
| 0 | food_evaporation | 8 | mutation_rate |
| 1 | alarm_evaporation | 9 | mutation_sigma |
| 2 | scent_evaporation | 10 | birth_cost |
| 3 | food_diffusion | 11 | harvest_rate |
| 4 | alarm_diffusion | 12 | refuel_rate |
| 5 | scent_diffusion | 13 | growth_threshold |
| 6 | tax_speed | 14 | food_regrow |
| 7 | tax_vision | 15 | attack_damage |

Ids 10–13 are not in the spec's slider list. They are here because the 500k-tick note identified them as the reason 97.7% of ants are born free from the extinction floor rather than paid for out of a colony's food store. The spec's list predates that run. Wiring the knobs the data implicated is the reason the note was written.

Evaporation values are clamped to `(0, 1)` on receipt; the rest are clamped to `>= 0`. An out-of-range value is clamped and logged, not rejected — the operator dragging a slider should never see the connection die.

---

## Task 1: Sim inspection API

**Files:** modify `crates/sim/src/ants.rs`, `apply.rs`, `world.rs`; regenerate `crates/sim/tests/golden_master.bin`.

**Produces:** `Ants::attacking: Vec<bool>`, `World::index_of(id) -> Option<usize>`, `World::nearest_ant(x, y) -> Option<u64>`, `World::activations(i) -> Activations`.

`attacking` is an observation field: set true in `apply_combat` when an ant lands an attack, cleared at the top of each tick. It does not feed sensing and cannot change the trajectory. It is serialized like every other SoA column, which **changes the snapshot layout and invalidates the golden master**. This is the fourth regeneration; it is intentional, and combat you cannot see is combat you cannot verify (requirement 6).

`activations` calls `sense::sense` then `Brain::forward`. It needs the spatial index, which stays private — hence the method lives on `World`.

- [ ] Write `an_attacking_ant_is_flagged_and_the_flag_clears_next_tick`
- [ ] Write `nearest_ant_finds_the_closest_living_ant`
- [ ] Write `activations_match_a_direct_forward_pass`
- [ ] Run, watch them fail
- [ ] Implement
- [ ] `REGENERATE_GOLDEN=1 cargo test -p sim --release golden`
- [ ] `cargo test -p sim --release` green
- [ ] Commit

## Task 2: Protocol module

**Files:** create `crates/server/Cargo.toml`, `src/protocol.rs`, `src/lib.rs`; `tests/fixtures.rs`.
**Consumes:** Task 1.
**Produces:** `encode_hello`, `encode_ants`, `encode_phero`, `encode_stats`, `encode_ant_detail`, `encode_ant_genome`, `encode_config`, `decode_command -> Option<Command>`, `enum Command`, `apply_config_field`.

Encoders write into a caller-supplied `&mut Vec<u8>` that the sim thread reuses, so publishing a 1 MB pheromone frame at 10 fps does not allocate 10 MB/sec.

- [ ] Unit-test each encoder's exact byte length and a spot-checked field offset
- [ ] Unit-test `decode_command` on every tag, plus a truncated payload and an unknown tag (both `None`)
- [ ] Unit-test that `apply_config_field` clamps evaporation into `(0,1)`
- [ ] `tests/fixtures.rs` writes `crates/server/tests/fixtures/*.bin` for a tiny known world
- [ ] `cargo test -p server` green; commit

## Task 3: Clock

**Files:** create `crates/server/src/clock.rs`.
**Produces:** `enum Speed { X1, X10, X100 }`, `struct Clock`, `Clock::ticks_due(&mut self, elapsed: Duration) -> u32`.

X1 = 60 tps, X10 = 600 tps, X100 = unbounded (tick as fast as the CPU allows). Pure: `ticks_due` takes elapsed time as an argument and reads no wall clock, so it is unit-testable. A per-call cap (`MAX_TICKS_PER_ITER = 4096`) guarantees the loop returns to drain commands — otherwise "100x" makes the pause button unresponsive.

Single-step sets `paused` and queues exactly one tick.

- [ ] Test: paused yields zero ticks regardless of elapsed
- [ ] Test: X1 over 1 second yields ~60
- [ ] Test: fractional accumulation does not lose ticks across calls
- [ ] Test: `ticks_due` never exceeds the cap
- [ ] Test: step yields exactly one tick, then zero
- [ ] Implement; green; commit

## Task 4: Sim thread

**Files:** create `crates/server/src/sim_thread.rs`.
**Consumes:** Tasks 1–3.
**Produces:** `struct Handles { commands: mpsc::Sender<Command>, ants: watch::Receiver<Arc<Vec<u8>>>, phero: ..., stats: ..., detail: ..., genome: ..., config: ... }`, `fn spawn(cfg, seed, save_path) -> Handles`.

Loop: drain commands → `ticks_due` → tick that many times → publish any frame whose cadence has elapsed. Cadences are wall-clock (20/10/4 fps), so fast-forward does not drown the browser.

`watch` is latest-value-wins: a backgrounded tab drops frames rather than accumulating a gigabyte of pheromone textures behind an unbounded queue. Nobody wants to watch a 30-second-old ant.

When zero ticks are due and nothing was published, the loop sleeps 1 ms rather than spinning a core.

- [ ] Test: 1000 ticks driven through the thread leave `state_hash` equal to a `World` ticked 1000 times directly (the clock cannot perturb physics)
- [ ] Test: a `SetConfig` command changes `World::cfg`
- [ ] Test: an ant frame decodes back to the live ant count
- [ ] Implement; green; commit

## Task 5: WebSocket server and CLI

**Files:** create `crates/server/src/ws.rs`, `src/main.rs`. Modify workspace `Cargo.toml`.
**Consumes:** Task 4.

`GET /ws` upgrades. Each connection spawns a send task that selects over the watch receivers and a receive task that decodes commands onto the mpsc. `GET /*` serves `web/dist` when it exists. Dropping a connection drops only its tasks.

CLI: `--port 8080`, `--seed 1`, `--load <path>`, `--save <path>` (default `snapshot.bin`), `--ants <n>`.

- [ ] Test: connect, receive Hello, assert width/height
- [ ] Test: send a malformed command; the connection survives and the sim still ticks
- [ ] Implement; `cargo test -p server` green; commit

## Task 6: Web scaffold and the cross-language guard

**Files:** create `web/` (package.json, tsconfig, vite.config.ts, index.html), `src/protocol.ts`, `tests/protocol.test.ts`.
**Consumes:** Task 2's fixtures.

`protocol.ts` decodes with a `DataView` and explicit `littleEndian = true` at every call. The vitest suite reads the Rust-generated `crates/server/tests/fixtures/*.bin` and asserts field-for-field. **This test is the reason the wire format is written down above.** Without it, a reordered field shows up as ants rendering in a diagonal line three weeks later.

- [ ] `npm create vite`, add vitest
- [ ] Write `tests/protocol.test.ts` against the fixtures; watch it fail
- [ ] Implement `protocol.ts`; `npm test` green; commit

## Task 7: Net layer and state store

**Files:** create `web/src/net.ts`, `src/state.ts`, `src/colors.ts`.

`net.ts` opens the socket, dispatches by tag, reconnects with backoff. `state.ts` holds the latest of each frame plus selection and playback state, and notifies subscribers. No simulation logic — the client cannot compute anything about an ant.

- [ ] Vitest: dispatch routes each tag to the right handler
- [ ] Vitest: state notifies on change
- [ ] Commit

## Task 8: Pheromone rendering

**Files:** create `web/src/render/shaders.ts`, `render/camera.ts`, `render/world.ts`.

Fullscreen quad, one RGBA8 texture, `texSubImage2D` per frame. The fragment shader blends whichever channels are toggled; the scent channel looks its color up from the colony id in A. `NEAREST` filtering — a smoothed pheromone field lies about where the gradient is.

- [ ] Implement; verify in the browser against a known seed; commit

## Task 9: Instanced ants and camera

**Files:** modify `web/src/render/world.ts`, `render/camera.ts`.

One instanced quad draw, the 8-byte record uploaded straight into a vertex buffer as the instance attributes. Color from colony, scale from size, a tint for carrying and a flash for attacking. Pan/zoom is a view-matrix uniform, so the camera costs nothing.

Unpacking x from u16 fixed-point 9.7 happens in the vertex shader: `float px = float(x) / 128.0;`

- [ ] Implement; verify 10k ants render at 60 fps; commit

## Task 10: Left rail

**Files:** create `web/src/ui/controls.ts`.

Playback (pause, step, 1x/10x/100x), three layer toggles, and a slider per tunable field driven by the `0x07` Config frame. Sliders send `0x06` on input, throttled to ~20 Hz.

Ranges: evaporation `0.9–0.9999` (log scale — the interesting region is the last decimal), diffusion `0–0.4`, taxes `0–0.05`, `mutation_rate` `0–0.5`, `birth_cost` `1–100`, `harvest_rate` `0.1–10`, `refuel_rate` `0–10`, `growth_threshold` `0.1–1.0`, `food_regrow` `0–0.02`, `attack_damage` `0–20`.

- [ ] Implement; commit

## Task 11: Right rail — colony stats and charts

**Files:** create `web/src/ui/colony.ts`.

Per-colony population, store, generation, and `delivered_total`. A canvas2d sparkline per colony, ring-buffered to ~600 samples (2.5 minutes at 4 fps), colored by colony.

The `delivered_total` chart is the whole point: the 500k-tick run showed the delivery rate does not bend upward until roughly tick 100,000, so the operator needs a curve, not a number.

- [ ] Implement; commit

## Task 12: Inspector and neural-net view

**Files:** create `web/src/ui/inspector.ts`, `src/ui/nnview.ts`.

Click the world → send `0x04` with world coordinates → server replies with `0x06` genome and starts streaming `0x05` detail.

`nnview.ts` is canvas2d: 44 + 16 + 16 + 8 = 84 nodes in four columns, ~1,088 edges. Nodes filled on a signed diverging scale (blue negative, red positive). Edges colored by weight sign, alpha by magnitude, with `|w| < 0.05` culled so structure is visible rather than a grey mat. Redrawn at 4 fps.

Pair this with single-step: pause, step one tick, watch the activations change. That is the difference between a pretty picture and a debugging tool.

- [ ] Implement; verify a selected ant's inputs change when it moves onto food; commit

## Task 13: Save / load / reset, verification, README

**Files:** modify `crates/server/src/sim_thread.rs`, `web/src/ui/controls.ts`, `README.md`.

Save and load go through `sim::snapshot` against the server's `--save` path. Reset rebuilds the `World` from a seed. All three are server-side; the client only sends the tag.

- [ ] Test: save, tick 100, load, tick 100 — `state_hash` matches a world that ticked 100 from the save point
- [ ] `cargo test --workspace --release` green
- [ ] `npm test` green
- [ ] Browser smoke test: 8 colonies visible, pause works, step works, a slider moves a number, an ant inspects
- [ ] README: how to run both halves; mark `server` and `web` built
- [ ] Commit
