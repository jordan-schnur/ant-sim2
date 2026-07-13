# Shaped Fitness (Growth Fix) — Implementation Plan (Plan A of B)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give evolution a dense gradient — reward food *harvested* as a stepping stone toward food *delivered* — so colonies stop starving at the 5-ant extinction floor and actually grow.

**Architecture:** Add one per-ant lifetime accumulator (`food_harvested`) and one tunable `Config` weight (`harvest_weight`). Parent selection and the hall of fame rank genomes by `fitness = food_delivered + harvest_weight · food_harvested` instead of by delivery alone. `harvest_weight = 0` reproduces the original delivery-only behavior exactly, so purity is a slider.

**Tech Stack:** Rust (`sim`, `server` crates), TypeScript (`web`). No new dependencies.

**Spec:** `docs/superpowers/specs/2026-07-13-antsim-growth-and-story.md` (Section 1).

## Global Constraints

- **`sim` stays pure:** no I/O, no `println!`, no sockets. (copied from prior plans)
- **Determinism is a tested property:** same seed + same config ⇒ identical `state_hash`, regardless of `RAYON_NUM_THREADS`. Every new field is written only in the serial apply phase, never read during the parallel think phase.
- **`food_harvested` is serialized** (unlike `attacking`), because fitness must survive save/load. Adding a serialized SoA column and a `Config` field **changes the snapshot layout and invalidates the golden master**; regenerating it is intentional and is a step in the tasks that cause it.
- **`delivered_total` on the wire is unchanged.** The colony charts must keep tracking the real objective (delivery), never the shaped proxy.
- **Config field ids 0–15 are frozen.** `harvest_weight` is id **16**.
- Rust edition 2021. Every task ends with a green `cargo test` for the crates it touched, and a commit.
- The default `harvest_weight = 0.02` is a *starting guess*, not a tuned value. Task 6 sweeps it against a real run.

---

## File Structure

- `crates/sim/src/config.rs` — add `harvest_weight` field + `Config::fitness()` method
- `crates/sim/src/ants.rs` — add `food_harvested: Vec<f32>` column
- `crates/sim/src/apply.rs` — credit `food_harvested` on grab; rank `record_death` by fitness
- `crates/sim/src/colony.rs` — `select_parent` weights by fitness
- `crates/sim/src/reproduce.rs` — pass `harvest_weight` into `select_parent`
- `crates/sim/src/world.rs` — add `food_harvested` to `state_hash`
- `crates/sim/tests/golden_master.bin` — regenerated fixture
- `crates/server/src/protocol.rs` — register field id 16
- `crates/server/tests/fixtures/*.bin` — regenerated config fixture
- `web/src/ui/tunables.ts` — add the `harvest weight` slider

---

## Task 1: `Config::harvest_weight` and `Config::fitness()`

**Files:**
- Modify: `crates/sim/src/config.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `Config.harvest_weight: f32` (default `0.02`); `Config::fitness(&self, delivered: f32, harvested: f32) -> f32`.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `crates/sim/src/config.rs`:

```rust
    #[test]
    fn harvest_weight_defaults_to_a_small_nudge() {
        assert_eq!(Config::default().harvest_weight, 0.02);
    }

    #[test]
    fn fitness_is_delivery_plus_weighted_harvest() {
        let c = Config { harvest_weight: 0.02, ..Config::default() };
        assert!((c.fitness(10.0, 100.0) - 12.0).abs() < 1e-6);
    }

    #[test]
    fn fitness_with_zero_weight_is_pure_delivery() {
        // The purity toggle: harvest_weight = 0 recovers the original thesis.
        let c = Config { harvest_weight: 0.0, ..Config::default() };
        assert_eq!(c.fitness(7.0, 999.0), 7.0);
    }

    #[test]
    fn a_single_delivery_outweighs_a_lifetime_of_harvesting() {
        // Anti-reward-hacking bound: any delivered unit must beat a plausible
        // lifetime of harvest-without-delivery at the default weight.
        let c = Config::default();
        let lifetime_harvest_only = c.fitness(0.0, 400.0); // busy forager, never delivers
        let one_delivery = c.fitness(10.0, 0.0);
        assert!(one_delivery > lifetime_harvest_only,
            "delivery {one_delivery} must dominate harvest {lifetime_harvest_only}");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sim --release harvest_weight fitness`
Expected: FAIL — `no field harvest_weight` / `no method named fitness`.

- [ ] **Step 3: Implement**

In the `Config` struct in `crates/sim/src/config.rs`, add the field in the mutation section (next to `mutation_rate`):

```rust
    // --- Fitness shaping ---
    /// Weight on lifetime food *harvested* in the selection fitness, relative to
    /// food *delivered* (weight 1.0). A dense gradient toward delivery: an ant
    /// that finds and grabs food is closer to a forager than one that never
    /// moves. Kept small so any real delivery dominates a lifetime of mere
    /// harvesting. `0.0` recovers the original delivery-only thesis exactly.
    pub harvest_weight: f32,
```

In `impl Default for Config`, add in the mutation block:

```rust
            harvest_weight: 0.02,
```

Add the method to `impl Config` (next to `cell_count`):

```rust
    /// Selection fitness: the real objective plus a small harvest nudge.
    #[inline]
    pub fn fitness(&self, delivered: f32, harvested: f32) -> f32 {
        delivered + self.harvest_weight * harvested
    }
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p sim --release harvest_weight fitness`
Expected: PASS.

- [ ] **Step 5: Regenerate the golden master**

Adding a `Config` field changes the serialized `World` layout, so the checked-in fixture no longer deserializes. Regenerate it (no physics changed yet, so this re-pins the same trajectory):

Run: `REGENERATE_GOLDEN=1 cargo test -p sim --release --test golden`
Then: `cargo test -p sim --release`
Expected: PASS (whole crate green).

- [ ] **Step 6: Commit**

```bash
git add crates/sim/src/config.rs crates/sim/tests/golden_master.bin
git commit -m "feat(sim): tunable harvest_weight and Config::fitness"
```

---

## Task 2: `Ants::food_harvested` accumulator column

**Files:**
- Modify: `crates/sim/src/ants.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `Ants.food_harvested: Vec<f32>` — a serialized SoA column, one entry per ant, starting at `0.0`, maintained in lockstep with the other columns.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `crates/sim/src/ants.rs`:

```rust
    #[test]
    fn newborn_food_harvested_starts_at_zero() {
        let mut a = Ants::new();
        a.push(spawn(0, 0, 0.0, 0.0));
        assert_eq!(a.food_harvested[0], 0.0);
    }
```

And add one line to the existing `every_parallel_vec_has_the_same_length` test, next to the `food_delivered` assertion:

```rust
        assert_eq!(a.food_harvested.len(), n);
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sim --release food_harvested every_parallel_vec`
Expected: FAIL — `no field food_harvested`.

- [ ] **Step 3: Implement**

In the `Ants` struct, add the column right after `food_delivered`:

```rust
    /// Lifetime food grabbed into cargo, whether or not it was ever delivered.
    /// A dense fitness stepping stone: see `Config::fitness`. Serialized (unlike
    /// `attacking`) because fitness must survive save/load.
    pub food_harvested: Vec<f32>,
```

In `push`, add next to `self.food_delivered.push(0.0);`:

```rust
        self.food_harvested.push(0.0);
```

In `retain_alive`, add next to `retain(&mut self.food_delivered, &keep);`:

```rust
        retain(&mut self.food_harvested, &keep);
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p sim --release food_harvested every_parallel_vec retain_alive`
Expected: PASS.

- [ ] **Step 5: Regenerate the golden master**

The new serialized column changes the snapshot layout. No physics changed yet (nothing reads or writes the column during a tick), so this re-pins the same trajectory:

Run: `REGENERATE_GOLDEN=1 cargo test -p sim --release --test golden`
Then: `cargo test -p sim --release`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/sim/src/ants.rs crates/sim/tests/golden_master.bin
git commit -m "feat(sim): food_harvested lifetime accumulator column"
```

---

## Task 3: Credit `food_harvested` on grab

**Files:**
- Modify: `crates/sim/src/apply.rs` (`apply_food`, around line 78-89)

**Interfaces:**
- Consumes: `Ants.food_harvested` (Task 2).
- Produces: `food_harvested[i]` increases by exactly the amount grabbed each tick.

This is a pure accumulator — nothing reads it during a tick yet, so the trajectory and `state_hash` are unchanged. No golden regeneration this task.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/sim/src/apply.rs` (the module already has a `Fixture` helper used by `grab_harvests_food_up_to_carry_capacity` — follow that test's setup):

```rust
    #[test]
    fn grabbing_food_credits_food_harvested() {
        let mut f = Fixture::one_ant_on_food();
        f.ants.carrying[0] = 0.0;
        f.ants.food_harvested[0] = 0.0;
        let intent = Intent { grab: true, ..Intent::default() };
        let mut ctx = f.ctx();
        apply_food(0, &intent, &mut f.ants, &mut ctx);
        assert!(f.ants.food_harvested[0] > 0.0, "grab must credit harvest");
        assert_eq!(f.ants.food_harvested[0], f.ants.carrying[0],
            "harvested equals what entered cargo this grab");
    }
```

> If `Fixture::one_ant_on_food` is not the exact helper name in the file, reuse whatever helper `grab_harvests_food_up_to_carry_capacity` uses — copy its construction lines verbatim into this test.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sim --release grabbing_food_credits`
Expected: FAIL — `food_harvested[0]` is still `0.0`.

- [ ] **Step 3: Implement**

In `apply_food`, in the `grab` branch (currently):

```rust
    if intent.grab && ants.carrying[i] < capacity {
        let want = ctx.cfg.harvest_rate.min(capacity - ants.carrying[i]);
        ants.carrying[i] += ctx.grid.harvest(c, want);
    } else if ...
```

change the two body lines to capture the taken amount and credit it:

```rust
    if intent.grab && ants.carrying[i] < capacity {
        let want = ctx.cfg.harvest_rate.min(capacity - ants.carrying[i]);
        let taken = ctx.grid.harvest(c, want);
        ants.carrying[i] += taken;
        ants.food_harvested[i] += taken;
    } else if ...
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p sim --release grabbing_food_credits`
Expected: PASS.

Then confirm the trajectory is untouched (the accumulator is not yet read):

Run: `cargo test -p sim --release`
Expected: PASS, golden master still green.

- [ ] **Step 5: Commit**

```bash
git add crates/sim/src/apply.rs
git commit -m "feat(sim): credit food_harvested when an ant grabs food"
```

---

## Task 4: Rank selection and the hall of fame by fitness

**Files:**
- Modify: `crates/sim/src/colony.rs` (`select_parent`)
- Modify: `crates/sim/src/reproduce.rs` (the `select_parent` call)
- Modify: `crates/sim/src/apply.rs` (`sweep_deaths`, the `record_death` call)
- Modify: `crates/sim/src/world.rs` (`state_hash`)

**Interfaces:**
- Consumes: `Config::fitness` (Task 1), `Ants.food_harvested` (Task 2).
- Produces: `ColonyState::select_parent(&self, ants: &Ants, harvest_weight: f32, rng: &mut Pcg32) -> Option<usize>` — **new signature** (adds `harvest_weight`). Selection weight becomes `delivered + harvest_weight·harvested + PARENT_EPS`. `record_death` is called with `cfg.fitness(delivered, harvested)`.

This task changes which genomes reproduce, so the trajectory and `state_hash` change. The golden master is regenerated to the new intended baseline.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `crates/sim/src/colony.rs`:

```rust
    #[test]
    fn select_parent_rewards_harvest_when_weight_is_positive() {
        // Two ants, neither has delivered. One harvested a lot. With a positive
        // weight the harvester should win the roulette almost always.
        let c = ColonyState::new(1);
        let mut ants = ants_with(&[(1, 0.0), (1, 0.0)]);
        ants.food_harvested[1] = 500.0;
        let mut r = Pcg32::new(21, 21);
        let wins = (0..1000)
            .filter(|_| c.select_parent(&ants, 0.02, &mut r) == Some(1))
            .count();
        assert!(wins > 850, "harvester won only {wins}/1000");
    }

    #[test]
    fn select_parent_with_zero_weight_ignores_harvest() {
        // The purity toggle at the selection layer: weight 0 => harvest is
        // invisible, so two zero-delivery ants are ~evenly chosen regardless of
        // how much one harvested.
        let c = ColonyState::new(1);
        let mut ants = ants_with(&[(1, 0.0), (1, 0.0)]);
        ants.food_harvested[1] = 500.0;
        let mut r = Pcg32::new(22, 22);
        let one = (0..1000)
            .filter(|_| c.select_parent(&ants, 0.0, &mut r) == Some(1))
            .count();
        assert!(one > 350 && one < 650, "weight 0 should stay fair, got {one}/1000");
    }
```

The existing `colony.rs` tests call `select_parent(&ants, &mut r)`. Update every such call to `select_parent(&ants, 0.0, &mut r)` — weight 0 preserves their delivery-only assertions. The calls are in: `select_parent_only_ever_returns_own_colony`, `select_parent_favours_higher_food_delivered`, `select_parent_never_strictly_excludes_a_zero_fitness_ant`, `select_parent_skips_the_dead`, `select_parent_returns_none_for_an_empty_colony`, `select_parent_is_deterministic_for_a_given_rng`.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sim --release select_parent`
Expected: FAIL — arity mismatch on `select_parent` and the two new tests unresolved.

- [ ] **Step 3: Implement**

In `crates/sim/src/colony.rs`, change `select_parent`'s signature and its two weight expressions:

```rust
    pub fn select_parent(&self, ants: &Ants, harvest_weight: f32, rng: &mut Pcg32) -> Option<usize> {
        let mut total = 0.0f32;
        for i in 0..ants.len() {
            if ants.alive[i] && ants.colony[i] == self.id {
                total += ants.food_delivered[i] + harvest_weight * ants.food_harvested[i] + PARENT_EPS;
            }
        }
        if total <= 0.0 {
            return None;
        }
        let mut target = rng.next_f32() * total;
        let mut last = None;
        for i in 0..ants.len() {
            if ants.alive[i] && ants.colony[i] == self.id {
                last = Some(i);
                target -= ants.food_delivered[i] + harvest_weight * ants.food_harvested[i] + PARENT_EPS;
                if target <= 0.0 {
                    return Some(i);
                }
            }
        }
        last
    }
```

In `crates/sim/src/reproduce.rs`, update the call (currently `colonies[ci].select_parent(ants, rng)`):

```rust
        let Some(p) = colonies[ci].select_parent(ants, cfg.harvest_weight, rng) else {
            break;
        };
```

In `crates/sim/src/apply.rs`, change the `record_death` call in `sweep_deaths` to rank the archive by fitness:

```rust
        let colony = &mut ctx.colonies[ants.colony[i] as usize];
        colony.record_death(
            ctx.cfg.fitness(ants.food_delivered[i], ants.food_harvested[i]),
            ants.lineage[i],
            &ants.genome[i],
            ctx.cfg.hall_of_fame_size,
        );
```

In `crates/sim/src/world.rs`, add `food_harvested` to `state_hash` so the hash reflects the new lifetime state, right after the `food_delivered` line:

```rust
            eat(&self.ants.food_harvested[i].to_bits().to_le_bytes());
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p sim --release select_parent`
Expected: PASS.

- [ ] **Step 5: Regenerate the golden master (new intended baseline)**

Physics changed on purpose — selection now rewards harvest. Regenerate:

Run: `REGENERATE_GOLDEN=1 cargo test -p sim --release --test golden`
Then: `cargo test -p sim --release`
Expected: PASS across the whole crate (determinism, snapshot, behavior, golden).

- [ ] **Step 6: Commit**

```bash
git add crates/sim/src/colony.rs crates/sim/src/reproduce.rs crates/sim/src/apply.rs crates/sim/src/world.rs crates/sim/tests/golden_master.bin
git commit -m "feat(sim): rank parent selection and hall of fame by shaped fitness"
```

---

## Task 5: Expose `harvest_weight` as a live tunable

**Files:**
- Modify: `crates/server/src/protocol.rs` (`CONFIG_FIELDS`, `field_mut`)
- Modify: `web/src/ui/tunables.ts`
- Regenerate: `crates/server/tests/fixtures/*.bin`

**Interfaces:**
- Consumes: `Config.harvest_weight` (Task 1).
- Produces: config field id **16** = `harvest_weight`, settable via the existing `0x06 SetConfig` command and rendered on the tuning rail.

Adding a field lengthens the `0x07` Config frame (count 16 → 17), which the cross-language fixtures cover, so they are regenerated here.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/server/src/protocol.rs`:

```rust
    #[test]
    fn field_id_16_sets_harvest_weight() {
        let mut cfg = Config::default();
        assert!(apply_config_field(&mut cfg, 16, 0.1));
        assert_eq!(cfg.harvest_weight, 0.1);
        // Clamped to >= 0 like the other non-evaporation fields.
        apply_config_field(&mut cfg, 16, -1.0);
        assert_eq!(cfg.harvest_weight, 0.0);
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p server field_id_16`
Expected: FAIL — id 16 returns `None` (`apply_config_field` is false).

- [ ] **Step 3: Implement**

In `crates/server/src/protocol.rs`, extend `CONFIG_FIELDS` to length 17 by appending:

```rust
pub const CONFIG_FIELDS: [&str; 17] = [
    // ... existing 16 entries unchanged ...
    "harvest_weight",
];
```

And add the arm to `field_mut` before `_ => return None`:

```rust
        16 => &mut cfg.harvest_weight,
```

(The `apply_config_field` clamp needs no new arm: id 16 falls through to `_ => value.max(0.0)`, which is the correct `>= 0` clamp.)

- [ ] **Step 4: Run to verify it passes, then regenerate fixtures**

Run: `cargo test -p server field_id_16`
Expected: PASS.

Regenerate the byte fixtures (the config frame grew by one field):

Run: `cargo test -p server --test fixtures`
Then the full server crate: `cargo test -p server`
Expected: PASS.

- [ ] **Step 5: Add the web slider and reconcile the web guard**

In `web/src/ui/tunables.ts`, append to the `TUNABLES` array:

```ts
  { id: 16, label: "harvest weight", min: 0, max: 0.2, scale: "linear", hint: "0 = deliver-only; nudge toward foraging" },
```

Run the web tests against the regenerated fixtures:

Run: `cd web && npm test`
Expected: PASS. If the config-frame test in `web/tests/protocol.test.ts` asserts a field **count**, update it from 16 to 17; if it asserts specific field values by id, add `harvest_weight` at id 16. Re-run until green.

- [ ] **Step 6: Commit**

```bash
git add crates/server/src/protocol.rs crates/server/tests/fixtures web/src/ui/tunables.ts web/tests/protocol.test.ts
git commit -m "feat(protocol,web): expose harvest_weight as config field 16"
```

---

## Task 6: Verify colonies actually grow (the empirical gate)

**Files:** none (verification + optional default tuning only).

This is the point of the whole plan. A green unit suite proves the mechanism is wired; it does **not** prove colonies grow. Run the sim and look at the numbers.

- [ ] **Step 1: Full workspace + web green**

Run: `cargo test --workspace --release`
Then: `cd web && npm test`
Expected: PASS everywhere.

- [ ] **Step 2: Baseline vs shaped, headless**

Run a long headless comparison at the default weight and at zero:

```bash
cargo run -p headless --release -- --ticks 200000 --every 20000 --seed 1 > shaped.csv
```

Then, to confirm the toggle recovers old behavior, temporarily run with harvest off. `headless` has no config flag for it, so verify the toggle in code instead: set `harvest_weight: 0.0` in a scratch `Config` in a throwaway test, or trust Task 1/4's zero-weight tests. The CSV that matters is `shaped.csv`.

- [ ] **Step 3: Read the growth signal**

Inspect `shaped.csv`. The columns to watch per colony:
- `population` — should exceed the extinction floor (5) for at least some colonies by tick ~100k. Before this plan it sat at 5.
- `births` — paid births should be **> 0** (the store is now funded by real foraging). Before this plan most colonies had zero.
- `delivered_total` — should bend upward, not stay flat.

Success criterion: at least a few of the 8 colonies climb above 5 ants with `births > 0`. If **every** colony is still pinned at 5 with zero paid births, the nudge is too weak.

- [ ] **Step 4: Tune `harvest_weight` if needed**

If growth is absent, raise the default in `crates/sim/src/config.rs` (try `0.05`, then `0.1`), regenerate the golden master (`REGENERATE_GOLDEN=1 cargo test -p sim --release --test golden`), and re-run Step 2. If growth is present but colonies balloon and crash (harvest is being over-rewarded and ants stop delivering), lower it (`0.01`). Record the chosen value and the observed effect in a note under `docs/superpowers/notes/`.

Guard against reward-hacking while tuning: watch that `delivered_total` keeps rising. If population grows while `delivered_total` stays flat, ants are gaming the harvest proxy (grabbing and dropping without delivering) — the weight is too high.

- [ ] **Step 5: Update the README and commit**

Update `README.md`'s "Does it evolve?" section to record that shaped fitness (harvest gradient) is now in, that `harvest_weight` on the tuning rail dials it (0 = original delivery-only thesis), and the observed effect on growth from Step 3.

```bash
git add README.md docs/superpowers/notes
git commit -m "docs: record the shaped-fitness growth result and harvest_weight tuning"
```

---

## Self-Review Notes

- **Spec coverage (Section 1):** `food_harvested` accumulator (Task 2/3), tunable `harvest_weight` with 0 = purity (Tasks 1, 4, 5), fitness used in both `select_parent` and `record_death` (Task 4), `delivered_total` wire stat untouched (verified — no task touches `encode_stats`), golden regenerated (Tasks 1, 2, 4), determinism preserved (new field written only in serial phase — Task 3 writes in `apply_food`, Task 4 reads in `sweep_deaths`, both serial). Covered.
- **Type consistency:** `select_parent(&self, ants, harvest_weight: f32, rng)` is defined in Task 4 and called with that arity in `reproduce.rs` (Task 4) and all `colony.rs` tests (Task 4). `Config::fitness(delivered, harvested)` defined Task 1, called in Task 4. `food_harvested` field name consistent across ants.rs, apply.rs, colony.rs, world.rs.
- **Empirical risk is isolated to Task 6**, which is why this is Plan A of B: growth is confirmed before the story/UI plan is written.
