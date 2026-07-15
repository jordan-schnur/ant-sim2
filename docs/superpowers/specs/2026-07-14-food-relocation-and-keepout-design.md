# Food Relocation, Nest Keep-Out, and Graph Tooltip — Design

**Date:** 2026-07-14
**Status:** Approved (brainstorm), pending implementation-plan

## Goal

Three changes the operator asked for while watching runs:

1. **Food should not regrow in place.** Today every food-patch cell stores a
   `fertility` and `Grid::regrow` refills it every tick, so a patch the ants
   found once refills forever. Instead, patches deplete for good, and fresh
   bundles appear elsewhere over time — so a depleted source forces the colony
   to *re-find* food, which is the behaviour the recent-productivity fitness was
   built to select for.
2. **Stop dumping food and stone on a colony's doorstep.** `food_patch` skips
   nest *tiles*, but nothing keeps a scattered patch or a stone blob out of the
   ring immediately around a nest, so colonies get walled in by rock or buried
   in food they never have to forage for.
3. **Graphs should show the exact value under the cursor** — a floating tooltip,
   not just the legend readout.

## Non-goal / clarification captured during brainstorm

The **colony store reading ~0 is not a bug** and is out of scope. The store is a
pass-through: `reproduce` spends it down to `< birth_cost` every tick and
refuelling ants take the remainder, so it sits near zero by design. A store that
*climbed* would mean the colony was failing to convert food into ants. If the
operator later wants visible reserves that is a separate economy change (a
birth-reserve floor), not part of this work.

## 1. Deplete-and-relocate food

### Patches become first-class state

Food amount still lives in `Grid::food` cells (ants sense it there, unchanged).
On top of that the world tracks the **patches** that produced it:

```rust
#[derive(Clone, Serialize, Deserialize)]
pub struct Patch {
    pub cx: f32,
    pub cy: f32,
    pub radius: f32, // captured at creation, so a later config change
    pub seed: f32,   // total food actually stamped (skips stone/nest/keepout cells)
}
```

`World` gains `pub patches: Vec<Patch>`, populated at worldgen (one guaranteed
patch per colony + `food_patch_count` scattered) and serialised in the snapshot.

### The spawn/relocation system

`Grid::regrow` and its per-tick call are **removed**; `fertility` is removed from
`Grid`. In its place, once every `food_spawn_interval` ticks, in the serial
fields phase (deterministic, uses `World::rng`):

- **Drop depleted patches:** a patch whose current live food
  (`sum of Grid::food over its stamp cells`) has fallen below
  `DEPLETION_FRAC * seed` is removed. `DEPLETION_FRAC = 0.15` ("depleted enough",
  not necessarily to zero — the operator's note).
- **Refill toward target:** while `patches.len() < food_patch_target`, stamp a
  new bundle at a random keep-out-respecting location and push it. Each stamp
  attempt caps its rejection retries and yields `None` on failure, so the loop
  always terminates.

Because this consumes `rng` inside the tick, the golden master regenerates (an
intended physics change). Patch fields are folded into `state_hash` so a
divergence in the patch set is caught by the determinism tests immediately, not
only once it leaks into `Grid::food`.

### Two live-tunable levers (wire-stable id scheme)

The retired `food_regrow` wire slot (**id 14**) is **repurposed** rather than
removed, so no existing field id shifts:

- **id 14:** `food_regrow` → `food_spawn_interval` (ticks between spawn passes).
- **id 23 (appended):** `food_patch_target` (steady-state live-patch count).

Both are `f32` in `Config` (the tuning wire carries only `f32`; they are cast to
integers at use). `CONFIG_FIELDS` grows 23 → 24. Headless `--set` arms and the
web `CONFIG_FIELDS` mirror + two sliders follow.

**Defaults (continuous with genesis, so only *relocation* is the new variable):**
`food_spawn_interval = 300`, `food_patch_target = 48` (`num_colonies` guaranteed
+ `food_patch_count` scattered at the shipped defaults).

## 2. Nest keep-out buffer

A worldgen constant `NEST_KEEPOUT_RADIUS = 4.0` (tiles from a nest centre):

- **Stone:** after nests are placed, clear stone within the keep-out radius of
  every nest centre (a post-pass, so it consumes no `rng` and does not perturb
  the map's blob layout). Colonies can never be walled in.
- **Food:** `food_patch` and every runtime spawn reject any cell within the
  keep-out radius of any nest centre, and reject a patch *centre* that lands
  within `keepout + food_patch_radius` of a nest so a bundle is never even
  partially dumped on the doorstep.

New guard tests: no stone and no food within `NEST_KEEPOUT_RADIUS` of any nest
centre, on several seeds.

## 3. Graph hover tooltip

A uPlot cursor plugin in `graphmodal.ts`: a floating box, positioned at the
cursor, listing the **tick** and each currently-visible series' **exact value**
at the hovered index (colony lines + world aggregate in single-metric mode;
each metric in overlay mode). Styled to match the existing dark modal; hidden
when the cursor leaves the plot. Pure front-end, no wire change.

## Determinism, golden master, fixtures

- `state_hash` folds the patch set (len + each patch's `cx,cy,radius,seed`).
- The golden master (`crates/sim/tests/golden_master.bin`) regenerates once per
  physics-changing task (regrow removal, keep-out, patch spawning).
- The NN is untouched — `N_INPUTS`/`N_PARAMS` unchanged — so the known-good
  forager fixture stays *valid*. But keep-out and patch spawning change the
  worldgen `rng` draw order, so the forager guard test must be **re-run**; it is
  regenerated (via the offline `search_for_a_forager` ignored test) *only if* it
  fails, never pre-emptively.
- The server cross-language fixtures (`expected.json`, `config.bin`) regenerate
  for the `CONFIG_FIELDS` 23 → 24 change; `web/tests/*` mirror the new size.

## Risks

- **RNG-order churn.** Any change to worldgen/tick `rng` consumption reshuffles
  every downstream draw for a seed. Contained by regenerating the golden master
  and re-verifying the forager guard; no other consumer depends on absolute
  draw order.
- **Patch-overlap accounting.** Live-food sums over overlapping patches
  double-count, so a patch may be judged alive/dead slightly early or late. Cheap
  to accept; patches rarely overlap and the threshold is soft.
- **Target vs genesis drift.** If `food_patch_target` is set below the genesis
  patch count, the map sheds patches down to target over the first few intervals
  rather than instantly; that is the intended, visible dynamic.

## Out of scope

- Colony store / birth-reserve economy (the "store is 0" non-bug above).
- Any NN input for patches (ants sense food from the grid, as today).
- Food *type* variety, moving patches, or patch size distribution changes.
