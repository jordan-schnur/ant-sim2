# First 500k-tick run — what the CSV actually shows

Plan 1, Task 21, Step 7. Default `Config`, seed 1, 8 colonies × 40 founders,
512×512, sampled every 5,000 ticks.

    cargo run -p headless --release -- --ticks 500000 --every 5000 --seed 1 \
        --save run1.bin > run1.csv

**Headline: evolution works, but slowly, and the extinction floor — not the food
economy — is what actually reproduces the ants.**

## Delivery rate rises about 8x

`delivered_total` is cumulative food carried home, summed over all 8 colonies.
The *rate* is what matters:

| tick window | food delivered per 1,000 ticks |
| --- | --- |
| 50k–100k | 64.7 |
| 100k–150k | 185.5 |
| 150k–200k | 606.5 |
| 200k–250k | 266.8 |
| 300k–350k | 284.5 |
| 400k–450k | 353.8 |
| 450k–500k | 494.4 |

Total delivered across the run: 140,915.

Read a short run and you will conclude the opposite. At tick 5,000 every colony
has collapsed to 2–4 ants, every store is empty, `births` is frozen at 12 (all
bought with the initial 600 store), and `delivered_total` is 34 across all eight
colonies. That looks exactly like the "nothing evolves" failure the spec
predicts. It takes roughly 100k ticks before the curve bends. **Do not tune
against a 10k-tick run.**

## Colonies diverge sharply

Per-colony, over the final 100k ticks:

| colony | food/1k ticks | generation | paid births | floor spawns | pop |
| --- | --- | --- | --- | --- | --- |
| 0 | 86.8 | 78.0 | 67 | 2463 | 4 |
| 1 | 0.0 | 4.0 | 12 | 2489 | 2 |
| 2 | 125.4 | 4.0 | 13 | 1471 | 5 |
| 3 | 31.1 | 3.0 | 12 | 2487 | 3 |
| 4 | 104.8 | 126.7 | 298 | 2243 | 11 |
| 5 | 0.0 | 3.0 | 12 | 2489 | 2 |
| 6 | 12.8 | 3.0 | 12 | 2488 | 3 |
| 7 | 63.1 | 3.0 | 12 | 2446 | 4 |

Six of eight colonies found foraging. Two (1 and 5) delivered **exactly zero**
over half a million ticks. Independent gene pools produce genuinely independent
outcomes, which is the point of the design — but it also means a single-colony
run tells you almost nothing.

Colony 4 is the clear winner: 298 paid births, 126 generations deep, and the
only colony whose population is meaningfully above the floor.

## The extinction floor is doing the reproducing

**18,576 free floor spawns against 438 paid births — 97.7% of every ant ever
born was free.**

This is the most important finding, and it is a design problem rather than a
bug. The intended loop is: forage → fill the store → buy births. Observed:
stores sit at ~0 permanently, so paid births essentially never happen, and the
population is maintained entirely by the extinction floor drawing mutated
copies from the hall of fame.

Selection has not disappeared — the archive ranks on food delivered, so the
floor is *itself* a selection mechanism, and that is why the delivery rate
climbs at all. But the economy the spec describes is not running. The floor was
introduced as a research affordance to stop a colony vanishing from the screen;
it has quietly become the engine.

Consequences worth knowing before Plan 2's tuning panel:

- Population is pinned near `extinction_floor` (5) rather than set by food
  supply, so `birth_cost` and `initial_food_store` currently have almost no
  influence on anything.
- Because every free ant descends from an *archived ancestor* rather than from
  the living population, lineage depth stays shallow for floor-fed colonies.
  That is why colonies 1, 3, 5, 6, 7 sit at generation 3–4 despite thousands of
  spawns. It is honest, not a counter bug — the counter bug (a free ant taking
  an unrelated global `next_lineage_hint + 1`) was fixed separately.
- `floor_respawn_interval` (200) is therefore a hard cap on colony growth rate
  for any colony that cannot afford paid births: at most one new ant per 200
  ticks.

## What to try next

In rough order of expected effect on getting the *food economy* to drive
reproduction:

1. **Lower `birth_cost`** (40) or **raise `harvest_rate`** (2.0). A round trip
   yields ~10 food; a birth costs 40. Four successful trips per birth is steep
   for a policy that only half-works.
2. **Cut `refuel_rate`** (2.0) or make refuelling cost less of the store. Ants
   loitering on the nest drain the store faster than foragers fill it, which is
   why stores never accumulate.
3. **Reconsider automatic growth.** `growth_threshold` 0.8 means any well-fed ant
   converts store-food into body mass, and upkeep scales linearly with size
   while `carry_capacity` does not. For a pure forager, growing is a strict
   loss. The scripted-forager diagnostic showed mean size climbing to 1.8 and
   upkeep with it.
4. Only then touch `food_evaporation` / `food_diffusion`. The homing gradient is
   already verified readable (`tests/gradient.rs`), so pheromones are not the
   bottleneck the spec assumed they would be.

Do not raise `extinction_floor` to "help" — it would deepen the dependence that
is already masking the economy.

## Cross-check: the world is not broken

Three independent results say the physics are sound and the problem is the
search and the economy, not the simulation:

- `tests/gradient.rs` — the nest scent gradient is monotone and unsaturated from
  2 to 20 cells (0.696 → 0.199), so homing is learnable.
- `tests/behavior.rs` — a hand-written forager grows the store 600 → ~1,700 and
  delivers 8,700 food in 4,000 ticks, and profits on every map the genome search
  uses.
- `tests/known_good.rs` — a 300-generation hill-climb lifts mean delivery from
  313 to 1,899.
