# Food Relocation, Nest Keep-Out, and Graph Tooltip Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Food patches deplete for good and fresh bundles relocate over time; colonies are never walled in or buried in food at their doorstep; graphs show the exact value under the cursor.

**Architecture:** Sim core (Rust) gains `World::patches` as first-class state, replacing per-tick in-place regrow with a deterministic deplete-and-relocate spawner in the serial fields phase. Worldgen gains a nest keep-out buffer. Two new levers ride the existing `f32` config wire (one repurposed slot, one appended). The front-end mirrors the wire and adds a uPlot cursor tooltip.

**Tech Stack:** Rust workspace (`sim`, `server`, `headless`), TypeScript + uPlot + Vite/Vitest web frontend. `cargo` at `$HOME/.cargo/bin/cargo` — every task's Rust commands must first `source "$HOME/.cargo/env"`.

## Global Constraints

- **Determinism is law.** All new per-tick state changes happen in the serial
  apply/fields phase, in a fixed order, using `World::rng`. Anything a tick can
  change and that affects the future must be folded into `World::state_hash`.
- **Golden master** (`crates/sim/tests/golden_master.bin`) pins physics via
  `state_hash`. Regenerate it, in the task that changes physics, with:
  `source "$HOME/.cargo/env" && REGENERATE_GOLDEN=1 cargo test -p sim --test golden`
  then confirm a plain `cargo test -p sim --test golden` passes.
- **Forager fixture** (known-good genome behavioural guard) must be **re-run**
  after any worldgen `rng`-order change and left green. Regenerate it (via the
  ignored `search_for_a_forager` test, ~5 min) **only if it fails** — never
  pre-emptively. `N_INPUTS`/`N_PARAMS` do not change in this plan.
- **Wire is hand-mirrored** between Rust `crates/server/src/protocol.rs` and
  TS `web/src/protocol.ts`, guarded by `crates/server/tests/fixtures/*` +
  `web/tests/protocol.test.ts`. Any wire change updates both sides and the
  fixtures together.
- **Config wire carries only `f32`.** New integer-valued config
  (`food_spawn_interval`, `food_patch_target`) is stored as `f32` and cast at
  use. `CONFIG_FIELDS` id 14 is repurposed (`food_regrow` → `food_spawn_interval`);
  `food_patch_target` is appended as id 23. No other id shifts.
- **Defaults:** `food_spawn_interval = 300.0`, `food_patch_target = 48.0`,
  `NEST_KEEPOUT_RADIUS = 4.0`, `DEPLETION_FRAC = 0.15`.
- No emoji in code or commits. Comments explain *why*. Match surrounding idioms.

---

### Task 1: Retire in-place food regrow

Remove the `fertility` field, `Grid::regrow`, its per-tick call, and the
`food_regrow` behaviour. Food no longer refills where it stood. (The `food_regrow`
config field is repurposed on the wire in Task 4, not deleted here — leave the
`Config` field in place for now so the wire keeps compiling; Task 3 renames it.)

**Files:**
- Modify: `crates/sim/src/grid.rs` (remove `fertility`, `regrow`; update tests)
- Modify: `crates/sim/src/worldgen.rs:99-108` (`food_patch` stops writing fertility)
- Modify: `crates/sim/src/world.rs:314` (remove the `regrow` call)
- Test: `crates/sim/src/grid.rs` inline tests; `crates/sim/tests/golden` (regen)

**Interfaces:**
- Consumes: nothing new.
- Produces: `Grid` no longer has `fertility` or `regrow`. `food_patch` in
  `worldgen.rs` still stamps `Grid::food` to `food_patch_max`, skipping
  stone/nest cells.

- [ ] **Step 1: Update the regrow tests to the new reality**

In `crates/sim/src/grid.rs`, delete the two tests that assert regrow behaviour
(`a_depleted_patch_regrows_toward_its_fertility`, `barren_ground_never_sprouts`)
and add one asserting harvested food stays gone:

```rust
#[test]
fn harvested_food_does_not_come_back_on_its_own() {
    // Regrow was removed: a drained cell stays drained until a new patch is
    // stamped on it. Nothing in Grid refills food.
    let mut g = Grid::new(&small());
    let i = g.idx(2, 2);
    g.food[i] = 10.0;
    g.harvest(i, 10.0);
    assert_eq!(g.food[i], 0.0);
    // No regrow method exists to call; the cell simply stays empty.
    assert_eq!(g.food[i], 0.0);
}
```

- [ ] **Step 2: Run it and watch it fail to compile**

Run: `source "$HOME/.cargo/env" && cargo test -p sim --lib grid 2>&1 | head -30`
Expected: compile error — `fertility`/`regrow` still referenced elsewhere (that
is fine; we remove them next).

- [ ] **Step 3: Remove `fertility` and `regrow` from `Grid`**

In `crates/sim/src/grid.rs`: delete the `pub fertility: Vec<f32>` field (and its
doc comment), delete its initialiser in `Grid::new`, and delete the entire
`pub fn regrow(&mut self, rate: f32)` method (and its doc comment). Update the
`new_grid_is_empty_dirt` test to drop the `fertility` assertion if present.

- [ ] **Step 4: Stop writing fertility in worldgen**

In `crates/sim/src/worldgen.rs`, `food_patch`:

```rust
fn food_patch(grid: &mut Grid, px: f32, py: f32, cfg: &Config) {
    let r = cfg.food_patch_radius;
    let maxf = cfg.food_patch_max;
    stamp(grid, px, py, r, |g, i| {
        if !g.stone[i] && g.nest[i] == crate::grid::NO_NEST {
            g.food[i] = maxf;
        }
    });
}
```

- [ ] **Step 5: Remove the per-tick regrow call**

In `crates/sim/src/world.rs`, delete the line `self.grid.regrow(self.cfg.food_regrow);`
(currently line 314, in "Phase 3: fields"). Leave the `phero.step` line above it.

- [ ] **Step 6: Build and run the sim suite (golden will fail — expected)**

Run: `source "$HOME/.cargo/env" && cargo test -p sim --lib 2>&1 | tail -20`
Expected: lib tests PASS. The golden integration test will fail on state_hash;
that is the intended physics change, fixed next.

- [ ] **Step 7: Regenerate the golden master, then verify**

Run:
```bash
source "$HOME/.cargo/env"
REGENERATE_GOLDEN=1 cargo test -p sim --test golden
cargo test -p sim --test golden
```
Expected: second run PASSES.

- [ ] **Step 8: Full workspace check + forager guard**

Run: `source "$HOME/.cargo/env" && cargo test 2>&1 | tail -30`
Expected: all PASS, including the forager guard (regrow removal does not change
worldgen `rng` order, so the map is unchanged; only the tick physics shifted).
If the forager guard fails, STOP and report — do not regenerate it in this task.

- [ ] **Step 9: Commit**

```bash
git add crates/sim/src/grid.rs crates/sim/src/worldgen.rs crates/sim/src/world.rs crates/sim/tests/golden_master.bin
git commit -m "feat(sim): retire in-place food regrow"
```

---

### Task 2: Nest keep-out buffer at worldgen

No stone and no food within `NEST_KEEPOUT_RADIUS` of any nest centre.

**Files:**
- Modify: `crates/sim/src/worldgen.rs` (constant, stone clear pass, food-patch keep-out)
- Test: `crates/sim/src/worldgen.rs` inline tests; golden (regen)

**Interfaces:**
- Consumes: `ColonyState::nest_center: (f32, f32)` (already set in `generate`).
- Produces: `pub const NEST_KEEPOUT_RADIUS: f32`; a helper
  `fn within_keepout(colonies: &[ColonyState], x: f32, y: f32, extra: f32) -> bool`
  used by `food_patch` and (Task 3) the runtime spawner.

- [ ] **Step 1: Write the failing guard tests**

Add to `crates/sim/src/worldgen.rs` tests:

```rust
#[test]
fn no_stone_within_keepout_of_any_nest() {
    let c = cfg();
    for seed in [1u64, 2, 7] {
        let (grid, colonies) = generate(&c, seed, &mut Pcg32::new(seed, 1));
        for col in &colonies {
            let (nx, ny) = col.nest_center;
            for i in 0..grid.stone.len() {
                if !grid.stone[i] { continue; }
                let (x, y) = ((i % grid.width as usize) as f32, (i / grid.width as usize) as f32);
                assert!(
                    (x + 0.5 - nx).hypot(y + 0.5 - ny) > NEST_KEEPOUT_RADIUS,
                    "stone inside keep-out of colony {} (seed {seed})", col.id
                );
            }
        }
    }
}

#[test]
fn no_food_within_keepout_of_any_nest() {
    let c = cfg();
    for seed in [1u64, 2, 7] {
        let (grid, colonies) = generate(&c, seed, &mut Pcg32::new(seed, 1));
        for col in &colonies {
            let (nx, ny) = col.nest_center;
            for i in 0..grid.food.len() {
                if grid.food[i] <= 0.0 { continue; }
                let (x, y) = ((i % grid.width as usize) as f32, (i / grid.width as usize) as f32);
                assert!(
                    (x + 0.5 - nx).hypot(y + 0.5 - ny) > NEST_KEEPOUT_RADIUS,
                    "food inside keep-out of colony {} (seed {seed})", col.id
                );
            }
        }
    }
}
```

- [ ] **Step 2: Run to confirm they fail**

Run: `source "$HOME/.cargo/env" && cargo test -p sim --lib worldgen 2>&1 | tail -20`
Expected: the two new tests FAIL (stone/food currently allowed next to nests).

- [ ] **Step 3: Add the constant and keep-out helper**

Near `NEST_RADIUS` in `crates/sim/src/worldgen.rs`:

```rust
/// No stone and no food within this many tiles of a nest centre, so a colony is
/// never walled in by rock or handed food on its own doorstep.
pub const NEST_KEEPOUT_RADIUS: f32 = 4.0;

/// True if `(x, y)` (a cell centre already offset by +0.5, or a patch centre)
/// lies within `NEST_KEEPOUT_RADIUS + extra` of any colony's nest centre.
pub fn within_keepout(colonies: &[ColonyState], x: f32, y: f32, extra: f32) -> bool {
    colonies.iter().any(|c| {
        let (nx, ny) = c.nest_center;
        (x - nx).hypot(y - ny) <= NEST_KEEPOUT_RADIUS + extra
    })
}
```

- [ ] **Step 4: Clear stone in the keep-out ring (post-pass, no rng)**

In `generate`, immediately AFTER the colony-placement loop (after the
`for id in 0..cfg.num_colonies` block closes, before the scattered-patch loop),
add a stone-clear pass over the whole grid:

```rust
// Clear stone around every nest centre. A post-pass, so it consumes no rng and
// leaves the blob layout (hence the golden) determined only by the stamp loop.
for i in 0..grid.stone.len() {
    if !grid.stone[i] { continue; }
    let (x, y) = ((i % grid.width as usize) as f32 + 0.5, (i / grid.width as usize) as f32 + 0.5);
    if within_keepout(&colonies, x, y, 0.0) {
        grid.stone[i] = false;
    }
}
```

- [ ] **Step 5: Make `food_patch` respect keep-out**

`food_patch` needs the colonies to test keep-out. Change its signature and body:

```rust
fn food_patch(grid: &mut Grid, colonies: &[ColonyState], px: f32, py: f32, cfg: &Config) {
    let r = cfg.food_patch_radius;
    let maxf = cfg.food_patch_max;
    stamp(grid, px, py, r, |g, i| {
        let (x, y) = ((i % g.width as usize) as f32 + 0.5, (i / g.width as usize) as f32 + 0.5);
        if !g.stone[i] && g.nest[i] == crate::grid::NO_NEST && !within_keepout(colonies, x, y, 0.0) {
            g.food[i] = maxf;
        }
    });
}
```

Update both call sites in `generate` to pass `&colonies`:
- the guaranteed seed patch: `food_patch(&mut grid, &colonies, px, py, cfg);` — note `colonies` is being built in the same loop, so move the guaranteed-patch stamp to a SECOND loop after all colonies exist (see Step 6), OR pass the slice built so far. Simplest and correct: stamp all guaranteed patches in a dedicated loop after the placement loop.

- [ ] **Step 6: Restructure so all guaranteed patches stamp after nests exist**

In `generate`, remove the `food_patch(...)` call from inside the colony
placement loop. After the stone-clear pass (Step 4), add:

```rust
// One guaranteed patch per colony, within foraging reach, now that every nest
// exists (so keep-out is tested against all nests, not just those placed so far).
for col in &colonies {
    let (nx, ny) = col.nest_center;
    let a = rng.next_f32() * std::f32::consts::TAU;
    let px = (nx + SEED_PATCH_DISTANCE * a.cos()).clamp(1.0, w - 2.0);
    let py = (ny + SEED_PATCH_DISTANCE * a.sin()).clamp(1.0, h - 2.0);
    food_patch(&mut grid, &colonies, px, py, cfg);
}
```

Keep the guaranteed-patch `rng.next_f32()` draw order identical to before (one
draw per colony, in id order) so the change to the golden is minimal and
intentional. Update the scattered-patch loop call to
`food_patch(&mut grid, &colonies, px, py, cfg);`.

- [ ] **Step 7: Run the worldgen tests**

Run: `source "$HOME/.cargo/env" && cargo test -p sim --lib worldgen 2>&1 | tail -25`
Expected: all worldgen tests PASS, including the two new keep-out tests and the
existing `each_colony_has_food_within_reach_of_its_nest`
(SEED_PATCH_DISTANCE 12 > keepout 4 + radius 6 = 10, so the seed patch still
lands partly outside keep-out and leaves food in reach). If
`each_colony_has_food_within_reach` fails, STOP and report — the keep-out may be
eating too much of the seed patch.

- [ ] **Step 8: Regenerate golden, verify, full suite + forager guard**

```bash
source "$HOME/.cargo/env"
REGENERATE_GOLDEN=1 cargo test -p sim --test golden
cargo test
```
Expected: all PASS. The worldgen `rng` order is unchanged (stone clear is a
post-pass; guaranteed-patch draws stay one-per-colony in id order), so the
forager guard should still pass. If it fails, STOP and report before regenerating.

- [ ] **Step 9: Commit**

```bash
git add crates/sim/src/worldgen.rs crates/sim/tests/golden_master.bin
git commit -m "feat(sim): nest keep-out buffer clears stone and food at the doorstep"
```

---

### Task 3: Patches as state + deplete-and-relocate spawner

**Files:**
- Modify: `crates/sim/src/config.rs` (rename `food_regrow` → `food_spawn_interval`, add `food_patch_target`)
- Modify: `crates/sim/src/worldgen.rs` (`Patch` struct, `generate` returns patches, `spawn_patch`, `patch_live`)
- Modify: `crates/sim/src/world.rs` (`patches` field, `maybe_spawn_food`, call it, fold into `state_hash`)
- Modify: `crates/headless/src/main.rs` (the `food_regrow` `--set` arm → `food_spawn_interval`, add `food_patch_target`)
- Test: inline in `worldgen.rs` and `world.rs`; golden (regen)

**Interfaces:**
- Consumes: `NEST_KEEPOUT_RADIUS`, `within_keepout`, `food_patch` (Task 2);
  `Config::food_patch_radius`, `food_patch_max`, `food_patch_count`.
- Produces:
  - `Config::food_spawn_interval: f32`, `Config::food_patch_target: f32`
    (both cast to integer at use). `Config::food_regrow` is REMOVED.
  - `worldgen::Patch { cx, cy, radius, seed }` (Serialize/Deserialize/Clone).
  - `generate(cfg, seed, rng) -> (Grid, Vec<ColonyState>, Vec<Patch>)`.
  - `worldgen::patch_live(grid, patch) -> f32` (sum of live food in a patch).
  - `worldgen::spawn_patch(grid, colonies, cfg, rng) -> Option<Patch>` (one
    keep-out-respecting bundle; `None` if no valid centre found within a retry cap).
  - `World::patches: Vec<Patch>`; `World::DEPLETION_FRAC` const.

- [ ] **Step 1: Config — rename and add fields**

In `crates/sim/src/config.rs`, in the `// --- Food ---` block: replace
`pub food_regrow: f32,` with:

```rust
    /// Ticks between food-relocation passes. Each pass drops depleted patches
    /// and tops the live count back up to `food_patch_target`. `< 1` disables.
    pub food_spawn_interval: f32,
    /// Steady-state number of live food patches the world maintains. Stored as
    /// f32 (the tuning wire is f32-only) and cast to a count at use.
    pub food_patch_target: f32,
```

In `Default`, replace `food_regrow: 0.002,` with:

```rust
            food_spawn_interval: 300.0,
            food_patch_target: 48.0,
```

- [ ] **Step 2: Fix the `food_regrow` config references (compile sweep)**

Search and update every remaining `food_regrow` reference in the workspace:
`source "$HOME/.cargo/env" && grep -rn food_regrow crates/`. Expected sites:
`crates/server/src/protocol.rs` (Task 4 owns the wire rename — for now change
its `field_mut` arm `14 => &mut cfg.food_regrow` to `14 => &mut cfg.food_spawn_interval`
and `CONFIG_FIELDS[14]` string to `"food_spawn_interval"` so the crate compiles),
and `crates/headless/src/main.rs` (Step 8 below). Do NOT append the new wire id
here — that is Task 4. This step only keeps the tree compiling.

- [ ] **Step 3: Write the `Patch` struct and helpers (worldgen)**

Add to `crates/sim/src/worldgen.rs`:

```rust
use serde::{Deserialize, Serialize};

/// A food source the world tracks so it can relocate food: the grid holds the
/// food amount, this holds where it came from and how much it started with.
#[derive(Clone, Serialize, Deserialize)]
pub struct Patch {
    pub cx: f32,
    pub cy: f32,
    pub radius: f32,
    pub seed: f32,
}

/// Total live food currently standing in a patch's footprint.
pub fn patch_live(grid: &Grid, p: &Patch) -> f32 {
    let mut sum = 0.0;
    let (x0, x1) = ((p.cx - p.radius) as i32, (p.cx + p.radius) as i32);
    let (y0, y1) = ((p.cy - p.radius) as i32, (p.cy + p.radius) as i32);
    for y in y0..=y1 {
        for x in x0..=x1 {
            if !grid.in_bounds(x, y) { continue; }
            if (x as f32 + 0.5 - p.cx).hypot(y as f32 + 0.5 - p.cy) <= p.radius {
                sum += grid.food[grid.idx_clamped(x, y)];
            }
        }
    }
    sum
}

/// Stamp one fresh bundle at a random keep-out-respecting location. Rejects
/// centres too close to any nest; gives up (returns None) after a retry cap so
/// the caller's fill loop always terminates.
pub fn spawn_patch(
    grid: &mut Grid,
    colonies: &[ColonyState],
    cfg: &Config,
    rng: &mut Pcg32,
) -> Option<Patch> {
    let w = cfg.width as f32;
    let h = cfg.height as f32;
    let r = cfg.food_patch_radius;
    for _ in 0..32 {
        let px = rng.next_f32() * w;
        let py = rng.next_f32() * h;
        // Keep the whole bundle off the doorstep: reject a centre within
        // keepout + radius of any nest.
        if within_keepout(colonies, px, py, r) {
            continue;
        }
        food_patch(grid, colonies, px, py, cfg);
        let mut patch = Patch { cx: px, cy: py, radius: r, seed: 0.0 };
        patch.seed = patch_live(grid, &patch);
        // A centre entirely on stone stamps nothing; treat as a failed attempt.
        if patch.seed <= 0.0 {
            continue;
        }
        return Some(patch);
    }
    None
}
```

- [ ] **Step 4: `generate` returns patches**

Change `generate` to build and return the patch list. Its signature becomes
`-> (Grid, Vec<ColonyState>, Vec<Patch>)`. Build `let mut patches = Vec::new();`
push a `Patch` for each guaranteed seed patch (in the Step-6/Task-2 loop) and
each scattered patch, using the same centre and `patch_live` for `seed`:

```rust
// guaranteed loop body, after food_patch(...):
let mut p = Patch { cx: px, cy: py, radius: cfg.food_patch_radius, seed: 0.0 };
p.seed = patch_live(&grid, &p);
if p.seed > 0.0 { patches.push(p); }
```
and identically in the scattered loop. Return `(grid, colonies, patches)`.

- [ ] **Step 5: Update `generate` call sites**

`source "$HOME/.cargo/env" && grep -rn "generate(" crates/sim/src`. Update the
worldgen inline tests (`let (g1, _) = generate(...)` → `let (g1, _, _) = ...`,
etc.) and `World::new` in `world.rs` (`let (grid, colonies) = generate(...)` →
`let (grid, colonies, patches) = generate(...)`; store `patches` on the world in
Step 7).

- [ ] **Step 6: Write the failing spawner test (world)**

Add to `crates/sim/src/world.rs` tests:

```rust
#[test]
fn a_depleted_patch_is_replaced_elsewhere() {
    let mut c = Config { width: 64, height: 64, num_colonies: 2, initial_ants_per_colony: 0,
        food_patch_count: 3, food_spawn_interval: 10.0, food_patch_target: 5.0, ..Config::default() };
    let mut w = World::new(&c, 1);
    let target = c.food_patch_target as usize;
    // Drain every patch to empty, so the next pass must drop and refill them.
    for p in w.patches.clone() {
        let (x0, x1) = ((p.cx - p.radius) as i32, (p.cx + p.radius) as i32);
        let (y0, y1) = ((p.cy - p.radius) as i32, (p.cy + p.radius) as i32);
        for y in y0..=y1 { for x in x0..=x1 {
            if w.grid.in_bounds(x, y) { let i = w.grid.idx_clamped(x, y); w.grid.food[i] = 0.0; }
        }}
    }
    // Run past one spawn interval.
    for _ in 0..(c.food_spawn_interval as u64 + 1) { w.step(); }
    assert_eq!(w.patches.len(), target, "world did not refill to target after depletion");
    let live: f32 = w.grid.food.iter().sum();
    assert!(live > 0.0, "no fresh food after relocation");
}
```

Run: `source "$HOME/.cargo/env" && cargo test -p sim --lib a_depleted_patch 2>&1 | tail -20`
Expected: FAIL (no `patches` field / no spawner yet).

- [ ] **Step 7: Add `patches` to `World` and the spawner**

In `crates/sim/src/world.rs`:
- Add `pub patches: Vec<crate::worldgen::Patch>,` to the `World` struct (a
  serialised field — place it before the `#[serde(skip)] spatial`).
- In `World::new`, store the returned `patches`.
- Add the constant and method:

```rust
/// A patch with less than this fraction of its seed food left is "depleted
/// enough" to relocate. Soft on purpose — the operator wanted "depleted enough",
/// not fully drained.
const DEPLETION_FRAC: f32 = 0.15;

fn maybe_spawn_food(&mut self) {
    let interval = self.cfg.food_spawn_interval;
    if interval < 1.0 { return; }
    if self.tick_count % (interval as u64) != 0 { return; }

    self.patches.retain(|p| {
        crate::worldgen::patch_live(&self.grid, p) >= DEPLETION_FRAC * p.seed
    });

    let target = self.cfg.food_patch_target.max(0.0) as usize;
    while self.patches.len() < target {
        match crate::worldgen::spawn_patch(&mut self.grid, &self.colonies, &self.cfg, &mut self.rng) {
            Some(p) => self.patches.push(p),
            None => break, // no valid location found this pass; try again next interval
        }
    }
}
```

- Call it in the tick, in "Phase 3: fields", where the `regrow` line used to be
  (after `self.phero.step(&self.cfg);`):

```rust
        self.phero.step(&self.cfg);
        self.maybe_spawn_food();
```

- [ ] **Step 8: Fold patches into `state_hash`**

In `World::state_hash`, after the `grid.food` loop and before returning `h`:

```rust
        eat(&(self.patches.len() as u32).to_le_bytes());
        for p in &self.patches {
            eat(&p.cx.to_bits().to_le_bytes());
            eat(&p.cy.to_bits().to_le_bytes());
            eat(&p.radius.to_bits().to_le_bytes());
            eat(&p.seed.to_bits().to_le_bytes());
        }
```

- [ ] **Step 9: Headless `--set` arms**

In `crates/headless/src/main.rs`, replace the
`"food_regrow" => cfg.food_regrow = f(value)?,` arm with:

```rust
        "food_spawn_interval" => cfg.food_spawn_interval = f(value)?,
        "food_patch_target" => cfg.food_patch_target = f(value)?,
```

- [ ] **Step 10: Build, run lib + headless tests**

Run: `source "$HOME/.cargo/env" && cargo test -p sim --lib 2>&1 | tail -25 && cargo test -p headless 2>&1 | tail -10`
Expected: all PASS, including `a_depleted_patch_is_replaced_elsewhere`.

- [ ] **Step 11: Regenerate golden, verify, full suite + forager guard**

```bash
source "$HOME/.cargo/env"
REGENERATE_GOLDEN=1 cargo test -p sim --test golden
cargo test
```
Expected: all PASS. `maybe_spawn_food` consumes `rng` only every
`food_spawn_interval` ticks; the golden run is short enough that whether it fires
or not, the regenerated hash is deterministic. If the forager guard fails, STOP
and report (do not regenerate here without confirming the failure is the
expected worldgen-rng shift, not a real regression).

- [ ] **Step 12: Commit**

```bash
git add crates/sim/src/config.rs crates/sim/src/worldgen.rs crates/sim/src/world.rs crates/headless/src/main.rs crates/server/src/protocol.rs crates/sim/tests/golden_master.bin
git commit -m "feat(sim): deplete-and-relocate food patches"
```

---

### Task 4: Wire the two food levers (server)

**Files:**
- Modify: `crates/server/src/protocol.rs` (`CONFIG_FIELDS` 23→24, `field_mut`, clamp)
- Modify: `crates/server/tests/fixtures/*` (regen), inline protocol tests
- Test: `crates/server/src/protocol.rs` inline tests; fixtures

**Interfaces:**
- Consumes: `Config::food_spawn_interval` (id 14), `Config::food_patch_target` (new id 23).
- Produces: `CONFIG_FIELDS: [&str; 24]`; wire round-trips both fields.

- [ ] **Step 1: Update the CONFIG_FIELDS count assertion test first**

In `crates/server/src/protocol.rs` inline tests, find the test that asserts the
field count / config size and bump its expectation from 23 to 24. Add a
round-trip assertion for the new field:

```rust
        apply_config_field(&mut cfg, 23, 60.0);
        assert_eq!(read_config_field(&cfg, 23), Some(60.0)); // food_patch_target
        apply_config_field(&mut cfg, 14, 250.0);
        assert_eq!(read_config_field(&cfg, 14), Some(250.0)); // food_spawn_interval
```

Run: `source "$HOME/.cargo/env" && cargo test -p server 2>&1 | tail -20`
Expected: FAIL (count still 23; id 23 unknown).

- [ ] **Step 2: Repurpose id 14, append id 23**

`CONFIG_FIELDS`: change `[&str; 23]` to `[&str; 24]`; entry 14 is already
`"food_spawn_interval"` (from Task 3 Step 2); append `"food_patch_target"` as the
last entry (id 23).

`field_mut`: entry `14 => &mut cfg.food_spawn_interval` (already changed in Task
3); add `23 => &mut cfg.food_patch_target,` before the `_ => return None` arm.

- [ ] **Step 3: Clamp the new levers sensibly**

In `apply_config_field`'s `match id`, add arms:

```rust
        // Spawn interval: at least 1 tick between passes (0 would divide-by-zero
        // in the `% interval` check; the sim also treats < 1 as "disabled").
        14 => value.max(1.0),
        // Patch target: a non-negative count.
        23 => value.max(0.0),
```

Place these before the catch-all `_ => value.max(0.0)`. (id 14 previously fell
through to the catch-all as `food_regrow`; it now needs its own floor of 1.)

- [ ] **Step 4: Run server unit tests**

Run: `source "$HOME/.cargo/env" && cargo test -p server --lib 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Regenerate the cross-language fixtures**

Find how fixtures are generated (a `REGENERATE`-style env or an ignored test):
`source "$HOME/.cargo/env" && grep -rn "REGEN\|expected.json\|fn.*fixture" crates/server/tests`.
Regenerate `expected.json` and any `config.bin` so they carry 24 fields, then
run the fixture test to confirm green. Record the exact command you used in the
report.

- [ ] **Step 6: Full workspace test**

Run: `source "$HOME/.cargo/env" && cargo test 2>&1 | tail -20`
Expected: all PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/server/src/protocol.rs crates/server/tests/fixtures
git commit -m "feat(protocol): wire food_spawn_interval and food_patch_target levers"
```

---

### Task 5: Web — mirror the wire and add the two sliders

**Files:**
- Modify: `web/src/protocol.ts` (`CONFIG_FIELDS` mirror: id 14 rename, append id 23)
- Modify: `web/src/ui/tunables.ts` (id 14 relabel, add id 23)
- Modify: `web/tests/state.test.ts:80` (config size 23 → 24) and any protocol test
- Test: `web/tests/protocol.test.ts`, `web/tests/state.test.ts`

**Interfaces:**
- Consumes: the Task 4 wire (24 config fields; id 14 = food_spawn_interval, id 23 = food_patch_target).
- Produces: two rail sliders.

- [ ] **Step 1: Update the config-size expectation**

In `web/tests/state.test.ts`, change `expect(store.state.config.size).toBe(23);`
to `toBe(24);`. If `web/tests/protocol.test.ts` asserts a field count or lists
`CONFIG_FIELDS`, update it to include the two changes.

Run: `cd web && npx vitest run 2>&1 | tail -20`
Expected: FAIL (still 23 on the TS side).

- [ ] **Step 2: Mirror `CONFIG_FIELDS` in `web/src/protocol.ts`**

Find the `CONFIG_FIELDS` array in `web/src/protocol.ts`. Rename index 14
`"food_regrow"` → `"food_spawn_interval"` and append `"food_patch_target"` as the
last (index 23) entry. Keep it byte-for-byte aligned with the Rust order.

- [ ] **Step 3: Relabel slider 14 and add slider 23 in `tunables.ts`**

In `web/src/ui/tunables.ts` `TUNABLES`, replace the id-14 entry:

```ts
  { id: 14, label: "food spawn interval", min: 20, max: 2000, scale: "linear", hint: "ticks between food relocation passes" },
```

and add before the closing bracket:

```ts
  { id: 23, label: "food patch target", min: 1, max: 80, scale: "linear", hint: "how many live food patches the world maintains" },
```

(`formatValue` already renders `max > 20` with one decimal and `max <= 80`
with two — both read fine for these ranges.)

- [ ] **Step 4: Run the web suite + typecheck**

Run: `cd web && npx tsc --noEmit && npx vitest run 2>&1 | tail -25`
Expected: typecheck clean; all tests PASS (config size 24, protocol mirror green).

- [ ] **Step 5: Commit**

```bash
git add web/src/protocol.ts web/src/ui/tunables.ts web/tests/state.test.ts web/tests/protocol.test.ts
git commit -m "feat(web): food spawn interval and patch target sliders"
```

---

### Task 6: Graph hover tooltip

**Files:**
- Modify: `web/src/ui/graphmodal.ts` (uPlot cursor tooltip plugin)
- Modify: `web/index.html` (tooltip CSS, near the existing graph CSS)
- Test: `web/tests/*` typecheck; manual/visual verification note

**Interfaces:**
- Consumes: the existing `openGraph` uPlot setup (`opts`, `plotHost`, `series`).
- Produces: a floating value tooltip that follows the cursor inside the plot.

- [ ] **Step 1: Add the tooltip element and plugin**

In `web/src/ui/graphmodal.ts`, inside `openGraph`, create a tooltip div and a
uPlot plugin that positions/fills it from the cursor. Add near `plotHost`:

```ts
  const tip = document.createElement("div");
  tip.className = "graph-tip";
  tip.style.display = "none";
  plotHost.append(tip);
```

Define a plugin factory (module scope, above `openGraph`):

```ts
/**
 * A cursor tooltip: on every cursor move uPlot gives us the focused data index;
 * we read each visible series' value there and render a small floating box. The
 * legend already shows values, but a box at the point is what the operator asked
 * for when reading an exact tick off the curve.
 */
function tooltipPlugin(tip: HTMLElement): uPlot.Plugin {
  return {
    hooks: {
      setCursor: (u: uPlot) => {
        const { idx, left, top } = u.cursor;
        if (idx == null || left == null || top == null || left < 0) {
          tip.style.display = "none";
          return;
        }
        const tick = u.data[0][idx];
        if (tick == null) { tip.style.display = "none"; return; }
        let rows = `<div class="graph-tip-x">tick ${Math.round(tick as number)}</div>`;
        for (let s = 1; s < u.series.length; s++) {
          const ser = u.series[s];
          if (ser.show === false) continue;
          const v = u.data[s][idx];
          if (v == null || Number.isNaN(v)) continue;
          const stroke = typeof ser.stroke === "function" ? ser.stroke(u, s) : ser.stroke;
          rows += `<div class="graph-tip-row"><span class="graph-tip-swatch" style="background:${stroke ?? "#9aa"}"></span>` +
            `${ser.label}: <b>${(v as number) >= 100 ? Math.round(v as number) : (v as number).toFixed(2)}</b></div>`;
        }
        tip.innerHTML = rows;
        tip.style.display = "";
        // Offset from the cursor; flip left near the right edge so it stays in view.
        const pad = 12;
        const flip = left > u.over.clientWidth - 160;
        tip.style.left = `${flip ? left - tip.offsetWidth - pad : left + pad}px`;
        tip.style.top = `${top + pad}px`;
      },
    },
  };
}
```

Wire it into `opts` in `build()`:

```ts
      plugins: [tooltipPlugin(tip)],
```

(add the `plugins` key alongside `series`, `hooks`, etc. in the `opts` object).

- [ ] **Step 2: Style the tooltip**

In `web/index.html`, near the existing `.graph-*` rules, add:

```css
.graph-plot { position: relative; }
.graph-tip {
  position: absolute; z-index: 5; pointer-events: none;
  background: rgba(20, 22, 28, 0.95); border: 1px solid rgba(255,255,255,0.15);
  border-radius: 6px; padding: 6px 8px; font-size: 12px; color: #dfe3ea;
  white-space: nowrap; box-shadow: 0 4px 14px rgba(0,0,0,0.4);
}
.graph-tip-x { font-weight: 600; margin-bottom: 3px; color: #9aa; }
.graph-tip-row { display: flex; align-items: center; gap: 5px; }
.graph-tip-swatch { width: 9px; height: 9px; border-radius: 2px; display: inline-block; }
```

(`.graph-plot` must be `position: relative` so the absolute tooltip is placed
relative to the plot; confirm it is not already set elsewhere and adjust rather
than duplicate.)

- [ ] **Step 3: Typecheck and build**

Run: `cd web && npx tsc --noEmit && npm run build 2>&1 | tail -15`
Expected: clean typecheck, successful build.

- [ ] **Step 4: Visual verification (run the app)**

Rebuild the server and web, launch, open the Stats tab, open a graph, hover the
plot. Confirm the tooltip follows the cursor and shows `tick N` plus each visible
series' value, flips near the right edge, and disappears when the cursor leaves.
Report what you saw (a screenshot if the harness supports it). Per the run skill:
driving it, not just launching it.

```bash
source "$HOME/.cargo/env" && cargo build --release -p server
cd web && npm run build
lsof -ti tcp:8080 | xargs kill -9 2>/dev/null; true
./target/release/server --web web/dist   # background; open http://127.0.0.1:8080
```

- [ ] **Step 5: Commit**

```bash
git add web/src/ui/graphmodal.ts web/index.html
git commit -m "feat(web): value tooltip under the graph cursor"
```

---

## Final verification (after all tasks)

- [ ] `source "$HOME/.cargo/env" && cargo test` — all green, golden current.
- [ ] `cd web && npx tsc --noEmit && npx vitest run` — all green.
- [ ] Forager guard still passing (or, if regenerated, note it in the branch summary with the new mean score).
- [ ] A short headless A/B sanity run: default `food_spawn_interval`/`food_patch_target` vs a scarcer setting (e.g. `--set food_patch_target=12`), confirming colonies survive and delivery continues under relocation.
- [ ] Launch and watch: patches visibly relocate over time, no colony walled in or ringed with food, graph tooltip works.
