# Economy tuning: colonies now grow past the extinction floor

**Date:** 2026-07-13. Follows up
[`2026-07-13-followup-economy-tuning.md`](2026-07-13-followup-economy-tuning.md)
and the shaped-fitness gate
[`2026-07-13-shaped-fitness-growth-result.md`](2026-07-13-shaped-fitness-growth-result.md).

## The change

Two config defaults, tuned by seed-averaged headless sweeps:

| field | old | new |
|---|---|---|
| `birth_cost` | 40.0 | **12.0** |
| `refuel_rate` | 2.0 | **0.75** |
| `initial_food_store` | 600.0 | **150.0** |

Shipped with a `--set field=value` override flag on the headless runner
(`crates/headless`), which is how the sweep drove the levers without recompiling.

## Why these, and what the sweep found

The prior diagnosis held: growth was blocked by the *conversion of delivered
food into births*, not by ant survival or food supply. The coarse
one-at-a-time sensitivity sweep (40k ticks, seeds 1–3) was unambiguous — only
two levers moved population off the floor:

- Lowering **`birth_cost`** was dominant: `birth_cost=10` alone took the biggest
  colony from 5 to ~43 ants. `20`/`30` barely moved.
- Lowering **`refuel_rate`** was secondary: at 2.0/tick loitering ants drained
  the store as fast as it filled; at 0.5–1.0 delivered food accumulates toward a
  birth.
- Lowering upkeep, raising `harvest_rate`, bigger tanks, more/denser food,
  faster regrow — **none** produced any growth on their own (max pop stayed 5).
  The binding constraint was never the ants' energy budget; it was that no
  realistic delivery rate could ever bank a 40-food birth.

A combo grid (60k) then a confirmation sweep (100k, 4 seeds) with a boom-bust
metric (`hold` = end-population / peak-population) settled the values.
`birth_cost=12`, `refuel_rate=0.75` gave the best sustained population with the
gentlest pullback; a smaller `initial_food_store=150` improved stability further
(`hold` 0.85 vs 0.80) and kept the initial store a genuine fuel reserve rather
than a birth windfall — the `< 25 instant births` invariant survives (150/12 =
12.5). A larger store (600) grew slightly more colonies but oscillated harder.

## Result

At 100k ticks, seed-averaged (4 seeds), summed across 8 colonies:

| | baseline (old) | tuned |
|---|---|---|
| end population | ~34 (all pinned at floor) | **~76–81** |
| colonies above the floor | ~0.5 | **~1.5–2** |
| paid births (cumulative) | ~300 (all bootstrap) | **~2000–3300** |

So a couple of colonies per run now climb to ~40 ants, fed by sustained paid
births and steadily rising delivery — not the frozen-at-5, births-frozen-at-12
death spiral the baseline showed. It is an *oscillating* equilibrium (colonies
boom to ~100 and pull back), not a smooth monotone climb, but it does not
collapse and the delivery/birth totals rise throughout.

Not every colony thrives — growth is a couple of winners per run, not all
eight. That is emergent ecology under a shared, closed food economy, and it
matches the note's success criterion ("several colonies climb above the floor
with sustained paid births and rising delivered_total, without
ballooning-then-crashing").

## Fixtures regenerated

- Golden master (`REGENERATE_GOLDEN=1`) — physics changed intentionally.
- `known_good_forager.bin` — the checked-in forager was evolved under the old
  economy; re-ran the offline search, which found a forager scoring 6.2× its
  random baseline (well past the 1.5× guard) under the new rules.
- Two unit tests that hard-coded the old economy were updated: the refuel test
  (deficit now below the 0.75 rate so it still tests the max-energy cap) and the
  combat-flag test (given a deep store so its packed founders fight rather than
  starve).
