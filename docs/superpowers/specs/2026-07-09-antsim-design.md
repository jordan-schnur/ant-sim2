# antsim2 — Design

**Date:** 2026-07-09
**Status:** Approved

## 1. Purpose

antsim2 is a neuroevolution simulation, not a game. Several ant colonies share one
contested 2D world. Each ant is controlled by its own small neural network. Ants forage,
starve, grow, fight, and reproduce. Each colony maintains a completely independent gene
pool, so the world runs several evolutionary experiments in parallel and lets us observe
whether colonies converge on the same strategy or diverge.

The question the simulation exists to answer: **given only "food delivered home" as a
selection pressure, do colonies evolve trail-following, physical specialization, and
warfare?** No behavior is scripted. No fitness function is hand-written beyond food
delivery.

Success means we can watch a colony discover something we did not implement.

## 2. Non-goals

- Not a game. No player, no win condition, no progression.
- No queen entity or queen lifecycle (deferred; see Future Directions).
- No diggable terrain or tunnel excavation (deferred).
- No cross-colony gene flow, ever.
- No hand-authored castes, roles, or job assignment.
- No deployment story. This runs on the author's Mac against `localhost`.

## 3. Architecture

One Cargo workspace and one web app.

| Component | Responsibility | Explicitly does not |
| --- | --- | --- |
| `sim` crate | The entire simulation. Pure, deterministic, no I/O. | Spawn threads it doesn't own, open sockets, print, render |
| `server` crate | Owns the clock and the WebSocket. Ticks `sim`, streams snapshots, applies commands. | Contain simulation rules |
| `web` app | Renders received bytes. TypeScript + Vite + WebGL2. | Hold simulation state or compute anything about ants |

`sim` exposes roughly:

```rust
let mut world = World::new(&Config::default(), seed);
world.tick();
world.ants();            // borrowed SoA accessors
world.colony_stats(id);
```

Because `sim` is pure and deterministic, evolution is testable by ticking a world in a
unit test. No server, no browser.

Data flow: `web` sends a command → `server` mutates the sim or its own clock → `sim`
ticks → `server` snapshots → `web` draws.

**Tick rate and frame rate are decoupled.** Paused means zero ticks. 1x means 60
ticks/sec. 100x means tick as fast as the CPU allows. The stream cadence is unaffected.
This is what makes fast-forward work without drowning the browser.

### 3.1 The tick: parallel think, serial apply

Chosen over fully-parallel sharding and over GPU compute. See Future Directions for both.

The world is struct-of-arrays (`Vec<f32>` for energy, `Vec<u16>` for x, and so on), not
`Vec<Ant>`.

1. **Sense + think (parallel, rayon).** Every ant reads the world, runs its network, and
   emits a small `Intent` (move, attack, grab, release). This phase never writes to the
   world.
2. **Apply (serial, ant-ID order).** Intents are applied in order. Conflicts resolve
   deterministically — two ants targeting the same cell: lower ID wins.
3. **Field update (parallel).** Pheromone evaporation and diffusion, food regrowth.

Because phase 1 is read-only and phase 2 is ordered, **the simulation is deterministic
regardless of thread count.** That determinism underwrites save/load, seeded replay, and
a trustworthy neural-network inspector.

Each ant carries its own RNG seeded from `(ant_id, birth_tick)`, so parallel iteration
order cannot influence outcomes.

**Performance sanity check.** A 44→16→16→8 network is 1,088 weights plus 40 biases, so
~1,128 multiply-accumulates per ant per tick. Ten thousand ants is ~11M MACs per tick;
at 60 ticks/sec that is under a billion multiply-accumulates per second, which one modern
core handles. **The neural networks are not the bottleneck.** The real costs are random
memory access during sensing, the serial apply phase, and streaming. Optimization effort belongs there.

Genome memory: ~1,128 f32 weights + 8 trait f32 ≈ 4.5 KB per ant, so ~45 MB at 10k ants.
Acceptable; noted so it is not a surprise.

## 4. The world

- **Grid:** 512 × 512 cells.
- **Cell:** terrain (`Dirt` = walkable, `Stone` = impassable) and a food amount.
- **Food:** a few dozen scattered patches. Depletes when harvested, slowly regrows, so
  the map does not become a dead husk and colonies must keep re-exploring.
- **Nests:** one small tile cluster per colony. Colonies default to 8.
- **Terrain variety is a requirement, not decoration.** Food patches at varying distances
  and stone chokepoints are what let different colonies profit from different strategies.
  A uniform map is the known cause of the "all colonies converge" failure mode.

### 4.1 Pheromone layers

Three layers over the grid. All three **evaporate and diffuse every tick**.

| Layer | Storage | Emitted by |
| --- | --- | --- |
| Food trail | scalar per cell | every ant, in proportion to the food it is carrying |
| Alarm | scalar per cell | any ant taking damage or attacking |
| Colony identity | strength + owner ID per cell | every ant constantly; nests emit far more strongly |

**Deposition is entirely passive.** No network output controls it. Ants leak these
chemicals as a function of their state, as real ants do. Sensing and interpretation are
100% evolved: a newborn ant with random weights will happily ignore a pheromone
superhighway. Evolution's job is to discover that the food-trail input is worth listening
to. That *is* the experiment.

Because only laden ants leak food-trail, a fresh trail always runs from food back toward
the nest, and repeated successful traffic strengthens it. **Trail reinforcement is
emergent, not coded.**

Colony identity stores one strength and one owner per cell rather than one layer per
colony. Depositing onto foreign-scented ground erodes the incumbent's mark, so territory
emerges as a contested field. This is a deliberate trade: per-colony layers would be more
faithful but cost 8x the diffusion work and would let colonies overlap invisibly.

**Diffusion and evaporation are load-bearing.** Diffusion turns a one-cell dotted line of
footsteps into a gradient the five-whisker sensor can detect from a distance. Evaporation
lets a trail to an exhausted patch fade instead of misleading the colony forever. Their
ratio sets trail sharpness versus reach and will make or break whether trails form at
all — hence live-tunable (§7.4).

## 5. Ants

### 5.1 Energy is health

Combat damage and starvation subtract from the same pool. Death at zero covers both.

A large, well-fed ant is therefore naturally hard to kill; a starving one is fragile; a
fighter that wins a brawl but does not eat still dies. One number, three behaviors.

### 5.2 Size and growth

Size rises as an ant eats past a threshold and shrinks when it starves — fat as a famine
buffer. Size multiplies max energy, attack damage, and carry capacity, **and** multiplies
metabolic drain. The genome caps maximum size.

Two tiers of "growth", both requested:

- **Colony growth** is population, which falls directly out of the nest birth rule (§6.2).
  This is the headline success metric.
- **Individual growth** is body size, per above.

Ants have a genetic lifespan and die of old age, so lineages must turn over.

### 5.3 The trait tax

Every tick, an ant pays upkeep proportional to its size *and* to its genetic traits —
speed, strength, armor, vision each levy a standing metabolic cost whether or not they
are used.

**This tax is the entire reason evolution has anything to discover.** If speed were free,
every lineage would max it and there would be no strategy space. Because it is not,
"fast and hungry" versus "slow and armored" is a real bet placed against the environment,
and different colonies can place different bets.

Tuning these coefficients is the most important tuning work in the project. They are
live-editable from the UI, never baked into the binary.

### 5.4 Combat

The network has an `attack` output. When it is high and a target is adjacent, damage is
dealt, scaled by `size × strength`, reduced by the target's `armor`. Attacking costs
energy. Killing yields energy (scavenging), so aggression *can* pay.

Ants sense nearby ants' colony scent, so "attack foreigners, ignore nestmates" is
learnable rather than hardcoded. Warfare, raiding, and peaceful coexistence are all
reachable outcomes.

## 6. Brains and evolution

### 6.1 Network

Fixed topology: **44 inputs → 16 → 16 → 8 outputs**, tanh activations, plus **4 recurrent
memory neurons** whose outputs feed back as inputs next tick. Only weights and biases
mutate; the shape never changes.

The memory neurons are load-bearing. Trail-following is not a reflex — "am I already
moving along something" and "am I heading home or away" are state, not sensation. A
purely feedforward ant would twitch toward the strongest neighboring cell and oscillate.

Inputs (44):

| Count | Source |
| --- | --- |
| 30 | 5 whisker directions (hard left, soft left, ahead, soft right, hard right) × 6 channels: food, food-pheromone, alarm-pheromone, own-colony scent, foreign-colony scent, blocked-by-stone |
| 3 | Underfoot: food, food-pheromone, alarm |
| 2 | Nearby friend count, nearby foe count |
| 4 | Proprioception: energy, size, carrying, age |
| 1 | Bias |
| 4 | Recurrent memory from last tick |

Sensing is egocentric and sparse — antennae, not eyes. Sample distance scales with the
genetic vision trait. A 5×5 patch would be 150 inputs and mostly wasted; real ants are
nearly blind and navigate chemically. The sparse fan keeps the network small, fast, and
legible in the live NN view.

Outputs (8): turn (continuous), forward throttle, attack, grab/release, and the 4 memory
values.

`grab/release` governs picking food up from a food cell and dropping it in the field.
Depositing into the colony store is **not** an output — it happens automatically when a
carrying ant stands on its own nest, as does refueling from the store. An ant must still
evolve to *go* there.

Position is continuous-valued on a discrete grid, so ants glide rather than snap.

**There is no homing compass input.** The nest emits a strong colony-identity scent that
diffuses into a gradient; an ant finds its way home by climbing its own colony's scent.
The same sensor that reports friend-or-foe also reports where home is. Homing, friend/foe
recognition, and territory all fall out of one layer. A lineage that never learns to read
that gradient never returns food and dies out.

The brain sits behind a `Brain` trait so an alternate implementation can be substituted
without touching the sim loop.

### 6.2 Genome and reproduction

A genome is the network's weights and biases, plus a body-trait vector: max speed,
strength, armor, vision range, carry capacity, max size, metabolic efficiency, lifespan.
Every trait carries the standing tax from §5.3.

**There is no queen.** A colony is a nest, a food store, and an independent gene pool.

Ants carrying food deposit it at the nest, growing the colony food store. The nest is
both the maternity ward and the fuel depot: ants standing on their nest refuel from the
store. This single rule creates the whole economy — a colony that forages badly starves
collectively, an ant that wanders too far from home dies alone, and a well-worn trail
between food and nest is worth real energy.

When the store crosses a birth threshold, the nest spends it and spawns one ant. The new
genome is a mutated copy of a **single parent, sampled from that colony's living ants
with probability weighted by lifetime food delivered**, plus a small constant so an
unlucky ant is not strictly excluded. Mutation is Gaussian noise on weights and traits,
with a small chance of a larger jump to escape local optima. Births are rate-limited per
tick.

**Fitness is not a formula. It is food delivered home.** Nothing scores an ant on speed
or aggression. If aggression pays — killing foragers at a contested patch means your
colony eats — aggressive lineages deliver more food and reproduce more, and warfare
emerges. If it does not pay, aggression is a tax and dies out. The world decides, not the
designer.

**Selection is per-colony and airtight.** Parents are only ever sampled from within the
same colony. Colonies never exchange genes. Eight colonies is eight independent
evolutionary experiments in one shared, contested world.

### 6.3 Extinction floor

Each colony keeps a hall-of-fame archive of its best-ever genomes by food delivered. If a
colony drops below five living ants, its nest spawns free ants from mutated archive
copies.

A colony can be beaten down and rebound, but never silently zeroes out while the operator
is away. This is a **research-tool decision, not a biology one**: a dead colony is an
empty region of screen that teaches nothing. Weak colonies still stay small and lose
territory, so selection is intact.

Combined with generous starting stores, large starting energy, and food patches seeded
near nests, this addresses the single most common way alife projects fail — total
extinction in the first minutes, leaving an empty grid.

### 6.4 Generations

Nothing resets. Each ant carries a lineage depth (parent's + 1). A colony's "generation"
is the mean lineage depth of its living ants, and rises smoothly. Short-lived,
fast-breeding colonies show a higher generation number than slow ones — itself worth
charting.

## 7. Protocol, rendering, UI

### 7.1 Protocol

Binary, little-endian, one-byte type tag per message. **No JSON in the hot path.** The
server pushes three frame kinds on independent cadences because their sizes differ wildly.

| Frame | Cadence | Payload | Size @ 10k ants |
| --- | --- | --- | --- |
| Ants | ~20 fps | 8 bytes/ant: x (u16), y (u16), colony ID (u8), size (u8), flags (u8: carrying, attacking), 1 pad byte for alignment | ~80 KB |
| Pheromones | ~10 fps | RGBA8 texture: R = food trail, G = alarm, B = colony scent strength, A = owning colony ID | 256 KB @ 256², 1 MB @ 512² |
| Stats + inspector | ~4 fps | Per-colony population, food store, births, deaths, mean size, mean lineage depth. Plus, when an ant is selected, its genome and full layer activations | ~340 bytes for the ant detail |

The grid is 512 wide, so 9 integer bits and 7 fractional bits fit exactly in a u16 —
sub-cell smoothness for free.

Colony colors are a client concern: the shader maps the alpha-channel colony ID through a
small color lookup.

Pheromone texture is downsampled to 256×256 by default, with a full-512 toggle.

Client → server is tiny and rare: pause, set speed, single-step, select ant, set a tuning
constant, save, load, reset with seed.

The protocol is defined once in a `protocol` module and mirrored in TypeScript.

### 7.2 Rendering

Two draw calls.

- **Pheromones:** fullscreen quad; the fragment shader blends whichever channels are
  toggled on.
- **Ants:** one instanced quad draw. The 80 KB array uploads straight into a vertex
  buffer, one instance per ant, color from colony ID, scale from size. Ten thousand
  instances is trivial for any modern GPU.

Pan and zoom are a view-matrix uniform, so the camera costs nothing.

### 7.3 Neural network view

Plain canvas2d, not WebGL — 84 nodes and ~1,088 edges, redrawn at 4 fps.

Nodes filled by activation on a signed diverging scale. Edges colored by weight sign,
made translucent by magnitude, with near-zero weights culled so structure is visible.

Because the sim is deterministic and single-steppable, the operator can step one tick and
watch activations change frame by frame. That is the difference between a pretty picture
and a debugging tool.

### 7.4 UI

Layout: world center, collapsible side rails.

```
+--------+------------------------+-----------+
| > || >>|                        | COLONY 3  |
| 1x 10x |                        | pop   412 |
| 100x   |      W O R L D         | food 1.2k |
|        |                        | gen  38.4 |
| LAYERS |     (pan / zoom)       | --------- |
| [x]food|                        | [ chart ] |
| [ ]alrm|                        |           |
| [x]scnt|                        | ANT #8213 |
|        |                        |  o--o--o  |
| TUNING |                        |  o--o--o  |
| evap-o-|                        |  o--o--o  |
| diff-o-|                        |  NN view  |
+--------+------------------------+-----------+
```

Left rail: playback (pause, step, 1x/10x/100x), pheromone layer toggles, tuning sliders.
Right rail: per-colony stats and time-series charts; swaps to the ant inspector plus NN
view on click. Both rails collapse for a fullscreen world. Plain flex layout, no
windowing library.

**Live-tunable constants** (applied to the running sim): evaporation rate, diffusion rate,
trait tax coefficients, mutation rate, birth cost. Tuning these by editing Rust and
recompiling would make the project miserable. After the pause button, this is the
highest-leverage UI feature.

**Ant inspector:** genome traits, energy, size, age, lineage depth, carried food, live
activations.

**Per-colony charts** are how we tell whether evolution is doing anything. Without them we
are staring at dots and guessing.

### 7.5 Save / load / seed

`serde` + `bincode` over the whole `World`, including RNG state. Determinism makes this
meaningful: load a snapshot, tick 100 times, get exactly what you got before. Genomes can
be exported individually.

## 8. Testing

Determinism is a **tested property, not an aspiration.**

- **Determinism test.** Build from a seed, tick 10,000 times, hash world state. Repeat
  with a different thread-pool size. Assert the hashes match. This fails immediately if
  someone introduces unordered iteration or an unsynchronized write.
- **Save/load round-trip.** Save → load → tick both → assert identical.
- **Golden master.** Serialize a small world, tick 1,000 times, compare against a
  checked-in snapshot. Catches unintended physics changes during tuning. Lives in one
  clearly-labeled test with a documented regeneration command, because intentional rule
  changes will require regenerating it.
- **Behavioral tests.** A hand-built straight-walking genome reaches the food patch ahead
  of it. A colony seeded with a hand-authored **known-good forager genome** grows its food
  store over 50k ticks. A colony of random genomes with no nearby food shrinks to the
  extinction floor and stays there.

That checked-in known-good forager genome is worth a great deal: it separates "the world
is broken" from "evolution has not found it yet," otherwise the hardest bug class in the
project to diagnose.

## 9. Error handling

The boundary holds the error surface; `sim` has almost none by construction (no fallible
I/O).

- A malformed client command is logged and dropped. It never panics the sim.
- A dead WebSocket does not stop the simulation. The sim keeps running headless;
  reconnecting resumes the view.

## 10. Expected failure modes

| Failure | Mitigation | How it is diagnosed |
| --- | --- | --- |
| Every colony dies in the first minute | Extinction floor, generous starting stores, food seeded near nests | The known-good forager test tells you whether the world is winnable at all |
| Nothing evolves; ants wander forever (**most likely real outcome**) | Live-tuning panel | Flat food-delivered curves on the per-colony charts. Usual causes: evaporation/diffusion ratio makes trails unreadable, or trait taxes so steep every mutation is worse than neutral |
| All colonies converge to one strategy | Terrain variety, mutation-rate slider | Identical trait charts across colonies |
| Serial apply phase becomes the bottleneck | Expected past ~100k ants | It is the documented trigger for the spatial-sharding work |

## 11. Future directions

Recorded in `README.md` and deliberately out of scope for v1:

- **NEAT / evolving topology.** Structure emerges rather than only weights. Slotted in
  behind the `Brain` trait. Deferred because irregular graphs evaluate 5–20× slower and
  fight the performance goal.
- **Spatial sharding.** Tile the grid, thread per tile, boundary pass for cross-tile
  interactions. Triggered when the serial apply phase caps throughput. Costs determinism
  unless done carefully.
- **wgpu compute.** Sensing, brains, and diffusion as compute shaders. The endgame for
  scale (~1M ants). Deferred: every debug session becomes a shader debug session, and the
  inspector needs GPU readback.
- **Queen and colony lifecycle.** Deferred because a queen bottleneck means long waits to
  observe anything.
- **Diggable terrain.** Tunnels and excavated chambers. Deferred because ants would have
  to evolve digging before anything else works.
- **Brain-allocated body plans.** The network chooses its own trait allocation at birth,
  allowing one genome to express multiple castes.
