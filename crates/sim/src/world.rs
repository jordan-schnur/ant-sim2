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
