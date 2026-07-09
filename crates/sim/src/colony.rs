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
                id: i as u64,
                colony: *c,
                x: 0.0,
                y: 0.0,
                heading: 0.0,
                energy: 10.0,
                size: 1.0,
                lineage: 0,
                genome: genome(i as u64),
                birth_tick: 0,
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
        let wins = (0..1000)
            .filter(|_| c.select_parent(&ants, &mut r) == Some(1))
            .count();
        assert!(wins > 900, "productive ant won only {wins}/1000");
    }

    #[test]
    fn select_parent_never_strictly_excludes_a_zero_fitness_ant() {
        let c = ColonyState::new(1);
        let ants = ants_with(&[(1, 0.0), (1, 0.0)]);
        let mut r = Pcg32::new(3, 3);
        let a = (0..500)
            .filter(|_| c.select_parent(&ants, &mut r) == Some(0))
            .count();
        assert!(
            a > 100 && a < 400,
            "PARENT_EPS should keep it roughly fair, got {a}/500"
        );
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
        let a: Vec<_> = (0..20)
            .scan(Pcg32::new(6, 6), |r, _| Some(c.select_parent(&ants, r)))
            .collect();
        let b: Vec<_> = (0..20)
            .scan(Pcg32::new(6, 6), |r, _| Some(c.select_parent(&ants, r)))
            .collect();
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
