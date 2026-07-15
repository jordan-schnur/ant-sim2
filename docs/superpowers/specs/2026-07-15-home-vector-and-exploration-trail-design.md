# Home vector + exploration trail

**Date:** 2026-07-15
**Branch:** `feat/home-vector-and-exploration-trail`

## Motivation

Real ants get home by two largely independent systems: **individual path
integration** (a maintained home vector — direction *and* distance to the nest,
built from a sky compass + step-counting odometer) and **social pheromone
trails** (a recruitment trail marking the route). The sim currently gives ants
only a degenerate form of the latter — climb the contested nest-scent gradient
through 5 whiskers — and *no* home vector at all. That is the hardest of the real
cues and the only one they had, which is a large part of why navigation home is
so hard to evolve.

This change adds both real mechanisms:

1. A **home vector** the network can sense (path integration, given directly).
2. A new **exploration trail** pheromone (the social route layer).

Neither addresses colony *growth* — that is the separate economic deficit
documented in `docs/superpowers/notes/2026-07-13-*`. This is purely about
navigation/homing legibility to the network.

## 1. Home vector (sensory input)

Three new inputs, **world-frame** to match the world-frame velocity outputs
(`brain.rs` OUT_VX/OUT_VY), so a competent net can nearly copy the home vector
straight to its velocity command to walk home:

- `home_vx` = `dx / dist` (unit vector x toward own nest center)
- `home_vy` = `dy / dist` (unit vector y)
- `home_dist` = `dist / map_diagonal`, clamped to `[0,1]`

`dist == 0` (on the nest) yields `(0, 0, 0)`. Computed in `sense()` from the
ant's own colony `nest_center` (same datum `apply.rs` already uses for the homing
reward). `sense()` and `think()` gain a `colonies: &[ColonyState]` parameter.

Real ants track distance too (a *Cataglyphis* switches to search when its vector
runs out), so the distance channel is included, not just the unit vector.

## 2. Exploration trail (new pheromone)

A new scalar field `Pheromones::home`, deposited by **unladen ants**
(`carrying == 0`) in `deposit_passive`, complementing the existing food trail
(laid by *laden* ants). Unladen ants start and re-converge at the nest, so with
diffusion + evaporation the field peaks near the nest and along outbound routes;
climbing its gradient trends homeward.

- Diffuses/evaporates with the plain `diffuse_decay` path (like food/alarm), not
  the owner-aware scent path.
- New config: `home_trail_emission`, `home_diffusion`, `home_evaporation`.
  **Off the tunable rail** for now (fixed constants) to keep scope contained;
  can be promoted later.
- **Shared across colonies, not colony-owned.** With 8 nests, "climb it uphill"
  only strictly points to the *nearest* nest — but the colony-correct home vector
  disambiguates, so the trail's role is "well-travelled route." Per-colony
  ownership is a possible follow-up.

Sensed by the network via a new 7th per-whisker channel (`CH_HOME_TRAIL`) and a
new underfoot channel.

## Input vector layout (N_INPUTS 46 → 55)

| range | group | len |
|---|---|---|
| 0..35 | whiskers (5 × **7** channels) | 35 |
| 35..39 | underfoot (food, food-trail, alarm, **home-trail**) | 4 |
| 39..41 | friend/foe counts | 2 |
| 41..45 | proprioception | 4 |
| 45 | bias | 1 |
| 46..50 | recurrent memory | 4 |
| 50..53 | **home vector (vx, vy, dist)** | 3 |
| 53..55 | facing (sin, cos) | 2 |

Genome layout changes (`W1` is `N_INPUTS × N_HIDDEN1`), so **existing snapshots
become incompatible** — expected for an architecture change; fresh worlds only.

## Wire protocol

Both hand-written copies (`crates/server/src/protocol.rs`,
`web/src/protocol.ts`) change together; regenerate the golden-master fixtures.

- **Pheromone frame:** append a single-channel (R8) home-trail block of `w*h`
  bytes *after* the existing `w*h*4` RGBA block, max-downsampled and
  `squash_phero`'d like the other layers. Every current offset is unchanged.
  Web decodes `home` as a second `Uint8Array` and uploads it as a second R8
  texture; new `uShowHome` shader uniform and a **"home" layer toggle**.
- **Ant-detail frame:** `ANT_DETAIL_LEN` 433 → 469 (nine extra input f32s).
  Web input/hidden/output offsets are computed from `N_INPUTS`, so they follow
  automatically; add labels for the new inputs in `nnlabels.ts`.

## Testing

- Sim unit tests: home-vector math (on-nest → zeros; due-north nest → +y unit;
  distance normalization) and unladen-ant home-trail deposition.
- Update `sense.rs` layout-constant test and protocol length/offset tests.
- Regenerate fixtures (`cargo test -p server --test fixtures`); `npm test` fails
  until `protocol.ts` agrees — that failure is the point.
- `cargo test --workspace` and `npm test` green; build web + release; launch and
  screenshot the new "home" layer.
