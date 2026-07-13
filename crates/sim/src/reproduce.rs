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
        let interval_elapsed = tick
            >= colonies[ci]
                .last_floor_spawn
                .saturating_add(cfg.floor_respawn_interval);
        if below_floor && (interval_elapsed || colonies[ci].floor_spawns == 0) {
            // A floor-spawned ant is a descendant of its archived parent and
            // inherits its lineage depth. Falling back on `next_lineage_hint`
            // for every free ant — as an earlier version did — freezes the
            // generation counter of any colony that lives on the floor, which
            // over a long run is nearly all of them.
            let (genome, parent_lineage) = match colonies[ci].archive_parent(rng) {
                Some((g, l)) => (g.mutated(cfg, rng), l),
                None => (Genome::random(rng), colonies[ci].next_lineage_hint),
            };
            let lineage = parent_lineage.saturating_add(1);
            colonies[ci].next_lineage_hint = colonies[ci].next_lineage_hint.max(lineage);
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
            colonies[ci].floor_spawns += 1;
            colonies[ci].last_floor_spawn = tick;
        }

        // --- Paid births from the food store. ---
        let mut births = 0;
        while colonies[ci].store >= cfg.birth_cost && births < cfg.max_births_per_tick {
            let Some(p) = colonies[ci].select_parent(ants, cfg.harvest_weight, rng) else {
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
            extinction_floor: 0,
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
        let c = Config {
            max_births_per_tick: 2,
            ..cfg()
        };
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
        let c = Config {
            width: 16,
            height: 16,
            num_colonies: 2,
            extinction_floor: 0,
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
    fn a_colony_below_the_floor_gets_one_free_ant_from_its_archive() {
        let c = Config {
            extinction_floor: 3,
            ..cfg()
        };
        let (mut ants, mut cols) = setup(&c, &[(0, 5.0)]);
        cols[0].store = 0.0;
        cols[0].record_death(9.0, 0, &Genome::random(&mut Pcg32::new(9, 9)), 5);
        let mut id = 1;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(3, 3));
        assert_eq!(ants.population(0), 2, "one free ant, not a full top-up");
        assert_eq!(cols[0].store, 0.0, "free ants cost nothing");
        assert_eq!(cols[0].floor_spawns, 1, "the cheat is counted");
    }

    #[test]
    fn free_ants_are_rate_limited_to_one_per_interval() {
        let c = Config {
            extinction_floor: 5,
            floor_respawn_interval: 100,
            ..cfg()
        };
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
        let c = Config {
            extinction_floor: 2,
            ..cfg()
        };
        let (mut ants, mut cols) = setup(&c, &[]);
        let mut id = 0;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(4, 4));
        assert_eq!(ants.population(0), 1);
        assert_eq!(ants.population(1), 1);
    }

    #[test]
    fn a_colony_at_the_floor_is_not_topped_up() {
        let c = Config {
            extinction_floor: 1,
            ..cfg()
        };
        let (mut ants, mut cols) = setup(&c, &[(0, 1.0), (1, 1.0)]);
        let mut id = 2;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(5, 5));
        assert_eq!(ants.len(), 2);
        assert_eq!(cols[0].floor_spawns, 0);
    }

    #[test]
    fn a_floor_spawn_is_one_generation_deeper_than_its_archived_parent() {
        // Previously a free ant took `next_lineage_hint + 1`, a global maximum
        // unrelated to whichever genome the archive actually handed back. A
        // colony living on the floor — over a long run, nearly all of them —
        // therefore reported the same generation number forever.
        let c = Config {
            extinction_floor: 2,
            ..cfg()
        };
        let (mut ants, mut cols) = setup(&c, &[]);
        cols[0].record_death(9.0, 41, &Genome::random(&mut Pcg32::new(9, 9)), 5);
        let mut id = 0;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(3, 3));

        let free_ant = (0..ants.len()).find(|&i| ants.colony[i] == 0).unwrap();
        assert_eq!(
            ants.lineage[free_ant], 42,
            "should descend from the archive"
        );
        assert_eq!(cols[0].next_lineage_hint, 42);
    }

    #[test]
    fn an_empty_archive_falls_back_to_the_colonys_own_depth() {
        let c = Config {
            extinction_floor: 2,
            ..cfg()
        };
        let (mut ants, mut cols) = setup(&c, &[]);
        cols[0].next_lineage_hint = 7;
        let mut id = 0;
        reproduce(&mut ants, &mut cols, &c, 0, &mut id, &mut Pcg32::new(4, 4));
        let free_ant = (0..ants.len()).find(|&i| ants.colony[i] == 0).unwrap();
        assert_eq!(ants.lineage[free_ant], 8);
    }

    #[test]
    fn a_colony_can_never_be_permanently_extinct() {
        let c = Config {
            extinction_floor: 3,
            floor_respawn_interval: 10,
            ..cfg()
        };
        let (mut ants, mut cols) = setup(&c, &[]);
        let mut id = 0;
        let mut rng = Pcg32::new(8, 8);
        for t in 0..100 {
            reproduce(&mut ants, &mut cols, &c, t, &mut id, &mut rng);
        }
        assert_eq!(
            ants.population(0),
            3,
            "should have trickled back up to the floor"
        );
    }

    #[test]
    fn ant_ids_stay_strictly_increasing_across_births() {
        let c = Config {
            extinction_floor: 2,
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
        let c = Config {
            extinction_floor: 2,
            ..cfg()
        };
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
