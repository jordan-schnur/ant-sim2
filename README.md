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
- `server` — owns the clock and a WebSocket. Tick rate is decoupled from frame rate. *(not built yet)*
- `web` — a dumb renderer. TypeScript + WebGL2. Holds no simulation state. *(not built yet)*

## Running it

```bash
cargo test --workspace --release
cargo run -p headless --release -- --ticks 500000 --every 5000 --seed 1 > run.csv
```

`headless` prints one CSV row per colony. The column to watch is
`delivered_total` — cumulative food carried home, the only fitness signal in the
project. (`food_delivered` counts only *living* ants and falls when a good
forager dies of old age.)

## Does it evolve?

Yes, slowly. Over 500,000 ticks the delivery rate rises about 8×, six of eight
colonies discover foraging, and two never do. But **97.7% of all ants are born
from the extinction floor rather than paid for out of a colony's food store** —
the safety net, not the economy, is doing the reproducing. Read
[`docs/superpowers/notes/2026-07-09-first-500k-tick-run.md`](docs/superpowers/notes/2026-07-09-first-500k-tick-run.md)
before tuning anything.

A short run is actively misleading: at tick 5,000 every colony looks dead. The
curve does not bend until roughly tick 100,000.

Performance on an M-series Mac, 512×512: 489 ticks/sec at 320 ants, 234
ticks/sec at 10,000 ants.

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
