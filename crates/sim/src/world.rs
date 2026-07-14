use crate::ants::{Ants, Spawn};
use crate::apply::{
    apply_combat, apply_food, apply_metabolism, apply_movement, apply_nest, deposit_passive,
    sweep_deaths, ApplyCtx,
};
use crate::brain::{Activations, Brain};
use crate::chronicle::Chronicle;
use crate::colony::ColonyState;
use crate::config::Config;
use crate::genome::Genome;
use crate::grid::Grid;
use crate::intent::{think, Intent};
use crate::pheromone::Pheromones;
use crate::reproduce::reproduce;
use crate::rng::Pcg32;
use crate::sense::sense;
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
    /// The story log: permanent "firsts" and rolling titles, streamed to the UI.
    pub chronicle: Chronicle,

    /// Derived each tick; never serialised.
    #[serde(skip)]
    spatial: Spatial,
}

impl World {
    pub fn new(cfg: &Config, seed: u64) -> Self {
        let mut rng = Pcg32::new(seed, 0xA17);
        let (grid, colonies) = generate(cfg, seed, &mut rng);

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
            chronicle: Chronicle::new(),
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
        if self.ants.attacking.len() != self.ants.len() {
            self.ants.clear_attacking();
        }
    }

    /// Cell index for a world position, or `None` if non-finite or off-map.
    /// Shared by the operator map-edit mutators below.
    fn cell_index(&self, x: f32, y: f32) -> Option<usize> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        if !self.grid.in_bounds(x as i32, y as i32) {
            return None;
        }
        Some(self.grid.idx(x as u16, y as u16))
    }

    /// Operator edit: set the standing food at a cell. Applied between ticks by
    /// the sim thread, so it mutates deterministically without racing the tick.
    pub fn set_food(&mut self, x: f32, y: f32, amount: f32) {
        if let Some(i) = self.cell_index(x, y) {
            self.grid.food[i] = amount.max(0.0);
        }
    }

    /// Operator edit: place or clear stone. A nest tile is never buried.
    pub fn set_stone(&mut self, x: f32, y: f32, solid: bool) {
        if let Some(i) = self.cell_index(x, y) {
            if self.grid.nest[i] == crate::grid::NO_NEST {
                self.grid.stone[i] = solid;
                if solid {
                    self.grid.food[i] = 0.0;
                }
            }
        }
    }

    /// Operator edit: add (or, with a negative amount, remove) food from a
    /// colony's store, floored at zero.
    pub fn add_to_store(&mut self, colony: u8, amount: f32) {
        if !amount.is_finite() {
            return;
        }
        if let Some(c) = self.colonies.get_mut(colony as usize) {
            c.store = (c.store + amount).max(0.0);
        }
    }

    /// Operator edit: rename a colony.
    pub fn rename_colony(&mut self, colony: u8, name: String) {
        if let Some(c) = self.colonies.get_mut(colony as usize) {
            c.name = name;
        }
    }

    /// Operator edit: spawn one ant of a colony at a position, bred from the
    /// colony's archive (or a random genome if the archive is empty). Uses the
    /// world id counter so `Ants::push`'s strictly-increasing-id invariant holds,
    /// and rebuilds the spatial index the edit invalidated.
    pub fn spawn_ant_at(&mut self, x: f32, y: f32, colony: u8) {
        if self.cell_index(x, y).is_none() {
            return;
        }
        if colony as usize >= self.colonies.len() {
            return;
        }
        let genome = match self.colonies[colony as usize].archive_parent(&mut self.rng) {
            Some((g, _)) => g.clone(),
            None => Genome::random(&mut self.rng),
        };
        let id = self.next_id;
        self.next_id += 1;
        let energy = crate::reproduce::NEWBORN_ENERGY_FRAC
            * genome.max_energy(&self.cfg, crate::reproduce::NEWBORN_SIZE);
        self.ants.push(Spawn {
            id,
            colony,
            x,
            y,
            heading: 0.0,
            energy,
            size: crate::reproduce::NEWBORN_SIZE,
            lineage: 0,
            genome,
            birth_tick: self.tick_count,
        });
        self.rebuild_index();
    }

    /// Index of a living ant by id. `Ants::id` is sorted, so this binary-searches.
    pub fn index_of(&self, id: u64) -> Option<usize> {
        self.ants.id.binary_search(&id).ok()
    }

    /// The living ant nearest a world coordinate. Used by click-to-select; the
    /// ant frame carries no ids, so the server resolves the pick.
    pub fn nearest_ant(&self, x: f32, y: f32) -> Option<u64> {
        let mut best: Option<(f32, u64)> = None;
        for i in 0..self.ants.len() {
            if !self.ants.alive[i] {
                continue;
            }
            let dx = self.ants.x[i] - x;
            let dy = self.ants.y[i] - y;
            let d2 = dx * dx + dy * dy;
            // Strict `<` keeps the lowest id on a tie, matching the apply phase.
            if best.map_or(true, |(bd, _)| d2 < bd) {
                best = Some((d2, self.ants.id[i]));
            }
        }
        best.map(|(_, id)| id)
    }

    /// One ant's full layer activations, recomputed against the current world.
    ///
    /// Lives here because `sense` needs the spatial index, which stays private.
    /// Read-only: it does not advance the ant's recurrent memory.
    pub fn activations(&self, i: usize) -> Activations {
        let inputs = sense(
            i,
            &self.ants,
            &self.grid,
            &self.phero,
            &self.spatial,
            &self.cfg,
        );
        self.ants.genome[i].forward(&inputs)
    }

    pub fn tick(&mut self) {
        self.spatial.rebuild(&self.ants);
        self.ants.clear_attacking();

        // --- Phase 1: parallel, read-only. Cannot race by construction. ---
        let intents: Vec<Intent> = (0..self.ants.len())
            .into_par_iter()
            .map(|i| {
                if self.ants.alive[i] {
                    think(
                        i,
                        &self.ants,
                        &self.grid,
                        &self.phero,
                        &self.spatial,
                        &self.cfg,
                    )
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
                self.phero
                    .deposit_scent(t, self.cfg.nest_scent_emission, col.id);
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

        // `retain_alive` compacted the arrays and `reproduce` appended newborns,
        // so the index built at the top of this tick now points past the live
        // ants. Rebuild it against the final array to leave the world queryable
        // between ticks — the server reads `activations` for the selected ant on
        // every frame, and a stale index there is an out-of-bounds panic. The
        // top-of-tick rebuild still governs the think phase, so this does not
        // touch determinism.
        self.spatial.rebuild(&self.ants);

        self.run_chronicle_detectors();

        // --- Phase 3: fields. ---
        self.phero.step(&self.cfg);
        self.grid.regrow(self.cfg.food_regrow);

        self.tick_count += 1;
    }

    /// Population thresholds announced as a colony grows.
    const MILESTONES: [u32; 4] = [10, 25, 50, 100];

    /// The event registry. Each block is one detector; add a milestone by adding
    /// a block. Runs in the serial phase, so it cannot perturb determinism.
    ///
    /// `FirstTrailFollow` reads the transient `Ants::followed_trail` flag, set in
    /// `apply_food` when an ant grabs food on a cell that already carried a
    /// nestmate's trail (see `TRAIL_FOLLOW_THRESHOLD`). The flag is per-tick and
    /// not serialised, so it costs nothing in the wire format.
    fn run_chronicle_detectors(&mut self) {
        let tick = self.tick_count;
        for ci in 0..self.colonies.len() {
            let cid = self.colonies[ci].id;

            // PopulationMilestone: crossed one or more thresholds this tick.
            {
                let pop = self.ants.population(cid);
                while self.colonies[ci].next_milestone_idx < Self::MILESTONES.len()
                    && pop >= Self::MILESTONES[self.colonies[ci].next_milestone_idx]
                {
                    let m = Self::MILESTONES[self.colonies[ci].next_milestone_idx];
                    let cname = self.colonies[ci].name.clone();
                    self.colonies[ci].next_milestone_idx += 1;
                    let mut flag = false;
                    self.chronicle.record(&mut flag, crate::chronicle::ChronicleEvent {
                        tick,
                        colony: cid,
                        kind: crate::chronicle::EventKind::PopulationMilestone,
                        ant_id: None,
                        ant_name: None,
                        text: format!("{cname} reached {m} ants"),
                    });
                }
            }

            // FirstKill: this colony landed its first killing blow.
            if !self.colonies[ci].first_kill_done {
                let killer = (0..self.ants.len()).find(|&i| {
                    self.ants.colony[i] == cid && self.ants.killed_this_tick(i)
                });
                if let Some(i) = killer {
                    let id = self.ants.id[i];
                    let cname = self.colonies[ci].name.clone();
                    let mut done = self.colonies[ci].first_kill_done;
                    self.chronicle.record(&mut done, crate::chronicle::ChronicleEvent {
                        tick,
                        colony: cid,
                        kind: crate::chronicle::EventKind::FirstKill,
                        ant_id: Some(id),
                        ant_name: Some(crate::names::ant_name(id)),
                        text: format!("{cname}: first blood"),
                    });
                    self.colonies[ci].first_kill_done = done;
                }
            }

            // FirstTrailFollow: an ant of this colony reached food by following
            // a nestmate's scent trail for the first time.
            if !self.colonies[ci].first_trail_follow_done {
                let follower = (0..self.ants.len()).find(|&i| {
                    self.ants.colony[i] == cid && self.ants.followed_trail_this_tick(i)
                });
                if let Some(i) = follower {
                    let id = self.ants.id[i];
                    let cname = self.colonies[ci].name.clone();
                    let mut done = self.colonies[ci].first_trail_follow_done;
                    self.chronicle.record(&mut done, crate::chronicle::ChronicleEvent {
                        tick,
                        colony: cid,
                        kind: crate::chronicle::EventKind::FirstTrailFollow,
                        ant_id: Some(id),
                        ant_name: Some(crate::names::ant_name(id)),
                        text: format!("{cname}: followed the scent to food"),
                    });
                    self.colonies[ci].first_trail_follow_done = done;
                }
            }

            // TopForager: a new single-ant delivery record for this colony.
            {
                let best = (0..self.ants.len())
                    .filter(|&i| self.ants.alive[i] && self.ants.colony[i] == cid)
                    .max_by(|&a, &b| {
                        self.ants.food_delivered[a].total_cmp(&self.ants.food_delivered[b])
                    });
                if let Some(i) = best {
                    let d = self.ants.food_delivered[i];
                    if d > self.colonies[ci].best_forager_delivered {
                        self.colonies[ci].best_forager_delivered = d;
                        let id = self.ants.id[i];
                        let cname = self.colonies[ci].name.clone();
                        let mut flag = false;
                        self.chronicle.record(&mut flag, crate::chronicle::ChronicleEvent {
                            tick,
                            colony: cid,
                            kind: crate::chronicle::EventKind::TopForager,
                            ant_id: Some(id),
                            ant_name: Some(crate::names::ant_name(id)),
                            text: format!("{cname}: new top forager, {d:.0} delivered"),
                        });
                    }
                }
            }

            // EldestAnt: a *new individual* out-lives every predecessor. Gated on
            // id so an ant aging past its own record does not fire every tick.
            {
                let oldest = (0..self.ants.len())
                    .filter(|&i| self.ants.alive[i] && self.ants.colony[i] == cid)
                    .max_by_key(|&i| self.ants.age[i]);
                if let Some(i) = oldest {
                    let age = self.ants.age[i] as u64;
                    let id = self.ants.id[i];
                    if age > self.colonies[ci].eldest_seen
                        && id != self.colonies[ci].eldest_id
                    {
                        self.colonies[ci].eldest_seen = age;
                        self.colonies[ci].eldest_id = id;
                        let cname = self.colonies[ci].name.clone();
                        let mut flag = false;
                        self.chronicle.record(&mut flag, crate::chronicle::ChronicleEvent {
                            tick,
                            colony: cid,
                            kind: crate::chronicle::EventKind::EldestAnt,
                            ant_id: Some(id),
                            ant_name: Some(crate::names::ant_name(id)),
                            text: format!("{cname}: oldest ant yet"),
                        });
                    } else if age > self.colonies[ci].eldest_seen {
                        // Same individual still aging: advance the record silently.
                        self.colonies[ci].eldest_seen = age;
                    }
                }
            }
        }
        for ci in 0..self.colonies.len() {
            // FirstDelivery: the colony's store has been fed for the first time.
            if !self.colonies[ci].first_delivery_done
                && self.colonies[ci].delivered_total > 0.0
            {
                let cid = self.colonies[ci].id;
                // Attribute to the living ant of this colony with the most
                // delivered — the likely deliverer this tick.
                let who = (0..self.ants.len())
                    .filter(|&i| self.ants.alive[i] && self.ants.colony[i] == cid)
                    .max_by(|&a, &b| {
                        self.ants.food_delivered[a]
                            .total_cmp(&self.ants.food_delivered[b])
                    });
                let (ant_id, ant_name) = match who {
                    Some(i) => (
                        Some(self.ants.id[i]),
                        Some(crate::names::ant_name(self.ants.id[i])),
                    ),
                    None => (None, None),
                };
                let cname = self.colonies[ci].name.clone();
                let mut done = self.colonies[ci].first_delivery_done;
                self.chronicle.record(&mut done, crate::chronicle::ChronicleEvent {
                    tick,
                    colony: cid,
                    kind: crate::chronicle::EventKind::FirstDelivery,
                    ant_id,
                    ant_name,
                    text: format!("{cname}: first food carried home"),
                });
                self.colonies[ci].first_delivery_done = done;
            }
        }
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
            eat(&self.ants.food_harvested[i].to_bits().to_le_bytes());
            eat(&self.ants.food_homing[i].to_bits().to_le_bytes());
        }
        for c in &self.colonies {
            eat(&c.store.to_bits().to_le_bytes());
            eat(&c.births.to_le_bytes());
            eat(&c.deaths.to_le_bytes());
        }
        for v in &self.phero.food {
            eat(&v.to_bits().to_le_bytes());
        }
        // The owned fields carry magnitude *and* ownership; a divergence in
        // either must change the hash. Trail is the newest such field.
        for f in [&self.phero.scent, &self.phero.trail] {
            for v in &f.mag {
                eat(&v.to_bits().to_le_bytes());
            }
            eat(&f.owner);
        }
        for v in &self.grid.food {
            eat(&v.to_bits().to_le_bytes());
        }
        h
    }
}

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
    fn set_food_writes_the_cell_and_clamps_out_of_bounds() {
        let mut w = World::new(&Config { width: 32, height: 32, ..Config::default() }, 1);
        w.set_food(5.0, 5.0, 123.0);
        let i = w.grid.idx(5, 5);
        assert_eq!(w.grid.food[i], 123.0);
        w.set_food(-9.0, -9.0, 1.0); // must not panic
    }

    #[test]
    fn add_to_store_credits_the_named_colony() {
        let mut w = World::new(&Config { num_colonies: 2, ..Config::default() }, 1);
        let before = w.colonies[1].store;
        w.add_to_store(1, 50.0);
        assert_eq!(w.colonies[1].store, before + 50.0);
    }

    #[test]
    fn spawn_ant_at_adds_one_living_ant_of_that_colony() {
        let mut w = World::new(
            &Config { width: 32, height: 32, num_colonies: 2, ..Config::default() },
            1,
        );
        let before = w.ants.population(0);
        w.spawn_ant_at(4.0, 4.0, 0);
        assert_eq!(w.ants.population(0), before + 1);
        assert!(w.ants.id.windows(2).all(|s| s[0] < s[1]), "ids stay sorted");
    }

    #[test]
    fn population_milestone_fires_when_a_colony_first_reaches_ten() {
        // Start above the first milestone so the detector fires on tick 1
        // regardless of whether the colony grows.
        let mut w = World::new(
            &Config {
                width: 96,
                height: 96,
                num_colonies: 2,
                initial_ants_per_colony: 12,
                ..Config::default()
            },
            1,
        );
        w.tick();
        assert!(
            w.chronicle.events.iter().any(|e| matches!(
                e.kind,
                crate::chronicle::EventKind::PopulationMilestone
            )),
            "no population milestone fired for a 12-ant colony"
        );
    }

    #[test]
    fn first_trail_follow_fires_once_and_latches() {
        let mut w = World::new(&small(), 1);
        w.tick(); // populate the ant arrays and size the transient flags

        // Make ant 0 look like it grabbed food on a nestmate's trail this tick.
        let cid = w.ants.colony[0];
        w.ants.followed_trail[0] = true;
        let ci = w.colonies.iter().position(|c| c.id == cid).unwrap();
        w.colonies[ci].first_trail_follow_done = false;

        w.run_chronicle_detectors();
        let count = |w: &World| {
            w.chronicle
                .events
                .iter()
                .filter(|e| {
                    e.colony == cid
                        && matches!(e.kind, crate::chronicle::EventKind::FirstTrailFollow)
                })
                .count()
        };
        assert_eq!(count(&w), 1, "the first trail-follow must be chronicled");
        assert!(w.colonies[ci].first_trail_follow_done, "and it must latch");

        // A second pass with the flag still set adds nothing: the latch holds.
        w.run_chronicle_detectors();
        assert_eq!(count(&w), 1, "the beat is a one-shot 'first'");
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
            assert!(w.phero.scent.mag[t] > 0.0);
            assert_eq!(w.phero.scent.owner[t], c.id);
        }
    }

    #[test]
    fn nests_never_lay_the_trail_field() {
        // The trail is un-fused from the beacon: only ants deposit it, never
        // nests. Kill every ant, then tick: the phase-2 deposit loop runs over
        // an empty population, the refounded cohort has not acted yet, so the
        // *only* field writer this tick is the nest beacon. It must touch scent
        // and leave the trail field completely empty.
        use crate::pheromone::NO_OWNER;
        let mut w = World::new(&small(), 1);
        w.ants.alive.iter_mut().for_each(|a| *a = false);
        w.ants.retain_alive();
        assert_eq!(w.ants.len(), 0);
        w.tick();
        assert!(
            w.phero.trail.owner.iter().all(|&o| o == NO_OWNER),
            "the nest beacon must never write the trail field"
        );
        assert!(
            w.phero.scent.owner.iter().any(|&o| o != NO_OWNER),
            "the nest beacon should have laid scent"
        );
    }

    #[test]
    fn no_ant_ever_stands_on_stone() {
        let mut w = World::new(&small(), 1);
        for _ in 0..300 {
            w.tick();
            for i in 0..w.ants.len() {
                let (x, y) = w.ants.cell(i);
                let c = w.grid.idx(x, y);
                assert!(!w.grid.stone[c], "ant {i} is inside a rock");
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
    fn a_collapsed_colony_is_refounded_the_same_tick() {
        // The extinction floor is retired. A colony can hit zero, but refounding
        // reseeds it the same tick it dies — so at the end of *every* tick no
        // colony is ever observed empty. This is the "world stays alive" property.
        let mut w = World::new(&small(), 7);
        for _ in 0..5000 {
            w.tick();
            for id in 0..w.cfg.num_colonies {
                assert!(
                    w.ants.population(id) > 0,
                    "colony {id} was left extinct after the tick"
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
    fn nearest_ant_finds_the_closest_living_ant() {
        // Founders share nest tiles, so several ants sit on identical
        // coordinates. Move one somewhere unambiguous rather than assuming
        // positions are unique.
        let mut w = World::new(&small(), 1);
        w.ants.x[7] = 30.0;
        w.ants.y[7] = 30.0;
        assert_eq!(w.nearest_ant(30.1, 30.1), Some(w.ants.id[7]));

        w.ants.alive[7] = false;
        assert_ne!(w.nearest_ant(30.1, 30.1), Some(w.ants.id[7]));
    }

    #[test]
    fn activations_are_queryable_after_a_tick_that_killed_ants() {
        // Regression: the spatial index is rebuilt at the top of `tick`, but
        // `retain_alive` compacts the ant arrays at the bottom. Between ticks the
        // grid holds indices past the shrunken array. The server reads exactly
        // this window every frame — `activations` for the selected ant — so a
        // tick with any death used to panic with an out-of-bounds grid index.
        let mut w = World::new(&small(), 7);
        w.tick(); // size the arrays and place the founders

        let n = w.ants.len();
        let victim = n - 1; // highest index; retain leaves it dangling in the grid
        // Co-locate a survivor with the victim so the survivor's neighbourhood
        // sensor reads the very cell that still holds the stale index.
        w.ants.x[0] = w.ants.x[victim];
        w.ants.y[0] = w.ants.y[victim];
        w.ants.energy[0] = 1e6; // ant 0 survives the tick
        w.ants.energy[victim] = -1e6; // and dies this tick, beyond any harvest
        for c in &mut w.colonies {
            c.store = 0.0; // no refuel (would save the victim) and no births
        }

        w.tick(); // victim dies, arrays shrink, grid left stale
        assert!(w.ants.len() < n, "the victim should have died");

        // Must not panic, and must read against the live arrays.
        for i in 0..w.ants.len() {
            if w.ants.alive[i] {
                let _ = w.activations(i);
            }
        }
    }

    #[test]
    fn nearest_ant_breaks_a_positional_tie_on_the_lower_id() {
        // Co-located ants are the common case at spawn, so the tie-break must
        // be defined rather than incidental. Lowest id wins, as in apply.
        let w = World::new(&small(), 1);
        // Find any cell that several founders share, rather than assuming ant 0
        // is one of them: which founders stack on which nest tile depends on the
        // RNG stream, so pin the tie-break behaviour, not a specific seed's layout.
        let (x, y, tied) = (0..w.ants.len())
            .map(|i| {
                let (x, y) = (w.ants.x[i], w.ants.y[i]);
                let ids: Vec<u64> = (0..w.ants.len())
                    .filter(|&j| w.ants.x[j] == x && w.ants.y[j] == y)
                    .map(|j| w.ants.id[j])
                    .collect();
                (x, y, ids)
            })
            .find(|(_, _, ids)| ids.len() > 1)
            .expect("expected some co-located founders at spawn");
        assert_eq!(w.nearest_ant(x, y), Some(*tied.iter().min().unwrap()));
    }

    #[test]
    fn nearest_ant_is_none_in_an_empty_world() {
        let mut w = World::new(&small(), 1);
        w.ants.alive.iter_mut().for_each(|a| *a = false);
        assert_eq!(w.nearest_ant(0.0, 0.0), None);
    }

    #[test]
    fn index_of_round_trips_an_ant_id() {
        let w = World::new(&small(), 1);
        let id = w.ants.id[5];
        assert_eq!(w.index_of(id), Some(5));
        assert_eq!(w.index_of(9_999_999), None);
    }

    #[test]
    fn activations_match_a_direct_forward_pass() {
        let mut w = World::new(&small(), 1);
        w.tick();
        let a = w.activations(3);
        let expected = w.ants.genome[3].forward(&a.inputs);
        assert_eq!(a.outputs, expected.outputs);
        assert_eq!(a.h1, expected.h1);
    }

    #[test]
    fn activations_do_not_advance_the_ants_memory() {
        // The inspector polls this ~4x a second. If it mutated recurrent state,
        // watching an ant would change what the ant does.
        let mut w = World::new(&small(), 1);
        w.tick();
        let before = w.ants.memory[3];
        let _ = w.activations(3);
        assert_eq!(before, w.ants.memory[3]);
    }

    #[test]
    fn an_attacking_ant_is_flagged_and_the_flag_clears_next_tick() {
        // Two colonies packed onto one nest so foes are adjacent from tick 1.
        let cfg = Config {
            width: 32,
            height: 32,
            num_colonies: 2,
            initial_ants_per_colony: 30,
            // A deep store so the packed founders stay fed long enough to fight:
            // the tuned lean economy (small store, slow refuel) otherwise starves
            // this crowd before any blow lands, and this test is about the
            // attack flag, not the economy.
            initial_food_store: 5000.0,
            ..Config::default()
        };
        let mut w = World::new(&cfg, 11);

        let mut ever_attacked = false;
        for _ in 0..400 {
            w.tick();
            if (0..w.ants.len()).any(|i| w.ants.is_attacking(i)) {
                ever_attacked = true;
                break;
            }
        }
        assert!(ever_attacked, "no ant ever landed an attack in 400 ticks");

        // The flag is per-tick, not cumulative: a tick in which nobody swings
        // must leave every flag false. Drive that by removing all foes.
        w.ants.colony.iter_mut().for_each(|c| *c = 0);
        w.tick();
        assert!(
            (0..w.ants.len()).all(|i| !w.ants.is_attacking(i)),
            "attacking flag survived a tick with no foes to attack"
        );
    }

    #[test]
    fn the_attacking_flag_never_perturbs_the_trajectory() {
        // It is an observation field. Two worlds that differ only in a stale
        // flag must tick to the same hash.
        let mut a = World::new(&small(), 5);
        let mut b = World::new(&small(), 5);
        b.ants.attacking.iter_mut().for_each(|f| *f = true);
        for _ in 0..50 {
            a.tick();
            b.tick();
        }
        assert_eq!(a.state_hash(), b.state_hash());
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
