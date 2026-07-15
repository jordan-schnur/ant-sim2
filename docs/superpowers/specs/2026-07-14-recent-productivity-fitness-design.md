# Recent-Productivity Fitness — Design

**Date:** 2026-07-14
**Status:** Approved (brainstorm), pending implementation-plan

## Goal

Stop selection from rewarding **one-time luck and old age**. Today fitness is
built from lifetime accumulators (`food_delivered`, `food_harvested`,
`food_homing`) that only ever rise, so an ant that harvested once at tick 500
and then idled for 3000 ticks can out-score a younger, genuinely better forager
purely by having lived longer. This has two costs:

- It partly selects for *survival time*, not *skill*.
- A single event is permanent credit — a fluke forager coasts forever.

It is also the same weakness behind the "a food patch runs out and they have to
find new food" worry: a memorizer that exploited one patch keeps its fitness
even after the patch is gone, so nothing selects for re-finding food. A signal
that **decays when an ant stops being productive** selects for ants that *keep*
harvesting, delivering, and fighting — including re-locating food when a source
depletes.

## The mechanism: a per-ant EMA "recent productivity"

Add one `f32` per ant, `recent_productivity`, a leaky accumulator:

- **Every tick** (serial apply phase, id order): `recent_productivity *=
  productivity_decay` (a config value in `(0,1)`, default `0.99`).
- **On a useful event**, add its magnitude:
  - harvest → `+ amount harvested this tick`
  - delivery → `+ amount delivered this tick`
  - kill → `+ energy absorbed from the victim` (reuses the value combat already
    computes; no new constant, and "fighting" reads as *winning* fights)

This is an exponentially-weighted sum of recent useful work: it climbs while an
ant is productive and bleeds back toward zero when it stops. The half-life is
`ln(0.5)/ln(decay)` — ~69 ticks at `0.99`, tunable by the slider.

Homing already has its own dense shaping term (`food_homing` / `homing_weight`);
it is **not** folded into the EMA (avoids double-counting the return leg).

## How it enters fitness

`Config::fitness` gains a fourth input and one weight:

```rust
fn fitness(&self, delivered, harvested, homing, recent) -> f32 {
    delivered
        + self.harvest_weight * harvested
        + self.homing_weight  * homing
        + self.productivity_weight * recent   // new
}
```

- **`delivered` stays cumulative and dominant** — the colony's real banked food
  and the anti-reward-hacking anchor. The EMA is an *added* shaping term, never
  a replacement.
- **Living selection** (`ColonyState::select_parent`, paid births) reads each
  living ant's current `recent_productivity`.
- **The hall of fame** records the ant's `recent_productivity` *snapshotted at
  death* alongside its cumulative stats, so the archived fitness (which feeds
  refounding via the world reservoir) reflects "was this ant productive *when it
  died*", not a fluke from its youth.

## Defaults (starting guesses, to be swept)

- `productivity_weight = 0.1` — ships **on** as a nudge. `0` disables it exactly
  (recovers today's selection), which is the A/B control the slider exposes.
- `productivity_decay = 0.99` — ~69-tick half-life.

Both are live-tunable sliders, like `homing_weight`.

## Determinism & wire

- `recent_productivity` is real per-ant state: updated in the serial phase in id
  order, serialized in the snapshot, and folded into `state_hash`. The golden
  fixture regenerates (a deliberate physics change). The NN is untouched —
  `N_INPUTS`/`N_PARAMS` are unchanged, so the known-good genome fixture stays
  valid (its guard test must still pass, not be regenerated).
- Wire: `CONFIG_FIELDS` 21 → 23 (`productivity_weight`, `productivity_decay`,
  ids 21–22), with clamps (weight `≥ 0`, decay in `(0,1)`). Headless `--set`
  arms. Web `CONFIG_FIELDS` + two sliders.
- The selected-ant detail frame carries `recent_productivity` (`ANT_DETAIL_LEN`
  453 → 457) so the inspector's fitness readout stays honest.

## Part tests (the ones that must exist)

- Harvest/deliver/kill each raise `recent_productivity`; an idle tick lowers it.
- Two ants with identical cumulative stats but different recent activity: the
  recently-active one wins `select_parent` far more often (with weight > 0).
- `productivity_weight = 0` recovers today's fitness exactly (purity toggle).
- Determinism: `state_hash` changes when `recent_productivity` diverges;
  identical runs still match.
- Payoff behavior test: on a world where a colony's food patch is exhausted
  mid-run, an EMA-on run keeps delivering (re-finds food) better than EMA-off —
  or at minimum does not regress world `delivered_total`.

## Risks & caveats

- **Not a guaranteed bootstrap fix.** It sharpens *what* is selected once there
  is variation; it does not by itself light up more colonies. Measure with the
  new Stats graphs (weight 0 vs 0.1).
- **Another knob.** `productivity_weight`/`productivity_decay` are guesses; too
  high a weight could let a burst of harvesting-without-delivering outrank real
  delivery — the `delivered`-cumulative anchor is what bounds that, so keep
  `productivity_weight` modest relative to a real delivery's value.
- **Kill reward could encourage aggression.** Absorbed-energy magnitude keeps it
  proportionate to a real kill, but watch colony aggression in runs; back the
  kill contribution out if it dominates.

## Out of scope

- Movement/exploration reward (rejected in brainstorm — risks rewarding aimless
  wandering).
- Making food *relocate* to force re-exploration (a separate food-dynamics
  experiment; measure the EMA first).
- Rate-by-age or sliding-window mechanisms (EMA chosen over both).
