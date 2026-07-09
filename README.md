# antsim2

A neuroevolution simulation of ant colonies. Not a game.

Several colonies share one contested 512×512 world. Every ant is driven by its own small
recurrent neural network. Ants forage, starve, grow, fight, and reproduce. Each colony
keeps a completely independent gene pool, so the world runs several evolutionary
experiments in parallel.

The only selection pressure is **food delivered home**. Nothing else is scored. The
question is whether trail-following, physical specialization, and warfare emerge anyway.

Design: [`docs/superpowers/specs/2026-07-09-antsim-design.md`](docs/superpowers/specs/2026-07-09-antsim-design.md)

## Shape

- `sim` — the simulation. Pure, deterministic, no I/O. Testable without a server or browser.
- `server` — owns the clock and a WebSocket. Tick rate is decoupled from frame rate.
- `web` — a dumb renderer. TypeScript + WebGL2. Holds no simulation state.

## Running it

Watch it in a browser:

```bash
cargo build --release
(cd web && npm install && npm run build)
cargo run -p server --release -- --web web/dist
# open http://127.0.0.1:8080
```

Pause, single-step, 1x/10x/100x, pan and zoom, toggle the pheromone layers,
click an ant to watch its network fire, and drag the tuning sliders to retune
the running simulation without recompiling. `f` fits the view, `Esc` clears the
selection, space toggles pause.

For development, `npm run dev` in `web/` serves the client on :5173 and proxies
`/ws` to the Rust server on :8080.

Or run it headless and read the numbers:

```bash
cargo test --workspace --release
cargo run -p headless --release -- --ticks 500000 --every 5000 --seed 1 > run.csv
```

`headless` prints one CSV row per colony. The column to watch is
`delivered_total` — cumulative food carried home, the only fitness signal in the
project. (`food_delivered` counts only *living* ants and falls when a good
forager dies of old age.)

## The wire format

`server` and `web` each hold a hand-written copy of the binary protocol, and
neither can see the other. A reordered field is silent — it renders as garbled
ants, not as an error. `crates/server/tests/fixtures.rs` emits byte fixtures and
`web/tests/protocol.test.ts` decodes them, which is the only place the two halves
meet. After an intentional format change:

```bash
cargo test -p server --test fixtures   # regenerate
cd web && npm test                     # expect failure until protocol.ts agrees
```

That failure is the point.

## Does it evolve?

Yes, slowly. Over 500,000 ticks the delivery rate rises about 8×, six of eight
colonies discover foraging, and two never do. But **97.7% of all ants are born
from the extinction floor rather than paid for out of a colony's food store** —
the safety net, not the economy, is doing the reproducing. Read
[`docs/superpowers/notes/2026-07-09-first-500k-tick-run.md`](docs/superpowers/notes/2026-07-09-first-500k-tick-run.md)
before tuning anything.

A short run is actively misleading: at tick 5,000 every colony looks dead. The
curve does not bend until roughly tick 100,000.

The colony cards in the UI show `free` — the share of a colony's ants that came
from the extinction floor rather than being paid for out of its food store.
Watch that number, and watch `birth_cost`, `harvest_rate`, `refuel_rate`, and
`growth_threshold`, which are on sliders for exactly this reason.

Performance on an M-series Mac, 512×512: 489 ticks/sec at 320 ants, 234
ticks/sec at 10,000 ants. The server caps a tick batch by wall clock as well as
by count, so 100x fast-forward keeps drawing at 20 fps and the pause button
stays responsive.

## Future directions

Deliberately out of scope for v1, recorded here so the reasons are not lost.

### Performance

The v1 tick is **parallel think, serial apply**: ants sense and run their networks in
parallel against a read-only world, then intents are applied serially in ant-ID order.
This makes the simulation deterministic regardless of thread count, which underwrites
save/load, seeded replay, and a trustworthy neural-net inspector.

The serial apply phase is expected to cap throughput somewhere past ~100k ants. When it
does:

1. **Spatial sharding.** Tile the grid, one thread per tile, with a boundary pass for
   cross-tile interactions. Scales further and removes the serial phase, but move
   conflicts and combat become race-prone and determinism is lost unless real work is done
   to preserve it.
2. **wgpu compute.** Sensing, brains, and pheromone diffusion as compute shaders. The
   endgame for scale — on the order of a million ants. The costs are real: every debugging
   session becomes a shader debugging session, and pulling one ant's activations out for
   the inspector means a GPU readback.

Neither is worth doing until the simulation is interesting. Note that the neural networks
are *not* the bottleneck — 10k ants is roughly 11M multiply-accumulates per tick, well
under 1 GFLOP/s at 60 ticks/sec. The costs are random memory access during sensing, the
serial apply phase, and streaming.

### Simulation

- **NEAT / evolving topology.** v1 uses a fixed-shape MLP where only weights mutate.
  NEAT would let network structure itself evolve — very satisfying to watch. Deferred
  because irregular graphs evaluate 5–20× slower than dense ones and would fight the
  performance goal. The brain sits behind a `Brain` trait so this can be substituted
  without touching the sim loop.
- **Queen and colony lifecycle.** v1 has no queen; a colony is a nest, a food store, and a
  gene pool. A queen would be a compelling story but introduces a bottleneck that means
  long waits before anything observable happens.
- **Diggable terrain.** Tunnels and excavated chambers. Deferred because ants would have to
  evolve digging before any other behavior could pay off.
- **Brain-allocated body plans.** Let the network choose its own trait allocation at birth,
  so one genome can express multiple castes depending on context.
