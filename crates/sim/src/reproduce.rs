use crate::ants::{Ants, Spawn};
use crate::colony::{world_reservoir_parent, ColonyState};
use crate::config::Config;
use crate::genome::Genome;
use crate::rng::Pcg32;

pub const NEWBORN_SIZE: f32 = 0.5;
/// Newborns start partly fed so they get a few hundred ticks to find food.
pub const NEWBORN_ENERGY_FRAC: f32 = 0.6;

/// Refound any collapsed colony from the world reservoir, then spend food
/// stores on births. Colonies are processed in id order and ants pushed with
/// increasing ids, so the whole pass is reproducible from `rng` alone.
///
/// The extinction floor is retired: a colony is allowed to reach zero, and is
/// reseeded the same tick from the best proven genomes across the *whole* world
/// (the one place gene pools cross). While a colony is alive it still breeds
/// pure — paid births below draw only from its own living ants.
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

        // --- Collapse + refound: a dead nest is reseeded this same tick. ---
        if ants.population(cid) == 0 {
            refound(ants, colonies, ci, cfg, tick, next_id, rng);
        }

        // --- Paid births from the food store. ---
        let mut births = 0;
        while colonies[ci].store >= cfg.birth_cost && births < cfg.max_births_per_tick {
            let Some(p) = colonies[ci].select_parent(ants, cfg.harvest_weight, cfg.homing_weight, rng) else {
                break;
            };
            let genome = ants.genome[p].mutated(cfg, rng);
            let lineage = ants.lineage[p].saturating_add(1);
            colonies[ci].store -= cfg.birth_cost;
            colonies[ci].births += 1;
            births += 1;
            spawn_into(
                ants,
                &colonies[ci],
                cid,
                genome,
                lineage,
                cfg,
                tick,
                next_id,
                rng,
            );
            colonies[ci].next_lineage_hint = colonies[ci].next_lineage_hint.max(lineage);
        }
    }
}

/// Reseed a collapsed colony with a fresh founding cohort, drawn from the world
/// reservoir. Genesis founding (`world.rs::new`) reproduced exactly, but with
/// genes from the reservoir instead of `Genome::random`:
///
/// - `initial_ants_per_colony` founders, same count as genesis.
/// - Each founder is an **independent** fitness-weighted draw from the union of
///   every colony's hall of fame, then mutated — so the cohort is a *hybrid* of
///   what is working across the map, a new competing lineage rather than a
///   photocopy of the current winner.
/// - Founder attributes mirror genesis: full energy, size 1.0, random heading,
///   on the colony's own nest tiles. No starter store, no grace period.
/// - Lineage is the drawn parent's archived depth + 1 (a descendant of a proven
///   queen). Cold start — an empty reservoir before any colony has archived a
///   death — falls back to `Genome::random` at lineage 0, identical to genesis.
fn refound(
    ants: &mut Ants,
    colonies: &mut [ColonyState],
    ci: usize,
    cfg: &Config,
    tick: u64,
    next_id: &mut u64,
    rng: &mut Pcg32,
) {
    let cid = colonies[ci].id;
    for _ in 0..cfg.initial_ants_per_colony {
        let (genome, lineage) = match world_reservoir_parent(colonies, rng) {
            Some((g, parent_lineage)) => (g.mutated(cfg, rng), parent_lineage.saturating_add(1)),
            None => (Genome::random(rng), 0),
        };
        spawn_founder(ants, &colonies[ci], cid, genome, lineage, cfg, tick, next_id, rng);
        colonies[ci].next_lineage_hint = colonies[ci].next_lineage_hint.max(lineage);
    }
    colonies[ci].refounds += 1;
}

/// Spawn one full-energy, size-1.0 founder on a nest tile. Mirrors genesis
/// founding, unlike `spawn_into` (which spawns partly-fed, half-size newborns).
#[allow(clippy::too_many_arguments)]
fn spawn_founder(
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
    let heading = (rng.next_f32() * 2.0 - 1.0) * std::f32::consts::PI;
    ants.push(Spawn {
        id: *next_id,
        colony: cid,
        x,
        y,
        heading,
        // Founders start completely full, exactly like genesis founders — the
        // cohort must survive long enough for selection to act.
        energy: genome.max_energy(cfg, 1.0),
        size: 1.0,
        lineage,
        genome,
        birth_tick: tick,
    });
    *next_id += 1;
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
                id: i as u64,
                colony: *c,
                x: 4.0,
                y: 4.0,
                heading: 0.0,
                energy: 50.0,
                size: 1.0,
                lineage: 3,
                genome: Genome::random(&mut Pcg32::new(i as u64, 1)),
                birth_tick: 0,
            });
            ants.food_delivered[i] = *fitness;
        }
        let mut colonies: Vec<ColonyState> = (0..cfg.num_colonies).map(ColonyState::new).collect();
        for c in colonies.iter_mut() {
            c.nest_tiles = vec![0, 1, 2];
            c.nest_center = (4.0, 4.0);
        }
        (ants, colonies)
    }

    fn cfg() -> Config {
        Config {
            width: 16,
            height: 16,
            num_colonies: 2,
            ..Config::default()
        }
    }

    #[test]
    fn a_full_store_produces_a_birth_and_is_debited() {
        let c = cfg();
        let (mut ants, mut cols) = setup(&c, &[(0, 10.0)]);
        cols[0].store = c.birth_cost * 1.5;
        let mut id = 1;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(1, 1));
        // Colony-0 scoped: the empty colony 1 refounds this same tick, so the
        // global count is no longer just "parent + newborn".
        assert_eq!(ants.population(0), 2);
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
        assert_eq!(ants.population(0), 1);
    }

    #[test]
    fn births_are_rate_limited_per_tick() {
        let c = Config {
            max_births_per_tick: 2,
            ..cfg()
        };
        let (mut ants, mut cols) = setup(&c, &[(0, 10.0)]);
        cols[0].store = c.birth_cost * 100.0;
        let mut id = 1;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(1, 1));
        assert_eq!(ants.population(0), 3, "one parent plus two newborns");
    }

    #[test]
    fn a_newborn_joins_its_parents_colony() {
        let c = cfg();
        // Both colonies alive (neither refounds); only colony 1 can afford a
        // birth, so the single appended ant must be colony 1's.
        let (mut ants, mut cols) = setup(&c, &[(0, 1.0), (1, 10.0)]);
        cols[1].store = c.birth_cost;
        let mut id = 2;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(1, 1));
        assert_eq!(*ants.colony.last().unwrap(), 1);
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
        let c = Config {
            width: 16,
            height: 16,
            num_colonies: 2,
            ..Config::default()
        };
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
    fn a_dead_colony_is_refounded_with_a_full_cohort() {
        // Population zero is the sole trigger; the cohort is genesis-sized.
        let c = Config {
            initial_ants_per_colony: 4,
            ..cfg()
        };
        // Colony 0 empty (dead), colony 1 alive so it is not itself refounded.
        let (mut ants, mut cols) = setup(&c, &[(1, 1.0)]);
        cols[0].record_death(5.0, 0, &Genome::random(&mut Pcg32::new(9, 9)), 5);
        let mut id = 1;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(3, 3));
        assert_eq!(ants.population(0), 4, "the dead colony gets a full founding cohort");
        assert_eq!(cols[0].refounds, 1, "the collapse is counted");
    }

    #[test]
    fn a_living_colony_is_never_refounded() {
        let c = Config {
            initial_ants_per_colony: 4,
            ..cfg()
        };
        let (mut ants, mut cols) = setup(&c, &[(0, 1.0), (1, 1.0)]);
        let mut id = 2;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(5, 5));
        assert_eq!(ants.len(), 2, "no colony was at zero, so no refound");
        assert_eq!(cols[0].refounds, 0);
        assert_eq!(cols[1].refounds, 0);
    }

    #[test]
    fn a_refound_draws_genes_from_another_colonys_archive() {
        // The scoped inverse of gene-pool sealing: colony 0 is dead with only a
        // shallow archive; colony 1's superstar (archived at depth 41) is in the
        // world reservoir, so a colony-0 founder inherits that proven depth + 1.
        let c = Config {
            initial_ants_per_colony: 3,
            ..cfg()
        };
        let (mut ants, mut cols) = setup(&c, &[(1, 1.0)]);
        // Only colony 1 has archived anything, at lineage depth 41.
        cols[1].record_death(1000.0, 41, &Genome::random(&mut Pcg32::new(9, 9)), 5);
        let mut id = 2;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(3, 3));
        let founder = (0..ants.len()).find(|&i| ants.colony[i] == 0).unwrap();
        assert_eq!(
            ants.lineage[founder], 42,
            "a refounded founder descends from the reservoir's proven queen"
        );
    }

    #[test]
    fn a_cold_start_refound_falls_back_to_random_at_lineage_zero() {
        // Before any colony has archived a death, the reservoir is empty; a
        // refound then mirrors genesis exactly — random genomes at lineage 0.
        let c = Config {
            initial_ants_per_colony: 2,
            ..cfg()
        };
        let (mut ants, mut cols) = setup(&c, &[(1, 1.0)]);
        let mut id = 1;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(4, 4));
        assert_eq!(ants.population(0), 2);
        for i in 0..ants.len() {
            if ants.colony[i] == 0 {
                assert_eq!(ants.lineage[i], 0, "cold-start founders start at genesis depth");
            }
        }
    }

    #[test]
    fn refounded_founders_mirror_genesis_full_energy_and_size_one() {
        let c = Config {
            initial_ants_per_colony: 2,
            ..cfg()
        };
        let (mut ants, mut cols) = setup(&c, &[(1, 1.0)]);
        let mut id = 1;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(4, 4));
        for i in 0..ants.len() {
            if ants.colony[i] == 0 {
                assert_eq!(ants.size[i], 1.0, "founders are full size, not newborns");
                let full = ants.genome[i].max_energy(&c, 1.0);
                assert!((ants.energy[i] - full).abs() < 1e-4, "founders start full");
            }
        }
    }

    #[test]
    fn ant_ids_stay_strictly_increasing_across_births() {
        let c = Config {
            max_births_per_tick: 3,
            ..cfg()
        };
        let (mut ants, mut cols) = setup(&c, &[(0, 1.0)]);
        cols[0].store = c.birth_cost * 10.0;
        cols[1].store = c.birth_cost * 10.0;
        let mut id = 1;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(6, 6));
        assert!(ants.id.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn reproduction_is_deterministic() {
        let c = cfg();
        let run = || {
            let (mut ants, mut cols) = setup(&c, &[(0, 4.0)]);
            cols[0].store = c.birth_cost * 3.0;
            let mut id = 1;
            reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(7, 7));
            (
                ants.len(),
                ants.genome.iter().map(|g| g.params[0]).collect::<Vec<_>>(),
            )
        };
        assert_eq!(run(), run());
    }
}
