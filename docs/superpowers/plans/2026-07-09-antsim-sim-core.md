# antsim2 Simulation Core — Implementation Plan (Plan 1 of 2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `sim` crate — a pure, deterministic ant-colony neuroevolution simulation — plus a headless CLI that runs it and prints per-colony stats, so we can verify evolution works before any rendering exists.

**Architecture:** A `World` owns a `Grid`, three `Pheromones` layers, a struct-of-arrays `Ants` store, and per-colony state. Each `tick()` runs three phases: ants sense and think **in parallel** against a read-only world, emitting `Intent`s; intents are applied **serially in ant-id order**; then pheromone fields update in parallel. Because phase 1 never writes and phase 2 is ordered, the simulation is bit-for-bit deterministic regardless of thread count. No I/O lives in `sim`.

**Tech Stack:** Rust (stable, installed via rustup), `rayon` for data parallelism, `serde` + `bincode` for snapshots, a hand-rolled PCG32 RNG (no `rand` dependency, so determinism cannot break under a dependency bump), `clap` for the headless CLI.

**Spec:** `docs/superpowers/specs/2026-07-09-antsim-design.md`

**Out of scope (Plan 2):** the `server` crate, the WebSocket protocol, the `web` app, WebGL rendering, the neural-net view, live tuning UI.

## Global Constraints

- **Grid is 512 × 512.** Width and height are `u16` in `Config`, defaulting to 512.
- **Colonies default to 8.** Gene pools never mix — a parent is only ever sampled from the same colony.
- **Network topology is fixed:** 44 inputs → 16 → 16 → 8 outputs, `tanh` activations. 1088 weights + 40 biases = **1128 f32 per genome**. These are `const` and must not drift.
- **4 of the 8 outputs are recurrent memory**, fed back as inputs 40..44 on the next tick.
- **Pheromone deposition is passive.** No network output controls it. Ants leak food-trail (∝ carried food), alarm (on damage/attack), and colony scent (always). Sensing them is evolved.
- **There is no queen.** A colony is a nest, a food store, and a gene pool.
- **Fitness is food delivered home.** No other fitness term exists anywhere in the codebase.
- **Determinism is a tested property.** Same seed + same config ⇒ identical state hash, regardless of `RAYON_NUM_THREADS`. Every RNG draw comes from a per-ant `Pcg32` seeded from `(ant_id, birth_tick)`. **No `HashMap` iteration, no `f32` sum-order dependence across threads, no `SystemTime`, no `rand` crate.**
- **`sim` performs zero I/O.** No `println!`, no file access, no sockets, no threads it does not own. Snapshots are `serde` in/out of byte buffers; the *caller* writes files.
- Rust edition 2021. `#![forbid(unsafe_code)]` in `sim/src/lib.rs`.
- Every task ends with a green `cargo test -p sim` and a commit.
- **The "Expected: PASS, N tests" counts are approximate.** They are a sanity check, not an assertion. The signal is that the *named* tests pass; do not chase an off-by-one in the total.
- **Three tests guard the project's foundations, and a failure in any of them means stop, not tune-and-continue:** `thread_count_does_not_change_the_outcome` (determinism), `the_nest_gradient_is_discriminable_at_foraging_range` (homing is possible at all), and `a_scripted_forager_grows_the_colony_food_store` (the economy is winnable). Everything downstream assumes these three.

## File Structure

```
antsim2/
├── Cargo.toml                  # workspace
├── crates/
│   ├── sim/
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs          # public API, re-exports, consts
│   │   │   ├── rng.rs          # Pcg32: deterministic, serde-able
│   │   │   ├── config.rs       # Config: every tunable constant
│   │   │   ├── grid.rs         # terrain, food, nest tiles, indexing
│   │   │   ├── pheromone.rs    # 3 layers: deposit, evaporate, diffuse
│   │   │   ├── genome.rs       # weights + Traits, mutation, hand-wired forager
│   │   │   ├── brain.rs        # Brain trait + Mlp forward pass
│   │   │   ├── ants.rs         # struct-of-arrays ant store
│   │   │   ├── spatial.rs      # CSR spatial index + occupancy
│   │   │   ├── sense.rs        # build the 44-input vector
│   │   │   ├── intent.rs       # Intent struct
│   │   │   ├── apply.rs        # serial: movement, grab, combat, death
│   │   │   ├── colony.rs       # nest, store, births, hall of fame
│   │   │   ├── worldgen.rs     # seeded map generation
│   │   │   ├── stats.rs        # per-colony stats
│   │   │   ├── snapshot.rs     # save/load bytes
│   │   │   └── world.rs        # World::tick() orchestration
│   │   └── tests/
│   │       ├── determinism.rs
│   │       ├── snapshot.rs
│   │       ├── golden.rs
│   │       ├── golden_master.bin   # checked-in fixture
│   │       └── behavior.rs
│   └── headless/
│       ├── Cargo.toml
│       └── src/main.rs         # CLI: run N ticks, print CSV stats
```

Each `sim` module owns one responsibility and is small enough to hold in context. `apply.rs` is the only place that mutates the world during a tick; `sense.rs` is the only place that reads it during the think phase. That split is what makes determinism auditable — if a write appears in `sense.rs`, the design is violated.

---

### Task 1: Toolchain and workspace scaffold

Rust is **not installed** on this machine. Homebrew 6.0.5 is present. This task ends with `cargo test` running green on an empty crate.

**Files:**
- Create: `rust-toolchain.toml`
- Create: `Cargo.toml` (workspace root)
- Create: `crates/sim/Cargo.toml`
- Create: `crates/sim/src/lib.rs`
- Create: `.gitignore`

**Interfaces:**
- Consumes: nothing.
- Produces: a `sim` library crate that compiles; `cargo test -p sim` works.

- [ ] **Step 1: Install Rust via rustup**

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
source "$HOME/.cargo/env"
rustc --version
```

Expected: prints `rustc 1.8x.x (...)`. If `curl` is blocked, `brew install rustup-init && rustup-init -y` is the fallback.

Note: `--no-modify-path` avoids editing the user's shell profile. Add `source "$HOME/.cargo/env"` to your session, or use absolute `~/.cargo/bin/cargo`.

- [ ] **Step 2: Pin the toolchain**

Create `rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

- [ ] **Step 3: Create the workspace root**

Create `Cargo.toml`:

`headless` is deliberately absent from `members` — it does not exist until Task 21, and Cargo fails to load a workspace whose member manifest is missing.

```toml
[workspace]
resolver = "2"
members = ["crates/sim"]

[workspace.package]
edition = "2021"
license = "MIT"

[workspace.dependencies]
rayon = "1.10"
serde = { version = "1", features = ["derive"] }
bincode = "1.3"
clap = { version = "4", features = ["derive"] }
```

`bincode` 1.3 is pinned deliberately: 2.x changed its encoding, which would silently invalidate the golden-master fixture.

- [ ] **Step 4: Create the sim crate manifest**

Create `crates/sim/Cargo.toml`:

```toml
[package]
name = "sim"
version = "0.1.0"
edition.workspace = true

[dependencies]
rayon.workspace = true
serde.workspace = true
bincode.workspace = true
```

- [ ] **Step 5: Create the crate root with a failing placeholder test**

Create `crates/sim/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

/// Number of sensory inputs fed to every ant's network.
pub const N_INPUTS: usize = 44;
/// First hidden layer width.
pub const N_HIDDEN1: usize = 16;
/// Second hidden layer width.
pub const N_HIDDEN2: usize = 16;
/// Network outputs: turn, throttle, attack, grab, + 4 recurrent memory values.
pub const N_OUTPUTS: usize = 8;
/// Recurrent memory values carried between ticks.
pub const N_MEMORY: usize = 4;

/// Total f32 parameters in one brain: weights + biases.
pub const N_PARAMS: usize = N_INPUTS * N_HIDDEN1
    + N_HIDDEN1
    + N_HIDDEN1 * N_HIDDEN2
    + N_HIDDEN2
    + N_HIDDEN2 * N_OUTPUTS
    + N_OUTPUTS;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_count_matches_spec() {
        assert_eq!(N_PARAMS, 1128);
    }
}
```

- [ ] **Step 6: Create .gitignore**

```
/target
```

- [ ] **Step 7: Run the test**

Run: `cargo test -p sim`
Expected: PASS, `test tests::param_count_matches_spec ... ok`.

This test exists to freeze the topology. If someone changes a layer width, this fails and forces them to update the spec.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml rust-toolchain.toml .gitignore crates/sim
git commit -m "chore: scaffold cargo workspace and sim crate"
```

---

### Task 2: Deterministic RNG (`Pcg32`)

We hand-roll PCG32 rather than depend on `rand`. Determinism is a *tested guarantee* here, and a `rand` minor-version bump can silently change stream output, which would invalidate the golden master with no compile error. Thirty lines of our own code removes that entire failure class.

**Files:**
- Create: `crates/sim/src/rng.rs`
- Modify: `crates/sim/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub struct Pcg32` — `Clone`, `Debug`, `Serialize`, `Deserialize`
  - `Pcg32::new(seed: u64, seq: u64) -> Pcg32`
  - `Pcg32::next_u32(&mut self) -> u32`
  - `Pcg32::next_f32(&mut self) -> f32` — uniform in `[0.0, 1.0)`
  - `Pcg32::next_gaussian(&mut self) -> f32` — mean 0, stddev 1
  - `Pcg32::next_below(&mut self, n: u32) -> u32` — uniform in `[0, n)`

- [ ] **Step 1: Write the failing tests**

Create `crates/sim/src/rng.rs` with only the tests at the bottom (no impl yet):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream() {
        let mut a = Pcg32::new(42, 1);
        let mut b = Pcg32::new(42, 1);
        for _ in 0..1000 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn different_streams_diverge() {
        let mut a = Pcg32::new(42, 1);
        let mut b = Pcg32::new(42, 2);
        let diff = (0..100).filter(|_| a.next_u32() != b.next_u32()).count();
        assert!(diff > 90, "streams should differ, only {diff}/100 differed");
    }

    #[test]
    fn f32_is_in_unit_interval() {
        let mut r = Pcg32::new(7, 7);
        for _ in 0..10_000 {
            let v = r.next_f32();
            assert!((0.0..1.0).contains(&v), "out of range: {v}");
        }
    }

    #[test]
    fn next_below_respects_bound() {
        let mut r = Pcg32::new(9, 9);
        for _ in 0..10_000 {
            assert!(r.next_below(7) < 7);
        }
    }

    #[test]
    fn gaussian_has_roughly_unit_variance() {
        let mut r = Pcg32::new(3, 3);
        let n = 100_000;
        let xs: Vec<f32> = (0..n).map(|_| r.next_gaussian()).collect();
        let mean = xs.iter().sum::<f32>() / n as f32;
        let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / n as f32;
        assert!(mean.abs() < 0.02, "mean {mean}");
        assert!((var - 1.0).abs() < 0.05, "var {var}");
    }

    #[test]
    fn roundtrips_through_serde() {
        let mut a = Pcg32::new(11, 13);
        a.next_u32();
        let bytes = bincode::serialize(&a).unwrap();
        let mut b: Pcg32 = bincode::deserialize(&bytes).unwrap();
        assert_eq!(a.next_u32(), b.next_u32());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

First add `pub mod rng;` to `crates/sim/src/lib.rs` (below the consts).

Run: `cargo test -p sim`
Expected: FAIL — `cannot find type Pcg32 in this scope`.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/sim/src/rng.rs`:

```rust
use serde::{Deserialize, Serialize};

/// PCG-XSH-RR 64/32. Hand-rolled so that determinism cannot be broken by a
/// dependency bump: the golden-master fixture depends on this exact stream.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pcg32 {
    state: u64,
    inc: u64,
}

const MULT: u64 = 6_364_136_223_846_793_005;

impl Pcg32 {
    /// `seq` selects one of 2^63 distinct streams for the same `seed`.
    pub fn new(seed: u64, seq: u64) -> Self {
        let mut r = Pcg32 { state: 0, inc: (seq << 1) | 1 };
        r.next_u32();
        r.state = r.state.wrapping_add(seed);
        r.next_u32();
        r
    }

    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(MULT).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Uniform in `[0.0, 1.0)`. Uses the top 24 bits, which is exactly the
    /// f32 mantissa width, so every representable value is equally likely.
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Uniform in `[0, n)`, rejection-sampled so it is unbiased.
    /// Panics if `n == 0`.
    pub fn next_below(&mut self, n: u32) -> u32 {
        assert!(n > 0, "next_below requires n > 0");
        let threshold = n.wrapping_neg() % n;
        loop {
            let v = self.next_u32();
            if v >= threshold {
                return v % n;
            }
        }
    }

    /// Box-Muller. Discards the second variate rather than caching it, which
    /// keeps the struct's serialized state trivially reproducible.
    pub fn next_gaussian(&mut self) -> f32 {
        let mut u1 = self.next_f32();
        if u1 <= f32::EPSILON {
            u1 = f32::EPSILON;
        }
        let u2 = self.next_f32();
        (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p sim`
Expected: PASS, 7 tests (6 rng + the param count).

- [ ] **Step 5: Commit**

```bash
git add crates/sim/src/rng.rs crates/sim/src/lib.rs
git commit -m "feat(sim): deterministic PCG32 rng with serde support"
```

---

### Task 3: `Config` — every tunable constant in one place

Nothing in `sim` may read a hardcoded magic number for a simulation rule. All of it lives here, because Plan 2's UI mutates these live, and because the "nothing evolves" failure mode is diagnosed by sweeping them.

**Files:**
- Create: `crates/sim/src/config.rs`
- Modify: `crates/sim/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `pub struct Config` (`Clone`, `Debug`, `Serialize`, `Deserialize`, `PartialEq`) with `Default`, and `Config::cell_count(&self) -> usize`.

- [ ] **Step 1: Write the failing test**

Create `crates/sim/src/config.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_grid_is_512_squared() {
        let c = Config::default();
        assert_eq!(c.width, 512);
        assert_eq!(c.height, 512);
        assert_eq!(c.cell_count(), 262_144);
    }

    #[test]
    fn default_has_eight_colonies() {
        assert_eq!(Config::default().num_colonies, 8);
    }

    #[test]
    fn evaporation_rates_are_decay_multipliers() {
        let c = Config::default();
        for r in [c.food_evaporation, c.alarm_evaporation, c.scent_evaporation] {
            assert!(r > 0.0 && r < 1.0, "evaporation must be in (0,1), got {r}");
        }
    }

    #[test]
    fn roundtrips_through_serde() {
        let c = Config::default();
        let bytes = bincode::serialize(&c).unwrap();
        assert_eq!(c, bincode::deserialize::<Config>(&bytes).unwrap());
    }

    /// Mean upkeep per tick at mean random traits, size 1.0. Mirrors
    /// `Genome::upkeep` without depending on it, so a change to either side
    /// of the economy trips this test rather than passing silently.
    fn mean_upkeep(c: &Config) -> f32 {
        c.base_upkeep + c.tax_speed * 0.525 + c.tax_strength * 0.5 + c.tax_armor * 0.5
            + c.tax_vision * 4.5
    }

    #[test]
    fn a_mean_forager_turns_a_profit_on_a_round_trip() {
        // The single most important invariant in the whole config: if a trip
        // costs more than it yields, no amount of evolution can save the
        // colony, and every downstream test is testing a corpse.
        let c = Config::default();
        let travel = 2.0 * 12.0; // to SEED_PATCH_DISTANCE and back
        let ticks = travel / 0.525 + 10.5 / c.harvest_rate;
        let cost = mean_upkeep(&c) * ticks + c.move_cost * travel;
        let yield_ = 10.5; // mean carry_capacity
        assert!(yield_ > 2.0 * cost, "trip yields {yield_} but costs {cost}");
    }

    #[test]
    fn starvation_bites_well_before_old_age() {
        // If an unfed ant outlives its minimum lifespan, starvation stops
        // selecting for anything.
        let c = Config::default();
        let ticks_to_starve = c.max_energy_per_size / mean_upkeep(&c);
        assert!(ticks_to_starve < 2000.0, "unfed ant survives {ticks_to_starve} ticks");
        assert!(ticks_to_starve > 200.0, "ants starve too fast to ever reach food");
    }

    #[test]
    fn the_initial_store_is_a_fuel_reserve_not_a_birth_windfall() {
        let c = Config::default();
        let instant_births = c.initial_food_store / c.birth_cost;
        assert!(instant_births < 25.0, "{instant_births} free births at t=0 is a population spike");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Add `pub mod config;` to `lib.rs`.

Run: `cargo test -p sim`
Expected: FAIL — `cannot find type Config`.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/sim/src/config.rs`:

```rust
use serde::{Deserialize, Serialize};

/// Every tunable simulation rule. No magic numbers live outside this struct.
///
/// The defaults are a *starting guess*, not a tuned equilibrium. Expect to
/// sweep evaporation/diffusion and the trait taxes before anything evolves.
///
/// # The break-even calculation
///
/// The defaults below are chosen so a competent forager turns a profit. If it
/// cannot, no amount of evolution helps — the game is unwinnable and the store
/// only ever drains. Task 20's scripted-forager test guards this. The
/// arithmetic, at mean random traits (speed 0.525, strength 0.5, armor 0.5,
/// vision 4.5, carry 10.5) and size 1.0:
///
/// - upkeep/tick = 0.010 + 0.020(0.525) + 0.010(0.5) + 0.010(0.5) + 0.005(4.5)
///                = 0.053
/// - a round trip to the guaranteed patch at `SEED_PATCH_DISTANCE` = 12 cells
///   is 24 cells of travel at 0.525 cells/tick = 46 ticks, plus 10.5 food at
///   `harvest_rate` 2.0 = 5 ticks. Call it 51 ticks.
/// - trip cost = 0.053 x 51 + 0.005 x 24 = ~2.8 energy
/// - trip yield = 10.5 food, and refuelling is 1:1, so the margin is ~3.7x.
///
/// Two ratios matter, and they pull against each other:
/// - **yield / trip cost** must be comfortably > 1, or the colony starves.
/// - **`max_energy_per_size` / upkeep** is how long an unfed ant lives:
///   30 / 0.053 = ~566 ticks. Push it much past ~2000 (the minimum lifespan)
///   and starvation stops selecting for anything, because every ant dies of
///   old age with a full tank.
///
/// Worked through with the values below: upkeep 0.0530/tick (vision is still
/// 42% of it), trip 51 ticks, cost 2.82, yield 10.5 — a **3.7x margin**. An
/// unfed founder lives 566 ticks; a newborn at 60% of a size-0.5 tank lives
/// 340. Both comfortably under the 2000-tick minimum lifespan.
///
/// Earlier defaults set `tax_vision` at 0.02, which alone was over half of all
/// upkeep and made every trip net-negative (cost ~19 against a yield of 10.5).
/// If you retune, redo this sum.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    // --- World ---
    pub width: u16,
    pub height: u16,
    pub num_colonies: u8,
    pub initial_ants_per_colony: u32,
    /// Target fraction of the map covered by stone. Blob *count* is derived
    /// from this and the map area, so terrain density is scale-invariant —
    /// a 64x64 test world and the 512x512 real one look alike.
    pub stone_density: f32,
    pub stone_blob_radius: f32,

    // --- Colony economy ---
    pub initial_food_store: f32,
    /// Food store spent to spawn one ant.
    pub birth_cost: f32,
    pub max_births_per_tick: u32,
    /// Below this population, the nest spawns free ants from the hall of fame.
    pub extinction_floor: u32,
    /// Minimum ticks between two free floor spawns for the same colony. Without
    /// this the floor tops a colony back up *in the tick its ants die*, which
    /// hands a besieging colony an infinite conveyor of free corpses to
    /// scavenge — energy created from nothing at a fixed, findable location.
    pub floor_respawn_interval: u64,
    pub hall_of_fame_size: usize,
    /// Energy per tick an ant regains while standing on its own nest.
    pub refuel_rate: f32,

    // --- Pheromones (per-tick decay multipliers, in (0,1)) ---
    pub food_evaporation: f32,
    pub alarm_evaporation: f32,
    pub scent_evaporation: f32,
    /// Fraction of the neighbour-average blended in per tick, per layer.
    pub food_diffusion: f32,
    pub alarm_diffusion: f32,
    pub scent_diffusion: f32,
    /// Food-trail deposited per unit of carried food, per tick.
    pub food_trail_emission: f32,
    /// Alarm deposited when an ant attacks or is damaged.
    pub alarm_emission: f32,
    /// Colony scent deposited by every ant, every tick.
    pub ant_scent_emission: f32,
    /// Colony scent deposited by each nest tile, every tick. Much larger than
    /// `ant_scent_emission`: this is the beacon ants climb to get home.
    pub nest_scent_emission: f32,
    /// Divisor for the logarithmic pheromone sensor compression (see
    /// `sense::squash_phero`). Pheromone magnitudes span four orders of
    /// magnitude between a stale trail and a nest tile; a linear or tanh
    /// squash saturates near the nest and erases the very gradient an ant
    /// needs to find its way home.
    pub phero_log_div: f32,

    // --- Metabolism and the trait tax ---
    pub base_upkeep: f32,
    pub tax_speed: f32,
    pub tax_strength: f32,
    pub tax_armor: f32,
    pub tax_vision: f32,
    /// Energy per cell of distance moved.
    pub move_cost: f32,

    // --- Combat ---
    pub attack_cost: f32,
    pub attack_damage: f32,
    /// Fraction of a victim's remaining energy the killer absorbs.
    pub kill_energy_frac: f32,

    // --- Growth ---
    pub max_energy_per_size: f32,
    /// Fraction of max energy above which an ant converts energy into size.
    pub growth_threshold: f32,
    pub growth_rate: f32,
    pub shrink_rate: f32,

    // --- Mutation ---
    /// Fraction of parameters perturbed per birth.
    pub mutation_rate: f32,
    pub mutation_sigma: f32,
    pub big_jump_chance: f32,
    pub big_jump_sigma: f32,

    // --- Food ---
    pub food_patch_count: u32,
    pub food_patch_radius: f32,
    pub food_patch_max: f32,
    pub food_regrow: f32,
    /// Food harvested per tick by an ant standing on a food cell.
    pub harvest_rate: f32,
}

impl Config {
    pub fn cell_count(&self) -> usize {
        self.width as usize * self.height as usize
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            width: 512,
            height: 512,
            num_colonies: 8,
            initial_ants_per_colony: 40,
            stone_density: 0.06,
            stone_blob_radius: 7.0,

            // Enough to refuel through a bad stretch, but NOT a birth windfall:
            // at birth_cost 40 this buys ~15 births, not 100. A huge initial
            // store just converts to a population spike that then starves.
            initial_food_store: 600.0,
            birth_cost: 40.0,
            max_births_per_tick: 2,
            extinction_floor: 5,
            floor_respawn_interval: 200,
            hall_of_fame_size: 10,
            refuel_rate: 2.0,

            food_evaporation: 0.995,
            alarm_evaporation: 0.97,
            scent_evaporation: 0.999,
            food_diffusion: 0.12,
            alarm_diffusion: 0.20,
            scent_diffusion: 0.06,
            food_trail_emission: 2.0,
            alarm_emission: 5.0,
            ant_scent_emission: 0.5,
            nest_scent_emission: 50.0,
            phero_log_div: 12.0,

            // See the break-even note above before touching these. `tax_vision`
            // is multiplied by a trait ranging to 8.0, so it is worth ~8x its
            // face value relative to the 0..1 traits.
            base_upkeep: 0.010,
            tax_speed: 0.020,
            tax_strength: 0.010,
            tax_armor: 0.010,
            tax_vision: 0.005,
            move_cost: 0.005,

            attack_cost: 0.5,
            attack_damage: 4.0,
            kill_energy_frac: 0.3,

            max_energy_per_size: 30.0,
            growth_threshold: 0.8,
            growth_rate: 0.002,
            shrink_rate: 0.004,

            mutation_rate: 0.08,
            mutation_sigma: 0.05,
            big_jump_chance: 0.002,
            big_jump_sigma: 0.5,

            food_patch_count: 40,
            food_patch_radius: 6.0,
            food_patch_max: 200.0,
            food_regrow: 0.002,
            harvest_rate: 2.0,
        }
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p sim`
Expected: PASS. `a_mean_forager_turns_a_profit_on_a_round_trip` is the one to watch — it is a cheap arithmetic stand-in for Task 20's expensive simulated forager test, and it fails first if you retune the taxes carelessly.

- [ ] **Step 5: Commit**

```bash
git add crates/sim/src/config.rs crates/sim/src/lib.rs
git commit -m "feat(sim): Config with every tunable simulation constant"
```

---

### Task 4: `Grid` — terrain, food, nest tiles

**Files:**
- Create: `crates/sim/src/grid.rs`
- Modify: `crates/sim/src/lib.rs`

**Interfaces:**
- Consumes: `Config`.
- Produces:
  - `pub const NO_NEST: u8 = 255;`
  - `pub struct Grid { pub width: u16, pub height: u16, pub stone: Vec<bool>, pub food: Vec<f32>, pub nest: Vec<u8> }` (`Clone`, `Serialize`, `Deserialize`)
  - `Grid::new(cfg: &Config) -> Grid` — all dirt, no food, no nests
  - `Grid::idx(&self, x: u16, y: u16) -> usize`
  - `Grid::in_bounds(&self, x: i32, y: i32) -> bool`
  - `Grid::idx_clamped(&self, x: i32, y: i32) -> usize` — clamps to the border
  - `Grid::is_stone(&self, x: i32, y: i32) -> bool` — out of bounds counts as stone
  - `Grid::harvest(&mut self, i: usize, amount: f32) -> f32` — removes and returns up to `amount`

- [ ] **Step 1: Write the failing tests**

Create `crates/sim/src/grid.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn small() -> Config {
        Config { width: 8, height: 4, ..Config::default() }
    }

    #[test]
    fn idx_is_row_major() {
        let g = Grid::new(&small());
        assert_eq!(g.idx(0, 0), 0);
        assert_eq!(g.idx(7, 0), 7);
        assert_eq!(g.idx(0, 1), 8);
        assert_eq!(g.idx(7, 3), 31);
    }

    #[test]
    fn new_grid_is_empty_dirt() {
        let g = Grid::new(&small());
        assert_eq!(g.stone.len(), 32);
        assert!(g.stone.iter().all(|s| !s));
        assert!(g.food.iter().all(|f| *f == 0.0));
        assert!(g.nest.iter().all(|n| *n == NO_NEST));
    }

    #[test]
    fn out_of_bounds_counts_as_stone() {
        let g = Grid::new(&small());
        assert!(g.is_stone(-1, 0));
        assert!(g.is_stone(0, -1));
        assert!(g.is_stone(8, 0));
        assert!(g.is_stone(0, 4));
        assert!(!g.is_stone(0, 0));
    }

    #[test]
    fn idx_clamped_pins_to_border() {
        let g = Grid::new(&small());
        assert_eq!(g.idx_clamped(-5, -5), g.idx(0, 0));
        assert_eq!(g.idx_clamped(100, 100), g.idx(7, 3));
    }

    #[test]
    fn harvest_takes_at_most_what_is_there() {
        let mut g = Grid::new(&small());
        let i = g.idx(2, 2);
        g.food[i] = 3.0;
        assert_eq!(g.harvest(i, 10.0), 3.0);
        assert_eq!(g.food[i], 0.0);
        g.food[i] = 10.0;
        assert_eq!(g.harvest(i, 4.0), 4.0);
        assert_eq!(g.food[i], 6.0);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Add `pub mod grid;` to `lib.rs`.

Run: `cargo test -p sim`
Expected: FAIL — `cannot find type Grid`.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/sim/src/grid.rs`:

```rust
use crate::config::Config;
use serde::{Deserialize, Serialize};

/// Sentinel in `Grid::nest` meaning "this cell is not a nest tile".
pub const NO_NEST: u8 = 255;

#[derive(Clone, Serialize, Deserialize)]
pub struct Grid {
    pub width: u16,
    pub height: u16,
    pub stone: Vec<bool>,
    pub food: Vec<f32>,
    /// Colony id owning this nest tile, or `NO_NEST`.
    pub nest: Vec<u8>,
}

impl Grid {
    pub fn new(cfg: &Config) -> Self {
        let n = cfg.cell_count();
        Grid {
            width: cfg.width,
            height: cfg.height,
            stone: vec![false; n],
            food: vec![0.0; n],
            nest: vec![NO_NEST; n],
        }
    }

    #[inline]
    pub fn idx(&self, x: u16, y: u16) -> usize {
        y as usize * self.width as usize + x as usize
    }

    #[inline]
    pub fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.width as i32 && y < self.height as i32
    }

    /// Used by sensing and diffusion, where reading past the border should
    /// return the border cell rather than panic or wrap.
    #[inline]
    pub fn idx_clamped(&self, x: i32, y: i32) -> usize {
        let cx = x.clamp(0, self.width as i32 - 1) as usize;
        let cy = y.clamp(0, self.height as i32 - 1) as usize;
        cy * self.width as usize + cx
    }

    /// Off-grid is stone, so ants are walled in without a special case at
    /// every movement site.
    #[inline]
    pub fn is_stone(&self, x: i32, y: i32) -> bool {
        if !self.in_bounds(x, y) {
            return true;
        }
        self.stone[self.idx_clamped(x, y)]
    }

    pub fn harvest(&mut self, i: usize, amount: f32) -> f32 {
        let taken = amount.min(self.food[i]);
        self.food[i] -= taken;
        taken
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p sim`
Expected: PASS, 16 tests.

- [ ] **Step 5: Commit**

```bash
git add crates/sim/src/grid.rs crates/sim/src/lib.rs
git commit -m "feat(sim): Grid with terrain, food, and nest tiles"
```

---

### Task 5: `Pheromones` — three layers, deposit, evaporate, diffuse

The colony-scent layer stores **one strength and one owner per cell**, not one layer per colony. Depositing onto foreign-scented ground erodes the incumbent's mark and, once it hits zero, flips ownership. That makes territory a contested field for 1/8th the diffusion cost.

**Files:**
- Create: `crates/sim/src/pheromone.rs`
- Modify: `crates/sim/src/lib.rs`

**Interfaces:**
- Consumes: `Config`, `NO_NEST`-style sentinel `NO_OWNER`.
- Produces:
  - `pub const NO_OWNER: u8 = 255;`
  - `pub struct Pheromones { pub width: u16, pub height: u16, pub food: Vec<f32>, pub alarm: Vec<f32>, pub scent: Vec<f32>, pub owner: Vec<u8> }` (`Clone`, `Serialize`, `Deserialize`)
  - `Pheromones::new(cfg: &Config) -> Pheromones`
  - `Pheromones::deposit_food(&mut self, i: usize, amount: f32)`
  - `Pheromones::deposit_alarm(&mut self, i: usize, amount: f32)`
  - `Pheromones::deposit_scent(&mut self, i: usize, amount: f32, colony: u8)`
  - `Pheromones::scent_for(&self, i: usize, colony: u8) -> (f32, f32)` — returns `(own, foreign)`
  - `Pheromones::step(&mut self, cfg: &Config)` — evaporate + diffuse all three layers

- [ ] **Step 1: Write the failing tests**

Create `crates/sim/src/pheromone.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn small() -> Config {
        Config { width: 8, height: 8, ..Config::default() }
    }

    #[test]
    fn deposit_then_read_own_and_foreign() {
        let mut p = Pheromones::new(&small());
        p.deposit_scent(10, 4.0, 3);
        assert_eq!(p.scent_for(10, 3), (4.0, 0.0));
        assert_eq!(p.scent_for(10, 5), (0.0, 4.0));
    }

    #[test]
    fn same_colony_scent_accumulates() {
        let mut p = Pheromones::new(&small());
        p.deposit_scent(10, 2.0, 1);
        p.deposit_scent(10, 3.0, 1);
        assert_eq!(p.scent[10], 5.0);
        assert_eq!(p.owner[10], 1);
    }

    #[test]
    fn foreign_scent_erodes_the_incumbent() {
        let mut p = Pheromones::new(&small());
        p.deposit_scent(10, 5.0, 1);
        p.deposit_scent(10, 2.0, 2);
        assert_eq!(p.owner[10], 1, "incumbent holds while strength remains");
        assert_eq!(p.scent[10], 3.0);
    }

    #[test]
    fn overwhelming_foreign_scent_flips_ownership() {
        let mut p = Pheromones::new(&small());
        p.deposit_scent(10, 2.0, 1);
        p.deposit_scent(10, 5.0, 2);
        assert_eq!(p.owner[10], 2);
        assert_eq!(p.scent[10], 3.0);
    }

    #[test]
    fn unowned_cell_takes_the_depositor_as_owner() {
        let mut p = Pheromones::new(&small());
        assert_eq!(p.owner[10], NO_OWNER);
        p.deposit_scent(10, 1.0, 6);
        assert_eq!(p.owner[10], 6);
    }

    #[test]
    fn evaporation_decays_an_isolated_deposit() {
        let cfg = small();
        let mut p = Pheromones::new(&cfg);
        p.deposit_food(0, 100.0);
        let before = p.food[0];
        p.step(&cfg);
        assert!(p.food[0] < before, "food should decay");
    }

    #[test]
    fn diffusion_spreads_to_neighbours() {
        let cfg = small();
        let mut p = Pheromones::new(&cfg);
        let center = 8 * 4 + 4;
        p.deposit_food(center, 100.0);
        p.step(&cfg);
        assert!(p.food[center - 1] > 0.0, "should spread left");
        assert!(p.food[center + 1] > 0.0, "should spread right");
        assert!(p.food[center - 8] > 0.0, "should spread up");
        assert!(p.food[center + 8] > 0.0, "should spread down");
    }

    #[test]
    fn diffusion_does_not_leak_off_the_border() {
        let cfg = small();
        let mut p = Pheromones::new(&cfg);
        // Fill uniformly, disable evaporation: total must be conserved.
        let cfg = Config { food_evaporation: 1.0, ..cfg };
        for i in 0..cfg.cell_count() {
            p.food[i] = 1.0;
        }
        let before: f32 = p.food.iter().sum();
        p.step(&cfg);
        let after: f32 = p.food.iter().sum();
        assert!((before - after).abs() < 1e-3, "{before} vs {after}");
    }

    #[test]
    fn a_trail_fades_to_nothing_eventually() {
        let cfg = small();
        let mut p = Pheromones::new(&cfg);
        p.deposit_food(30, 1000.0);
        for _ in 0..5000 {
            p.step(&cfg);
        }
        let total: f32 = p.food.iter().sum();
        assert!(total < 1.0, "stale trail should evaporate, total={total}");
    }
}
```

That last test is the one that matters most: it proves a trail to an exhausted food patch cannot mislead the colony forever.

- [ ] **Step 2: Run to verify it fails**

Add `pub mod pheromone;` to `lib.rs`.

Run: `cargo test -p sim`
Expected: FAIL — `cannot find type Pheromones`.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/sim/src/pheromone.rs`:

```rust
use crate::config::Config;
use serde::{Deserialize, Serialize};

/// Sentinel in `Pheromones::owner` meaning "no colony has marked this cell".
pub const NO_OWNER: u8 = 255;

#[derive(Clone, Serialize, Deserialize)]
pub struct Pheromones {
    pub width: u16,
    pub height: u16,
    pub food: Vec<f32>,
    pub alarm: Vec<f32>,
    /// Strength of the *owning* colony's mark. Never negative.
    pub scent: Vec<f32>,
    pub owner: Vec<u8>,
}

impl Pheromones {
    pub fn new(cfg: &Config) -> Self {
        let n = cfg.cell_count();
        Pheromones {
            width: cfg.width,
            height: cfg.height,
            food: vec![0.0; n],
            alarm: vec![0.0; n],
            scent: vec![0.0; n],
            owner: vec![NO_OWNER; n],
        }
    }

    #[inline]
    pub fn deposit_food(&mut self, i: usize, amount: f32) {
        self.food[i] += amount;
    }

    #[inline]
    pub fn deposit_alarm(&mut self, i: usize, amount: f32) {
        self.alarm[i] += amount;
    }

    /// Same colony reinforces. A different colony erodes, and takes ownership
    /// if it erodes the incumbent past zero. This is why territory is a
    /// contested field rather than eight independent maps.
    pub fn deposit_scent(&mut self, i: usize, amount: f32, colony: u8) {
        if self.owner[i] == colony {
            self.scent[i] += amount;
        } else if self.owner[i] == NO_OWNER || self.scent[i] <= amount {
            self.scent[i] = amount - self.scent[i];
            self.owner[i] = colony;
        } else {
            self.scent[i] -= amount;
        }
    }

    /// `(own_scent, foreign_scent)` as seen by `colony`. Exactly one is nonzero.
    #[inline]
    pub fn scent_for(&self, i: usize, colony: u8) -> (f32, f32) {
        if self.owner[i] == colony {
            (self.scent[i], 0.0)
        } else if self.owner[i] == NO_OWNER {
            (0.0, 0.0)
        } else {
            (0.0, self.scent[i])
        }
    }

    /// Evaporate then diffuse every layer. Diffusion is a 4-point blend toward
    /// the neighbour average; out-of-bounds neighbours read as the cell itself,
    /// so nothing leaks off the border.
    ///
    /// The scent layer diffuses only its magnitude; ownership is not blended,
    /// because a cell has exactly one owner by construction.
    pub fn step(&mut self, cfg: &Config) {
        diffuse_decay(&mut self.food, self.width, self.height, cfg.food_diffusion, cfg.food_evaporation);
        diffuse_decay(&mut self.alarm, self.width, self.height, cfg.alarm_diffusion, cfg.alarm_evaporation);
        diffuse_decay(&mut self.scent, self.width, self.height, cfg.scent_diffusion, cfg.scent_evaporation);
        for i in 0..self.scent.len() {
            if self.scent[i] < 1e-6 {
                self.scent[i] = 0.0;
                self.owner[i] = NO_OWNER;
            }
        }
    }
}

fn diffuse_decay(layer: &mut [f32], w: u16, h: u16, diffusion: f32, evaporation: f32) {
    let w = w as usize;
    let h = h as usize;
    let src = layer.to_vec();
    let at = |x: usize, y: usize| src[y * w + x];
    for y in 0..h {
        for x in 0..w {
            let v = at(x, y);
            let l = if x > 0 { at(x - 1, y) } else { v };
            let r = if x + 1 < w { at(x + 1, y) } else { v };
            let u = if y > 0 { at(x, y - 1) } else { v };
            let d = if y + 1 < h { at(x, y + 1) } else { v };
            let avg = 0.25 * (l + r + u + d);
            layer[y * w + x] = (v + diffusion * (avg - v)) * evaporation;
        }
    }
}
```

Note the `src = layer.to_vec()` clone: diffusion must read the *previous* state everywhere, or the result depends on iteration order. That allocation is a known cost; Task 13 hoists it into a reusable scratch buffer once the profile says it matters.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p sim`
Expected: PASS, 25 tests.

- [ ] **Step 5: Commit**

```bash
git add crates/sim/src/pheromone.rs crates/sim/src/lib.rs
git commit -m "feat(sim): three pheromone layers with contested colony scent"
```

---

### Task 6: `Genome` and `Traits` — heritable brain + taxed body

The trait tax is why evolution has anything to discover. If speed were free every lineage would max it. Traits are clamped to fixed ranges after mutation so a runaway value cannot produce a `NaN` ant.

**Files:**
- Create: `crates/sim/src/genome.rs`
- Modify: `crates/sim/src/lib.rs`

**Interfaces:**
- Consumes: `Config`, `Pcg32`, `N_PARAMS`.
- Produces:
  - `pub struct Traits { pub max_speed: f32, pub strength: f32, pub armor: f32, pub vision: f32, pub carry_capacity: f32, pub max_size: f32, pub metabolic_efficiency: f32, pub lifespan: f32 }` (`Clone`, `Debug`, `Serialize`, `Deserialize`)
  - `Traits::clamp(&mut self)`
  - `Traits::as_array(&self) -> [f32; 8]` / `Traits::from_array([f32; 8]) -> Traits`
  - `pub struct Genome { pub params: Vec<f32>, pub traits: Traits }` (`Clone`, `Serialize`, `Deserialize`)
  - `Genome::random(rng: &mut Pcg32) -> Genome`
  - `Genome::mutated(&self, cfg: &Config, rng: &mut Pcg32) -> Genome`
  - `Genome::upkeep(&self, cfg: &Config, size: f32) -> f32`
  - `Genome::max_energy(&self, cfg: &Config, size: f32) -> f32`

- [ ] **Step 1: Write the failing tests**

Create `crates/sim/src/genome.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::rng::Pcg32;

    #[test]
    fn random_genome_has_the_fixed_param_count() {
        let mut r = Pcg32::new(1, 1);
        assert_eq!(Genome::random(&mut r).params.len(), crate::N_PARAMS);
    }

    #[test]
    fn traits_roundtrip_through_array() {
        let mut r = Pcg32::new(2, 2);
        let t = Genome::random(&mut r).traits;
        let back = Traits::from_array(t.as_array());
        assert_eq!(back.as_array(), t.as_array());
    }

    #[test]
    fn clamp_pins_traits_into_legal_ranges() {
        let mut t = Traits {
            max_speed: 99.0,
            strength: -5.0,
            armor: 99.0,
            vision: 0.0,
            carry_capacity: -1.0,
            max_size: 99.0,
            metabolic_efficiency: 0.0,
            lifespan: 1.0,
        };
        t.clamp();
        assert_eq!(t.max_speed, TRAIT_RANGES[0].1);
        assert_eq!(t.strength, TRAIT_RANGES[1].0);
        assert_eq!(t.armor, TRAIT_RANGES[2].1);
        assert_eq!(t.vision, TRAIT_RANGES[3].0);
        assert_eq!(t.lifespan, TRAIT_RANGES[7].0);
    }

    #[test]
    fn mutation_changes_some_params_but_not_all() {
        let cfg = Config::default();
        let mut r = Pcg32::new(3, 3);
        let parent = Genome::random(&mut r);
        let child = parent.mutated(&cfg, &mut r);
        let changed = parent
            .params
            .iter()
            .zip(&child.params)
            .filter(|(a, b)| a != b)
            .count();
        assert!(changed > 0, "mutation changed nothing");
        assert!(
            changed < parent.params.len(),
            "mutation changed everything; mutation_rate should be partial"
        );
    }

    #[test]
    fn mutation_is_deterministic_for_a_given_rng_state() {
        let cfg = Config::default();
        let parent = Genome::random(&mut Pcg32::new(4, 4));
        let a = parent.mutated(&cfg, &mut Pcg32::new(5, 5));
        let b = parent.mutated(&cfg, &mut Pcg32::new(5, 5));
        assert_eq!(a.params, b.params);
        assert_eq!(a.traits.as_array(), b.traits.as_array());
    }

    #[test]
    fn mutated_traits_stay_in_range() {
        let cfg = Config { mutation_sigma: 10.0, big_jump_chance: 1.0, ..Config::default() };
        let mut r = Pcg32::new(6, 6);
        let mut g = Genome::random(&mut r);
        for _ in 0..200 {
            g = g.mutated(&cfg, &mut r);
        }
        for (i, v) in g.traits.as_array().iter().enumerate() {
            let (lo, hi) = TRAIT_RANGES[i];
            assert!(*v >= lo && *v <= hi, "trait {i} = {v} escaped [{lo},{hi}]");
            assert!(v.is_finite());
        }
    }

    #[test]
    fn a_fast_armored_ant_costs_more_than_a_plain_one() {
        let cfg = Config::default();
        let mut r = Pcg32::new(7, 7);
        let mut cheap = Genome::random(&mut r);
        cheap.traits = Traits::from_array([0.1, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 5000.0]);
        let mut pricey = cheap.clone();
        pricey.traits = Traits::from_array([1.0, 1.0, 1.0, 8.0, 1.0, 1.0, 1.0, 5000.0]);
        assert!(pricey.upkeep(&cfg, 1.0) > cheap.upkeep(&cfg, 1.0));
    }

    #[test]
    fn upkeep_scales_with_size() {
        let cfg = Config::default();
        let g = Genome::random(&mut Pcg32::new(8, 8));
        assert!(g.upkeep(&cfg, 2.0) > g.upkeep(&cfg, 1.0));
    }

    #[test]
    fn better_metabolic_efficiency_lowers_upkeep() {
        let cfg = Config::default();
        let mut r = Pcg32::new(9, 9);
        let mut a = Genome::random(&mut r);
        a.traits = Traits::from_array([0.5, 0.5, 0.5, 4.0, 5.0, 1.0, 0.6, 5000.0]);
        let mut b = a.clone();
        b.traits.metabolic_efficiency = 1.4;
        assert!(b.upkeep(&cfg, 1.0) < a.upkeep(&cfg, 1.0));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Add `pub mod genome;` to `lib.rs`.

Run: `cargo test -p sim`
Expected: FAIL — `cannot find type Genome`.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/sim/src/genome.rs`:

```rust
use crate::config::Config;
use crate::rng::Pcg32;
use crate::N_PARAMS;
use serde::{Deserialize, Serialize};

/// Legal `(min, max)` for each trait, in `Traits::as_array` order. Mutation is
/// clamped to these, so no lineage can evolve a NaN or an infinite lifespan.
///
/// **`max_speed`'s upper bound of 1.0 is load-bearing.** `apply_movement` only
/// collision-checks the destination cell, not the cells swept through on the
/// way. At up to one cell per tick an ant cannot skip over a wall. Raise this
/// bound and ants will tunnel through stone; you would need a swept collision
/// check first.
pub const TRAIT_RANGES: [(f32, f32); 8] = [
    (0.05, 1.00),      // max_speed, cells per tick — see note above
    (0.00, 1.00),      // strength
    (0.00, 1.00),      // armor, fraction of damage negated
    (1.00, 8.00),      // vision, whisker sample distance in cells
    (1.00, 20.00),     // carry_capacity, food units
    (0.50, 3.00),      // max_size
    (0.50, 1.50),      // metabolic_efficiency, divides upkeep
    (2000.0, 20000.0), // lifespan, ticks
];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Traits {
    pub max_speed: f32,
    pub strength: f32,
    pub armor: f32,
    pub vision: f32,
    pub carry_capacity: f32,
    pub max_size: f32,
    pub metabolic_efficiency: f32,
    pub lifespan: f32,
}

impl Traits {
    pub fn as_array(&self) -> [f32; 8] {
        [
            self.max_speed,
            self.strength,
            self.armor,
            self.vision,
            self.carry_capacity,
            self.max_size,
            self.metabolic_efficiency,
            self.lifespan,
        ]
    }

    pub fn from_array(a: [f32; 8]) -> Self {
        Traits {
            max_speed: a[0],
            strength: a[1],
            armor: a[2],
            vision: a[3],
            carry_capacity: a[4],
            max_size: a[5],
            metabolic_efficiency: a[6],
            lifespan: a[7],
        }
    }

    pub fn clamp(&mut self) {
        let mut a = self.as_array();
        for (i, v) in a.iter_mut().enumerate() {
            if !v.is_finite() {
                *v = TRAIT_RANGES[i].0;
            }
            *v = v.clamp(TRAIT_RANGES[i].0, TRAIT_RANGES[i].1);
        }
        *self = Traits::from_array(a);
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Genome {
    /// Flattened weights then biases, layer by layer. Length is `N_PARAMS`.
    pub params: Vec<f32>,
    pub traits: Traits,
}

impl Genome {
    pub fn random(rng: &mut Pcg32) -> Self {
        // Small initial weights keep tanh in its near-linear regime, so a
        // newborn ant drifts rather than saturating hard left or hard right.
        let params = (0..N_PARAMS).map(|_| rng.next_gaussian() * 0.3).collect();
        let traits_arr = std::array::from_fn(|i| {
            let (lo, hi) = TRAIT_RANGES[i];
            lo + rng.next_f32() * (hi - lo)
        });
        Genome { params, traits: Traits::from_array(traits_arr) }
    }

    pub fn mutated(&self, cfg: &Config, rng: &mut Pcg32) -> Self {
        let mut params = self.params.clone();
        for p in params.iter_mut() {
            if rng.next_f32() < cfg.mutation_rate {
                let sigma = if rng.next_f32() < cfg.big_jump_chance {
                    cfg.big_jump_sigma
                } else {
                    cfg.mutation_sigma
                };
                *p += rng.next_gaussian() * sigma;
            }
        }

        let mut arr = self.traits.as_array();
        for (i, v) in arr.iter_mut().enumerate() {
            if rng.next_f32() < cfg.mutation_rate {
                let (lo, hi) = TRAIT_RANGES[i];
                let span = hi - lo;
                let sigma = if rng.next_f32() < cfg.big_jump_chance {
                    cfg.big_jump_sigma
                } else {
                    cfg.mutation_sigma
                };
                // Trait sigma is a fraction of the trait's own range, so
                // lifespan (thousands) and armor (0..1) mutate comparably.
                *v += rng.next_gaussian() * sigma * span;
            }
        }
        let mut traits = Traits::from_array(arr);
        traits.clamp();

        Genome { params, traits }
    }

    /// Standing metabolic cost per tick. Every trait is taxed whether or not
    /// it is used; this is the pressure that makes specialisation a real bet.
    pub fn upkeep(&self, cfg: &Config, size: f32) -> f32 {
        let t = &self.traits;
        let traits_cost = cfg.tax_speed * t.max_speed
            + cfg.tax_strength * t.strength
            + cfg.tax_armor * t.armor
            + cfg.tax_vision * t.vision;
        (cfg.base_upkeep + traits_cost) * size / t.metabolic_efficiency
    }

    pub fn max_energy(&self, cfg: &Config, size: f32) -> f32 {
        cfg.max_energy_per_size * size
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p sim`
Expected: PASS, 34 tests.

- [ ] **Step 5: Commit**

```bash
git add crates/sim/src/genome.rs crates/sim/src/lib.rs
git commit -m "feat(sim): Genome, clamped Traits, and the metabolic trait tax"
```

---

### Task 7: `Brain` — the recurrent MLP forward pass

**Files:**
- Create: `crates/sim/src/brain.rs`
- Modify: `crates/sim/src/lib.rs`

**Interfaces:**
- Consumes: `Genome`, the layer-size consts.
- Produces:
  - `pub struct Activations { pub inputs: [f32; N_INPUTS], pub h1: [f32; N_HIDDEN1], pub h2: [f32; N_HIDDEN2], pub outputs: [f32; N_OUTPUTS] }` (`Clone`, `Debug`)
  - `pub trait Brain { fn forward(&self, inputs: &[f32; N_INPUTS]) -> Activations; }`
  - `impl Brain for Genome`
  - Output index consts: `pub const OUT_TURN: usize = 0; OUT_THROTTLE: usize = 1; OUT_ATTACK: usize = 2; OUT_GRAB: usize = 3; OUT_MEMORY: usize = 4;`

`Activations` carries every layer, not just outputs, because Plan 2's inspector renders them. Making the forward pass return them costs nothing here and avoids a second code path later.

The `Brain` trait exists so a future NEAT implementation can be substituted without touching the sim loop (spec §11).

- [ ] **Step 1: Write the failing tests**

Create `crates/sim/src/brain.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::genome::Genome;
    use crate::rng::Pcg32;
    use crate::{N_INPUTS, N_MEMORY, N_OUTPUTS};

    #[test]
    fn output_indices_leave_room_for_memory() {
        assert_eq!(OUT_MEMORY + N_MEMORY, N_OUTPUTS);
    }

    #[test]
    fn forward_is_pure() {
        let g = Genome::random(&mut Pcg32::new(1, 1));
        let inputs = [0.3f32; N_INPUTS];
        let a = g.forward(&inputs);
        let b = g.forward(&inputs);
        assert_eq!(a.outputs, b.outputs);
    }

    #[test]
    fn all_outputs_are_bounded_by_tanh() {
        let g = Genome::random(&mut Pcg32::new(2, 2));
        let inputs = [1e6f32; N_INPUTS];
        for o in g.forward(&inputs).outputs {
            assert!(o >= -1.0 && o <= 1.0, "output {o} escaped tanh range");
            assert!(o.is_finite());
        }
    }

    #[test]
    fn different_inputs_give_different_outputs() {
        let g = Genome::random(&mut Pcg32::new(3, 3));
        let a = g.forward(&[0.0; N_INPUTS]);
        let b = g.forward(&[1.0; N_INPUTS]);
        assert!(a.outputs != b.outputs);
    }

    #[test]
    fn a_zero_genome_outputs_zero() {
        let mut g = Genome::random(&mut Pcg32::new(4, 4));
        g.params.iter_mut().for_each(|p| *p = 0.0);
        for o in g.forward(&[0.7; N_INPUTS]).outputs {
            assert_eq!(o, 0.0);
        }
    }

    #[test]
    fn activations_expose_every_layer() {
        let g = Genome::random(&mut Pcg32::new(5, 5));
        let a = g.forward(&[0.2; N_INPUTS]);
        assert_eq!(a.inputs.len(), N_INPUTS);
        assert_eq!(a.h1.len(), crate::N_HIDDEN1);
        assert_eq!(a.h2.len(), crate::N_HIDDEN2);
        assert_eq!(a.outputs.len(), N_OUTPUTS);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Add `pub mod brain;` to `lib.rs`.

Run: `cargo test -p sim`
Expected: FAIL — `cannot find trait Brain`.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/sim/src/brain.rs`:

```rust
use crate::genome::Genome;
use crate::{N_HIDDEN1, N_HIDDEN2, N_INPUTS, N_OUTPUTS};

pub const OUT_TURN: usize = 0;
pub const OUT_THROTTLE: usize = 1;
pub const OUT_ATTACK: usize = 2;
pub const OUT_GRAB: usize = 3;
/// Outputs `[OUT_MEMORY .. N_OUTPUTS)` are recurrent state, fed back as the
/// final `N_MEMORY` inputs on the next tick.
pub const OUT_MEMORY: usize = 4;

/// Every layer's activation. The inspector in Plan 2 renders all of these, so
/// the forward pass returns them rather than only the outputs.
#[derive(Clone, Debug)]
pub struct Activations {
    pub inputs: [f32; N_INPUTS],
    pub h1: [f32; N_HIDDEN1],
    pub h2: [f32; N_HIDDEN2],
    pub outputs: [f32; N_OUTPUTS],
}

pub trait Brain {
    fn forward(&self, inputs: &[f32; N_INPUTS]) -> Activations;
}

/// Parameter layout, in order:
///   W1 [N_INPUTS  x N_HIDDEN1], B1 [N_HIDDEN1],
///   W2 [N_HIDDEN1 x N_HIDDEN2], B2 [N_HIDDEN2],
///   W3 [N_HIDDEN2 x N_OUTPUTS], B3 [N_OUTPUTS]
impl Brain for Genome {
    fn forward(&self, inputs: &[f32; N_INPUTS]) -> Activations {
        let p = &self.params;
        let (w1, rest) = p.split_at(N_INPUTS * N_HIDDEN1);
        let (b1, rest) = rest.split_at(N_HIDDEN1);
        let (w2, rest) = rest.split_at(N_HIDDEN1 * N_HIDDEN2);
        let (b2, rest) = rest.split_at(N_HIDDEN2);
        let (w3, b3) = rest.split_at(N_HIDDEN2 * N_OUTPUTS);

        let mut h1 = [0.0f32; N_HIDDEN1];
        for (j, hj) in h1.iter_mut().enumerate() {
            let mut acc = b1[j];
            for (i, x) in inputs.iter().enumerate() {
                acc += x * w1[i * N_HIDDEN1 + j];
            }
            *hj = acc.tanh();
        }

        let mut h2 = [0.0f32; N_HIDDEN2];
        for (j, hj) in h2.iter_mut().enumerate() {
            let mut acc = b2[j];
            for (i, x) in h1.iter().enumerate() {
                acc += x * w2[i * N_HIDDEN2 + j];
            }
            *hj = acc.tanh();
        }

        let mut outputs = [0.0f32; N_OUTPUTS];
        for (j, oj) in outputs.iter_mut().enumerate() {
            let mut acc = b3[j];
            for (i, x) in h2.iter().enumerate() {
                acc += x * w3[i * N_OUTPUTS + j];
            }
            *oj = acc.tanh();
        }

        Activations { inputs: *inputs, h1, h2, outputs }
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p sim`
Expected: PASS, 40 tests.

- [ ] **Step 5: Commit**

```bash
git add crates/sim/src/brain.rs crates/sim/src/lib.rs
git commit -m "feat(sim): Brain trait and recurrent MLP forward pass"
```

---

### Task 8: `Ants` — struct-of-arrays store

Struct-of-arrays, not `Vec<Ant>`, because the think phase streams over a few fields across every ant and `Vec<Ant>` would drag whole 4.5 KB genomes through cache to read an `f32` position.

**Invariant this task establishes and tests:** `ants.id` is strictly increasing, so iterating `0..len()` *is* iterating in ant-id order. Task 11's serial apply phase depends on that for its "lower id wins" conflict rule.

**Files:**
- Create: `crates/sim/src/ants.rs`
- Modify: `crates/sim/src/lib.rs`

**Interfaces:**
- Consumes: `Genome`, `Pcg32`, `N_MEMORY`.
- Produces:
  - `pub struct Spawn { pub id: u64, pub colony: u8, pub x: f32, pub y: f32, pub heading: f32, pub energy: f32, pub size: f32, pub lineage: u32, pub genome: Genome, pub birth_tick: u64 }`
  - `pub struct Ants { pub id: Vec<u64>, pub colony: Vec<u8>, pub x: Vec<f32>, pub y: Vec<f32>, pub heading: Vec<f32>, pub energy: Vec<f32>, pub size: Vec<f32>, pub age: Vec<u32>, pub carrying: Vec<f32>, pub lineage: Vec<u32>, pub food_delivered: Vec<f32>, pub memory: Vec<[f32; N_MEMORY]>, pub genome: Vec<Genome>, pub rng: Vec<Pcg32>, pub alive: Vec<bool> }` (`Clone`, `Serialize`, `Deserialize`)
  - `Ants::new() -> Ants`, `Ants::len(&self) -> usize`, `Ants::is_empty(&self) -> bool`
  - `Ants::push(&mut self, s: Spawn)`
  - `Ants::cell(&self, i: usize) -> (u16, u16)` — floored position, clamped to the grid
  - `Ants::retain_alive(&mut self)` — order-preserving compaction
  - `Ants::population(&self, colony: u8) -> u32`

- [ ] **Step 1: Write the failing tests**

Create `crates/sim/src/ants.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::genome::Genome;
    use crate::rng::Pcg32;

    fn spawn(id: u64, colony: u8, x: f32, y: f32) -> Spawn {
        Spawn {
            id,
            colony,
            x,
            y,
            heading: 0.0,
            energy: 50.0,
            size: 1.0,
            lineage: 0,
            genome: Genome::random(&mut Pcg32::new(id, 1)),
            birth_tick: 0,
        }
    }

    #[test]
    fn push_appends_and_len_tracks() {
        let mut a = Ants::new();
        assert!(a.is_empty());
        a.push(spawn(0, 0, 1.0, 1.0));
        a.push(spawn(1, 0, 2.0, 2.0));
        assert_eq!(a.len(), 2);
        assert_eq!(a.id, vec![0, 1]);
    }

    #[test]
    fn every_parallel_vec_has_the_same_length() {
        let mut a = Ants::new();
        for i in 0..5 {
            a.push(spawn(i, 0, 0.0, 0.0));
        }
        let n = a.len();
        assert_eq!(a.colony.len(), n);
        assert_eq!(a.x.len(), n);
        assert_eq!(a.y.len(), n);
        assert_eq!(a.heading.len(), n);
        assert_eq!(a.energy.len(), n);
        assert_eq!(a.size.len(), n);
        assert_eq!(a.age.len(), n);
        assert_eq!(a.carrying.len(), n);
        assert_eq!(a.lineage.len(), n);
        assert_eq!(a.food_delivered.len(), n);
        assert_eq!(a.memory.len(), n);
        assert_eq!(a.genome.len(), n);
        assert_eq!(a.rng.len(), n);
        assert_eq!(a.alive.len(), n);
    }

    #[test]
    fn cell_floors_the_position() {
        let mut a = Ants::new();
        a.push(spawn(0, 0, 3.9, 7.1));
        assert_eq!(a.cell(0), (3, 7));
    }

    #[test]
    fn retain_alive_preserves_id_order() {
        let mut a = Ants::new();
        for i in 0..6 {
            a.push(spawn(i, 0, i as f32, 0.0));
        }
        a.alive[1] = false;
        a.alive[4] = false;
        a.retain_alive();
        assert_eq!(a.id, vec![0, 2, 3, 5]);
        assert_eq!(a.x, vec![0.0, 2.0, 3.0, 5.0]);
        assert!(a.id.windows(2).all(|w| w[0] < w[1]), "ids must stay sorted");
    }

    #[test]
    fn retain_alive_keeps_vecs_in_lockstep() {
        let mut a = Ants::new();
        for i in 0..4 {
            a.push(spawn(i, (i % 2) as u8, 0.0, 0.0));
        }
        a.alive[0] = false;
        a.retain_alive();
        assert_eq!(a.len(), 3);
        assert_eq!(a.colony.len(), 3);
        assert_eq!(a.genome.len(), 3);
    }

    #[test]
    fn population_counts_only_the_named_colony() {
        let mut a = Ants::new();
        a.push(spawn(0, 1, 0.0, 0.0));
        a.push(spawn(1, 1, 0.0, 0.0));
        a.push(spawn(2, 2, 0.0, 0.0));
        assert_eq!(a.population(1), 2);
        assert_eq!(a.population(2), 1);
        assert_eq!(a.population(3), 0);
    }

    #[test]
    fn newborn_memory_starts_at_zero() {
        let mut a = Ants::new();
        a.push(spawn(0, 0, 0.0, 0.0));
        assert_eq!(a.memory[0], [0.0; crate::N_MEMORY]);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Add `pub mod ants;` to `lib.rs`.

Run: `cargo test -p sim`
Expected: FAIL — `cannot find type Ants`.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/sim/src/ants.rs`:

```rust
use crate::genome::Genome;
use crate::rng::Pcg32;
use crate::N_MEMORY;
use serde::{Deserialize, Serialize};

pub struct Spawn {
    pub id: u64,
    pub colony: u8,
    pub x: f32,
    pub y: f32,
    pub heading: f32,
    pub energy: f32,
    pub size: f32,
    pub lineage: u32,
    pub genome: Genome,
    pub birth_tick: u64,
}

/// Struct-of-arrays. The think phase streams position/energy/size across all
/// ants; a `Vec<Ant>` would pull 4.5 KB genomes through cache to read an f32.
///
/// Invariant: `id` is strictly increasing. Iterating `0..len()` therefore
/// iterates in ant-id order, which is what makes the serial apply phase's
/// "lowest id wins" rule well defined.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Ants {
    pub id: Vec<u64>,
    pub colony: Vec<u8>,
    pub x: Vec<f32>,
    pub y: Vec<f32>,
    pub heading: Vec<f32>,
    pub energy: Vec<f32>,
    pub size: Vec<f32>,
    pub age: Vec<u32>,
    pub carrying: Vec<f32>,
    pub lineage: Vec<u32>,
    /// Lifetime food delivered to the nest. This is the *only* fitness signal.
    pub food_delivered: Vec<f32>,
    pub memory: Vec<[f32; N_MEMORY]>,
    pub genome: Vec<Genome>,
    pub rng: Vec<Pcg32>,
    pub alive: Vec<bool>,
}

impl Ants {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.id.is_empty()
    }

    pub fn push(&mut self, s: Spawn) {
        debug_assert!(
            self.id.last().map_or(true, |&last| s.id > last),
            "ant ids must be pushed in increasing order"
        );
        // Reserved, and currently unread. Every ant carries a private stream
        // seeded from (id, birth_tick) so that any future *stochastic* ant
        // behaviour — noisy sensors, probabilistic actions — stays independent
        // of thread scheduling. Today the think phase is fully deterministic
        // and all randomness lives in the serial reproduce phase, drawing from
        // `World::rng`. Keep this field: adding it later would change every
        // ant's stream and invalidate the golden master.
        self.rng.push(Pcg32::new(s.id, s.birth_tick.wrapping_add(1)));
        self.id.push(s.id);
        self.colony.push(s.colony);
        self.x.push(s.x);
        self.y.push(s.y);
        self.heading.push(s.heading);
        self.energy.push(s.energy);
        self.size.push(s.size);
        self.age.push(0);
        self.carrying.push(0.0);
        self.lineage.push(s.lineage);
        self.food_delivered.push(0.0);
        self.memory.push([0.0; N_MEMORY]);
        self.genome.push(s.genome);
        self.alive.push(true);
    }

    /// Floored cell.
    ///
    /// The upper bound is guaranteed by `apply_movement`, not re-checked here:
    /// a move that would leave the grid is rejected (`Grid::is_stone` reports
    /// out-of-bounds as stone), and a move that stays within the current cell
    /// cannot cross `width`. `World`'s `every_ant_stays_on_the_map` test pins
    /// that invariant. The `debug_assert` catches a non-finite position, which
    /// would otherwise cast to 0 and silently teleport the ant to the corner.
    #[inline]
    pub fn cell(&self, i: usize) -> (u16, u16) {
        debug_assert!(
            self.x[i].is_finite() && self.y[i].is_finite(),
            "ant {i} has a non-finite position: ({}, {})",
            self.x[i],
            self.y[i]
        );
        (self.x[i].max(0.0) as u16, self.y[i].max(0.0) as u16)
    }

    pub fn population(&self, colony: u8) -> u32 {
        self.colony
            .iter()
            .zip(&self.alive)
            .filter(|(c, a)| **c == colony && **a)
            .count() as u32
    }

    /// Order-preserving compaction. Order preservation is load-bearing: it is
    /// what keeps `id` sorted across ticks.
    pub fn retain_alive(&mut self) {
        let keep = self.alive.clone();
        let mut k = 0usize;
        retain(&mut self.id, &keep);
        retain(&mut self.colony, &keep);
        retain(&mut self.x, &keep);
        retain(&mut self.y, &keep);
        retain(&mut self.heading, &keep);
        retain(&mut self.energy, &keep);
        retain(&mut self.size, &keep);
        retain(&mut self.age, &keep);
        retain(&mut self.carrying, &keep);
        retain(&mut self.lineage, &keep);
        retain(&mut self.food_delivered, &keep);
        retain(&mut self.memory, &keep);
        retain(&mut self.genome, &keep);
        retain(&mut self.rng, &keep);
        self.alive.retain(|_| {
            let v = keep[k];
            k += 1;
            v
        });
    }
}

fn retain<T>(v: &mut Vec<T>, keep: &[bool]) {
    let mut i = 0usize;
    v.retain(|_| {
        let k = keep[i];
        i += 1;
        k
    });
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p sim`
Expected: PASS, 47 tests.

- [ ] **Step 5: Commit**

```bash
git add crates/sim/src/ants.rs crates/sim/src/lib.rs
git commit -m "feat(sim): struct-of-arrays Ants store with id-ordered compaction"
```

---

### Task 9: `Spatial` — CSR neighbour index and cell occupancy

Rebuilt once per tick by counting sort — `O(n)`, allocation-free after warmup, and deterministic because ants are scattered in index order.

**Files:**
- Create: `crates/sim/src/spatial.rs`
- Modify: `crates/sim/src/lib.rs`

**Interfaces:**
- Consumes: `Config`, `Ants`, `Grid` (for `NO_NEST`).
- Produces:
  - `pub const NO_OCCUPANT: u32 = u32::MAX;`
  - `pub struct Spatial { width: u16, height: u16, cell_start: Vec<u32>, items: Vec<u32>, occupant: Vec<u32> }` (`Clone`)
  - `Spatial::new(cfg: &Config) -> Spatial`
  - `Spatial::rebuild(&mut self, ants: &Ants)`
  - `Spatial::cell_ants(&self, i: usize) -> &[u32]`
  - `Spatial::occupant(&self, i: usize) -> Option<u32>`
  - `Spatial::set_occupant(&mut self, i: usize, ant: u32)` / `Spatial::clear_occupant(&mut self, i: usize)`
  - `Spatial::counts_in_radius(&self, ants: &Ants, cx: i32, cy: i32, r: i32, colony: u8) -> (u32, u32)` — `(friends, foes)`, excluding no one
  - `Spatial::first_adjacent_foe(&self, ants: &Ants, cx: i32, cy: i32, colony: u8) -> Option<u32>` — 8-neighbourhood plus own cell, lowest ant index

- [ ] **Step 1: Write the failing tests**

Create `crates/sim/src/spatial.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ants::{Ants, Spawn};
    use crate::config::Config;
    use crate::genome::Genome;
    use crate::rng::Pcg32;

    fn cfg() -> Config {
        Config { width: 8, height: 8, ..Config::default() }
    }

    fn ants_at(positions: &[(f32, f32, u8)]) -> Ants {
        let mut a = Ants::new();
        for (i, (x, y, c)) in positions.iter().enumerate() {
            a.push(Spawn {
                id: i as u64,
                colony: *c,
                x: *x,
                y: *y,
                heading: 0.0,
                energy: 10.0,
                size: 1.0,
                lineage: 0,
                genome: Genome::random(&mut Pcg32::new(i as u64, 1)),
                birth_tick: 0,
            });
        }
        a
    }

    #[test]
    fn cell_ants_lists_occupants_of_that_cell() {
        let c = cfg();
        let ants = ants_at(&[(2.5, 3.5, 0), (2.1, 3.9, 1), (5.0, 5.0, 0)]);
        let mut s = Spatial::new(&c);
        s.rebuild(&ants);
        let cell = 3 * 8 + 2;
        assert_eq!(s.cell_ants(cell), &[0, 1]);
        assert_eq!(s.cell_ants(5 * 8 + 5), &[2]);
        assert!(s.cell_ants(0).is_empty());
    }

    #[test]
    fn cell_ants_are_sorted_by_ant_index() {
        let c = cfg();
        let ants = ants_at(&[(1.0, 1.0, 0), (1.5, 1.5, 0), (1.2, 1.2, 0)]);
        let mut s = Spatial::new(&c);
        s.rebuild(&ants);
        assert_eq!(s.cell_ants(1 * 8 + 1), &[0, 1, 2]);
    }

    #[test]
    fn occupant_defaults_to_the_lowest_index_in_the_cell() {
        let c = cfg();
        let ants = ants_at(&[(4.0, 4.0, 0), (4.5, 4.5, 0)]);
        let mut s = Spatial::new(&c);
        s.rebuild(&ants);
        assert_eq!(s.occupant(4 * 8 + 4), Some(0));
    }

    #[test]
    fn occupant_is_none_for_an_empty_cell() {
        let c = cfg();
        let mut s = Spatial::new(&c);
        s.rebuild(&ants_at(&[]));
        assert_eq!(s.occupant(0), None);
    }

    #[test]
    fn set_and_clear_occupant() {
        let c = cfg();
        let mut s = Spatial::new(&c);
        s.rebuild(&ants_at(&[]));
        s.set_occupant(9, 3);
        assert_eq!(s.occupant(9), Some(3));
        s.clear_occupant(9);
        assert_eq!(s.occupant(9), None);
    }

    #[test]
    fn counts_in_radius_splits_friend_from_foe() {
        let c = cfg();
        let ants = ants_at(&[(4.0, 4.0, 1), (5.0, 4.0, 1), (3.0, 4.0, 2), (0.0, 0.0, 2)]);
        let mut s = Spatial::new(&c);
        s.rebuild(&ants);
        let (friends, foes) = s.counts_in_radius(&ants, 4, 4, 1, 1);
        assert_eq!(friends, 2, "self plus the neighbour at (5,4)");
        assert_eq!(foes, 1, "the ant at (3,4); the one at (0,0) is out of range");
    }

    #[test]
    fn counts_in_radius_clips_at_the_border() {
        let c = cfg();
        let ants = ants_at(&[(0.0, 0.0, 1)]);
        let mut s = Spatial::new(&c);
        s.rebuild(&ants);
        let (friends, foes) = s.counts_in_radius(&ants, 0, 0, 2, 1);
        assert_eq!((friends, foes), (1, 0));
    }

    #[test]
    fn first_adjacent_foe_ignores_nestmates() {
        let c = cfg();
        let ants = ants_at(&[(4.0, 4.0, 1), (5.0, 4.0, 1)]);
        let mut s = Spatial::new(&c);
        s.rebuild(&ants);
        assert_eq!(s.first_adjacent_foe(&ants, 4, 4, 1), None);
    }

    #[test]
    fn first_adjacent_foe_picks_the_lowest_index() {
        let c = cfg();
        // Two foes adjacent; ant index 2 sits at (5,4), index 1 at (3,4).
        let ants = ants_at(&[(4.0, 4.0, 1), (3.0, 4.0, 2), (5.0, 4.0, 2)]);
        let mut s = Spatial::new(&c);
        s.rebuild(&ants);
        assert_eq!(s.first_adjacent_foe(&ants, 4, 4, 1), Some(1));
    }

    #[test]
    fn first_adjacent_foe_skips_the_dead() {
        let c = cfg();
        let mut ants = ants_at(&[(4.0, 4.0, 1), (3.0, 4.0, 2)]);
        ants.alive[1] = false;
        let mut s = Spatial::new(&c);
        s.rebuild(&ants);
        assert_eq!(s.first_adjacent_foe(&ants, 4, 4, 1), None);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Add `pub mod spatial;` to `lib.rs`.

Run: `cargo test -p sim`
Expected: FAIL — `cannot find type Spatial`.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/sim/src/spatial.rs`:

```rust
use crate::ants::Ants;
use crate::config::Config;

pub const NO_OCCUPANT: u32 = u32::MAX;

/// Compressed-sparse-row index from cell to the ants standing in it, rebuilt
/// each tick by counting sort. Ants are scattered in index order, so
/// `cell_ants` is always sorted ascending — which is what makes the serial
/// apply phase's tie-breaks deterministic.
#[derive(Clone)]
pub struct Spatial {
    width: u16,
    height: u16,
    cell_start: Vec<u32>,
    items: Vec<u32>,
    occupant: Vec<u32>,
}

impl Spatial {
    pub fn new(cfg: &Config) -> Self {
        Spatial {
            width: cfg.width,
            height: cfg.height,
            cell_start: vec![0; cfg.cell_count() + 1],
            items: Vec::new(),
            occupant: vec![NO_OCCUPANT; cfg.cell_count()],
        }
    }

    #[inline]
    fn idx(&self, x: i32, y: i32) -> usize {
        y as usize * self.width as usize + x as usize
    }

    pub fn rebuild(&mut self, ants: &Ants) {
        let cells = self.cell_start.len() - 1;
        self.cell_start.iter_mut().for_each(|c| *c = 0);
        self.occupant.iter_mut().for_each(|o| *o = NO_OCCUPANT);

        let mut counts = vec![0u32; cells];
        for i in 0..ants.len() {
            if !ants.alive[i] {
                continue;
            }
            let (x, y) = ants.cell(i);
            counts[self.idx(x as i32, y as i32)] += 1;
        }

        let mut acc = 0u32;
        for c in 0..cells {
            self.cell_start[c] = acc;
            acc += counts[c];
        }
        self.cell_start[cells] = acc;

        self.items.clear();
        self.items.resize(acc as usize, 0);
        let mut cursor: Vec<u32> = self.cell_start[..cells].to_vec();
        for i in 0..ants.len() {
            if !ants.alive[i] {
                continue;
            }
            let (x, y) = ants.cell(i);
            let c = self.idx(x as i32, y as i32);
            self.items[cursor[c] as usize] = i as u32;
            cursor[c] += 1;
            if self.occupant[c] == NO_OCCUPANT {
                self.occupant[c] = i as u32;
            }
        }
    }

    pub fn cell_ants(&self, i: usize) -> &[u32] {
        let s = self.cell_start[i] as usize;
        let e = self.cell_start[i + 1] as usize;
        &self.items[s..e]
    }

    #[inline]
    pub fn occupant(&self, i: usize) -> Option<u32> {
        match self.occupant[i] {
            NO_OCCUPANT => None,
            v => Some(v),
        }
    }

    #[inline]
    pub fn set_occupant(&mut self, i: usize, ant: u32) {
        self.occupant[i] = ant;
    }

    #[inline]
    pub fn clear_occupant(&mut self, i: usize) {
        self.occupant[i] = NO_OCCUPANT;
    }

    /// Square neighbourhood of radius `r`, inclusive. Counts the querying ant
    /// itself among the friends, which the sensor normalises away.
    pub fn counts_in_radius(&self, ants: &Ants, cx: i32, cy: i32, r: i32, colony: u8) -> (u32, u32) {
        let (mut friends, mut foes) = (0, 0);
        for y in (cy - r).max(0)..=(cy + r).min(self.height as i32 - 1) {
            for x in (cx - r).max(0)..=(cx + r).min(self.width as i32 - 1) {
                for &a in self.cell_ants(self.idx(x, y)) {
                    let a = a as usize;
                    if !ants.alive[a] {
                        continue;
                    }
                    if ants.colony[a] == colony {
                        friends += 1;
                    } else {
                        foes += 1;
                    }
                }
            }
        }
        (friends, foes)
    }

    /// Lowest-indexed living ant of another colony in the 3x3 block centred on
    /// `(cx, cy)`. Deterministic by construction.
    pub fn first_adjacent_foe(&self, ants: &Ants, cx: i32, cy: i32, colony: u8) -> Option<u32> {
        let mut best: Option<u32> = None;
        for y in (cy - 1).max(0)..=(cy + 1).min(self.height as i32 - 1) {
            for x in (cx - 1).max(0)..=(cx + 1).min(self.width as i32 - 1) {
                for &a in self.cell_ants(self.idx(x, y)) {
                    let ai = a as usize;
                    if ants.alive[ai] && ants.colony[ai] != colony && best.map_or(true, |b| a < b) {
                        best = Some(a);
                    }
                }
            }
        }
        best
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p sim`
Expected: PASS, 57 tests.

- [ ] **Step 5: Commit**

```bash
git add crates/sim/src/spatial.rs crates/sim/src/lib.rs
git commit -m "feat(sim): CSR spatial index with deterministic neighbour queries"
```

---

### Task 10: `sense` — build the 44-input vector

Sensing is egocentric and sparse: five whiskers, not a grid patch. **This module is read-only.** If a write ever appears here, determinism is gone — that is the one review rule for this file.

There is deliberately **no homing compass input.** The nest emits colony scent, which diffuses into a gradient; an ant gets home by climbing its own scent. Homing, friend/foe recognition, and territory all come out of one layer.

**Files:**
- Create: `crates/sim/src/sense.rs`
- Modify: `crates/sim/src/lib.rs`

**Interfaces:**
- Consumes: `Ants`, `Grid`, `Pheromones`, `Spatial`, `Config`.
- Produces:
  - Layout consts: `pub const IN_WHISKERS: usize = 0; IN_UNDERFOOT: usize = 30; IN_COUNTS: usize = 33; IN_PROPRIO: usize = 35; IN_BIAS: usize = 39; IN_MEMORY: usize = 40;`
  - `pub const WHISKER_ANGLES: [f32; 5] = [-1.2, -0.6, 0.0, 0.6, 1.2];`
  - `pub const CHANNELS_PER_WHISKER: usize = 6;`
  - `pub const NEIGHBOUR_RADIUS: i32 = 2;`
  - `pub fn squash_phero(v: f32, log_div: f32) -> f32` — logarithmic, monotone, non-saturating
  - `pub fn sense(i: usize, ants: &Ants, grid: &Grid, phero: &Pheromones, spatial: &Spatial, cfg: &Config) -> [f32; N_INPUTS]`

**Why the pheromone sensor is logarithmic, not `tanh`.** A nest tile emits 50 scent per tick against an evaporation of 0.999, so its equilibrium value is in the tens of thousands, while a faint trail twenty cells away is order 1. A `tanh(0.1 * v)` squash returns `1.0` for anything above about 50 — meaning the entire neighbourhood of the nest reads as a flat, saturated `1.0` with **zero gradient**. An ant standing in it would be blind to the very signal it is supposed to climb home on. `ln(1 + v) / log_div` stays monotone and discriminable across four orders of magnitude, and `ln(1 + 0) = 0` keeps the empty case at exactly zero.

- [ ] **Step 1: Write the failing tests**

Create `crates/sim/src/sense.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ants::{Ants, Spawn};
    use crate::config::Config;
    use crate::genome::{Genome, Traits};
    use crate::grid::Grid;
    use crate::pheromone::Pheromones;
    use crate::rng::Pcg32;
    use crate::spatial::Spatial;
    use crate::N_INPUTS;

    fn cfg() -> Config {
        Config { width: 16, height: 16, ..Config::default() }
    }

    /// An ant at (8,8) facing +x, vision 3, all traits mid-range.
    fn setup() -> (Config, Ants, Grid, Pheromones, Spatial) {
        let c = cfg();
        let mut a = Ants::new();
        let mut g = Genome::random(&mut Pcg32::new(1, 1));
        g.traits = Traits::from_array([0.5, 0.5, 0.5, 3.0, 10.0, 2.0, 1.0, 10000.0]);
        a.push(Spawn {
            id: 0,
            colony: 1,
            x: 8.5,
            y: 8.5,
            heading: 0.0,
            energy: 100.0,
            size: 1.0,
            lineage: 0,
            genome: g,
            birth_tick: 0,
        });
        let grid = Grid::new(&c);
        let phero = Pheromones::new(&c);
        let mut s = Spatial::new(&c);
        s.rebuild(&a);
        (c, a, grid, phero, s)
    }

    fn whisker(inputs: &[f32; N_INPUTS], w: usize, ch: usize) -> f32 {
        inputs[IN_WHISKERS + w * CHANNELS_PER_WHISKER + ch]
    }

    #[test]
    fn layout_constants_sum_to_the_input_count() {
        assert_eq!(IN_UNDERFOOT, WHISKER_ANGLES.len() * CHANNELS_PER_WHISKER);
        assert_eq!(IN_MEMORY + crate::N_MEMORY, N_INPUTS);
    }

    #[test]
    fn bias_input_is_always_one() {
        let (c, a, g, p, s) = setup();
        assert_eq!(sense(0, &a, &g, &p, &s, &c)[IN_BIAS], 1.0);
    }

    #[test]
    fn every_input_is_finite_and_bounded() {
        let (c, a, g, p, s) = setup();
        for (i, v) in sense(0, &a, &g, &p, &s, &c).iter().enumerate() {
            assert!(v.is_finite(), "input {i} is not finite");
            assert!(*v >= -1.0 && *v <= 1.0, "input {i} = {v} out of [-1,1]");
        }
    }

    #[test]
    fn memory_inputs_mirror_the_ants_memory() {
        let (c, mut a, g, p, s) = setup();
        a.memory[0] = [0.1, -0.2, 0.3, -0.4];
        let inputs = sense(0, &a, &g, &p, &s, &c);
        assert_eq!(&inputs[IN_MEMORY..], &[0.1, -0.2, 0.3, -0.4]);
    }

    #[test]
    fn the_forward_whisker_sees_food_placed_ahead() {
        let (c, a, mut g, p, s) = setup();
        // vision = 3, heading = 0 (+x), so the forward whisker samples ~(11,8).
        let i = g.idx(11, 8);
        g.food[i] = c.food_patch_max;
        let inputs = sense(0, &a, &g, &p, &s, &c);
        assert!(whisker(&inputs, 2, CH_FOOD) > 0.9, "forward whisker should see it");
        assert_eq!(whisker(&inputs, 0, CH_FOOD), 0.0, "hard-left should not");
    }

    #[test]
    fn whiskers_rotate_with_heading() {
        let (c, mut a, mut g, p, s) = setup();
        let i = g.idx(8, 11); // directly +y of the ant
        g.food[i] = c.food_patch_max;
        // Facing +y, the forward whisker should now find it.
        a.heading[0] = std::f32::consts::FRAC_PI_2;
        let inputs = sense(0, &a, &g, &p, &s, &c);
        assert!(whisker(&inputs, 2, CH_FOOD) > 0.9);
    }

    #[test]
    fn stone_reads_as_blocked() {
        let (c, a, mut g, p, s) = setup();
        let i = g.idx(11, 8);
        g.stone[i] = true;
        assert_eq!(whisker(&sense(0, &a, &g, &p, &s, &c), 2, CH_BLOCKED), 1.0);
    }

    #[test]
    fn off_grid_reads_as_blocked() {
        let (c, mut a, g, p, s) = setup();
        a.x[0] = 0.5; // vision 3 to the left is off the map
        a.heading[0] = std::f32::consts::PI;
        assert_eq!(whisker(&sense(0, &a, &g, &p, &s, &c), 2, CH_BLOCKED), 1.0);
    }

    #[test]
    fn own_and_foreign_scent_land_in_different_channels() {
        let (c, a, g, mut p, s) = setup();
        let ahead = g.idx(11, 8);
        p.deposit_scent(ahead, 10.0, 1); // ant's own colony
        let inputs = sense(0, &a, &g, &p, &s, &c);
        assert!(whisker(&inputs, 2, CH_OWN_SCENT) > 0.0);
        assert_eq!(whisker(&inputs, 2, CH_FOE_SCENT), 0.0);

        let mut p2 = Pheromones::new(&c);
        p2.deposit_scent(ahead, 10.0, 7); // a foreign colony
        let inputs = sense(0, &a, &g, &p2, &s, &c);
        assert_eq!(whisker(&inputs, 2, CH_OWN_SCENT), 0.0);
        assert!(whisker(&inputs, 2, CH_FOE_SCENT) > 0.0);
    }

    #[test]
    fn underfoot_channels_read_the_ants_own_cell() {
        let (c, a, mut g, mut p, s) = setup();
        let here = g.idx(8, 8);
        g.food[here] = c.food_patch_max;
        p.deposit_food(here, 100.0);
        p.deposit_alarm(here, 100.0);
        let inputs = sense(0, &a, &g, &p, &s, &c);
        assert!(inputs[IN_UNDERFOOT] > 0.9);
        assert!(inputs[IN_UNDERFOOT + 1] > 0.0);
        assert!(inputs[IN_UNDERFOOT + 2] > 0.0);
    }

    #[test]
    fn friend_and_foe_counts_are_normalised() {
        let c = cfg();
        let mut a = Ants::new();
        for (i, (x, y, col)) in [(8.5, 8.5, 1u8), (9.5, 8.5, 1), (7.5, 8.5, 2)].iter().enumerate() {
            let mut g = Genome::random(&mut Pcg32::new(i as u64, 1));
            g.traits = Traits::from_array([0.5, 0.5, 0.5, 3.0, 10.0, 2.0, 1.0, 10000.0]);
            a.push(Spawn {
                id: i as u64, colony: *col, x: *x, y: *y, heading: 0.0,
                energy: 100.0, size: 1.0, lineage: 0, genome: g, birth_tick: 0,
            });
        }
        let grid = Grid::new(&c);
        let phero = Pheromones::new(&c);
        let mut s = Spatial::new(&c);
        s.rebuild(&a);
        let inputs = sense(0, &a, &grid, &phero, &s, &c);
        assert!(inputs[IN_COUNTS] > 0.0, "should see one friend besides itself");
        assert!(inputs[IN_COUNTS + 1] > 0.0, "should see one foe");
    }

    #[test]
    fn proprioception_reports_fullness_not_raw_energy() {
        let (c, mut a, g, p, s) = setup();
        a.energy[0] = a.genome[0].max_energy(&c, a.size[0]);
        assert_eq!(sense(0, &a, &g, &p, &s, &c)[IN_PROPRIO], 1.0);
        a.energy[0] = 0.0;
        assert_eq!(sense(0, &a, &g, &p, &s, &c)[IN_PROPRIO], 0.0);
    }

    #[test]
    fn carrying_input_saturates_at_capacity() {
        let (c, mut a, g, p, s) = setup();
        a.carrying[0] = a.genome[0].traits.carry_capacity;
        assert_eq!(sense(0, &a, &g, &p, &s, &c)[IN_PROPRIO + 2], 1.0);
    }

    #[test]
    fn squash_phero_maps_nothing_to_zero() {
        assert_eq!(squash_phero(0.0, 12.0), 0.0);
    }

    #[test]
    fn squash_phero_stays_in_range_for_absurd_inputs() {
        for v in [0.0, 1.0, 1e3, 1e6, 1e30] {
            let s = squash_phero(v, 12.0);
            assert!((0.0..=1.0).contains(&s), "{v} -> {s}");
        }
    }

    #[test]
    fn squash_phero_stays_discriminable_across_four_decades() {
        // This is the property that makes homing possible. A tanh squash would
        // return 1.0 for every one of these, and the ant would see no gradient.
        let d = 12.0;
        let samples: Vec<f32> = [1.0, 10.0, 100.0, 1_000.0, 10_000.0]
            .iter()
            .map(|v| squash_phero(*v, d))
            .collect();
        for w in samples.windows(2) {
            assert!(w[1] - w[0] > 0.05, "adjacent decades too close: {w:?}");
        }
        assert!(*samples.last().unwrap() < 1.0, "saturated at the top decade");
    }

    #[test]
    fn a_stronger_scent_always_reads_higher() {
        let d = 12.0;
        let mut prev = -1.0;
        for v in [0.0, 0.5, 2.0, 50.0, 5_000.0, 50_000.0] {
            let s = squash_phero(v, d);
            assert!(s > prev, "not monotone at {v}");
            prev = s;
        }
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Add `pub mod sense;` to `lib.rs`.

Run: `cargo test -p sim`
Expected: FAIL — `cannot find function sense`.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/sim/src/sense.rs`:

```rust
use crate::ants::Ants;
use crate::config::Config;
use crate::grid::Grid;
use crate::pheromone::Pheromones;
use crate::spatial::Spatial;
use crate::{N_INPUTS, N_MEMORY};

// --- Input vector layout. These indices are the contract with `brain.rs`. ---
pub const IN_WHISKERS: usize = 0; // 5 whiskers x 6 channels = 30
pub const IN_UNDERFOOT: usize = 30; // food, food-pheromone, alarm
pub const IN_COUNTS: usize = 33; // friends, foes
pub const IN_PROPRIO: usize = 35; // energy, size, carrying, age
pub const IN_BIAS: usize = 39;
pub const IN_MEMORY: usize = 40; // N_MEMORY recurrent values

/// Radians relative to the ant's heading. Antennae, not eyes.
pub const WHISKER_ANGLES: [f32; 5] = [-1.2, -0.6, 0.0, 0.6, 1.2];
pub const CHANNELS_PER_WHISKER: usize = 6;

pub const CH_FOOD: usize = 0;
pub const CH_FOOD_PHERO: usize = 1;
pub const CH_ALARM: usize = 2;
pub const CH_OWN_SCENT: usize = 3;
pub const CH_FOE_SCENT: usize = 4;
pub const CH_BLOCKED: usize = 5;

/// Square radius, in cells, for the friend/foe counters.
pub const NEIGHBOUR_RADIUS: i32 = 2;
/// Count at which the friend/foe inputs saturate.
const CROWD_SATURATION: f32 = 8.0;

/// Compress an unbounded pheromone magnitude into `[0, 1]`.
///
/// Logarithmic, deliberately. Scent near a nest is ~10^4; a faint trail is
/// ~10^0. A `tanh` squash saturates at 1.0 across the whole nest neighbourhood,
/// flattening the gradient an ant must climb to get home. `ln` keeps every
/// decade discriminable, and `ln(1 + 0) == 0` pins the empty case to zero.
#[inline]
pub fn squash_phero(v: f32, log_div: f32) -> f32 {
    ((v.max(0.0) + 1.0).ln() / log_div).min(1.0)
}

/// Build one ant's sensory vector. **Read-only by contract** — this runs in the
/// parallel think phase, and a single write here would destroy determinism.
pub fn sense(
    i: usize,
    ants: &Ants,
    grid: &Grid,
    phero: &Pheromones,
    spatial: &Spatial,
    cfg: &Config,
) -> [f32; N_INPUTS] {
    let mut inputs = [0.0f32; N_INPUTS];

    let colony = ants.colony[i];
    let (px, py) = (ants.x[i], ants.y[i]);
    let heading = ants.heading[i];
    let traits = &ants.genome[i].traits;

    // --- Whiskers ---
    for (w, angle) in WHISKER_ANGLES.iter().enumerate() {
        let a = heading + angle;
        let sx = px + a.cos() * traits.vision;
        let sy = py + a.sin() * traits.vision;
        let (ix, iy) = (sx.floor() as i32, sy.floor() as i32);
        let base = IN_WHISKERS + w * CHANNELS_PER_WHISKER;

        if !grid.in_bounds(ix, iy) {
            inputs[base + CH_BLOCKED] = 1.0;
            continue;
        }
        let c = grid.idx_clamped(ix, iy);
        let (own, foe) = phero.scent_for(c, colony);
        let d = cfg.phero_log_div;
        inputs[base + CH_FOOD] = (grid.food[c] / cfg.food_patch_max).min(1.0);
        inputs[base + CH_FOOD_PHERO] = squash_phero(phero.food[c], d);
        inputs[base + CH_ALARM] = squash_phero(phero.alarm[c], d);
        inputs[base + CH_OWN_SCENT] = squash_phero(own, d);
        inputs[base + CH_FOE_SCENT] = squash_phero(foe, d);
        inputs[base + CH_BLOCKED] = if grid.stone[c] { 1.0 } else { 0.0 };
    }

    // --- Underfoot ---
    let (cx, cy) = ants.cell(i);
    let here = grid.idx(cx, cy);
    inputs[IN_UNDERFOOT] = (grid.food[here] / cfg.food_patch_max).min(1.0);
    inputs[IN_UNDERFOOT + 1] = squash_phero(phero.food[here], cfg.phero_log_div);
    inputs[IN_UNDERFOOT + 2] = squash_phero(phero.alarm[here], cfg.phero_log_div);

    // --- Crowding ---
    let (friends, foes) =
        spatial.counts_in_radius(ants, cx as i32, cy as i32, NEIGHBOUR_RADIUS, colony);
    // `friends` includes this ant, so subtract it before normalising.
    inputs[IN_COUNTS] = (friends.saturating_sub(1) as f32 / CROWD_SATURATION).min(1.0);
    inputs[IN_COUNTS + 1] = (foes as f32 / CROWD_SATURATION).min(1.0);

    // --- Proprioception ---
    let max_e = ants.genome[i].max_energy(cfg, ants.size[i]);
    inputs[IN_PROPRIO] = (ants.energy[i] / max_e).clamp(0.0, 1.0);
    inputs[IN_PROPRIO + 1] = (ants.size[i] / traits.max_size).clamp(0.0, 1.0);
    inputs[IN_PROPRIO + 2] = (ants.carrying[i] / traits.carry_capacity).clamp(0.0, 1.0);
    inputs[IN_PROPRIO + 3] = (ants.age[i] as f32 / traits.lifespan).clamp(0.0, 1.0);

    inputs[IN_BIAS] = 1.0;

    inputs[IN_MEMORY..IN_MEMORY + N_MEMORY].copy_from_slice(&ants.memory[i]);

    inputs
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p sim`
Expected: PASS, 70 tests.

- [ ] **Step 5: Commit**

```bash
git add crates/sim/src/sense.rs crates/sim/src/lib.rs
git commit -m "feat(sim): egocentric five-whisker sensing, no homing compass"
```

---

### Task 11: `Intent` and `think` — the parallel phase

`think` is the entire parallel phase: sense, forward, decide. It takes `&` everywhere and returns a value. That signature *is* the determinism guarantee — a function that cannot write cannot race.

**Files:**
- Create: `crates/sim/src/intent.rs`
- Modify: `crates/sim/src/lib.rs`

**Interfaces:**
- Consumes: `sense`, `Brain`, `Ants`, `Grid`, `Pheromones`, `Spatial`, `Config`.
- Produces:
  - `pub struct Intent { pub heading: f32, pub speed: f32, pub attack: bool, pub grab: bool, pub release: bool, pub memory: [f32; N_MEMORY] }` (`Clone`, `Debug`)
  - `pub const MAX_TURN: f32 = 0.4;` (radians per tick)
  - `pub const ATTACK_THRESHOLD: f32 = 0.5;`
  - `pub const GRAB_THRESHOLD: f32 = 0.3;`
  - `pub fn think(i: usize, ants: &Ants, grid: &Grid, phero: &Pheromones, spatial: &Spatial, cfg: &Config) -> Intent`

- [ ] **Step 1: Write the failing tests**

Create `crates/sim/src/intent.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ants::{Ants, Spawn};
    use crate::config::Config;
    use crate::genome::{Genome, Traits};
    use crate::grid::Grid;
    use crate::pheromone::Pheromones;
    use crate::rng::Pcg32;
    use crate::spatial::Spatial;

    fn world() -> (Config, Ants, Grid, Pheromones, Spatial) {
        let c = Config { width: 16, height: 16, ..Config::default() };
        let mut a = Ants::new();
        let mut g = Genome::random(&mut Pcg32::new(1, 1));
        g.traits = Traits::from_array([0.5, 0.5, 0.5, 3.0, 10.0, 2.0, 1.0, 10000.0]);
        a.push(Spawn {
            id: 0, colony: 1, x: 8.5, y: 8.5, heading: 0.0,
            energy: 100.0, size: 1.0, lineage: 0, genome: g, birth_tick: 0,
        });
        let grid = Grid::new(&c);
        let p = Pheromones::new(&c);
        let mut s = Spatial::new(&c);
        s.rebuild(&a);
        (c, a, grid, p, s)
    }

    /// Force every output to a chosen constant by zeroing the net and setting
    /// the final biases. tanh(atanh(v)) = v.
    fn force_outputs(g: &mut Genome, values: [f32; crate::N_OUTPUTS]) {
        g.params.iter_mut().for_each(|p| *p = 0.0);
        let bias_start = crate::N_PARAMS - crate::N_OUTPUTS;
        for (j, v) in values.iter().enumerate() {
            g.params[bias_start + j] = v.atanh();
        }
    }

    #[test]
    fn think_is_pure() {
        let (c, a, g, p, s) = world();
        let x = think(0, &a, &g, &p, &s, &c);
        let y = think(0, &a, &g, &p, &s, &c);
        assert_eq!(x.heading, y.heading);
        assert_eq!(x.speed, y.speed);
    }

    #[test]
    fn speed_is_never_negative_and_is_capped_by_the_trait() {
        let (c, mut a, g, p, s) = world();
        force_outputs(&mut a.genome[0], [0.0, -1.0 + 1e-6, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(think(0, &a, &g, &p, &s, &c).speed, 0.0, "reverse is not a thing");

        force_outputs(&mut a.genome[0], [0.0, 1.0 - 1e-6, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        let sp = think(0, &a, &g, &p, &s, &c).speed;
        assert!(sp <= a.genome[0].traits.max_speed + 1e-4, "speed {sp} exceeded trait");
        assert!(sp > 0.0);
    }

    #[test]
    fn turn_is_capped_at_max_turn_per_tick() {
        let (c, mut a, g, p, s) = world();
        force_outputs(&mut a.genome[0], [1.0 - 1e-6, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        let delta = think(0, &a, &g, &p, &s, &c).heading - a.heading[0];
        assert!(delta.abs() <= MAX_TURN + 1e-4, "turned {delta} in one tick");
    }

    #[test]
    fn attack_fires_only_above_threshold() {
        let (c, mut a, g, p, s) = world();
        force_outputs(&mut a.genome[0], [0.0, 0.0, 0.9, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert!(think(0, &a, &g, &p, &s, &c).attack);
        force_outputs(&mut a.genome[0], [0.0, 0.0, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert!(!think(0, &a, &g, &p, &s, &c).attack);
    }

    #[test]
    fn grab_and_release_are_opposite_signs_and_never_both() {
        let (c, mut a, g, p, s) = world();
        force_outputs(&mut a.genome[0], [0.0, 0.0, 0.0, 0.9, 0.0, 0.0, 0.0, 0.0]);
        let i = think(0, &a, &g, &p, &s, &c);
        assert!(i.grab && !i.release);

        force_outputs(&mut a.genome[0], [0.0, 0.0, 0.0, -0.9, 0.0, 0.0, 0.0, 0.0]);
        let i = think(0, &a, &g, &p, &s, &c);
        assert!(i.release && !i.grab);

        force_outputs(&mut a.genome[0], [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        let i = think(0, &a, &g, &p, &s, &c);
        assert!(!i.release && !i.grab);
    }

    #[test]
    fn memory_outputs_are_carried_on_the_intent() {
        let (c, mut a, g, p, s) = world();
        force_outputs(&mut a.genome[0], [0.0, 0.0, 0.0, 0.0, 0.5, -0.5, 0.25, -0.25]);
        let i = think(0, &a, &g, &p, &s, &c);
        for (got, want) in i.memory.iter().zip([0.5, -0.5, 0.25, -0.25]) {
            assert!((got - want).abs() < 1e-5, "{got} != {want}");
        }
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Add `pub mod intent;` to `lib.rs`.

Run: `cargo test -p sim`
Expected: FAIL — `cannot find function think`.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/sim/src/intent.rs`:

```rust
use crate::ants::Ants;
use crate::brain::{Brain, OUT_ATTACK, OUT_GRAB, OUT_MEMORY, OUT_THROTTLE, OUT_TURN};
use crate::config::Config;
use crate::grid::Grid;
use crate::pheromone::Pheromones;
use crate::sense::sense;
use crate::spatial::Spatial;
use crate::N_MEMORY;

/// Maximum heading change per tick, radians. Caps how sharply an ant can turn
/// regardless of what its network asks for.
pub const MAX_TURN: f32 = 0.4;
pub const ATTACK_THRESHOLD: f32 = 0.5;
pub const GRAB_THRESHOLD: f32 = 0.3;

/// What one ant wants to do this tick. Produced by the read-only parallel
/// phase, consumed by the serial apply phase.
#[derive(Clone, Debug)]
pub struct Intent {
    pub heading: f32,
    /// Cells per tick, always >= 0.
    pub speed: f32,
    pub attack: bool,
    pub grab: bool,
    pub release: bool,
    pub memory: [f32; N_MEMORY],
}

/// The entire parallel phase. Borrows everything immutably and returns a value;
/// it structurally cannot race, which is the whole determinism argument.
pub fn think(
    i: usize,
    ants: &Ants,
    grid: &Grid,
    phero: &Pheromones,
    spatial: &Spatial,
    cfg: &Config,
) -> Intent {
    let inputs = sense(i, ants, grid, phero, spatial, cfg);
    let act = ants.genome[i].forward(&inputs);
    let o = act.outputs;

    let heading = ants.heading[i] + o[OUT_TURN] * MAX_TURN;
    // Backwards is not modelled; a negative throttle simply means "stop".
    let speed = o[OUT_THROTTLE].max(0.0) * ants.genome[i].traits.max_speed;

    let mut memory = [0.0f32; N_MEMORY];
    memory.copy_from_slice(&o[OUT_MEMORY..OUT_MEMORY + N_MEMORY]);

    Intent {
        heading,
        speed,
        attack: o[OUT_ATTACK] > ATTACK_THRESHOLD,
        grab: o[OUT_GRAB] > GRAB_THRESHOLD,
        release: o[OUT_GRAB] < -GRAB_THRESHOLD,
        memory,
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p sim`
Expected: PASS, 76 tests.

- [ ] **Step 5: Commit**

```bash
git add crates/sim/src/intent.rs crates/sim/src/lib.rs
git commit -m "feat(sim): Intent and the read-only parallel think phase"
```

---

### Task 12: `ColonyState` — store, hall of fame, weighted parent selection

Fitness is food delivered. It appears here and nowhere else.

**Files:**
- Create: `crates/sim/src/colony.rs`
- Modify: `crates/sim/src/lib.rs`

**Interfaces:**
- Consumes: `Ants`, `Genome`, `Pcg32`, `Config`.
- Produces:
  - `pub const PARENT_EPS: f32 = 1.0;`
  - `pub struct ColonyState { pub id: u8, pub store: f32, pub nest_tiles: Vec<usize>, pub nest_center: (f32, f32), pub births: u64, pub deaths: u64, pub floor_spawns: u64, pub last_floor_spawn: u64, pub hall_of_fame: Vec<(f32, Genome)>, pub next_lineage_hint: u32 }` (`Clone`, `Serialize`, `Deserialize`)
  - `ColonyState::new(id: u8) -> ColonyState`
  - `ColonyState::record_death(&mut self, fitness: f32, genome: &Genome, cap: usize)`
  - `ColonyState::select_parent(&self, ants: &Ants, rng: &mut Pcg32) -> Option<usize>`
  - `ColonyState::archive_parent(&self, rng: &mut Pcg32) -> Option<&Genome>`

- [ ] **Step 1: Write the failing tests**

Create `crates/sim/src/colony.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ants::{Ants, Spawn};
    use crate::genome::Genome;
    use crate::rng::Pcg32;

    fn genome(seed: u64) -> Genome {
        Genome::random(&mut Pcg32::new(seed, 1))
    }

    fn ants_with(colony_and_fitness: &[(u8, f32)]) -> Ants {
        let mut a = Ants::new();
        for (i, (c, f)) in colony_and_fitness.iter().enumerate() {
            a.push(Spawn {
                id: i as u64, colony: *c, x: 0.0, y: 0.0, heading: 0.0,
                energy: 10.0, size: 1.0, lineage: 0, genome: genome(i as u64), birth_tick: 0,
            });
            a.food_delivered[i] = *f;
        }
        a
    }

    #[test]
    fn hall_of_fame_keeps_the_best_and_respects_the_cap() {
        let mut c = ColonyState::new(0);
        for f in [5.0, 1.0, 9.0, 3.0, 7.0] {
            c.record_death(f, &genome(f as u64), 3);
        }
        let fits: Vec<f32> = c.hall_of_fame.iter().map(|(f, _)| *f).collect();
        assert_eq!(fits, vec![9.0, 7.0, 5.0]);
    }

    #[test]
    fn hall_of_fame_ignores_a_worse_genome_when_full() {
        let mut c = ColonyState::new(0);
        c.record_death(10.0, &genome(1), 1);
        c.record_death(2.0, &genome(2), 1);
        assert_eq!(c.hall_of_fame.len(), 1);
        assert_eq!(c.hall_of_fame[0].0, 10.0);
    }

    #[test]
    fn record_death_increments_the_death_counter() {
        let mut c = ColonyState::new(0);
        c.record_death(1.0, &genome(1), 5);
        c.record_death(1.0, &genome(2), 5);
        assert_eq!(c.deaths, 2);
    }

    #[test]
    fn select_parent_only_ever_returns_own_colony() {
        let c = ColonyState::new(1);
        let ants = ants_with(&[(1, 5.0), (2, 500.0), (1, 5.0)]);
        let mut r = Pcg32::new(1, 1);
        for _ in 0..200 {
            let p = c.select_parent(&ants, &mut r).unwrap();
            assert_eq!(ants.colony[p], 1, "gene pools must never mix");
        }
    }

    #[test]
    fn select_parent_favours_higher_food_delivered() {
        let c = ColonyState::new(1);
        let ants = ants_with(&[(1, 0.0), (1, 1000.0)]);
        let mut r = Pcg32::new(2, 2);
        let wins = (0..1000).filter(|_| c.select_parent(&ants, &mut r) == Some(1)).count();
        assert!(wins > 900, "productive ant won only {wins}/1000");
    }

    #[test]
    fn select_parent_never_strictly_excludes_a_zero_fitness_ant() {
        let c = ColonyState::new(1);
        let ants = ants_with(&[(1, 0.0), (1, 0.0)]);
        let mut r = Pcg32::new(3, 3);
        let a = (0..500).filter(|_| c.select_parent(&ants, &mut r) == Some(0)).count();
        assert!(a > 100 && a < 400, "PARENT_EPS should keep it roughly fair, got {a}/500");
    }

    #[test]
    fn select_parent_skips_the_dead() {
        let c = ColonyState::new(1);
        let mut ants = ants_with(&[(1, 100.0), (1, 1.0)]);
        ants.alive[0] = false;
        let mut r = Pcg32::new(4, 4);
        assert_eq!(c.select_parent(&ants, &mut r), Some(1));
    }

    #[test]
    fn select_parent_returns_none_for_an_empty_colony() {
        let c = ColonyState::new(9);
        let ants = ants_with(&[(1, 1.0)]);
        assert_eq!(c.select_parent(&ants, &mut Pcg32::new(5, 5)), None);
    }

    #[test]
    fn select_parent_is_deterministic_for_a_given_rng() {
        let c = ColonyState::new(1);
        let ants = ants_with(&[(1, 3.0), (1, 4.0), (1, 5.0)]);
        let a: Vec<_> = (0..20).scan(Pcg32::new(6, 6), |r, _| Some(c.select_parent(&ants, r))).collect();
        let b: Vec<_> = (0..20).scan(Pcg32::new(6, 6), |r, _| Some(c.select_parent(&ants, r))).collect();
        assert_eq!(a, b);
    }

    #[test]
    fn archive_parent_is_none_when_the_hall_of_fame_is_empty() {
        let c = ColonyState::new(0);
        assert!(c.archive_parent(&mut Pcg32::new(7, 7)).is_none());
    }

    #[test]
    fn archive_parent_draws_from_the_hall_of_fame() {
        let mut c = ColonyState::new(0);
        c.record_death(1.0, &genome(1), 5);
        assert!(c.archive_parent(&mut Pcg32::new(8, 8)).is_some());
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Add `pub mod colony;` to `lib.rs`.

Run: `cargo test -p sim`
Expected: FAIL — `cannot find type ColonyState`.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/sim/src/colony.rs`:

```rust
use crate::ants::Ants;
use crate::genome::Genome;
use crate::rng::Pcg32;
use serde::{Deserialize, Serialize};

/// Added to every ant's selection weight so a zero-fitness ant is unlikely,
/// not impossible, to be a parent. Without this, the first generation — where
/// nobody has delivered anything — would have an all-zero weight vector.
pub const PARENT_EPS: f32 = 1.0;

/// A colony is a nest, a food store, and a gene pool. There is no queen.
#[derive(Clone, Serialize, Deserialize)]
pub struct ColonyState {
    pub id: u8,
    pub store: f32,
    pub nest_tiles: Vec<usize>,
    pub nest_center: (f32, f32),
    pub births: u64,
    pub deaths: u64,
    /// Ants conjured by the extinction floor, free of charge. Surfaced in
    /// `ColonyStats` because this is the one place the simulation cheats:
    /// a colony propped up by the floor is not a colony that is winning, and
    /// the operator must be able to see the difference.
    pub floor_spawns: u64,
    pub last_floor_spawn: u64,
    /// Best genomes ever seen, by food delivered, sorted descending. Used only
    /// by the extinction floor. A research-tool affordance, not biology.
    pub hall_of_fame: Vec<(f32, Genome)>,
    pub next_lineage_hint: u32,
}

impl ColonyState {
    pub fn new(id: u8) -> Self {
        ColonyState {
            id,
            store: 0.0,
            nest_tiles: Vec::new(),
            nest_center: (0.0, 0.0),
            births: 0,
            deaths: 0,
            floor_spawns: 0,
            last_floor_spawn: 0,
            hall_of_fame: Vec::new(),
            next_lineage_hint: 0,
        }
    }

    pub fn record_death(&mut self, fitness: f32, genome: &Genome, cap: usize) {
        self.deaths += 1;
        if self.hall_of_fame.len() >= cap {
            // Sorted descending, so the last entry is the weakest.
            if self.hall_of_fame.last().map_or(false, |(f, _)| *f >= fitness) {
                return;
            }
            self.hall_of_fame.pop();
        }
        let pos = self
            .hall_of_fame
            .iter()
            .position(|(f, _)| *f < fitness)
            .unwrap_or(self.hall_of_fame.len());
        self.hall_of_fame.insert(pos, (fitness, genome.clone()));
    }

    /// Roulette-wheel over living ants **of this colony only**, weighted by
    /// lifetime food delivered. Accumulates in ant-index order, so the draw is
    /// reproducible for a given rng state.
    pub fn select_parent(&self, ants: &Ants, rng: &mut Pcg32) -> Option<usize> {
        let mut total = 0.0f32;
        for i in 0..ants.len() {
            if ants.alive[i] && ants.colony[i] == self.id {
                total += ants.food_delivered[i] + PARENT_EPS;
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
                target -= ants.food_delivered[i] + PARENT_EPS;
                if target <= 0.0 {
                    return Some(i);
                }
            }
        }
        // Float rounding can leave `target` a hair above zero; fall back to the
        // last eligible ant rather than returning None.
        last
    }

    pub fn archive_parent(&self, rng: &mut Pcg32) -> Option<&Genome> {
        if self.hall_of_fame.is_empty() {
            return None;
        }
        let k = rng.next_below(self.hall_of_fame.len() as u32) as usize;
        Some(&self.hall_of_fame[k].1)
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p sim`
Expected: PASS, 87 tests.

- [ ] **Step 5: Commit**

```bash
git add crates/sim/src/colony.rs crates/sim/src/lib.rs
git commit -m "feat(sim): ColonyState with hall of fame and per-colony parent selection"
```

---

### Task 13: `apply` part 1 — movement, food, nest, passive deposition

The serial phase. Ants are visited in index order, which is id order, so "lower id wins" is well defined.

Two rules worth stating up front, because they are easy to get subtly wrong:
- **Cell exclusion, except on nest tiles.** One ant per cell keeps trails congested and legible. Nest tiles are exempt, or newborns would be unable to spawn onto a crowded nest and returning foragers would deadlock at the door.
- **Deposition is unconditional.** No `Intent` field gates it. Food-trail is proportional to carried food, so only laden ants lay trail, so trails run from food back to the nest.

**Files:**
- Create: `crates/sim/src/apply.rs`
- Modify: `crates/sim/src/lib.rs`

**Interfaces:**
- Consumes: `Ants`, `Intent`, `Grid`, `Pheromones`, `Spatial`, `ColonyState`, `Config`.
- Produces:
  - `pub struct ApplyCtx<'a> { pub cfg: &'a Config, pub grid: &'a mut Grid, pub phero: &'a mut Pheromones, pub spatial: &'a mut Spatial, pub colonies: &'a mut [ColonyState] }`
  - `pub fn apply_movement(i: usize, intent: &Intent, ants: &mut Ants, ctx: &mut ApplyCtx)`
  - `pub fn apply_food(i: usize, intent: &Intent, ants: &mut Ants, ctx: &mut ApplyCtx)`
  - `pub fn apply_nest(i: usize, ants: &mut Ants, ctx: &mut ApplyCtx)`
  - `pub fn deposit_passive(cell: usize, carrying: f32, colony: u8, ctx: &mut ApplyCtx)` — takes loose fields, not `&Ants`, so the caller can hold `&mut Ants` at the same time
  - `pub fn wrap_angle(a: f32) -> f32`

- [ ] **Step 1: Write the failing tests**

Create `crates/sim/src/apply.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ants::{Ants, Spawn};
    use crate::config::Config;
    use crate::genome::{Genome, Traits};
    use crate::grid::{Grid, NO_NEST};
    use crate::pheromone::Pheromones;
    use crate::rng::Pcg32;
    use crate::spatial::Spatial;
    use crate::N_MEMORY;

    struct Fixture {
        cfg: Config,
        ants: Ants,
        grid: Grid,
        phero: Pheromones,
        spatial: Spatial,
        colonies: Vec<ColonyState>,
    }

    impl Fixture {
        fn ctx(&mut self) -> ApplyCtx<'_> {
            ApplyCtx {
                cfg: &self.cfg,
                grid: &mut self.grid,
                phero: &mut self.phero,
                spatial: &mut self.spatial,
                colonies: &mut self.colonies,
            }
        }
        fn rebuild(&mut self) {
            self.spatial.rebuild(&self.ants);
        }
    }

    fn fixture(positions: &[(f32, f32, u8)]) -> Fixture {
        let cfg = Config { width: 16, height: 16, ..Config::default() };
        let mut ants = Ants::new();
        for (i, (x, y, c)) in positions.iter().enumerate() {
            let mut g = Genome::random(&mut Pcg32::new(i as u64, 1));
            g.traits = Traits::from_array([1.0, 0.5, 0.5, 3.0, 10.0, 2.0, 1.0, 10000.0]);
            ants.push(Spawn {
                id: i as u64, colony: *c, x: *x, y: *y, heading: 0.0,
                energy: 100.0, size: 1.0, lineage: 0, genome: g, birth_tick: 0,
            });
        }
        let grid = Grid::new(&cfg);
        let phero = Pheromones::new(&cfg);
        let mut spatial = Spatial::new(&cfg);
        spatial.rebuild(&ants);
        let colonies = (0..4).map(ColonyState::new).collect();
        Fixture { cfg, ants, grid, phero, spatial, colonies }
    }

    fn intent() -> Intent {
        Intent { heading: 0.0, speed: 0.0, attack: false, grab: false, release: false, memory: [0.0; N_MEMORY] }
    }

    #[test]
    fn wrap_angle_keeps_headings_bounded() {
        for a in [-100.0f32, -3.5, 0.0, 3.5, 100.0] {
            let w = wrap_angle(a);
            assert!(w >= -std::f32::consts::PI && w < std::f32::consts::PI, "{a} -> {w}");
        }
    }

    #[test]
    fn an_ant_moves_along_its_heading() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let i = Intent { heading: 0.0, speed: 1.0, ..intent() };
        apply_movement(0, &i, &mut f.ants, &mut f.ctx());
        assert!((f.ants.x[0] - 9.5).abs() < 1e-5);
        assert!((f.ants.y[0] - 8.5).abs() < 1e-5);
    }

    #[test]
    fn movement_costs_energy_proportional_to_distance() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let before = f.ants.energy[0];
        let i = Intent { heading: 0.0, speed: 1.0, ..intent() };
        apply_movement(0, &i, &mut f.ants, &mut f.ctx());
        let expected = before - f.cfg.move_cost * 1.0;
        assert!((f.ants.energy[0] - expected).abs() < 1e-4);
    }

    #[test]
    fn stone_blocks_movement_and_costs_nothing() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let s = f.grid.idx(9, 8);
        f.grid.stone[s] = true;
        let before = f.ants.energy[0];
        let i = Intent { heading: 0.0, speed: 1.0, ..intent() };
        apply_movement(0, &i, &mut f.ants, &mut f.ctx());
        assert_eq!(f.ants.x[0], 8.5, "should not have entered stone");
        assert_eq!(f.ants.energy[0], before, "a blocked ant pays no move cost");
    }

    #[test]
    fn the_map_border_blocks_movement() {
        let mut f = fixture(&[(0.5, 8.5, 1)]);
        let i = Intent { heading: std::f32::consts::PI, speed: 1.0, ..intent() };
        apply_movement(0, &i, &mut f.ants, &mut f.ctx());
        assert_eq!(f.ants.x[0], 0.5);
    }

    #[test]
    fn moving_within_the_same_cell_is_always_allowed() {
        let mut f = fixture(&[(8.1, 8.5, 1)]);
        let i = Intent { heading: 0.0, speed: 0.2, ..intent() };
        apply_movement(0, &i, &mut f.ants, &mut f.ctx());
        assert!((f.ants.x[0] - 8.3).abs() < 1e-5);
    }

    #[test]
    fn an_occupied_cell_blocks_the_higher_id_ant() {
        let mut f = fixture(&[(9.5, 8.5, 1), (8.5, 8.5, 1)]);
        f.rebuild();
        // Ant 1 tries to walk into ant 0's cell.
        let i = Intent { heading: 0.0, speed: 1.0, ..intent() };
        apply_movement(1, &i, &mut f.ants, &mut f.ctx());
        assert_eq!(f.ants.x[1], 8.5, "blocked by the incumbent");
    }

    #[test]
    fn nest_tiles_are_exempt_from_cell_exclusion() {
        let mut f = fixture(&[(9.5, 8.5, 1), (8.5, 8.5, 1)]);
        let n = f.grid.idx(9, 8);
        f.grid.nest[n] = 1;
        f.rebuild();
        let i = Intent { heading: 0.0, speed: 1.0, ..intent() };
        apply_movement(1, &i, &mut f.ants, &mut f.ctx());
        assert!((f.ants.x[1] - 9.5).abs() < 1e-5, "should stack on the nest");
    }

    #[test]
    fn grab_harvests_food_up_to_carry_capacity() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let c = f.grid.idx(8, 8);
        f.grid.food[c] = 100.0;
        f.ants.carrying[0] = 9.7; // capacity is 10.0
        let i = Intent { grab: true, ..intent() };
        apply_food(0, &i, &mut f.ants, &mut f.ctx());
        assert!((f.ants.carrying[0] - 10.0).abs() < 1e-5);
        assert!((f.grid.food[c] - 99.7).abs() < 1e-4);
    }

    #[test]
    fn grab_takes_nothing_from_an_empty_cell() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let i = Intent { grab: true, ..intent() };
        apply_food(0, &i, &mut f.ants, &mut f.ctx());
        assert_eq!(f.ants.carrying[0], 0.0);
    }

    #[test]
    fn release_drops_the_load_back_onto_the_ground() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        f.ants.carrying[0] = 4.0;
        let i = Intent { release: true, ..intent() };
        apply_food(0, &i, &mut f.ants, &mut f.ctx());
        assert_eq!(f.ants.carrying[0], 0.0);
        assert_eq!(f.grid.food[f.grid.idx(8, 8)], 4.0);
    }

    #[test]
    fn standing_on_your_own_nest_deposits_the_load_into_the_store() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let n = f.grid.idx(8, 8);
        f.grid.nest[n] = 1;
        f.ants.carrying[0] = 6.0;
        apply_nest(0, &mut f.ants, &mut f.ctx());
        assert_eq!(f.ants.carrying[0], 0.0);
        assert_eq!(f.colonies[1].store, 6.0);
    }

    #[test]
    fn depositing_credits_food_delivered_which_is_the_only_fitness_signal() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        f.grid.nest[f.grid.idx(8, 8)] = 1;
        f.ants.carrying[0] = 6.0;
        apply_nest(0, &mut f.ants, &mut f.ctx());
        assert_eq!(f.ants.food_delivered[0], 6.0);
    }

    #[test]
    fn a_foreign_nest_accepts_nothing() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        f.grid.nest[f.grid.idx(8, 8)] = 2;
        f.ants.carrying[0] = 6.0;
        apply_nest(0, &mut f.ants, &mut f.ctx());
        assert_eq!(f.ants.carrying[0], 6.0);
        assert_eq!(f.colonies[2].store, 0.0);
    }

    #[test]
    fn refuelling_draws_from_the_store_and_is_capped_by_max_energy() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        f.grid.nest[f.grid.idx(8, 8)] = 1;
        f.colonies[1].store = 1000.0;
        let max_e = f.ants.genome[0].max_energy(&f.cfg, f.ants.size[0]);
        f.ants.energy[0] = max_e - 1.0;
        apply_nest(0, &mut f.ants, &mut f.ctx());
        assert!((f.ants.energy[0] - max_e).abs() < 1e-4);
        assert!((f.colonies[1].store - 999.0).abs() < 1e-3, "took only what it needed");
    }

    #[test]
    fn an_empty_store_cannot_refuel_anyone() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        f.grid.nest[f.grid.idx(8, 8)] = 1;
        f.colonies[1].store = 0.0;
        f.ants.energy[0] = 1.0;
        apply_nest(0, &mut f.ants, &mut f.ctx());
        assert_eq!(f.ants.energy[0], 1.0);
    }

    #[test]
    fn every_ant_leaks_colony_scent_unconditionally() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let c = f.grid.idx(8, 8);
        deposit_passive(c, 0.0, 1, &mut f.ctx());
        assert_eq!(f.phero.scent[c], f.cfg.ant_scent_emission);
        assert_eq!(f.phero.owner[c], 1);
    }

    #[test]
    fn only_a_laden_ant_lays_food_trail() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let c = f.grid.idx(8, 8);

        deposit_passive(c, 0.0, 1, &mut f.ctx());
        assert_eq!(f.phero.food[c], 0.0, "an empty-handed ant lays no trail");

        deposit_passive(c, 3.0, 1, &mut f.ctx());
        assert!((f.phero.food[c] - 3.0 * f.cfg.food_trail_emission).abs() < 1e-4);
    }

    #[test]
    fn release_onto_a_nest_tile_is_ignored_so_food_cannot_be_dumped_at_the_door() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        f.grid.nest[f.grid.idx(8, 8)] = 1;
        f.ants.carrying[0] = 5.0;
        let i = Intent { release: true, ..intent() };
        apply_food(0, &i, &mut f.ants, &mut f.ctx());
        assert_eq!(f.grid.food[f.grid.idx(8, 8)], 0.0);
        assert_eq!(f.ants.carrying[0], 5.0, "apply_nest handles nest deposits, not apply_food");
    }
}
```

That last test pins a real ambiguity: dropping food *onto* a nest tile must not create a food pile there, because `apply_nest` already banks it. Otherwise an ant could farm the ground under its own nest.

- [ ] **Step 2: Run to verify it fails**

Add `pub mod apply;` to `lib.rs`.

Run: `cargo test -p sim`
Expected: FAIL — `cannot find function apply_movement`.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/sim/src/apply.rs`:

```rust
use crate::ants::Ants;
use crate::colony::ColonyState;
use crate::config::Config;
use crate::grid::{Grid, NO_NEST};
use crate::intent::Intent;
use crate::pheromone::Pheromones;
use crate::spatial::Spatial;

/// Everything the serial apply phase is allowed to mutate.
pub struct ApplyCtx<'a> {
    pub cfg: &'a Config,
    pub grid: &'a mut Grid,
    pub phero: &'a mut Pheromones,
    pub spatial: &'a mut Spatial,
    /// Indexed by colony id.
    pub colonies: &'a mut [ColonyState],
}

/// Normalise to `[-PI, PI)` so headings cannot drift to a magnitude where f32
/// loses angular precision.
pub fn wrap_angle(a: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let mut r = (a + PI).rem_euclid(TAU);
    if r < 0.0 {
        r += TAU;
    }
    r - PI
}

/// Heading, then translation. One ant per cell, except on nest tiles: without
/// that exemption newborns could not spawn onto a busy nest and returning
/// foragers would jam in the doorway.
pub fn apply_movement(i: usize, intent: &Intent, ants: &mut Ants, ctx: &mut ApplyCtx) {
    ants.memory[i] = intent.memory;
    ants.age[i] += 1;
    ants.heading[i] = wrap_angle(intent.heading);

    if intent.speed <= 0.0 {
        return;
    }

    let (cx, cy) = ants.cell(i);
    let cur = ctx.grid.idx(cx, cy);
    let nx = ants.x[i] + ants.heading[i].cos() * intent.speed;
    let ny = ants.y[i] + ants.heading[i].sin() * intent.speed;
    let (tx, ty) = (nx.floor() as i32, ny.floor() as i32);

    // Staying inside the current cell needs no occupancy check.
    if tx == cx as i32 && ty == cy as i32 {
        ants.x[i] = nx;
        ants.y[i] = ny;
        ants.energy[i] -= ctx.cfg.move_cost * intent.speed;
        return;
    }

    if ctx.grid.is_stone(tx, ty) {
        return;
    }
    let target = ctx.grid.idx_clamped(tx, ty);
    let is_nest = ctx.grid.nest[target] != NO_NEST;
    if !is_nest && ctx.spatial.occupant(target).is_some() {
        return;
    }

    if ctx.spatial.occupant(cur) == Some(i as u32) {
        ctx.spatial.clear_occupant(cur);
    }
    if ctx.spatial.occupant(target).is_none() {
        ctx.spatial.set_occupant(target, i as u32);
    }
    ants.x[i] = nx;
    ants.y[i] = ny;
    ants.energy[i] -= ctx.cfg.move_cost * intent.speed;
}

/// Grab from the ground, or drop onto it. Nest tiles are handled by
/// `apply_nest`, so releasing on one is a no-op rather than a food pile.
pub fn apply_food(i: usize, intent: &Intent, ants: &mut Ants, ctx: &mut ApplyCtx) {
    let (cx, cy) = ants.cell(i);
    let c = ctx.grid.idx(cx, cy);
    let capacity = ants.genome[i].traits.carry_capacity;

    if intent.grab && ants.carrying[i] < capacity {
        let want = ctx.cfg.harvest_rate.min(capacity - ants.carrying[i]);
        ants.carrying[i] += ctx.grid.harvest(c, want);
    } else if intent.release && ants.carrying[i] > 0.0 && ctx.grid.nest[c] == NO_NEST {
        ctx.grid.food[c] += ants.carrying[i];
        ants.carrying[i] = 0.0;
    }
}

/// Standing on your own nest banks your load and refuels you. Both are
/// automatic; the network must only evolve to *go there*.
pub fn apply_nest(i: usize, ants: &mut Ants, ctx: &mut ApplyCtx) {
    let (cx, cy) = ants.cell(i);
    let c = ctx.grid.idx(cx, cy);
    if ctx.grid.nest[c] != ants.colony[i] {
        return;
    }
    let colony = &mut ctx.colonies[ants.colony[i] as usize];

    let load = ants.carrying[i];
    if load > 0.0 {
        colony.store += load;
        ants.food_delivered[i] += load;
        ants.carrying[i] = 0.0;
    }

    let max_e = ants.genome[i].max_energy(ctx.cfg, ants.size[i]);
    let want = (max_e - ants.energy[i]).max(0.0).min(ctx.cfg.refuel_rate);
    let taken = want.min(colony.store);
    colony.store -= taken;
    ants.energy[i] += taken;
}

/// Passive chemical leakage. No `Intent` field gates this: ants leak because
/// they are ants. Food-trail is proportional to the load, so only a laden ant
/// marks a path — which is why trails run from food back toward the nest.
///
/// Takes loose fields rather than `&Ants` so the caller can hold `&mut Ants`
/// across the call without cloning the whole store every iteration.
pub fn deposit_passive(cell: usize, carrying: f32, colony: u8, ctx: &mut ApplyCtx) {
    if carrying > 0.0 {
        ctx.phero.deposit_food(cell, ctx.cfg.food_trail_emission * carrying);
    }
    ctx.phero.deposit_scent(cell, ctx.cfg.ant_scent_emission, colony);
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p sim`
Expected: PASS, 105 tests.

- [ ] **Step 5: Commit**

```bash
git add crates/sim/src/apply.rs crates/sim/src/lib.rs
git commit -m "feat(sim): serial apply — movement, food, nest banking, passive deposition"
```

---

### Task 14: `apply` part 2 — combat, metabolism, growth, death

**Energy is health.** Combat damage and starvation subtract from the same pool; death at zero covers both. A big well-fed ant is naturally hard to kill; a fighter that wins but does not eat still dies.

Combat never sets `alive = false` directly — it drives energy to zero and lets the single death sweep do the bookkeeping. One code path for "an ant died" means one place that drops its load, records its fitness in the hall of fame, and increments the death counter.

**Files:**
- Modify: `crates/sim/src/apply.rs`

**Interfaces:**
- Consumes: everything from Task 13.
- Produces:
  - `pub const MIN_SIZE: f32 = 0.2;`
  - `pub fn apply_combat(i: usize, intent: &Intent, ants: &mut Ants, ctx: &mut ApplyCtx)`
  - `pub fn apply_metabolism(i: usize, ants: &mut Ants, cfg: &Config)`
  - `pub fn sweep_deaths(ants: &mut Ants, ctx: &mut ApplyCtx)`

- [ ] **Step 1: Write the failing tests**

Append to the `mod tests` block in `crates/sim/src/apply.rs`:

```rust
    #[test]
    fn attacking_costs_energy_and_damages_the_foe() {
        let mut f = fixture(&[(8.5, 8.5, 1), (9.5, 8.5, 2)]);
        f.rebuild();
        let att_before = f.ants.energy[0];
        let def_before = f.ants.energy[1];
        let i = Intent { attack: true, ..intent() };
        apply_combat(0, &i, &mut f.ants, &mut f.ctx());
        assert!(f.ants.energy[0] < att_before, "attacker pays");
        assert!(f.ants.energy[1] < def_before, "defender bleeds");
    }

    #[test]
    fn a_nestmate_is_never_attacked() {
        let mut f = fixture(&[(8.5, 8.5, 1), (9.5, 8.5, 1)]);
        f.rebuild();
        let before = f.ants.energy[1];
        let i = Intent { attack: true, ..intent() };
        apply_combat(0, &i, &mut f.ants, &mut f.ctx());
        assert_eq!(f.ants.energy[1], before);
    }

    #[test]
    fn a_distant_foe_is_out_of_reach() {
        let mut f = fixture(&[(2.5, 2.5, 1), (12.5, 12.5, 2)]);
        f.rebuild();
        let before = f.ants.energy[1];
        let i = Intent { attack: true, ..intent() };
        apply_combat(0, &i, &mut f.ants, &mut f.ctx());
        assert_eq!(f.ants.energy[1], before);
    }

    #[test]
    fn damage_scales_with_size_and_strength_and_is_reduced_by_armor() {
        let base = |att: (f32, f32), def_armor: f32| {
            let mut f = fixture(&[(8.5, 8.5, 1), (9.5, 8.5, 2)]);
            f.ants.size[0] = att.0;
            f.ants.genome[0].traits.strength = att.1;
            f.ants.genome[1].traits.armor = def_armor;
            f.ants.energy[1] = 1000.0;
            f.rebuild();
            let i = Intent { attack: true, ..intent() };
            apply_combat(0, &i, &mut f.ants, &mut f.ctx());
            1000.0 - f.ants.energy[1]
        };
        assert!(base((2.0, 1.0), 0.0) > base((1.0, 1.0), 0.0), "size raises damage");
        assert!(base((1.0, 1.0), 0.0) > base((1.0, 0.2), 0.0), "strength raises damage");
        assert!(base((1.0, 1.0), 0.9) < base((1.0, 1.0), 0.0), "armor cuts damage");
    }

    #[test]
    fn attacking_raises_the_alarm_pheromone() {
        let mut f = fixture(&[(8.5, 8.5, 1), (9.5, 8.5, 2)]);
        f.rebuild();
        let i = Intent { attack: true, ..intent() };
        apply_combat(0, &i, &mut f.ants, &mut f.ctx());
        assert!(f.phero.alarm[f.grid.idx(9, 8)] > 0.0, "alarm marks the victim's cell");
    }

    #[test]
    fn a_killer_scavenges_energy_from_the_body() {
        let mut f = fixture(&[(8.5, 8.5, 1), (9.5, 8.5, 2)]);
        f.ants.energy[0] = 10.0;
        f.ants.energy[1] = 0.01; // one hit from death
        f.ants.genome[0].traits.strength = 1.0;
        f.rebuild();
        let i = Intent { attack: true, ..intent() };
        apply_combat(0, &i, &mut f.ants, &mut f.ctx());
        assert!(f.ants.energy[1] <= 0.0, "victim is dead by the sweep's reckoning");
        assert!(f.ants.energy[0] > 10.0 - f.cfg.attack_cost, "killer absorbed the corpse");
    }

    #[test]
    fn combat_does_not_mark_the_dead_the_sweep_does() {
        let mut f = fixture(&[(8.5, 8.5, 1), (9.5, 8.5, 2)]);
        f.ants.energy[1] = 0.01;
        f.rebuild();
        let i = Intent { attack: true, ..intent() };
        apply_combat(0, &i, &mut f.ants, &mut f.ctx());
        assert!(f.ants.alive[1], "still flagged alive until the sweep runs");
    }

    #[test]
    fn only_the_killing_blow_scavenges_so_a_mob_cannot_mint_energy() {
        // Three attackers, one nearly-dead victim. Because deaths are flagged
        // by the sweep and not by combat, the corpse stays a legal target all
        // tick. Exactly one attacker may collect the bounty.
        let mut f = fixture(&[(8.5, 8.5, 1), (9.5, 8.5, 1), (8.5, 9.5, 1), (9.5, 9.5, 2)]);
        f.ants.energy[3] = 0.01;
        for a in 0..3 {
            f.ants.energy[a] = 10.0;
            f.ants.genome[a].traits.strength = 1.0;
        }
        f.ants.genome[3].traits.armor = 0.0;
        f.rebuild();

        let i = Intent { attack: true, ..intent() };
        for a in 0..3 {
            apply_combat(a, &i, &mut f.ants, &mut f.ctx());
        }

        let bounty = f.cfg.kill_energy_frac * f.cfg.max_energy_per_size * f.ants.size[3];
        let gained: f32 = (0..3)
            .map(|a| f.ants.energy[a] - (10.0 - f.cfg.attack_cost))
            .sum();
        assert!(
            (gained - bounty).abs() < 1e-3,
            "mob scavenged {gained} from a corpse worth {bounty}: energy was created"
        );
    }

    #[test]
    fn hitting_an_already_dead_victim_yields_nothing() {
        let mut f = fixture(&[(8.5, 8.5, 1), (9.5, 8.5, 2)]);
        f.ants.energy[0] = 10.0;
        f.ants.energy[1] = -5.0; // already below zero, sweep has not run
        f.rebuild();
        let i = Intent { attack: true, ..intent() };
        apply_combat(0, &i, &mut f.ants, &mut f.ctx());
        assert!(
            (f.ants.energy[0] - (10.0 - f.cfg.attack_cost)).abs() < 1e-4,
            "attacker gained energy from a corpse it did not kill"
        );
    }

    #[test]
    fn an_exhausted_ant_cannot_afford_to_attack() {
        let mut f = fixture(&[(8.5, 8.5, 1), (9.5, 8.5, 2)]);
        f.ants.energy[0] = f.cfg.attack_cost * 0.5;
        let before = f.ants.energy[1];
        f.rebuild();
        let i = Intent { attack: true, ..intent() };
        apply_combat(0, &i, &mut f.ants, &mut f.ctx());
        assert_eq!(f.ants.energy[1], before);
    }

    #[test]
    fn metabolism_drains_energy_every_tick() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let before = f.ants.energy[0];
        apply_metabolism(0, &mut f.ants, &f.cfg);
        assert!(f.ants.energy[0] < before);
    }

    #[test]
    fn a_well_fed_ant_grows_and_pays_for_the_tissue() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        f.ants.size[0] = 1.0;
        f.ants.energy[0] = f.ants.genome[0].max_energy(&f.cfg, 1.0);
        let size_before = f.ants.size[0];
        apply_metabolism(0, &mut f.ants, &f.cfg);
        assert!(f.ants.size[0] > size_before, "should grow when nearly full");
    }

    #[test]
    fn growth_stops_at_the_genetic_max_size() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let max = f.ants.genome[0].traits.max_size;
        f.ants.size[0] = max;
        f.ants.energy[0] = f.ants.genome[0].max_energy(&f.cfg, max);
        apply_metabolism(0, &mut f.ants, &f.cfg);
        assert!(f.ants.size[0] <= max + 1e-6);
    }

    #[test]
    fn a_starving_ant_burns_its_own_body_for_energy() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        f.ants.size[0] = 2.0;
        f.ants.energy[0] = 0.0;
        apply_metabolism(0, &mut f.ants, &f.cfg);
        assert!(f.ants.size[0] < 2.0, "fat is a famine buffer");
        assert!(f.ants.energy[0] > 0.0, "and it buys another tick");
    }

    #[test]
    fn shrinking_bottoms_out_at_min_size() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        f.ants.size[0] = MIN_SIZE;
        f.ants.energy[0] = 0.0;
        apply_metabolism(0, &mut f.ants, &f.cfg);
        assert!(f.ants.size[0] >= MIN_SIZE - 1e-6);
    }

    #[test]
    fn the_sweep_kills_the_starved_and_records_their_fitness() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        f.ants.energy[0] = 0.0;
        f.ants.food_delivered[0] = 12.0;
        sweep_deaths(&mut f.ants, &mut f.ctx());
        assert!(!f.ants.alive[0]);
        assert_eq!(f.colonies[1].deaths, 1);
        assert_eq!(f.colonies[1].hall_of_fame[0].0, 12.0);
    }

    #[test]
    fn the_sweep_kills_ants_that_outlive_their_genetic_lifespan() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        f.ants.age[0] = f.ants.genome[0].traits.lifespan as u32 + 1;
        sweep_deaths(&mut f.ants, &mut f.ctx());
        assert!(!f.ants.alive[0], "nobody lives forever");
    }

    #[test]
    fn a_corpse_drops_the_food_it_was_carrying() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        f.ants.energy[0] = 0.0;
        f.ants.carrying[0] = 7.0;
        sweep_deaths(&mut f.ants, &mut f.ctx());
        assert_eq!(f.grid.food[f.grid.idx(8, 8)], 7.0);
    }

    #[test]
    fn the_sweep_leaves_the_living_alone() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        f.ants.energy[0] = 5.0;
        sweep_deaths(&mut f.ants, &mut f.ctx());
        assert!(f.ants.alive[0]);
        assert_eq!(f.colonies[1].deaths, 0);
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p sim`
Expected: FAIL — `cannot find function apply_combat`.

- [ ] **Step 3: Write the implementation**

Append to the implementation section of `crates/sim/src/apply.rs` (above `mod tests`):

```rust
/// An ant may not shrink below this, however starved.
pub const MIN_SIZE: f32 = 0.2;

/// Attack the lowest-indexed adjacent foe. Damage is `size x strength`,
/// negated in proportion to the target's armor. Aggression is never free:
/// it costs energy up front, and only pays if the corpse is worth more.
///
/// Energy is health, so this simply drains the victim. `sweep_deaths` decides
/// who actually died — one code path for death bookkeeping.
pub fn apply_combat(i: usize, intent: &Intent, ants: &mut Ants, ctx: &mut ApplyCtx) {
    if !intent.attack || ants.energy[i] < ctx.cfg.attack_cost {
        return;
    }
    let (cx, cy) = ants.cell(i);
    let Some(v) = ctx.spatial.first_adjacent_foe(ants, cx as i32, cy as i32, ants.colony[i]) else {
        return;
    };
    let v = v as usize;

    let damage = ctx.cfg.attack_damage
        * ants.size[i]
        * ants.genome[i].traits.strength
        * (1.0 - ants.genome[v].traits.armor);

    ants.energy[i] -= ctx.cfg.attack_cost;
    let victim_energy_before = ants.energy[v];
    ants.energy[v] -= damage;

    // Alarm is leaked involuntarily by both parties, as in real ants.
    let (vx, vy) = ants.cell(v);
    let here = ctx.grid.idx(cx, cy);
    let there = ctx.grid.idx(vx, vy);
    ctx.phero.deposit_alarm(here, ctx.cfg.alarm_emission);
    ctx.phero.deposit_alarm(there, ctx.cfg.alarm_emission);

    // Only the blow that *crosses* zero scavenges. Deaths are flagged by the
    // end-of-tick sweep, so a victim already at or below zero stays a valid
    // target for the rest of the serial phase — without this guard, every ant
    // in a mob would "kill" the same corpse and each mint a full kill bonus
    // from nothing.
    let killing_blow = victim_energy_before > 0.0 && ants.energy[v] <= 0.0;
    if killing_blow {
        let scavenged = ctx.cfg.kill_energy_frac * ctx.cfg.max_energy_per_size * ants.size[v];
        let max_e = ants.genome[i].max_energy(ctx.cfg, ants.size[i]);
        ants.energy[i] = (ants.energy[i] + scavenged).min(max_e);
    }
}

/// Upkeep, then growth or famine-shrink. Size multiplies both what an ant can
/// do and what it costs to be.
pub fn apply_metabolism(i: usize, ants: &mut Ants, cfg: &Config) {
    ants.energy[i] -= ants.genome[i].upkeep(cfg, ants.size[i]);

    let max_e = ants.genome[i].max_energy(cfg, ants.size[i]);
    let max_size = ants.genome[i].traits.max_size;

    if ants.energy[i] > cfg.growth_threshold * max_e && ants.size[i] < max_size {
        let grow = cfg.growth_rate.min(max_size - ants.size[i]);
        ants.size[i] += grow;
        ants.energy[i] -= grow * cfg.max_energy_per_size;
    } else if ants.energy[i] <= 0.0 && ants.size[i] > MIN_SIZE {
        let shrink = cfg.shrink_rate.min(ants.size[i] - MIN_SIZE);
        ants.size[i] -= shrink;
        ants.energy[i] += shrink * cfg.max_energy_per_size;
    }
}

/// The single place an ant dies. Runs after every ant has acted, so an ant
/// driven to zero energy by a lower-id attacker may still have taken its own
/// turn this tick. That is deterministic, and cheaper than a mid-tick recheck.
pub fn sweep_deaths(ants: &mut Ants, ctx: &mut ApplyCtx) {
    for i in 0..ants.len() {
        if !ants.alive[i] {
            continue;
        }
        let starved = ants.energy[i] <= 0.0;
        let elderly = ants.age[i] as f32 > ants.genome[i].traits.lifespan;
        if !starved && !elderly {
            continue;
        }

        ants.alive[i] = false;

        let (cx, cy) = ants.cell(i);
        let c = ctx.grid.idx(cx, cy);
        if ants.carrying[i] > 0.0 {
            ctx.grid.food[c] += ants.carrying[i];
            ants.carrying[i] = 0.0;
        }
        if ctx.spatial.occupant(c) == Some(i as u32) {
            ctx.spatial.clear_occupant(c);
        }

        let colony = &mut ctx.colonies[ants.colony[i] as usize];
        colony.record_death(ants.food_delivered[i], &ants.genome[i], ctx.cfg.hall_of_fame_size);
    }
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p sim`
Expected: PASS, 121 tests.

- [ ] **Step 5: Commit**

```bash
git add crates/sim/src/apply.rs
git commit -m "feat(sim): combat, metabolism, growth, and the single death sweep"
```

---

### Task 15: `reproduce` — nest births and the extinction floor

No queen. The nest spends the food store to spawn an ant whose genome is a mutated copy of one parent, sampled from that colony's living ants weighted by food delivered. Parents are only ever sampled from the same colony: **gene pools never mix.**

The extinction floor is an explicit research-tool affordance, not biology. A colony that hits zero ants is an empty region of screen that teaches nothing, so a colony below the floor gets free ants from its hall of fame. Weak colonies still stay small and lose ground.

**Files:**
- Create: `crates/sim/src/reproduce.rs`
- Modify: `crates/sim/src/lib.rs`

**Interfaces:**
- Consumes: `Ants`, `ColonyState`, `Grid`, `Config`, `Pcg32`, `Genome`.
- Produces:
  - `pub const NEWBORN_SIZE: f32 = 0.5;`
  - `pub const NEWBORN_ENERGY_FRAC: f32 = 0.6;`
  - `pub fn reproduce(ants: &mut Ants, colonies: &mut [ColonyState], cfg: &Config, tick: u64, next_id: &mut u64, rng: &mut Pcg32)`

- [ ] **Step 1: Write the failing tests**

Create `crates/sim/src/reproduce.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ants::{Ants, Spawn};
    use crate::config::Config;
    use crate::genome::Genome;
    use crate::rng::Pcg32;

    fn setup(cfg: &Config, members: &[(u8, f32)]) -> (Ants, Vec<ColonyState>) {
        let mut ants = Ants::new();
        for (i, (c, fitness)) in members.iter().enumerate() {
            ants.push(Spawn {
                id: i as u64, colony: *c, x: 4.0, y: 4.0, heading: 0.0,
                energy: 50.0, size: 1.0, lineage: 3,
                genome: Genome::random(&mut Pcg32::new(i as u64, 1)), birth_tick: 0,
            });
            ants.food_delivered[i] = *fitness;
        }
        let mut colonies: Vec<ColonyState> =
            (0..cfg.num_colonies).map(ColonyState::new).collect();
        for c in colonies.iter_mut() {
            c.nest_tiles = vec![0, 1, 2];
            c.nest_center = (4.0, 4.0);
        }
        (ants, colonies)
    }

    fn cfg() -> Config {
        Config { width: 16, height: 16, num_colonies: 2, extinction_floor: 0, ..Config::default() }
    }

    #[test]
    fn a_full_store_produces_a_birth_and_is_debited() {
        let c = cfg();
        let (mut ants, mut cols) = setup(&c, &[(0, 10.0)]);
        cols[0].store = c.birth_cost * 1.5;
        let mut id = 1;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(1, 1));
        assert_eq!(ants.len(), 2);
        assert_eq!(cols[0].births, 1);
        assert!((cols[0].store - c.birth_cost * 0.5).abs() < 1e-4);
    }

    #[test]
    fn an_empty_store_produces_nothing() {
        let c = cfg();
        let (mut ants, mut cols) = setup(&c, &[(0, 10.0)]);
        cols[0].store = c.birth_cost * 0.9;
        let mut id = 1;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(1, 1));
        assert_eq!(ants.len(), 1);
    }

    #[test]
    fn births_are_rate_limited_per_tick() {
        let c = Config { max_births_per_tick: 2, ..cfg() };
        let (mut ants, mut cols) = setup(&c, &[(0, 10.0)]);
        cols[0].store = c.birth_cost * 100.0;
        let mut id = 1;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(1, 1));
        assert_eq!(ants.len(), 3, "one parent plus two newborns");
    }

    #[test]
    fn a_newborn_joins_its_parents_colony() {
        let c = cfg();
        let (mut ants, mut cols) = setup(&c, &[(1, 10.0)]);
        cols[1].store = c.birth_cost;
        let mut id = 1;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(1, 1));
        assert_eq!(ants.colony[1], 1);
    }

    #[test]
    fn a_newborn_is_a_mutated_copy_not_a_clone() {
        let c = cfg();
        let (mut ants, mut cols) = setup(&c, &[(0, 10.0)]);
        cols[0].store = c.birth_cost;
        let mut id = 1;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(1, 1));
        assert_ne!(ants.genome[0].params, ants.genome[1].params);
    }

    #[test]
    fn a_newborn_lineage_is_one_deeper_than_its_parent() {
        let c = cfg();
        let (mut ants, mut cols) = setup(&c, &[(0, 10.0)]);
        cols[0].store = c.birth_cost;
        let mut id = 1;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(1, 1));
        assert_eq!(ants.lineage[1], 4, "parent lineage was 3");
    }

    #[test]
    fn a_newborn_spawns_on_one_of_its_nest_tiles() {
        let c = Config { width: 16, height: 16, num_colonies: 2, extinction_floor: 0, ..Config::default() };
        let (mut ants, mut cols) = setup(&c, &[(0, 10.0)]);
        cols[0].nest_tiles = vec![16 * 5 + 5];
        cols[0].store = c.birth_cost;
        let mut id = 1;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(1, 1));
        assert_eq!((ants.x[1].floor(), ants.y[1].floor()), (5.0, 5.0));
    }

    #[test]
    fn gene_pools_never_mix() {
        let c = cfg();
        // Colony 1 has a superstar; colony 0 is spending. Colony 0 must not use it.
        let (mut ants, mut cols) = setup(&c, &[(0, 0.0), (1, 10_000.0)]);
        cols[0].store = c.birth_cost * 10.0;
        let mut id = 2;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(2, 2));
        for i in 0..ants.len() {
            if ants.colony[i] == 0 && i > 0 {
                assert_eq!(ants.colony[i], 0);
            }
        }
        assert_eq!(cols[1].births, 0, "colony 1 never paid for a birth");
    }

    #[test]
    fn a_colony_below_the_floor_gets_one_free_ant_from_its_archive() {
        let c = Config { extinction_floor: 3, ..cfg() };
        let (mut ants, mut cols) = setup(&c, &[(0, 5.0)]);
        cols[0].store = 0.0;
        cols[0].record_death(9.0, &Genome::random(&mut Pcg32::new(9, 9)), 5);
        let mut id = 1;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(3, 3));
        assert_eq!(ants.population(0), 2, "one free ant, not a full top-up");
        assert_eq!(cols[0].store, 0.0, "free ants cost nothing");
        assert_eq!(cols[0].floor_spawns, 1, "the cheat is counted");
    }

    #[test]
    fn free_ants_are_rate_limited_to_one_per_interval() {
        let c = Config { extinction_floor: 5, floor_respawn_interval: 100, ..cfg() };
        let (mut ants, mut cols) = setup(&c, &[]);
        let mut id = 0;
        let mut rng = Pcg32::new(3, 3);

        // Ticks 0..99: only the very first is eligible.
        for t in 0..100 {
            reproduce(&mut ants, &mut cols, &c, t, &mut id, &mut rng);
        }
        assert_eq!(cols[0].floor_spawns, 1, "the interval was not honoured");

        // Tick 100 clears the interval.
        reproduce(&mut ants, &mut cols, &c, 100, &mut id, &mut rng);
        assert_eq!(cols[0].floor_spawns, 2);
    }

    #[test]
    fn the_floor_falls_back_to_a_random_genome_when_the_archive_is_empty() {
        let c = Config { extinction_floor: 2, ..cfg() };
        let (mut ants, mut cols) = setup(&c, &[]);
        let mut id = 0;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(4, 4));
        assert_eq!(ants.population(0), 1);
        assert_eq!(ants.population(1), 1);
    }

    #[test]
    fn a_colony_at_the_floor_is_not_topped_up() {
        let c = Config { extinction_floor: 1, ..cfg() };
        let (mut ants, mut cols) = setup(&c, &[(0, 1.0), (1, 1.0)]);
        let mut id = 2;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(5, 5));
        assert_eq!(ants.len(), 2);
        assert_eq!(cols[0].floor_spawns, 0);
    }

    #[test]
    fn a_colony_can_never_be_permanently_extinct() {
        let c = Config { extinction_floor: 3, floor_respawn_interval: 10, ..cfg() };
        let (mut ants, mut cols) = setup(&c, &[]);
        let mut id = 0;
        let mut rng = Pcg32::new(8, 8);
        for t in 0..100 {
            reproduce(&mut ants, &mut cols, &c, t, &mut id, &mut rng);
        }
        assert_eq!(ants.population(0), 3, "should have trickled back up to the floor");
    }

    #[test]
    fn ant_ids_stay_strictly_increasing_across_births() {
        let c = Config { extinction_floor: 2, max_births_per_tick: 3, ..cfg() };
        let (mut ants, mut cols) = setup(&c, &[(0, 1.0)]);
        cols[0].store = c.birth_cost * 10.0;
        cols[1].store = c.birth_cost * 10.0;
        let mut id = 1;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(6, 6));
        assert!(ants.id.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn reproduction_is_deterministic() {
        let c = Config { extinction_floor: 2, ..cfg() };
        let run = || {
            let (mut ants, mut cols) = setup(&c, &[(0, 4.0)]);
            cols[0].store = c.birth_cost * 3.0;
            let mut id = 1;
            reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(7, 7));
            (ants.len(), ants.genome.iter().map(|g| g.params[0]).collect::<Vec<_>>())
        };
        assert_eq!(run(), run());
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Add `pub mod reproduce;` to `lib.rs`.

Run: `cargo test -p sim`
Expected: FAIL — `cannot find function reproduce`.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/sim/src/reproduce.rs`:

```rust
use crate::ants::{Ants, Spawn};
use crate::colony::ColonyState;
use crate::config::Config;
use crate::genome::Genome;
use crate::rng::Pcg32;

pub const NEWBORN_SIZE: f32 = 0.5;
/// Newborns start partly fed so they get a few hundred ticks to find food.
pub const NEWBORN_ENERGY_FRAC: f32 = 0.6;

/// Top up colonies below the extinction floor, then spend food stores on
/// births. Colonies are processed in id order and ants pushed with increasing
/// ids, so the whole pass is reproducible from `rng` alone.
pub fn reproduce(
    ants: &mut Ants,
    colonies: &mut [ColonyState],
    cfg: &Config,
    tick: u64,
    next_id: &mut u64,
    rng: &mut Pcg32,
) {
    for ci in 0..colonies.len() {
        let cid = colonies[ci].id;

        // --- Extinction floor: at most ONE free ant per interval. ---
        //
        // Rate-limited on purpose. Topping a colony straight back up to the
        // floor in the same tick its ants die turns a besieged nest into an
        // energy fountain: an enemy camped on it kills and scavenges an endless
        // stream of free bodies. A slow trickle lets a colony rebuild without
        // subsidising its attacker.
        let below_floor = ants.population(cid) < cfg.extinction_floor;
        let interval_elapsed =
            tick >= colonies[ci].last_floor_spawn.saturating_add(cfg.floor_respawn_interval);
        if below_floor && (interval_elapsed || colonies[ci].floor_spawns == 0) {
            let genome = match colonies[ci].archive_parent(rng) {
                Some(g) => g.mutated(cfg, rng),
                None => Genome::random(rng),
            };
            let lineage = colonies[ci].next_lineage_hint.saturating_add(1);
            spawn_into(ants, &colonies[ci], cid, genome, lineage, cfg, tick, next_id, rng);
            colonies[ci].floor_spawns += 1;
            colonies[ci].last_floor_spawn = tick;
        }

        // --- Paid births from the food store. ---
        let mut births = 0;
        while colonies[ci].store >= cfg.birth_cost && births < cfg.max_births_per_tick {
            let Some(p) = colonies[ci].select_parent(ants, rng) else {
                break;
            };
            let genome = ants.genome[p].mutated(cfg, rng);
            let lineage = ants.lineage[p].saturating_add(1);
            colonies[ci].store -= cfg.birth_cost;
            colonies[ci].births += 1;
            births += 1;
            spawn_into(ants, &colonies[ci], cid, genome, lineage, cfg, tick, next_id, rng);
            colonies[ci].next_lineage_hint = colonies[ci].next_lineage_hint.max(lineage);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_into(
    ants: &mut Ants,
    colony: &ColonyState,
    cid: u8,
    genome: Genome,
    lineage: u32,
    cfg: &Config,
    tick: u64,
    next_id: &mut u64,
    rng: &mut Pcg32,
) {
    let (x, y) = if colony.nest_tiles.is_empty() {
        colony.nest_center
    } else {
        let k = rng.next_below(colony.nest_tiles.len() as u32) as usize;
        let cell = colony.nest_tiles[k];
        let w = cfg.width as usize;
        ((cell % w) as f32 + 0.5, (cell / w) as f32 + 0.5)
    };

    let energy = NEWBORN_ENERGY_FRAC * genome.max_energy(cfg, NEWBORN_SIZE);
    let heading = (rng.next_f32() * 2.0 - 1.0) * std::f32::consts::PI;

    ants.push(Spawn {
        id: *next_id,
        colony: cid,
        x,
        y,
        heading,
        energy,
        size: NEWBORN_SIZE,
        lineage,
        genome,
        birth_tick: tick,
    });
    *next_id += 1;
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p sim`
Expected: PASS, 134 tests.

- [ ] **Step 5: Commit**

```bash
git add crates/sim/src/reproduce.rs crates/sim/src/lib.rs
git commit -m "feat(sim): nest births, weighted parent selection, extinction floor"
```

---

### Task 16: `worldgen` — seeded terrain, food patches, nests

Terrain variety is a requirement, not decoration: a uniform map is the known cause of the "all colonies converge on one strategy" failure mode. Food patches sit at varying distances from the nests so different bets pay off in different places.

**Files:**
- Create: `crates/sim/src/worldgen.rs`
- Modify: `crates/sim/src/lib.rs`

**Interfaces:**
- Consumes: `Config`, `Grid`, `ColonyState`, `Pcg32`.
- Produces:
  - `pub const NEST_RADIUS: i32 = 1;` (a 3×3 nest)
  - `pub fn generate(cfg: &Config, rng: &mut Pcg32) -> (Grid, Vec<ColonyState>)`

- [ ] **Step 1: Write the failing tests**

Create `crates/sim/src/worldgen.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::grid::NO_NEST;
    use crate::rng::Pcg32;

    fn cfg() -> Config {
        Config { width: 128, height: 128, num_colonies: 4, food_patch_count: 8, ..Config::default() }
    }

    #[test]
    fn generation_is_deterministic_for_a_seed() {
        let c = cfg();
        let (g1, _) = generate(&c, &mut Pcg32::new(1, 1));
        let (g2, _) = generate(&c, &mut Pcg32::new(1, 1));
        assert_eq!(g1.stone, g2.stone);
        assert_eq!(g1.food, g2.food);
        assert_eq!(g1.nest, g2.nest);
    }

    #[test]
    fn different_seeds_give_different_maps() {
        let c = cfg();
        let (g1, _) = generate(&c, &mut Pcg32::new(1, 1));
        let (g2, _) = generate(&c, &mut Pcg32::new(2, 2));
        assert_ne!(g1.stone, g2.stone);
    }

    #[test]
    fn one_colony_state_per_configured_colony() {
        let c = cfg();
        let (_, colonies) = generate(&c, &mut Pcg32::new(1, 1));
        assert_eq!(colonies.len(), c.num_colonies as usize);
        for (i, col) in colonies.iter().enumerate() {
            assert_eq!(col.id, i as u8);
        }
    }

    #[test]
    fn every_colony_starts_with_a_full_store() {
        let c = cfg();
        let (_, colonies) = generate(&c, &mut Pcg32::new(1, 1));
        assert!(colonies.iter().all(|col| col.store == c.initial_food_store));
    }

    #[test]
    fn every_colony_has_nest_tiles_and_they_are_tagged_on_the_grid() {
        let c = cfg();
        let (grid, colonies) = generate(&c, &mut Pcg32::new(1, 1));
        for col in &colonies {
            assert!(!col.nest_tiles.is_empty());
            for &t in &col.nest_tiles {
                assert_eq!(grid.nest[t], col.id);
            }
        }
    }

    #[test]
    fn nests_are_never_stone() {
        let c = cfg();
        let (grid, colonies) = generate(&c, &mut Pcg32::new(1, 1));
        for col in &colonies {
            for &t in &col.nest_tiles {
                assert!(!grid.stone[t], "a nest tile was buried in stone");
            }
        }
    }

    #[test]
    fn nests_do_not_overlap() {
        let c = cfg();
        let (_, colonies) = generate(&c, &mut Pcg32::new(1, 1));
        let mut all: Vec<usize> = colonies.iter().flat_map(|c| c.nest_tiles.clone()).collect();
        let before = all.len();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), before);
    }

    #[test]
    fn some_food_exists_and_none_sits_on_stone() {
        let c = cfg();
        let (grid, _) = generate(&c, &mut Pcg32::new(1, 1));
        let total: f32 = grid.food.iter().sum();
        assert!(total > 0.0, "map has no food at all");
        for i in 0..grid.food.len() {
            if grid.stone[i] {
                assert_eq!(grid.food[i], 0.0);
            }
        }
    }

    #[test]
    fn food_never_exceeds_the_patch_maximum() {
        let c = cfg();
        let (grid, _) = generate(&c, &mut Pcg32::new(1, 1));
        assert!(grid.food.iter().all(|f| *f <= c.food_patch_max + 1e-3));
    }

    #[test]
    fn each_colony_has_food_within_reach_of_its_nest() {
        // Guards the "every colony dies in the first minute" failure mode.
        let c = cfg();
        let (grid, colonies) = generate(&c, &mut Pcg32::new(1, 1));
        for col in &colonies {
            let (nx, ny) = col.nest_center;
            let near: f32 = (0..grid.food.len())
                .filter(|&i| {
                    let (x, y) = ((i % grid.width as usize) as f32, (i / grid.width as usize) as f32);
                    (x - nx).hypot(y - ny) < SEED_PATCH_DISTANCE + c.food_patch_radius
                })
                .map(|i| grid.food[i])
                .sum();
            assert!(near > 0.0, "colony {} has no food near its nest", col.id);
        }
    }

    #[test]
    fn the_map_has_some_stone_but_is_not_a_wall() {
        let c = cfg();
        let (grid, _) = generate(&c, &mut Pcg32::new(1, 1));
        let stones = grid.stone.iter().filter(|s| **s).count();
        let frac = stones as f32 / grid.stone.len() as f32;
        assert!(frac > 0.01, "no terrain variety: {frac}");
        assert!(frac < 0.30, "map is mostly wall: {frac}");
    }

    #[test]
    fn stone_coverage_is_independent_of_map_size() {
        // A fixed blob count would bury the small worlds the tests use while
        // barely speckling the real 512x512 map.
        let frac_at = |side: u16| {
            let c = Config { width: side, height: side, num_colonies: 2, ..cfg() };
            let (grid, _) = generate(&c, &mut Pcg32::new(1, 1));
            grid.stone.iter().filter(|s| **s).count() as f32 / grid.stone.len() as f32
        };
        for side in [64u16, 128, 256] {
            let f = frac_at(side);
            assert!(
                (0.01..0.30).contains(&f),
                "{side}x{side} map has {f} stone coverage, outside the workable band"
            );
        }
    }

    #[test]
    fn a_tiny_test_world_still_gets_at_least_one_blob() {
        let c = Config { width: 32, height: 32, num_colonies: 1, ..cfg() };
        let (grid, _) = generate(&c, &mut Pcg32::new(1, 1));
        assert!(grid.stone.iter().any(|s| *s));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Add `pub mod worldgen;` to `lib.rs`.

Run: `cargo test -p sim`
Expected: FAIL — `cannot find function generate`.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/sim/src/worldgen.rs`:

```rust
use crate::colony::ColonyState;
use crate::config::Config;
use crate::grid::Grid;
use crate::rng::Pcg32;

/// Nests are 3x3 blocks: big enough that returning foragers do not queue,
/// small enough to be a real place on the map.
pub const NEST_RADIUS: i32 = 1;
/// Each colony gets one guaranteed food patch this far from its nest, so no
/// colony starts in a barren corner. The rest are scattered.
///
/// Kept short deliberately: the round trip must pay for itself at mean traits
/// (see the break-even note on `Config`), and the nest scent gradient has to
/// still be readable at this range (see `tests/gradient.rs`).
pub const SEED_PATCH_DISTANCE: f32 = 12.0;
/// Colonies are placed on a circle at this fraction of the map's half-width.
const NEST_RING_FRAC: f32 = 0.72;

/// How many stone blobs to stamp for a given map, from a target coverage.
///
/// A fixed blob *count* does not survive changing the map size: 60 blobs is 3%
/// of a 512x512 map and more than 100% of the 48x48 worlds the tests use, which
/// would bury every test colony in solid rock and make the behavioural tests
/// fail for terrain reasons while pointing at the economy.
///
/// Mean blob radius is `radius * (0.4 + E[U(0,1)]) = 0.9 * radius`, so mean
/// area is `PI * (0.9r)^2 ~= 2.54 r^2`. Overlap means realised coverage lands a
/// little under the target, which is fine.
fn stone_blob_count(cfg: &Config) -> u32 {
    let mean_blob_area = 2.54 * cfg.stone_blob_radius * cfg.stone_blob_radius;
    let target_cells = cfg.stone_density * cfg.cell_count() as f32;
    ((target_cells / mean_blob_area).round() as u32).max(1)
}

pub fn generate(cfg: &Config, rng: &mut Pcg32) -> (Grid, Vec<ColonyState>) {
    let mut grid = Grid::new(cfg);
    let w = cfg.width as f32;
    let h = cfg.height as f32;
    let (cxm, cym) = (w * 0.5, h * 0.5);

    // --- Stone blobs: chokepoints, so different regions reward different bets.
    for _ in 0..stone_blob_count(cfg) {
        let bx = rng.next_f32() * w;
        let by = rng.next_f32() * h;
        let r = cfg.stone_blob_radius * (0.4 + rng.next_f32());
        stamp(&mut grid, bx, by, r, |g, i| g.stone[i] = true);
    }

    // --- Colonies on a ring, evenly spaced. ---
    let mut colonies = Vec::with_capacity(cfg.num_colonies as usize);
    let ring = cxm.min(cym) * NEST_RING_FRAC;
    for id in 0..cfg.num_colonies {
        let theta = std::f32::consts::TAU * id as f32 / cfg.num_colonies as f32;
        let nx = cxm + ring * theta.cos();
        let ny = cym + ring * theta.sin();

        let mut col = ColonyState::new(id);
        col.store = cfg.initial_food_store;
        col.nest_center = (nx, ny);

        let (ix, iy) = (nx as i32, ny as i32);
        for dy in -NEST_RADIUS..=NEST_RADIUS {
            for dx in -NEST_RADIUS..=NEST_RADIUS {
                let (x, y) = (ix + dx, iy + dy);
                if !grid.in_bounds(x, y) {
                    continue;
                }
                let i = grid.idx_clamped(x, y);
                // A nest is never stone, and never steals another colony's tile.
                if grid.nest[i] == crate::grid::NO_NEST {
                    grid.stone[i] = false;
                    grid.food[i] = 0.0;
                    grid.nest[i] = id;
                    col.nest_tiles.push(i);
                }
            }
        }

        // One guaranteed patch within foraging reach of this nest.
        let a = rng.next_f32() * std::f32::consts::TAU;
        let px = (nx + SEED_PATCH_DISTANCE * a.cos()).clamp(1.0, w - 2.0);
        let py = (ny + SEED_PATCH_DISTANCE * a.sin()).clamp(1.0, h - 2.0);
        food_patch(&mut grid, px, py, cfg);

        colonies.push(col);
    }

    // --- Scattered patches at varied distances. ---
    for _ in 0..cfg.food_patch_count {
        let px = rng.next_f32() * w;
        let py = rng.next_f32() * h;
        food_patch(&mut grid, px, py, cfg);
    }

    (grid, colonies)
}

fn food_patch(grid: &mut Grid, px: f32, py: f32, cfg: &Config) {
    let r = cfg.food_patch_radius;
    let maxf = cfg.food_patch_max;
    stamp(grid, px, py, r, |g, i| {
        if !g.stone[i] && g.nest[i] == crate::grid::NO_NEST {
            g.food[i] = maxf;
        }
    });
}

fn stamp(grid: &mut Grid, cx: f32, cy: f32, r: f32, mut f: impl FnMut(&mut Grid, usize)) {
    let (x0, x1) = ((cx - r) as i32, (cx + r) as i32);
    let (y0, y1) = ((cy - r) as i32, (cy + r) as i32);
    for y in y0..=y1 {
        for x in x0..=x1 {
            if !grid.in_bounds(x, y) {
                continue;
            }
            if (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy) <= r {
                let i = grid.idx_clamped(x, y);
                f(grid, i);
            }
        }
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p sim`
Expected: PASS, 145 tests.

If the stone-coverage tests fail, tune `Config::stone_density` (and, if blobs look wrong, `stone_blob_radius`) rather than the derived blob count. Realised coverage runs slightly under `stone_density` because blobs overlap; at the default 0.06 expect roughly 5–6%.

- [ ] **Step 5: Commit**

```bash
git add crates/sim/src/worldgen.rs crates/sim/src/lib.rs
git commit -m "feat(sim): seeded worldgen with stone chokepoints and reachable food"
```

---

### Task 17: `World::tick` — the three-phase loop, plus stats

This is where parallel-think / serial-apply actually happens, and it is the only place `rayon` appears.

Two support changes ride along, both discovered by this task's needs:
- `Grid` gains a `fertility` field. Without it, a patch harvested to zero can never regrow — the map becomes a dead husk, which the spec forbids. Regrowth is toward fertility, not from remaining food.
- `Spatial` gains `Default` and `resize`, so `World` can `#[serde(skip)]` it and rebuild after a snapshot load rather than serialising a derived index.

**Files:**
- Modify: `crates/sim/src/grid.rs` (add `fertility`)
- Modify: `crates/sim/src/worldgen.rs` (set `fertility` on patches)
- Modify: `crates/sim/src/spatial.rs` (add `Default`, `resize`)
- Create: `crates/sim/src/stats.rs`
- Create: `crates/sim/src/world.rs`
- Modify: `crates/sim/src/lib.rs`

**Interfaces:**
- Produces:
  - `Grid { .., pub fertility: Vec<f32> }` and `Grid::regrow(&mut self, rate: f32)`
  - `impl Default for Spatial` and `Spatial::resize(&mut self, cfg: &Config)`
  - `pub struct ColonyStats { pub id: u8, pub population: u32, pub store: f32, pub births: u64, pub deaths: u64, pub floor_spawns: u64, pub mean_size: f32, pub mean_lineage: f32, pub food_delivered: f32 }` (`Clone`, `Debug`, `Serialize`)
  - `pub fn colony_stats(ants: &Ants, colonies: &[ColonyState]) -> Vec<ColonyStats>`
  - `pub struct World { pub cfg: Config, pub tick_count: u64, pub grid: Grid, pub phero: Pheromones, pub ants: Ants, pub colonies: Vec<ColonyState>, pub rng: Pcg32, pub next_id: u64 }` (`Clone`, `Serialize`, `Deserialize`)
  - `World::new(cfg: &Config, seed: u64) -> World`
  - `World::tick(&mut self)`
  - `World::stats(&self) -> Vec<ColonyStats>`
  - `World::state_hash(&self) -> u64`
  - `World::rebuild_index(&mut self)`

- [ ] **Step 1: Add `fertility` to `Grid` with its test**

In `crates/sim/src/grid.rs`, add the field to the struct and to `Grid::new` (`fertility: vec![0.0; n]`), then add:

```rust
impl Grid {
    /// Regrow toward each cell's fertility. Cells with zero fertility (dirt,
    /// stone, nests) never grow food, so a harvested patch recovers but the
    /// rest of the map does not sprout.
    pub fn regrow(&mut self, rate: f32) {
        for i in 0..self.food.len() {
            if self.fertility[i] > 0.0 && self.food[i] < self.fertility[i] {
                self.food[i] = (self.food[i] + rate).min(self.fertility[i]);
            }
        }
    }
}
```

And its tests, appended to `grid.rs`'s `mod tests`:

```rust
    #[test]
    fn a_depleted_patch_regrows_toward_its_fertility() {
        let mut g = Grid::new(&small());
        let i = g.idx(2, 2);
        g.fertility[i] = 10.0;
        g.food[i] = 0.0;
        g.regrow(3.0);
        assert_eq!(g.food[i], 3.0);
        g.regrow(100.0);
        assert_eq!(g.food[i], 10.0, "never exceeds fertility");
    }

    #[test]
    fn barren_ground_never_sprouts() {
        let mut g = Grid::new(&small());
        g.regrow(5.0);
        assert!(g.food.iter().all(|f| *f == 0.0));
    }
```

In `crates/sim/src/worldgen.rs`, inside `food_patch`'s closure, set fertility alongside food:

```rust
        if !g.stone[i] && g.nest[i] == crate::grid::NO_NEST {
            g.food[i] = maxf;
            g.fertility[i] = maxf;
        }
```

- [ ] **Step 2: Add `Default` and `resize` to `Spatial`**

In `crates/sim/src/spatial.rs`:

```rust
impl Default for Spatial {
    fn default() -> Self {
        Spatial { width: 0, height: 0, cell_start: vec![0], items: Vec::new(), occupant: Vec::new() }
    }
}

impl Spatial {
    /// Re-shape an index for a given config. Used after loading a snapshot,
    /// where the index is rebuilt rather than serialised.
    pub fn resize(&mut self, cfg: &Config) {
        *self = Spatial::new(cfg);
    }
}
```

- [ ] **Step 3: Run the grid tests**

Run: `cargo test -p sim grid`
Expected: PASS, including the two new regrow tests.

- [ ] **Step 4: Write the failing stats test**

Create `crates/sim/src/stats.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ants::{Ants, Spawn};
    use crate::genome::Genome;
    use crate::rng::Pcg32;

    fn ants_with(rows: &[(u8, f32, u32, f32)]) -> Ants {
        let mut a = Ants::new();
        for (i, (c, size, lineage, delivered)) in rows.iter().enumerate() {
            a.push(Spawn {
                id: i as u64, colony: *c, x: 0.0, y: 0.0, heading: 0.0,
                energy: 1.0, size: *size, lineage: *lineage,
                genome: Genome::random(&mut Pcg32::new(i as u64, 1)), birth_tick: 0,
            });
            a.food_delivered[i] = *delivered;
        }
        a
    }

    #[test]
    fn stats_are_per_colony_and_in_id_order() {
        let ants = ants_with(&[(0, 1.0, 2, 5.0), (1, 3.0, 4, 7.0)]);
        let cols: Vec<ColonyState> = (0..2).map(ColonyState::new).collect();
        let s = colony_stats(&ants, &cols);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].id, 0);
        assert_eq!(s[1].id, 1);
        assert_eq!(s[0].population, 1);
    }

    #[test]
    fn mean_lineage_is_the_generation_counter() {
        let ants = ants_with(&[(0, 1.0, 2, 0.0), (0, 1.0, 6, 0.0)]);
        let cols: Vec<ColonyState> = (0..1).map(ColonyState::new).collect();
        assert_eq!(colony_stats(&ants, &cols)[0].mean_lineage, 4.0);
    }

    #[test]
    fn mean_size_averages_the_living_only() {
        let mut ants = ants_with(&[(0, 1.0, 0, 0.0), (0, 3.0, 0, 0.0)]);
        ants.alive[1] = false;
        let cols: Vec<ColonyState> = (0..1).map(ColonyState::new).collect();
        assert_eq!(colony_stats(&ants, &cols)[0].mean_size, 1.0);
    }

    #[test]
    fn an_empty_colony_reports_zeroes_not_nan() {
        let ants = ants_with(&[]);
        let cols: Vec<ColonyState> = (0..1).map(ColonyState::new).collect();
        let s = &colony_stats(&ants, &cols)[0];
        assert_eq!(s.population, 0);
        assert_eq!(s.mean_size, 0.0);
        assert_eq!(s.mean_lineage, 0.0);
    }

    #[test]
    fn food_delivered_sums_across_the_colony() {
        let ants = ants_with(&[(0, 1.0, 0, 5.0), (0, 1.0, 0, 7.0)]);
        let cols: Vec<ColonyState> = (0..1).map(ColonyState::new).collect();
        assert_eq!(colony_stats(&ants, &cols)[0].food_delivered, 12.0);
    }

    #[test]
    fn floor_spawns_are_reported_so_life_support_is_visible() {
        let ants = ants_with(&[(0, 1.0, 0, 0.0)]);
        let mut cols: Vec<ColonyState> = (0..1).map(ColonyState::new).collect();
        cols[0].floor_spawns = 17;
        assert_eq!(colony_stats(&ants, &cols)[0].floor_spawns, 17);
    }
}
```

- [ ] **Step 5: Implement stats**

Prepend to `crates/sim/src/stats.rs`:

```rust
use crate::ants::Ants;
use crate::colony::ColonyState;
use serde::Serialize;

/// One row per colony, in colony-id order. `mean_lineage` is the "generation"
/// number: nothing resets, so it rises smoothly.
#[derive(Clone, Debug, Serialize)]
pub struct ColonyStats {
    pub id: u8,
    pub population: u32,
    pub store: f32,
    pub births: u64,
    pub deaths: u64,
    /// Free ants granted by the extinction floor. A colony whose population is
    /// held up by this number is on life support, not thriving. Reported so the
    /// simulation never silently flatters a losing colony.
    pub floor_spawns: u64,
    pub mean_size: f32,
    pub mean_lineage: f32,
    pub food_delivered: f32,
}

pub fn colony_stats(ants: &Ants, colonies: &[ColonyState]) -> Vec<ColonyStats> {
    colonies
        .iter()
        .map(|c| {
            let mut population = 0u32;
            let (mut size_sum, mut lineage_sum, mut delivered) = (0.0f32, 0.0f32, 0.0f32);
            for i in 0..ants.len() {
                if ants.alive[i] && ants.colony[i] == c.id {
                    population += 1;
                    size_sum += ants.size[i];
                    lineage_sum += ants.lineage[i] as f32;
                    delivered += ants.food_delivered[i];
                }
            }
            let n = population.max(1) as f32;
            ColonyStats {
                id: c.id,
                population,
                store: c.store,
                births: c.births,
                deaths: c.deaths,
                floor_spawns: c.floor_spawns,
                mean_size: if population == 0 { 0.0 } else { size_sum / n },
                mean_lineage: if population == 0 { 0.0 } else { lineage_sum / n },
                food_delivered: delivered,
            }
        })
        .collect()
}
```

- [ ] **Step 6: Write the failing world tests**

Create `crates/sim/src/world.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn small() -> Config {
        Config {
            width: 64,
            height: 64,
            num_colonies: 2,
            initial_ants_per_colony: 10,
            food_patch_count: 6,
            ..Config::default()
        }
    }

    #[test]
    fn a_new_world_seeds_every_colony_with_ants() {
        let w = World::new(&small(), 1);
        assert_eq!(w.ants.len(), 20);
        assert_eq!(w.ants.population(0), 10);
        assert_eq!(w.ants.population(1), 10);
    }

    #[test]
    fn a_new_worlds_ant_ids_are_strictly_increasing() {
        let w = World::new(&small(), 1);
        assert!(w.ants.id.windows(2).all(|p| p[0] < p[1]));
    }

    #[test]
    fn ticking_advances_the_counter() {
        let mut w = World::new(&small(), 1);
        w.tick();
        w.tick();
        assert_eq!(w.tick_count, 2);
    }

    #[test]
    fn ticking_keeps_ant_ids_sorted() {
        let mut w = World::new(&small(), 1);
        for _ in 0..200 {
            w.tick();
        }
        assert!(w.ants.id.windows(2).all(|p| p[0] < p[1]));
    }

    #[test]
    fn nests_beacon_their_own_colony_scent() {
        let mut w = World::new(&small(), 1);
        w.tick();
        for c in &w.colonies {
            let t = c.nest_tiles[0];
            assert!(w.phero.scent[t] > 0.0);
            assert_eq!(w.phero.owner[t], c.id);
        }
    }

    #[test]
    fn no_ant_ever_stands_on_stone() {
        let mut w = World::new(&small(), 1);
        for _ in 0..300 {
            w.tick();
            for i in 0..w.ants.len() {
                let (x, y) = w.ants.cell(i);
                assert!(!w.grid.stone[w.grid.idx(x, y)], "ant {i} is inside a rock");
            }
        }
    }

    #[test]
    fn every_ant_stays_on_the_map() {
        let mut w = World::new(&small(), 1);
        for _ in 0..300 {
            w.tick();
            for i in 0..w.ants.len() {
                assert!(w.grid.in_bounds(w.ants.x[i] as i32, w.ants.y[i] as i32));
            }
        }
    }

    #[test]
    fn no_colony_ever_goes_permanently_extinct() {
        // The floor is rate-limited, so a colony CAN dip below it — even to
        // zero — for up to `floor_respawn_interval` ticks. What it may not do
        // is stay there.
        let mut w = World::new(&small(), 7);
        let mut ticks_at_zero = vec![0u64; w.cfg.num_colonies as usize];
        for _ in 0..5000 {
            w.tick();
            for id in 0..w.cfg.num_colonies {
                if w.ants.population(id) == 0 {
                    ticks_at_zero[id as usize] += 1;
                } else {
                    ticks_at_zero[id as usize] = 0;
                }
                assert!(
                    ticks_at_zero[id as usize] <= w.cfg.floor_respawn_interval + 1,
                    "colony {id} stayed extinct past the respawn interval"
                );
            }
        }
    }

    #[test]
    fn state_never_goes_non_finite() {
        let mut w = World::new(&small(), 3);
        for _ in 0..500 {
            w.tick();
        }
        for i in 0..w.ants.len() {
            assert!(w.ants.x[i].is_finite() && w.ants.y[i].is_finite());
            assert!(w.ants.energy[i].is_finite());
            assert!(w.ants.size[i].is_finite());
        }
        assert!(w.phero.food.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn stats_report_one_row_per_colony() {
        let w = World::new(&small(), 1);
        assert_eq!(w.stats().len(), 2);
    }

    #[test]
    fn state_hash_is_stable_for_an_unchanged_world() {
        let w = World::new(&small(), 1);
        assert_eq!(w.state_hash(), w.state_hash());
    }

    #[test]
    fn state_hash_changes_when_the_world_ticks() {
        let mut w = World::new(&small(), 1);
        let before = w.state_hash();
        w.tick();
        assert_ne!(before, w.state_hash());
    }
}
```

- [ ] **Step 7: Run to verify they fail**

Add `pub mod stats;` and `pub mod world;` to `lib.rs`.

Run: `cargo test -p sim`
Expected: FAIL — `cannot find type World`.

- [ ] **Step 8: Implement `World`**

Prepend to `crates/sim/src/world.rs`:

```rust
use crate::ants::{Ants, Spawn};
use crate::apply::{
    apply_combat, apply_food, apply_metabolism, apply_movement, apply_nest, deposit_passive,
    sweep_deaths, ApplyCtx,
};
use crate::colony::ColonyState;
use crate::config::Config;
use crate::genome::Genome;
use crate::grid::Grid;
use crate::intent::{think, Intent};
use crate::pheromone::Pheromones;
use crate::reproduce::reproduce;
use crate::rng::Pcg32;
use crate::spatial::Spatial;
use crate::stats::{colony_stats, ColonyStats};
use crate::worldgen::generate;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct World {
    pub cfg: Config,
    pub tick_count: u64,
    pub grid: Grid,
    pub phero: Pheromones,
    pub ants: Ants,
    pub colonies: Vec<ColonyState>,
    /// Drives births and worldgen only. Ants draw from their own streams.
    pub rng: Pcg32,
    pub next_id: u64,

    /// Derived each tick; never serialised.
    #[serde(skip)]
    spatial: Spatial,
}

impl World {
    pub fn new(cfg: &Config, seed: u64) -> Self {
        let mut rng = Pcg32::new(seed, 0xA17);
        let (grid, colonies) = generate(cfg, &mut rng);

        let mut ants = Ants::new();
        let mut next_id = 0u64;
        for col in &colonies {
            for _ in 0..cfg.initial_ants_per_colony {
                let genome = Genome::random(&mut rng);
                let k = rng.next_below(col.nest_tiles.len() as u32) as usize;
                let cell = col.nest_tiles[k];
                let w = cfg.width as usize;
                ants.push(Spawn {
                    id: next_id,
                    colony: col.id,
                    x: (cell % w) as f32 + 0.5,
                    y: (cell / w) as f32 + 0.5,
                    heading: (rng.next_f32() * 2.0 - 1.0) * std::f32::consts::PI,
                    // Generous starting energy: founders start completely full,
                    // because the first generation must survive long enough for
                    // selection to have anything to act on. (Newborns get only
                    // `NEWBORN_ENERGY_FRAC` of theirs; see `reproduce`.)
                    energy: genome.max_energy(cfg, 1.0),
                    size: 1.0,
                    lineage: 0,
                    genome,
                    birth_tick: 0,
                });
                next_id += 1;
            }
        }

        let mut w = World {
            cfg: cfg.clone(),
            tick_count: 0,
            grid,
            phero: Pheromones::new(cfg),
            ants,
            colonies,
            rng,
            next_id,
            spatial: Spatial::new(cfg),
        };
        w.rebuild_index();
        w
    }

    /// Rebuild the derived spatial index. Call after deserialising a snapshot.
    pub fn rebuild_index(&mut self) {
        if self.spatial.cell_count() != self.cfg.cell_count() {
            self.spatial.resize(&self.cfg);
        }
        self.spatial.rebuild(&self.ants);
    }

    pub fn tick(&mut self) {
        self.spatial.rebuild(&self.ants);

        // --- Phase 1: parallel, read-only. Cannot race by construction. ---
        let intents: Vec<Intent> = (0..self.ants.len())
            .into_par_iter()
            .map(|i| {
                if self.ants.alive[i] {
                    think(i, &self.ants, &self.grid, &self.phero, &self.spatial, &self.cfg)
                } else {
                    Intent {
                        heading: 0.0,
                        speed: 0.0,
                        attack: false,
                        grab: false,
                        release: false,
                        memory: [0.0; crate::N_MEMORY],
                    }
                }
            })
            .collect();

        // --- Phase 2: serial, in ant-id order. ---
        {
            let mut ctx = ApplyCtx {
                cfg: &self.cfg,
                grid: &mut self.grid,
                phero: &mut self.phero,
                spatial: &mut self.spatial,
                colonies: &mut self.colonies,
            };
            for i in 0..self.ants.len() {
                if !self.ants.alive[i] {
                    continue;
                }
                apply_movement(i, &intents[i], &mut self.ants, &mut ctx);
                apply_food(i, &intents[i], &mut self.ants, &mut ctx);
                apply_nest(i, &mut self.ants, &mut ctx);

                let (cx, cy) = self.ants.cell(i);
                let cell = ctx.grid.idx(cx, cy);
                deposit_passive(cell, self.ants.carrying[i], self.ants.colony[i], &mut ctx);

                apply_combat(i, &intents[i], &mut self.ants, &mut ctx);
                apply_metabolism(i, &mut self.ants, ctx.cfg);
            }
            sweep_deaths(&mut self.ants, &mut ctx);
        }
        self.ants.retain_alive();

        // Nests beacon their scent: the gradient ants climb to get home.
        for col in &self.colonies {
            for &t in &col.nest_tiles {
                self.phero.deposit_scent(t, self.cfg.nest_scent_emission, col.id);
            }
        }

        reproduce(
            &mut self.ants,
            &mut self.colonies,
            &self.cfg,
            self.tick_count,
            &mut self.next_id,
            &mut self.rng,
        );

        // --- Phase 3: fields. ---
        self.phero.step(&self.cfg);
        self.grid.regrow(self.cfg.food_regrow);

        self.tick_count += 1;
    }

    pub fn stats(&self) -> Vec<ColonyStats> {
        colony_stats(&self.ants, &self.colonies)
    }

    /// FNV-1a over the state that a tick can change. Used by the determinism
    /// tests; iterates in a fixed order, so it is thread-count independent.
    pub fn state_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut eat = |bytes: &[u8]| {
            for b in bytes {
                h ^= *b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        };
        eat(&self.tick_count.to_le_bytes());
        for i in 0..self.ants.len() {
            eat(&self.ants.id[i].to_le_bytes());
            eat(&[self.ants.colony[i]]);
            eat(&self.ants.x[i].to_bits().to_le_bytes());
            eat(&self.ants.y[i].to_bits().to_le_bytes());
            eat(&self.ants.heading[i].to_bits().to_le_bytes());
            eat(&self.ants.energy[i].to_bits().to_le_bytes());
            eat(&self.ants.size[i].to_bits().to_le_bytes());
            eat(&self.ants.carrying[i].to_bits().to_le_bytes());
            eat(&self.ants.food_delivered[i].to_bits().to_le_bytes());
        }
        for c in &self.colonies {
            eat(&c.store.to_bits().to_le_bytes());
            eat(&c.births.to_le_bytes());
            eat(&c.deaths.to_le_bytes());
        }
        for v in &self.phero.food {
            eat(&v.to_bits().to_le_bytes());
        }
        for v in &self.grid.food {
            eat(&v.to_bits().to_le_bytes());
        }
        h
    }
}
```

Also add to `spatial.rs`:

```rust
impl Spatial {
    pub fn cell_count(&self) -> usize {
        self.occupant.len()
    }
}
```

Note the import list omits `NEWBORN_ENERGY_FRAC` — founders start full, newborns do not, and only `reproduce` needs that constant.

- [ ] **Step 9: Run the full suite**

Run: `cargo test -p sim`
Expected: PASS. `no_colony_ever_goes_permanently_extinct` and `no_ant_ever_stands_on_stone` are the two that would expose a broken tick.

- [ ] **Step 10: Check it is not pathologically slow before moving on**

```bash
cargo test -p sim --release -- --nocapture no_colony_ever_goes_permanently_extinct
```

Expected: completes in a few seconds. If a 64×64 world with 20 ants takes minutes, stop and profile — something is quadratic.

- [ ] **Step 11: Commit**

```bash
git add crates/sim/src
git commit -m "feat(sim): World::tick with parallel think and serial apply"
```

---

### Task 18: Determinism, tested against thread count

The determinism guarantee is worth nothing unless a test would notice it breaking. This machine has 16 cores, so `rayon` defaults to 16 threads; running the same world on 1 and on 16 must produce the identical hash.

**Files:**
- Create: `crates/sim/tests/determinism.rs`
- Modify: `crates/sim/Cargo.toml` (add `rayon` as a dev-dependency usage is already there)

**Interfaces:**
- Consumes: `World::new`, `World::tick`, `World::state_hash`.
- Produces: nothing; this is a guard.

- [ ] **Step 1: Write the failing test**

Create `crates/sim/tests/determinism.rs`:

```rust
use sim::config::Config;
use sim::world::World;

fn small() -> Config {
    Config { width: 64, height: 64, num_colonies: 3, initial_ants_per_colony: 20, ..Config::default() }
}

fn run(seed: u64, ticks: u32) -> u64 {
    let mut w = World::new(&small(), seed);
    for _ in 0..ticks {
        w.tick();
    }
    w.state_hash()
}

fn run_on(threads: usize, seed: u64, ticks: u32) -> u64 {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap()
        .install(|| run(seed, ticks))
}

#[test]
fn same_seed_same_state() {
    assert_eq!(run(42, 500), run(42, 500));
}

#[test]
fn different_seeds_diverge() {
    assert_ne!(run(42, 500), run(43, 500));
}

#[test]
fn thread_count_does_not_change_the_outcome() {
    let one = run_on(1, 7, 1000);
    let many = run_on(16, 7, 1000);
    assert_eq!(one, many, "the parallel think phase is writing somewhere it must not");
}

#[test]
fn a_long_run_stays_deterministic() {
    assert_eq!(run_on(2, 99, 10_000), run_on(13, 99, 10_000));
}
```

- [ ] **Step 2: Run to verify the first three pass and the last is slow**

Run: `cargo test -p sim --release --test determinism`
Expected: PASS, 4 tests. If `thread_count_does_not_change_the_outcome` fails, there is a write in the think phase or an unordered iteration — fix it now, before any more behavior lands on top.

- [ ] **Step 3: Commit**

```bash
git add crates/sim/tests/determinism.rs
git commit -m "test(sim): determinism is invariant to thread count"
```

---

### Task 19: Snapshots and the golden master

`sim` performs no I/O: `save` returns bytes, `load` takes bytes. The *caller* touches the filesystem.

**Files:**
- Create: `crates/sim/src/snapshot.rs`
- Create: `crates/sim/tests/snapshot.rs`
- Create: `crates/sim/tests/golden.rs`
- Create (generated): `crates/sim/tests/golden_master.bin`
- Modify: `crates/sim/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub fn save(world: &World) -> Result<Vec<u8>, SnapshotError>`
  - `pub fn load(bytes: &[u8]) -> Result<World, SnapshotError>` — rebuilds the spatial index before returning
  - `pub enum SnapshotError { Encode(String), Decode(String) }` (`Debug`, `Display`, `std::error::Error`)

- [ ] **Step 1: Write the failing snapshot tests**

Create `crates/sim/tests/snapshot.rs`:

```rust
use sim::config::Config;
use sim::snapshot::{load, save};
use sim::world::World;

fn small() -> Config {
    Config { width: 48, height: 48, num_colonies: 2, initial_ants_per_colony: 15, ..Config::default() }
}

#[test]
fn a_snapshot_round_trips_to_an_identical_world() {
    let mut w = World::new(&small(), 5);
    for _ in 0..200 {
        w.tick();
    }
    let bytes = save(&w).unwrap();
    let w2 = load(&bytes).unwrap();
    assert_eq!(w.state_hash(), w2.state_hash());
}

#[test]
fn a_loaded_world_ticks_identically_to_the_original() {
    let mut a = World::new(&small(), 6);
    for _ in 0..100 {
        a.tick();
    }
    let mut b = load(&save(&a).unwrap()).unwrap();
    for _ in 0..100 {
        a.tick();
        b.tick();
    }
    assert_eq!(a.state_hash(), b.state_hash(), "the rng or the spatial index did not survive");
}

#[test]
fn garbage_bytes_are_an_error_not_a_panic() {
    assert!(load(&[0xde, 0xad, 0xbe, 0xef]).is_err());
}

#[test]
fn an_empty_buffer_is_an_error() {
    assert!(load(&[]).is_err());
}
```

`a_loaded_world_ticks_identically_to_the_original` is the one that earns its keep: it fails if `rebuild_index` is forgotten, or if the RNG state is not serialised.

- [ ] **Step 2: Run to verify it fails**

Add `pub mod snapshot;` to `lib.rs`.

Run: `cargo test -p sim --test snapshot`
Expected: FAIL — unresolved import `sim::snapshot`.

- [ ] **Step 3: Implement snapshot**

Create `crates/sim/src/snapshot.rs`:

```rust
use crate::world::World;
use std::fmt;

#[derive(Debug)]
pub enum SnapshotError {
    Encode(String),
    Decode(String),
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapshotError::Encode(e) => write!(f, "failed to encode snapshot: {e}"),
            SnapshotError::Decode(e) => write!(f, "failed to decode snapshot: {e}"),
        }
    }
}

impl std::error::Error for SnapshotError {}

pub fn save(world: &World) -> Result<Vec<u8>, SnapshotError> {
    bincode::serialize(world).map_err(|e| SnapshotError::Encode(e.to_string()))
}

/// The spatial index is derived, not stored, so it is rebuilt here. Forgetting
/// this yields a world that looks right and then behaves wrong on the next tick.
pub fn load(bytes: &[u8]) -> Result<World, SnapshotError> {
    let mut world: World =
        bincode::deserialize(bytes).map_err(|e| SnapshotError::Decode(e.to_string()))?;
    world.rebuild_index();
    Ok(world)
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p sim --test snapshot`
Expected: PASS, 4 tests.

- [ ] **Step 5: Write the golden-master test**

Create `crates/sim/tests/golden.rs`:

```rust
//! Golden master: pins the exact physics of the tick.
//!
//! This test WILL fail whenever you intentionally change a simulation rule or a
//! default in `Config`. That is the point. To accept the new behaviour:
//!
//!     REGENERATE_GOLDEN=1 cargo test -p sim --test golden
//!
//! Then review the diff on `golden_master.bin` in your commit — a changed
//! fixture is a claim that you meant to change the simulation.
//!
//! # This fixture pins the platform, not just the code
//!
//! `tanh`, `ln`, `sin`, and `cos` are libm calls, and their final-ULP results
//! differ across operating systems and architectures. The determinism tests
//! guarantee that a given *machine* reproduces itself across thread counts;
//! nothing guarantees an aarch64 Mac and an x86_64 Linux box agree bit for bit.
//!
//! So: **regenerate this fixture when you move to a new platform**, and do not
//! read a failure on a fresh CI runner as a physics regression until you have
//! confirmed the same binary passes on the machine that generated it.

use sim::config::Config;
use sim::snapshot::{load, save};
use sim::world::World;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden_master.bin");

fn cfg() -> Config {
    Config { width: 32, height: 32, num_colonies: 2, initial_ants_per_colony: 8, food_patch_count: 3, ..Config::default() }
}

fn advanced_world() -> World {
    let mut w = World::new(&cfg(), 2024);
    for _ in 0..1_000 {
        w.tick();
    }
    w
}

#[test]
fn the_tick_still_produces_the_recorded_world() {
    let w = advanced_world();

    if std::env::var("REGENERATE_GOLDEN").is_ok() {
        std::fs::write(FIXTURE, save(&w).unwrap()).unwrap();
        eprintln!("regenerated {FIXTURE}");
        return;
    }

    let bytes = std::fs::read(FIXTURE)
        .unwrap_or_else(|e| panic!("missing fixture {FIXTURE}: {e}. Run with REGENERATE_GOLDEN=1"));
    let expected = load(&bytes).unwrap();

    assert_eq!(
        w.state_hash(),
        expected.state_hash(),
        "the simulation's physics changed. If intentional, regenerate with REGENERATE_GOLDEN=1"
    );
}
```

- [ ] **Step 6: Generate the fixture, then verify it holds**

```bash
REGENERATE_GOLDEN=1 cargo test -p sim --release --test golden
cargo test -p sim --release --test golden
```

Expected: the first regenerates, the second passes.

- [ ] **Step 7: Commit**

```bash
git add crates/sim/src/snapshot.rs crates/sim/src/lib.rs crates/sim/tests/snapshot.rs crates/sim/tests/golden.rs crates/sim/tests/golden_master.bin
git commit -m "feat(sim): snapshots, plus a golden master pinning tick physics"
```

---

### Task 20: Behavioral tests — is the world winnable?

The hardest bug class in this project is **"is the world broken, or has evolution just not found it yet?"** These tests answer that question, and they are the reason Plan 1 exists before any rendering.

A note on the approach, because the obvious plan does not survive contact with the maths. I originally wanted a *hand-wired* forager genome: weights set by hand so the ant climbs the food gradient when empty and climbs its own colony scent when laden. That policy needs a **product** — `carrying × scent_gradient` — and a plain tanh MLP cannot multiply two of its inputs, only add them. Hand-wiring one would mean approximating a product with saturating units, which is fiddly, fragile, and not something I can promise works.

So we split the question in two, and each half gets a test that can actually be written:

1. **Is the economy winnable at all?** Drive `apply_*` directly with a *scripted* controller — a Rust function implementing the forager policy, bypassing the network entirely. If the store does not grow under a policy we wrote by hand, the world's physics are broken, full stop. This test cannot fail for neural-network reasons.
2. **Can a genome express it?** Run a tiny seeded hill-climber offline, once, and check the winner into the repo as `known_good_forager.bin`. If the hill-climber cannot find a forager, that is itself the diagnostic: the policy may not be expressible, or the taxes may be too steep. Either way you learn it in minutes rather than after a week of watching dots.

**Files:**
- Create: `crates/sim/tests/gradient.rs`
- Create: `crates/sim/tests/behavior.rs`
- Create: `crates/sim/tests/known_good.rs` (the hill-climber, `#[ignore]`d)
- Create (generated): `crates/sim/tests/known_good_forager.bin`

**Interfaces:**
- Consumes: `World`, `apply_*`, `Genome`, `Intent`, `sense::squash_phero`.
- Produces: `crates/sim/tests/known_good_forager.bin`, loadable via `bincode::deserialize::<Genome>`.

- [ ] **Step 1: Prove an ant can actually read the way home**

Before asking whether a forager can profit, check that the sensory signal it must follow exists at the distance it must follow it from. The nest scent gradient is the *only* homing cue in the design — there is no compass input — so if it is flat or saturated at foraging range, no genome can ever learn to return, and every downstream test fails for a reason that looks like "evolution didn't work."

Create `crates/sim/tests/gradient.rs`:

```rust
//! Diagnostics for the one signal homing depends on: the nest scent gradient.
//!
//! If these fail, no amount of evolution produces a forager, because the
//! information an ant would need is not in its inputs.

use sim::config::Config;
use sim::pheromone::Pheromones;
use sim::sense::squash_phero;

/// Emit nest scent from a 3x3 block at the centre, let it reach equilibrium,
/// and report what an ant's sensor would read at each distance.
fn equilibrium_profile(cfg: &Config, ticks: u32) -> Vec<(i32, f32)> {
    let mut p = Pheromones::new(cfg);
    let (w, h) = (cfg.width as i32, cfg.height as i32);
    let (cx, cy) = (w / 2, h / 2);
    let idx = |x: i32, y: i32| (y * w + x) as usize;

    for _ in 0..ticks {
        for dy in -1..=1 {
            for dx in -1..=1 {
                p.deposit_scent(idx(cx + dx, cy + dy), cfg.nest_scent_emission, 0);
            }
        }
        p.step(cfg);
    }

    [2, 4, 6, 9, 12, 16, 20]
        .iter()
        .map(|&d| {
            let (raw, _) = p.scent_for(idx(cx + d, cy), 0);
            (d, squash_phero(raw, cfg.phero_log_div))
        })
        .collect()
}

fn cfg() -> Config {
    Config { width: 96, height: 96, ..Config::default() }
}

#[test]
fn the_nest_gradient_decreases_monotonically_with_distance() {
    let profile = equilibrium_profile(&cfg(), 3_000);
    for w in profile.windows(2) {
        assert!(
            w[0].1 > w[1].1,
            "scent is not monotonically decreasing: {:?} then {:?}\nfull profile: {profile:?}",
            w[0],
            w[1]
        );
    }
}

#[test]
fn the_nest_gradient_is_not_saturated_near_the_nest() {
    // The failure mode a tanh squash produces: everything within ~20 cells of
    // the nest reads exactly 1.0, so the ant standing in it is gradient-blind.
    let profile = equilibrium_profile(&cfg(), 3_000);
    for (d, v) in &profile {
        assert!(*v < 1.0, "sensor saturated at distance {d}: {v}\nprofile: {profile:?}");
    }
}

#[test]
fn the_nest_gradient_is_discriminable_at_foraging_range() {
    // An ant's whiskers sample a few cells apart. If two adjacent sample points
    // differ by less than a whisker's worth of f32 noise, the gradient carries
    // no usable information. Checks specifically around SEED_PATCH_DISTANCE.
    let profile = equilibrium_profile(&cfg(), 3_000);
    let at = |d: i32| profile.iter().find(|(x, _)| *x == d).unwrap().1;
    let near_far = at(9) - at(16);
    assert!(
        near_far > 0.01,
        "gradient between 9 and 16 cells is only {near_far}; an ant at foraging \
         range cannot tell which way is home. Lower scent_diffusion, raise \
         scent_evaporation's decay, or shorten SEED_PATCH_DISTANCE.\nprofile: {profile:?}"
    );
}

#[test]
fn scent_reaches_beyond_the_guaranteed_food_patch() {
    // SEED_PATCH_DISTANCE is 12. A laden ant standing on that patch must be
    // able to sense home from where it stands.
    let profile = equilibrium_profile(&cfg(), 3_000);
    let at_patch = profile.iter().find(|(d, _)| *d == 12).unwrap().1;
    assert!(at_patch > 0.0, "no scent at all at the food patch: an ant there is lost");
}
```

- [ ] **Step 2: Run the gradient diagnostics**

Run: `cargo test -p sim --release --test gradient`
Expected: PASS, 4 tests.

For reference, the equilibrium profile these constants produce (96×96, 3,000 ticks, a 3×3 nest emitting 50/tile/tick, `scent_diffusion` 0.06, `scent_evaporation` 0.999, `phero_log_div` 12.0), computed ahead of time:

| distance | raw scent | `squash_phero` | old `tanh(0.1·v)` |
| --- | --- | --- | --- |
| 2 | 4243.67 | 0.696 | 1.00000000 |
| 4 | 1933.05 | 0.631 | 1.00000000 |
| 6 | 955.68 | 0.572 | 1.00000000 |
| 9 | 355.40 | 0.490 | 1.00000000 |
| 12 | 135.91 | 0.410 | 1.00000000 |
| 16 | 37.44 | 0.304 | 0.99888092 |
| 20 | 9.83 | 0.199 | 0.75456570 |

The right-hand column is why this test exists. Under a `tanh` squash every distance from 2 to 12 cells reads as **exactly 1.0** — an ant anywhere in its own territory, including standing on the guaranteed food patch at distance 12, would sense a perfectly flat field and have no information about which way home is. Homing would not have been hard to evolve; it would have been impossible, and the symptom would have been "the ants just wander."

**If `the_nest_gradient_is_discriminable_at_foraging_range` fails, stop and fix it before anything else.** The lever is the ratio of `scent_diffusion` to the scent's decay rate `1 - scent_evaporation`: their ratio sets the gradient's length scale. Raising `scent_diffusion` spreads the beacon farther but flattens it; lowering `scent_evaporation` sharpens it but shortens its reach. Print the profile (`--nocapture` and a `dbg!`) and tune until the values at 9 and 16 cells are clearly separated. This ratio is the single most important pheromone constant in the project, and it is why both fields are live-tunable in Plan 2.

- [ ] **Step 3: Write the "world is winnable" test with a scripted controller**

Create `crates/sim/tests/behavior.rs`:

```rust
use sim::ants::Ants;
use sim::apply::{apply_food, apply_metabolism, apply_movement, apply_nest, ApplyCtx};
use sim::config::Config;
use sim::grid::NO_NEST;
use sim::intent::Intent;
use sim::spatial::Spatial;
use sim::world::World;
use sim::N_MEMORY;

fn cfg() -> Config {
    Config { width: 64, height: 64, num_colonies: 1, initial_ants_per_colony: 20, food_patch_count: 4, ..Config::default() }
}

/// The forager policy, written by hand. Walk toward the nearest food when
/// empty; walk toward the nest when laden; grab when standing on food.
/// This bypasses the network entirely: it tests the *world*, not the brain.
fn scripted_intent(i: usize, ants: &Ants, w: &World) -> Intent {
    let (ax, ay) = (ants.x[i], ants.y[i]);
    let laden = ants.carrying[i] >= ants.genome[i].traits.carry_capacity * 0.5;

    let target = if laden {
        w.colonies[0].nest_center
    } else {
        // Nearest food cell. O(cells) — fine, this is a test.
        let mut best = (f32::MAX, (ax, ay));
        for c in 0..w.grid.food.len() {
            if w.grid.food[c] > 0.0 {
                let (fx, fy) = ((c % w.grid.width as usize) as f32, (c / w.grid.width as usize) as f32);
                let d = (fx - ax).hypot(fy - ay);
                if d < best.0 {
                    best = (d, (fx + 0.5, fy + 0.5));
                }
            }
        }
        best.1
    };

    let heading = (target.1 - ay).atan2(target.0 - ax);
    let (cx, cy) = ants.cell(i);
    let on_food = w.grid.food[w.grid.idx(cx, cy)] > 0.0;

    Intent {
        heading,
        speed: ants.genome[i].traits.max_speed,
        attack: false,
        grab: on_food && !laden,
        release: false,
        memory: [0.0; N_MEMORY],
    }
}

#[test]
fn a_scripted_forager_grows_the_colony_food_store() {
    let c = cfg();
    let mut w = World::new(&c, 11);
    let start_store = w.colonies[0].store;

    let mut spatial = Spatial::new(&c);
    for _ in 0..4_000 {
        spatial.rebuild(&w.ants);
        let intents: Vec<Intent> =
            (0..w.ants.len()).map(|i| scripted_intent(i, &w.ants, &w)).collect();
        let mut ctx = ApplyCtx {
            cfg: &w.cfg,
            grid: &mut w.grid,
            phero: &mut w.phero,
            spatial: &mut spatial,
            colonies: &mut w.colonies,
        };
        for i in 0..w.ants.len() {
            apply_movement(i, &intents[i], &mut w.ants, &mut ctx);
            apply_food(i, &intents[i], &mut w.ants, &mut ctx);
            apply_nest(i, &mut w.ants, &mut ctx);
            apply_metabolism(i, &mut w.ants, ctx.cfg);
        }
    }

    assert!(
        w.colonies[0].store > start_store,
        "a hand-written forager could not profit: the world's economy is unwinnable. \
         store {} -> {}. Check harvest_rate, refuel_rate, birth_cost, and the trait taxes.",
        start_store,
        w.colonies[0].store
    );
}

#[test]
fn a_scripted_forager_actually_delivers_food() {
    let c = cfg();
    let mut w = World::new(&c, 12);
    let mut spatial = Spatial::new(&c);
    for _ in 0..4_000 {
        spatial.rebuild(&w.ants);
        let intents: Vec<Intent> =
            (0..w.ants.len()).map(|i| scripted_intent(i, &w.ants, &w)).collect();
        let mut ctx = ApplyCtx {
            cfg: &w.cfg, grid: &mut w.grid, phero: &mut w.phero,
            spatial: &mut spatial, colonies: &mut w.colonies,
        };
        for i in 0..w.ants.len() {
            apply_movement(i, &intents[i], &mut w.ants, &mut ctx);
            apply_food(i, &intents[i], &mut w.ants, &mut ctx);
            apply_nest(i, &mut w.ants, &mut ctx);
            apply_metabolism(i, &mut w.ants, ctx.cfg);
        }
    }
    let delivered: f32 = w.ants.food_delivered.iter().sum();
    assert!(delivered > 0.0, "no ant reached the nest with a load");
}

#[test]
fn a_colony_with_no_reachable_food_shrinks_to_the_extinction_floor() {
    let c = Config { width: 64, height: 64, num_colonies: 1, initial_ants_per_colony: 40,
                     food_patch_count: 0, initial_food_store: 10.0, ..Config::default() };
    let mut w = World::new(&c, 13);
    // worldgen still seeds one guaranteed patch per colony; remove all food.
    w.grid.food.iter_mut().for_each(|f| *f = 0.0);
    w.grid.fertility.iter_mut().for_each(|f| *f = 0.0);

    for _ in 0..20_000 {
        w.tick();
    }
    // The floor trickles in one free ant per interval, and each starves. So the
    // population hovers at or below the floor, never above it, and the colony
    // is visibly on life support rather than thriving.
    assert!(
        w.ants.population(0) <= w.cfg.extinction_floor,
        "starvation should have pruned the colony to the floor, got {}",
        w.ants.population(0)
    );
    assert!(w.colonies[0].floor_spawns > 0, "the floor should have been propping it up");
    assert!(w.colonies[0].births == 0, "a starving colony must not afford paid births");
}

#[test]
fn a_random_colony_does_not_immediately_explode_in_population() {
    let mut w = World::new(&cfg(), 14);
    for _ in 0..5_000 {
        w.tick();
    }
    assert!(w.ants.len() < 5_000, "population ran away: birth_cost is too cheap");
}
```

- [ ] **Step 4: Run them**

Run: `cargo test -p sim --release --test behavior`
Expected: PASS, 4 tests.

If `a_scripted_forager_grows_the_colony_food_store` fails, **do not proceed to the hill-climber.** The economy is misconfigured. Re-derive the break-even sum in `Config`'s doc comment with your current constants — most likely a tax coefficient dominates, as `tax_vision` once did. Raise `harvest_rate`, lower the taxes, or shorten `SEED_PATCH_DISTANCE` until a hand-written forager can turn a profit. There is no point asking evolution to solve a game that cannot be won.

- [ ] **Step 5: Commit the behavioral tests**

```bash
git add crates/sim/tests/gradient.rs crates/sim/tests/behavior.rs
git commit -m "test(sim): gradient diagnostics and a scripted forager that profits"
```

- [ ] **Step 6: Write the offline hill-climber as an ignored test**

Create `crates/sim/tests/known_good.rs`:

```rust
//! Searches for a genome that can forage, and checks it in as a fixture.
//!
//!     cargo test -p sim --release --test known_good -- --ignored --nocapture
//!
//! This is a tool, not a guard: it is `#[ignore]`d so CI never runs it.
//! Its output, `known_good_forager.bin`, separates "the world is broken" from
//! "evolution has not found it yet".

use sim::config::Config;
use sim::genome::Genome;
use sim::rng::Pcg32;
use sim::world::World;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/known_good_forager.bin");

fn cfg() -> Config {
    Config { width: 48, height: 48, num_colonies: 1, initial_ants_per_colony: 30,
             food_patch_count: 4, ..Config::default() }
}

/// Total food delivered by a colony seeded entirely with `g`.
fn score(g: &Genome, seed: u64, ticks: u32) -> f32 {
    let c = cfg();
    let mut w = World::new(&c, seed);
    for i in 0..w.ants.len() {
        w.ants.genome[i] = g.clone();
    }
    for _ in 0..ticks {
        w.tick();
    }
    w.ants.food_delivered.iter().sum::<f32>() + w.colonies[0].store
}

#[test]
#[ignore = "offline tool; regenerates the known-good forager fixture"]
fn search_for_a_forager() {
    let c = cfg();
    let mut rng = Pcg32::new(0xF00D, 1);
    let mut best = Genome::random(&mut rng);
    let mut best_score = score(&best, 1, 3_000);

    for gen in 0..300 {
        let candidate = best.mutated(&c, &mut rng);
        // Average two seeds so we do not overfit one map.
        let s = 0.5 * (score(&candidate, 1, 3_000) + score(&candidate, 2, 3_000));
        if s > best_score {
            best = candidate;
            best_score = s;
            println!("gen {gen}: new best {best_score:.1}");
        }
    }

    println!("final score {best_score:.1}");
    std::fs::write(FIXTURE, bincode::serialize(&best).unwrap()).unwrap();
}

#[test]
fn the_known_good_forager_still_forages() {
    let Ok(bytes) = std::fs::read(FIXTURE) else {
        eprintln!("no fixture yet; run the ignored `search_for_a_forager` first");
        return;
    };
    let g: Genome = bincode::deserialize(&bytes).unwrap();
    let delivered = score(&g, 1, 3_000);
    assert!(
        delivered > 0.0,
        "the checked-in forager delivers nothing. Either a simulation rule changed \
         under it, or the fixture was generated from a failed search."
    );
}
```

- [ ] **Step 7: Run the search and inspect the output**

```bash
cargo test -p sim --release --test known_good -- --ignored --nocapture
```

Expected: printed `new best` lines with a rising score, ending well above zero, and a written fixture.

**If the final score is 0.0**, that is a real finding, not a failing step. It means a mutation hill-climb over 300 generations cannot discover foraging from a random start. By this point Step 2 has proved the homing gradient is readable and Step 4 has proved a competent forager profits, so the world is sound and the problem is the search. Try, in order: raise `mutation_rate`, lengthen the evaluation to 10,000 ticks, and lower `food_evaporation` toward 0.99 so trails persist long enough to be worth following. Note in the commit message what you changed and what the score became. This tuning loop **is** the project.

- [ ] **Step 8: Verify the guard test passes and commit**

Run: `cargo test -p sim --release --test known_good`
Expected: PASS (1 test; the search is ignored).

```bash
git add crates/sim/tests/known_good.rs crates/sim/tests/known_good_forager.bin
git commit -m "test(sim): hill-climbed known-good forager fixture and its guard"
```

---

### Task 21: `headless` — the CLI that makes evolution observable

The deliverable of Plan 1. Run a world, print per-colony stats as CSV, and you can see food-delivered curves in a terminal — the exact signal that distinguishes "evolution is working" from "ants wander forever."

**Files:**
- Create: `crates/headless/Cargo.toml`
- Create: `crates/headless/src/main.rs`

**Interfaces:**
- Consumes: the whole `sim` public API.
- Produces: a binary, `cargo run -p headless --release -- --ticks 100000`.

- [ ] **Step 1: Create the manifest and register the workspace member**

In the root `Cargo.toml`, change `members` to:

```toml
members = ["crates/sim", "crates/headless"]
```

Create `crates/headless/Cargo.toml`:

```toml
[package]
name = "headless"
version = "0.1.0"
edition.workspace = true

[dependencies]
sim = { path = "../sim" }
clap.workspace = true
bincode.workspace = true
```

- [ ] **Step 2: Write the CLI**

Create `crates/headless/src/main.rs`:

```rust
//! Runs a world with no renderer and prints per-colony stats as CSV.
//! This is where you find out whether evolution does anything.

use clap::Parser;
use sim::config::Config;
use sim::snapshot::{load, save};
use sim::world::World;
use std::io::Write;

#[derive(Parser)]
#[command(about = "Headless antsim2 runner")]
struct Args {
    #[arg(long, default_value_t = 100_000)]
    ticks: u64,
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// Emit one CSV row per colony every N ticks.
    #[arg(long, default_value_t = 1_000)]
    every: u64,
    /// Resume from a snapshot instead of generating a fresh world.
    #[arg(long)]
    load: Option<String>,
    /// Write a snapshot here when the run finishes.
    #[arg(long)]
    save: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let mut world = match &args.load {
        Some(path) => load(&std::fs::read(path)?)?,
        None => World::new(&Config::default(), args.seed),
    };

    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    writeln!(
        out,
        "tick,colony,population,store,births,deaths,floor_spawns,mean_size,generation,food_delivered"
    )?;

    for _ in 0..args.ticks {
        world.tick();
        if world.tick_count % args.every == 0 {
            for s in world.stats() {
                writeln!(
                    out,
                    "{},{},{},{:.1},{},{},{},{:.3},{:.2},{:.1}",
                    world.tick_count, s.id, s.population, s.store, s.births, s.deaths,
                    s.floor_spawns, s.mean_size, s.mean_lineage, s.food_delivered
                )?;
            }
            out.flush()?;
        }
    }

    if let Some(path) = &args.save {
        std::fs::write(path, save(&world)?)?;
        eprintln!("wrote snapshot to {path}");
    }
    Ok(())
}
```

- [ ] **Step 3: Build and smoke-test it**

```bash
cargo run -p headless --release -- --ticks 2000 --every 500
```

Expected: a CSV header and 4 rows per colony (8 colonies × 4 sample points = 32 rows), every population at or above the extinction floor.

- [ ] **Step 4: Measure the tick rate — this is the performance gate**

```bash
time cargo run -p headless --release -- --ticks 10000 --every 100000
```

Expected: with 8 colonies × 40 founders = 320 ants on a 512×512 grid, this should complete in well under a minute. Record the ticks/second.

Then check it scales. Ten thousand ants is the spec's target:

```bash
cargo run -p headless --release -- --ticks 2000 --every 100000
```

after temporarily raising `initial_ants_per_colony` to 1250 in `Config::default()`. If the tick rate collapses, profile before proceeding — the likely culprits, in order, are the `to_vec()` clone inside `diffuse_decay` (hoist it into a reusable scratch buffer on `Pheromones`), `Ants::population` being O(n) and called in a loop by `reproduce` (cache a per-colony count), and `colony_stats` doing a full scan (only called on demand, so probably fine).

- [ ] **Step 5: Run the whole suite once more**

Run: `cargo test --workspace --release`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add crates/headless Cargo.toml
git commit -m "feat(headless): CSV stats runner for observing evolution"
```

- [ ] **Step 7: Actually look at the output**

```bash
cargo run -p headless --release -- --ticks 500000 --every 5000 --save /tmp/run1.bin > /tmp/run1.csv
```

Open `/tmp/run1.csv` and look at `food_delivered` and `population` per colony over time.

- **Rising food_delivered:** evolution is working. Plan 2 is worth building.
- **Flat, near-zero food_delivered:** the expected first result. Sweep `food_evaporation` (try 0.99 and 0.999), `food_diffusion` (0.05 to 0.3), `mutation_rate`, and the trait taxes. The `known_good_forager` fixture tells you whether a good genome exists at all under the current constants.
- **Colonies pinned at the extinction floor:** the economy is too harsh. Revisit `base_upkeep` and `birth_cost`.

Write down what you find. That note is the input to Plan 2's live-tuning panel design.

---

## Plan 2 (not this document)

Once `sim` is green and the CSV shows something interesting, Plan 2 covers the `server` crate (WebSocket, tick/frame decoupling, live `Config` mutation), the binary protocol, and the `web` app (WebGL2 instanced ant rendering, pheromone texture, canvas2d neural-net view, per-colony charts, ant inspector). Its interfaces are already fixed by this plan: `World::tick`, `World::stats`, `Activations`, `snapshot::save`/`load`, and a mutable `World::cfg`.












