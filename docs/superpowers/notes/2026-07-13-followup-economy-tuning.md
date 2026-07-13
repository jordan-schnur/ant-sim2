# Follow-up: tune the colony economy so populations grow

**Status:** deferred backlog item, opened 2026-07-13. Not yet specced.

## Why this exists

Plan A (shaped fitness) shipped and improved foraging, but colonies still
never grow past the extinction floor (5 ants). The empirical gate showed the
barrier is economic, not a fitness-signal problem:
[`2026-07-13-shaped-fitness-growth-result.md`](2026-07-13-shaped-fitness-growth-result.md).

The core imbalance, measured at seed 1 / 200k ticks: the strongest colony
delivers ~0.068 food/tick while its 5 ants burn ~0.265 energy/tick — a ~4×
deficit. The food store can never reach `birth_cost` (40), so no paid births
happen after the initial store drains, and the extinction floor does all the
reproducing. A colony cannot grow a population it cannot feed.

## What a future spec should explore

The levers, all already on the tuning rail (no new plumbing needed):

- **`birth_cost`** (default 40) — lower it so a modest surplus buys a birth.
- **`refuel_rate`** (default 2.0) — the store's biggest drain. Lower it so
  delivered food accumulates instead of being immediately re-burned. Trades
  off against ants starving.
- **`harvest_rate`** (default 2.0) — raise per-trip yield so delivery rate
  can exceed upkeep.
- **trait-tax upkeep** (`base_upkeep`, `tax_*`) — lower total upkeep so a
  colony's break-even delivery rate drops.
- Possibly a **structural** change: reserve a fraction of delivered food for
  a birth fund that refuel cannot touch, so foraging success always buys
  *some* growth rather than only topping up energy.

## How to work it

Headless sweeps, seed-averaged (seed-to-seed variance spans two orders of
magnitude — a single run tells you nothing). Success = several colonies climb
above the extinction floor with sustained paid births and rising
`delivered_total`, without ballooning-then-crashing. Then set the defaults and
regenerate the golden master.

This is its own brainstorm → spec → plan cycle when picked up.
