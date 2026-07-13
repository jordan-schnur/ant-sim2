# Shaped fitness: the growth result (Plan A, Task 6 gate)

**Date:** 2026-07-13
**Seed:** 1, 200,000 ticks, 8 colonies, `--every 20000`.

## What shipped

Tasks 1–5 of the shaped-fitness plan are committed and green:
`fitness = food_delivered + harvest_weight · food_harvested`, wired into
parent selection and the hall of fame, with `harvest_weight` as tunable
config field 16 (default 0.02, `0.0` = original delivery-only thesis).

## The gate: did colonies grow? No.

The plan's Task 6 success criterion was: *at least a few colonies climb
above the extinction floor (5 ants) with sustained paid births > 0.*

**Not met, at either `harvest_weight = 0.02` or `0.1`:**

| metric | hw = 0.02 | hw = 0.10 |
|---|---|---|
| max population, any colony, any tick | **5** | **5** |
| colonies delivering > 0 at 200k | 3 / 8 | 3 / 8 |
| best colony `delivered_total` | 13,547 | 16,967 |
| paid births after the initial store drained (~tick 20k) | ~0 | ~2 |

`births` is cumulative and froze at ~12 per colony by tick 20k — i.e. the
only paid births in the whole run were the ones the initial 600-food store
bought at the start. After that, `store` sits at 0.0 essentially everywhere
and reproduction is entirely the extinction floor's free trickle.

## Why: the mechanism works, but growth is economics, not fitness

Shaped fitness did what it was designed to do — it improved foraging.
Colony 2 evolved foragers delivering **13,547** food (the README's old
baseline had colonies delivering *exactly zero*). Raising the weight to 0.1
pulled colony 0 from 52 to 6,727 delivered. Delivery is real and rising.

It made no difference to population, because the binding constraint on
*growth* is the closed economy, not the selection signal:

- Best colony delivers ~13,547 food / 200k ticks = **0.068 food/tick**.
- 5 ants at mean upkeep ~0.053 burn **~0.265 energy/tick**.
- The colony delivers **~4× less than its own ants consume.** The food
  store can never accumulate the `birth_cost` (40) needed to pay for a
  birth; refuel drains it as fast as delivery fills it. Population is
  pinned at the extinction floor by a structural energy deficit.

A better forager (higher `harvest_weight`) cannot fix this: it changes
*which* genomes breed, not the delivery-rate-vs-upkeep balance. Even a
colony delivering 17,000 food banked a store of 0.0 and stayed at 5 ants.

## Conclusion

Shaped fitness is **necessary but not sufficient** for the growth the user
asked for. It gets colonies foraging (they were not, before). Turning
foraging into population growth is a separate, economic problem: the
delivery rate a colony can achieve must exceed the upkeep of the population
it is trying to feed. The levers are `birth_cost`, `refuel_rate`,
`harvest_rate`, and the trait-tax upkeep — all already on the tuning rail,
per the README's "Does it evolve?" section. Default `harvest_weight` is
left at 0.02 (it passes the anti-reward-hacking bound in the Task 1 test;
0.1 does not).
