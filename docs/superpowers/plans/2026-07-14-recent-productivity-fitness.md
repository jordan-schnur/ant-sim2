# Recent-Productivity Fitness — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use superpowers:subagent-driven-development or superpowers:executing-plans to implement task-by-task. Each task ends with its tests green and a commit.

**Goal:** Add a decaying per-ant "recent productivity" (EMA of harvest + delivery + kills) as a shaping term in selection fitness, so evolution rewards ants that *keep* being productive rather than one-time luck or old age. Cumulative delivered food stays the permanent objective.

**Architecture:** One `f32` per ant, `recent_productivity`, a leaky accumulator updated in the deterministic serial apply phase. It enters `Config::fitness` behind a weight (`productivity_weight`, default 0.1; `0` = today's behavior exactly). Everything else is plumbing: a second config field for the decay rate, the wire/UI for the two sliders, and determinism/snapshot coverage of the new state.

**Tech Stack:** Rust workspace (`sim`, `server`, `headless`) + TypeScript/Vite web.

## Global Constraints

- **Determinism is sacred.** `recent_productivity` is updated only in the serial apply phase in ant-id order; `state_hash` and the snapshot must cover it. The golden fixture (`crates/sim/tests/golden_master.bin`) regenerates — a deliberate physics change.
- **The NN is untouched.** `N_INPUTS`/`N_PARAMS` do not change. The known-good forager fixture (`known_good_forager.bin`) is **not** regenerated; its guard test must still pass on its own.
- **`delivered` stays cumulative and dominant.** The EMA is an *added* term, never a replacement. Keep `productivity_weight` modest so a burst of harvesting cannot outrank a real delivery.
- **Wire targets:** `CONFIG_FIELDS` 21 → 23 (`productivity_weight` id 21, `productivity_decay` id 22); `ANT_DETAIL_LEN` 453 → 457 (Task 9 only).
- **New config defaults:** `productivity_weight = 0.1`, `productivity_decay = 0.99`. `productivity_weight = 0` must recover today's selection exactly.
- **Clamps:** weight `≥ 0`; decay in `(0,1)`.

---

## Task 1: Config — fields + `fitness` gains the recent term

**Files:**
- Modify: `crates/sim/src/config.rs`

**Interfaces produced:** `Config { productivity_weight: f32, productivity_decay: f32 }`; `Config::fitness(delivered, harvested, homing, recent) -> f32`.

- [ ] **Step 1: Failing tests** (add to `config.rs` tests module):

```rust
#[test]
fn fitness_adds_weighted_recent_productivity() {
    let c = Config { productivity_weight: 0.1, ..Config::default() };
    // delivered 10 + 0.02*100 + 0.05*20 + 0.1*50 = 10 + 2 + 1 + 5 = 18
    assert!((c.fitness(10.0, 100.0, 20.0, 50.0) - 18.0).abs() < 1e-5);
}

#[test]
fn zero_productivity_weight_recovers_prior_fitness() {
    let c = Config { productivity_weight: 0.0, ..Config::default() };
    // recent is invisible: matches the old three-arg result.
    assert_eq!(c.fitness(7.0, 0.0, 0.0, 999.0), 7.0);
}

#[test]
fn productivity_decay_is_a_valid_rate() {
    let d = Config::default().productivity_decay;
    assert!(d > 0.0 && d < 1.0, "decay must be in (0,1), got {d}");
}
```

- [ ] **Step 2: Run — expect FAIL** (`fitness` takes 3 args; fields absent).

Run: `cargo test -p sim config::`

- [ ] **Step 3: Implement.** Add fields after `homing_weight`:

```rust
    /// Weight on a decaying EMA of recent useful work (harvest + delivery +
    /// kills). Rewards ants that *keep* producing rather than fluking once and
    /// coasting. `0.0` disables it, recovering pure cumulative selection.
    pub productivity_weight: f32,
    /// Per-tick decay of `Ants::recent_productivity`. ~69-tick half-life at 0.99.
    pub productivity_decay: f32,
```

Defaults (in `Default`, near `homing_weight`):

```rust
            productivity_weight: 0.1,
            productivity_decay: 0.99,
```

Update `fitness`:

```rust
    #[inline]
    pub fn fitness(&self, delivered: f32, harvested: f32, homing: f32, recent: f32) -> f32 {
        delivered
            + self.harvest_weight * harvested
            + self.homing_weight * homing
            + self.productivity_weight * recent
    }
```

Fix the two existing fitness tests (`fitness_is_delivery_plus_weighted_harvest_and_homing`, `fitness_with_zero_weights_is_pure_delivery`, `a_single_delivery_outweighs_a_lifetime_of_harvesting`) to pass a 4th arg `0.0` where they call `fitness`.

- [ ] **Step 4: Run — expect PASS.** `cargo test -p sim config::`
- [ ] **Step 5: Commit.** `feat(sim): productivity_weight/decay config + fitness recent term`

---

## Task 2: Ants — the `recent_productivity` field

**Files:**
- Modify: `crates/sim/src/ants.rs`

**Interfaces produced:** `Ants::recent_productivity: Vec<f32>`, initialised `0.0` per spawn, compacted by `retain_alive`, serialized.

- [ ] **Step 1: Failing test** (in `ants.rs` tests, or a new one):

```rust
#[test]
fn a_new_ant_starts_with_zero_recent_productivity() {
    let mut a = Ants::new();
    a.push(test_spawn(0, 1)); // reuse the module's spawn helper / inline a Spawn
    assert_eq!(a.recent_productivity[0], 0.0);
}
```

(If `ants.rs` has no spawn helper, mirror the `Spawn { .. }` used in other tests here.)

- [ ] **Step 2: Run — expect FAIL** (field absent).
- [ ] **Step 3: Implement.**
  - Declaration, beside `food_homing` (~line 46):

```rust
    /// Decaying EMA of recent useful work — see `Config::productivity_weight`.
    /// Serialized: it is real selection state, not a per-tick scratch value.
    pub recent_productivity: Vec<f32>,
```

  - `Ants::new()` initializer: add `recent_productivity: Vec::new(),`.
  - `push` (~line 145): add `self.recent_productivity.push(0.0);`.
  - `retain_alive` (~line 198): add `retain(&mut self.recent_productivity, &keep);`.
  - Confirm `Ants` still derives `Serialize, Deserialize` (it does) — the field rides along automatically. Snapshot format changes; that is acceptable (old snapshots are already versioned by content).

- [ ] **Step 4: Run — expect PASS.** `cargo test -p sim ants::`
- [ ] **Step 5: Commit.** `feat(sim): per-ant recent_productivity accumulator`

---

## Task 3: Apply — feed the EMA and decay it

**Files:**
- Modify: `crates/sim/src/apply.rs`

**Consumes:** Task 2's `recent_productivity`. **Produces:** the field rises on harvest (`apply_food`, ~line 153), delivery (`apply_food`, ~line 179), and kill (`apply_combat`, ~line 257 `scavenged`); decays once per tick in `apply_metabolism`.

- [ ] **Step 1: Failing tests** (in `apply.rs` tests):

```rust
#[test]
fn harvesting_raises_recent_productivity() {
    let mut f = fixture(&[(8.5, 8.5, 1)]);
    // place food under the ant and grab it (mirror grabbing_food_credits_food_harvested)
    // ... after the grab:
    assert!(f.ants.recent_productivity[0] > 0.0, "a harvest must register");
}

#[test]
fn an_idle_tick_decays_recent_productivity() {
    let mut f = fixture(&[(8.5, 8.5, 1)]);
    f.ants.recent_productivity[0] = 100.0;
    let (ants, mut ctx) = f.split();
    apply_metabolism(0, ants, ctx.cfg);
    assert!(f.ants.recent_productivity[0] < 100.0, "decay must shrink it");
    assert!((f.ants.recent_productivity[0] - 100.0 * f.cfg.productivity_decay).abs() < 1e-3);
}

#[test]
fn a_kill_registers_recent_productivity() {
    // Mirror the existing combat kill test (`killer absorbed the corpse`), then:
    // assert the killer's recent_productivity rose by the scavenged energy.
}
```

(Use the file's existing `fixture`/`split` harness and the harvest/kill test setups as templates — do not invent a new harness.)

- [ ] **Step 2: Run — expect FAIL.**
- [ ] **Step 3: Implement.**
  - After `ants.food_harvested[i] += taken;` (~153): `ants.recent_productivity[i] += taken;`
  - After `ants.food_delivered[i] += load;` (~179): `ants.recent_productivity[i] += load;`
  - In `apply_combat`, after the attacker absorbs `scavenged` (~257): `ants.recent_productivity[i] += scavenged;` (attacker is `i`; confirm the index name at that site).
  - In `apply_metabolism`, add the decay (runs once per living ant per tick, in id order — the serial phase, so deterministic):

```rust
    // Recent-productivity EMA bleeds toward zero every tick; harvest/deliver/
    // kill re-inflate it elsewhere in the apply phase.
    ants.recent_productivity[i] *= cfg.productivity_decay;
```

  Add a one-line comment where the field is fed noting it mirrors the cumulative counter it sits beside.

- [ ] **Step 4: Run — expect PASS.** `cargo test -p sim apply::`
- [ ] **Step 5: Commit.** `feat(sim): feed and decay recent_productivity in the apply phase`

---

## Task 4: Selection — living picks and the death archive use the term

**Files:**
- Modify: `crates/sim/src/colony.rs` (`select_parent`)
- Modify: `crates/sim/src/apply.rs` (`sweep_deaths`, the `record_death` fitness call ~line 313)

**Consumes:** `recent_productivity`, `Config::fitness`'s 4th arg. **Produces:** an ant's live selection weight and its archived (at-death) fitness both include `productivity_weight * recent_productivity`.

- [ ] **Step 1: Failing test** (in `colony.rs` tests):

```rust
#[test]
fn select_parent_favours_recent_activity_at_equal_cumulative_stats() {
    // Two same-colony ants, identical delivered/harvested; ant 1 is recently active.
    let c = ColonyState::new(1);
    let mut ants = ants_with(&[(1, 0.0), (1, 0.0)]);
    ants.recent_productivity[1] = 500.0;
    let mut r = Pcg32::new(31, 31);
    let wins = (0..1000)
        .filter(|_| c.select_parent(&ants, 0.02, 0.05, 0.1, &mut r) == Some(1))
        .count();
    assert!(wins > 850, "recently-active ant won only {wins}/1000");
}
```

Note: this task changes `select_parent`'s signature to take `productivity_weight`. Update the **existing** `select_parent` tests' call sites to pass the new arg (use `0.0` where they intend the old behavior, so their assertions still hold).

- [ ] **Step 2: Run — expect FAIL** (arity mismatch + no recent term).
- [ ] **Step 3: Implement.**
  - `ColonyState::select_parent` gains a `productivity_weight: f32` parameter; its inner `weight` closure adds `+ productivity_weight * ants.recent_productivity[i]`. (Threading a param keeps `ColonyState` free of `Config`, matching the existing `harvest_weight`/`homing_weight` style.)
  - Update the two live call sites in `reproduce.rs` (paid births) to pass `cfg.productivity_weight`.
  - In `sweep_deaths` (`apply.rs` ~313), pass the ant's recent value as the 4th fitness arg:

```rust
        colony.record_death(
            ctx.cfg.fitness(
                ants.food_delivered[i],
                ants.food_harvested[i],
                ants.food_homing[i],
                ants.recent_productivity[i],
            ),
            // ...unchanged lineage/genome/cap args
        );
```

- [ ] **Step 4: Run — expect PASS.** `cargo test -p sim colony:: reproduce::`
- [ ] **Step 5: Commit.** `feat(sim): recent_productivity in living selection and the death archive`

---

## Task 5: Determinism — hash and golden

**Files:**
- Modify: `crates/sim/src/world.rs` (`state_hash`)
- Regenerate: `crates/sim/tests/golden_master.bin`

- [ ] **Step 1: Extend `state_hash`.** In the per-ant fold loop, after `food_homing`:

```rust
            eat(&self.ants.recent_productivity[i].to_bits().to_le_bytes());
```

- [ ] **Step 2: Run the determinism tests — expect PASS** (identical runs still match; the existing `state_hash_changes_when_the_world_ticks` still holds): `cargo test -p sim world::`
- [ ] **Step 3: Regenerate the golden fixture** (physics intentionally changed):

Run: `REGENERATE_GOLDEN=1 cargo test -p sim --test golden`
Then: `cargo test -p sim --test golden` (expect PASS).

- [ ] **Step 4: Confirm the known-good forager still passes without regeneration:**

Run: `cargo test -p sim --test known_good` (expect PASS — the genome fixture is untouched; only its dynamics shifted, and a real forager must still beat random).
If it fails narrowly, sweep `productivity_weight` down (e.g. 0.05) before regenerating anything — the NN fixture must not be rebuilt for a fitness-only change.

- [ ] **Step 5: Commit.** `feat(sim): fold recent_productivity into state_hash; regen golden`

---

## Task 6: Server wire — two config fields

**Files:**
- Modify: `crates/server/src/protocol.rs`

- [ ] **Step 1: Failing test.** Extend the config-table test to expect 23 fields and round-trip the two new ids:

```rust
#[test]
fn productivity_fields_round_trip() {
    let mut cfg = Config::default();
    assert!(apply_config_field(&mut cfg, 21, 0.3)); // productivity_weight
    assert!(apply_config_field(&mut cfg, 22, 0.95)); // productivity_decay
    assert_eq!(cfg.productivity_weight, 0.3);
    assert_eq!(cfg.productivity_decay, 0.95);
    // decay clamps into (0,1)
    apply_config_field(&mut cfg, 22, 5.0);
    assert!(cfg.productivity_decay < 1.0);
}
```

Also update `the_config_table_is_dense_and_stops_where_it_says` (and any test asserting `CONFIG_FIELDS.len()`) to 23.

- [ ] **Step 2: Run — expect FAIL.**
- [ ] **Step 3: Implement.**
  - `CONFIG_FIELDS`: append `"productivity_weight", "productivity_decay"` (len 23).
  - `field_mut`: `21 => &mut cfg.productivity_weight,` and `22 => &mut cfg.productivity_decay,`.
  - `apply_config_field` clamp: add `22` to the evaporation group and leave `21` in the `≥ 0` default:

```rust
        0..=2 | 19 | 22 => value.clamp(1e-4, 0.999_99),
```

- [ ] **Step 4: Run — expect PASS.** `cargo test -p server protocol::`
- [ ] **Step 5: Commit.** `feat(server): wire productivity_weight/decay config fields`

---

## Task 7: Headless — `--set` arms

**Files:**
- Modify: `crates/headless/src/main.rs`

- [ ] **Step 1: Failing test:**

```rust
#[test]
fn productivity_levers_parse() {
    let mut cfg = Config::default();
    apply_override(&mut cfg, "productivity_weight=0.2").unwrap();
    apply_override(&mut cfg, "productivity_decay=0.98").unwrap();
    assert_eq!(cfg.productivity_weight, 0.2);
    assert_eq!(cfg.productivity_decay, 0.98);
}
```

- [ ] **Step 2: Run — expect FAIL.**
- [ ] **Step 3: Implement.** In `apply_override`'s match:

```rust
        "productivity_weight" => cfg.productivity_weight = f(value)?,
        "productivity_decay" => cfg.productivity_decay = f(value)?,
```

- [ ] **Step 4: Run — expect PASS.** `cargo test -p headless`
- [ ] **Step 5: Commit.** `feat(headless): --set productivity_weight/decay`

---

## Task 8: Web — config fields + sliders

**Files:**
- Modify: `web/src/protocol.ts` (`CONFIG_FIELDS`)
- Modify: `web/src/ui/tunables.ts`
- Modify: `web/tests/state.test.ts` (config size 21 → 23), `web/tests/protocol.test.ts` if it asserts the count

**Consumes:** server ids 21–22.

- [ ] **Step 1: Failing test.** In `state.test.ts`, the dispatch test asserts `store.state.config.size` — bump 21 → 23.
- [ ] **Step 2: Run — expect FAIL.** `cd web && npx vitest run state`
- [ ] **Step 3: Implement.**
  - `web/src/protocol.ts` `CONFIG_FIELDS`: append `"productivity_weight", "productivity_decay"`.
  - `web/src/ui/tunables.ts` `TUNABLES`: append

```ts
  { id: 21, label: "productivity weight", min: 0, max: 1, scale: "linear", hint: "reward recent harvest/deliver/kills; 0 = off (cumulative only)" },
  { id: 22, label: "productivity decay", min: 0.9, max: 0.9999, scale: "decay", hint: "how fast 'recent' fades; 0.99 ≈ 69-tick half-life" },
```

- [ ] **Step 4: Run — expect PASS + tsc.** `cd web && npx tsc --noEmit && npx vitest run`
- [ ] **Step 5: Commit.** `feat(web): productivity_weight/decay tuning sliders`

---

## Task 9: Inspector honesty — recent_productivity in the ant detail frame

**Files:**
- Modify: `crates/server/src/protocol.rs` (`AntDetail`, `encode_ant_detail`, `ANT_DETAIL_LEN`)
- Modify: `crates/server/tests/fixtures.rs` (+ regenerate `expected.json`/`detail.bin`)
- Modify: `crates/server/src/protocol.rs` tests (offset assertions), `crates/server/src/sim_thread.rs` if it builds an `AntDetail`
- Modify: `web/src/protocol.ts` (`ANT_DETAIL_LEN`, detail decode), `web/src/ui/inspector.ts` (fitness readout)
- Modify: `web/tests/protocol.test.ts` (detail length / fields)

**Rationale:** the inspector shows an ant's fitness; without `recent_productivity` on the wire it would silently omit the new term. This task is separable — the sim change works without it — so land it after Task 8.

- [ ] **Step 1:** `ANT_DETAIL_LEN` 453 → 457. Append `recent_productivity: f32` to `AntDetail` and `encode_ant_detail` **after** `food_harvested` (so every existing offset is unchanged; new field at `ANT_DETAIL_LEN - 4`). Update the `debug_assert_eq!(out.len(), ANT_DETAIL_LEN)`.
- [ ] **Step 2:** Update the offset-hardcoded protocol tests (`food_harvested` moves to 449→453 was the last change; the new field lands at 453; adjust `an_ant_detail_frame_is_exactly_the_documented_length` and `the_detail_frames_activations_match_a_forward_pass` if they index past the activations). Populate `recent_productivity` where `AntDetail` is built (`sim_thread.rs` / wherever the selected ant's detail is assembled) from `world.ants.recent_productivity[idx]`.
- [ ] **Step 3:** `fixtures.rs`: set a distinct `w.ants.recent_productivity[0]` in `fixture_world`, add it to `AntDetail`, the `expected.json` template + args, and the non-zero guard list. Run `cargo test -p server --test fixtures` to regenerate.
- [ ] **Step 4:** Web: `ANT_DETAIL_LEN` 453 → 457; decode `recentProductivity` at `ANT_DETAIL_LEN - 4`; in `inspector.ts`, add `productivity_weight * recentProductivity` to the displayed fitness (read `productivity_weight` from `store.state.config`, id 21) and show the raw term.
- [ ] **Step 5:** `cargo test -p server && (cd web && npx tsc --noEmit && npx vitest run)`. Commit: `feat: surface recent_productivity in the ant inspector`.

---

## Task 10: Verify, measure, merge

- [ ] **Step 1:** Full suite green: `cargo test --workspace` and `cd web && npx vitest run && npx tsc --noEmit`.
- [ ] **Step 2:** Wire-format guard: `cargo test -p server --test fixtures` then `cd web && npm test` (the cross-language check).
- [ ] **Step 3: Payoff measurement (A/B).** Two headless runs, same seed, differing only in the lever:

```bash
cargo run -q -p headless --release -- --ticks 100000 --seed 1 --every 100000 --set productivity_weight=0
cargo run -q -p headless --release -- --ticks 100000 --seed 1 --every 100000 --set productivity_weight=0.1
```

Compare world `delivered_total` and colonies-alive. Record the result in the commit body. If 0.1 clearly regresses delivery, sweep the weight (0.05, 0.2) before merging and note the chosen default.

- [ ] **Step 4:** Use superpowers:finishing-a-development-branch to merge to `main`.

## Self-review notes (author)

- Spec coverage: EMA mechanism (T2–T3), harvest+deliver+kill feeds (T3), delivered-anchored fitness (T1, T4), living + archive application (T4), determinism/golden (T5), NN fixture untouched (T5 step 4), wire + sliders (T6–T8), inspector honesty (T9), payoff test (T10). ✓
- Type consistency: `fitness` is 4-arg everywhere after T1; `select_parent` gains `productivity_weight` and all call sites (reproduce.rs + tests) update in T4. ✓
- The one behavior risk (kill reward encouraging aggression) is measured in T10, not just assumed.
