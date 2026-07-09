# First 500k-tick run — what the CSV actually shows

> **Superseded in part, 2026-07-09.** The two colonies that delivered *exactly
> zero* over 500,000 ticks were not unlucky — the hall of fame had a bug that
> froze their gene pool solid. See [the postscript](#postscript-the-zeroes-were-a-bug)
> at the bottom. Everything below is the original write-up of the pre-fix run and
> the numbers in it no longer describe the simulation. The advice under
> "What to try next" is still untested.

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

## Postscript: the zeroes were a bug

The cross-check above was right that the world is fine, and wrong to conclude
the remaining fault was the *search*. It was the *archive*.

`record_death` rejected a tie:

```rust
if self.hall_of_fame.last().map_or(false, |(f, _, _)| *f >= fitness) {
    return;
}
```

A colony that has never delivered food scores every corpse 0.0. Once ten 0.0
entries were in, every later 0.0 was rejected by the 0.0 already sitting there,
and **the archive froze at that colony's first ten corpses and never changed
again.** Measured on seed 1: colonies 1, 3 and 5 took 138 deaths across 20,000
ticks without the archive moving once.

Because the extinction floor breeds from the archive, and the floor produces
97.7% of all ants, those colonies spent half a million ticks taking a single
mutation step away from ten fixed genomes and throwing the result away every
time. The search had no memory. It was not hill-climbing; it was resampling.
That is why colonies 1 and 5 delivered exactly zero, and why their generation
counter sat at 3–4 forever.

Two changes, both using only food delivered — the selection signal is unchanged:

1. **A tie displaces the weakest, and the newcomer is inserted in front of
   everyone it ties with.** A flat archive becomes a sliding window of the most
   recent corpses, so neutral mutations accumulate down a lineage and the search
   can cross the plateau. The first fix alone is not enough: with the newcomer
   inserted *behind* its ties it lands back in the slot `pop()` just freed, so
   nine of ten entries stay frozen. Both halves are needed.
2. **`archive_parent` draws roulette-weighted by fitness**, as `select_parent`
   already did for the living. Uniform sampling threw away the ordering the
   archive is maintained in — colony 0 held `[8, 8, 5.8, 4, 2, 2, 2, 0, 0, 0]`
   and bred from a genome known to deliver nothing 30% of the time.

Effect at 40,000 ticks on seed 1: total delivered **867 → 1,957**. Colony 2's
best archived ant went 17.6 → 178, its floor spawns halved (it now feeds itself),
and total population held at 37 instead of collapsing to 17.

### What this does not fix

Paid births still never happen after the opening spree: `births` freezes at 12
per colony around tick 1,500 and the stores sit at zero for the rest of the run.
Delivery is still one to two orders of magnitude below what the colony's ants
draw back out as `refuel_rate`, so nothing accumulates to `birth_cost`.

Before retuning that, note the trap: **the seed-to-seed variance is larger than
any effect you are likely to measure.** Over 40,000 ticks, total delivered was 98
(seed 3), 1,957 (seed 1) and 4,688 (seed 2). A three-seed A/B of "no paid births
at all" against the default came out 2281-vs-1957 for seed 1, 3096-vs-4688 for
seed 2, and 12-vs-98 for seed 3 — i.e. inconclusive in both directions. Any
economy retune needs a proper multi-seed sweep, not a single run.

Note also that `Config`'s `initial_food_store` comment claims it is "a fuel
reserve, not a birth windfall", and `reproduce` spends it to zero on births
within ~1,500 ticks, drawn from parents nobody has yet had reason to prefer. The
code and the comment disagree. Which one is wrong is a design question, not a
bug report.
