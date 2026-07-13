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
    /// Display name, generated deterministically from `(seed, id)` at worldgen.
    /// Empty only for a `ColonyState::new` built outside worldgen (tests).
    pub name: String,
    pub store: f32,
    pub nest_tiles: Vec<usize>,
    pub nest_center: (f32, f32),
    pub births: u64,
    pub deaths: u64,
    /// Every unit of food ever carried home, across all ants living and dead.
    ///
    /// This — not the sum of living ants' `food_delivered` — is the evolution
    /// signal. Summing the living undercounts every forager that has since died
    /// of old age, so it tracks population as much as skill and can fall while
    /// the colony is getting better.
    pub delivered_total: f32,
    /// Ants conjured by the extinction floor, free of charge. Surfaced in
    /// `ColonyStats` because this is the one place the simulation cheats:
    /// a colony propped up by the floor is not a colony that is winning, and
    /// the operator must be able to see the difference.
    pub floor_spawns: u64,
    pub last_floor_spawn: u64,
    /// Best genomes ever seen, by food delivered, sorted descending, each with
    /// the lineage depth of the ant that earned it. Used only by the extinction
    /// floor. A research-tool affordance, not biology.
    ///
    /// The lineage is stored because a floor-spawned ant is a *descendant* of
    /// its archived parent and must inherit its depth. Without it, a colony that
    /// lives on the floor — which is most of them — reports the same generation
    /// number forever.
    pub hall_of_fame: Vec<(f32, u32, Genome)>,
    pub next_lineage_hint: u32,
    /// One-shot chronicle flags: latched the first time the milestone happens.
    pub first_delivery_done: bool,
    pub first_kill_done: bool,
}

impl ColonyState {
    pub fn new(id: u8) -> Self {
        ColonyState {
            id,
            name: String::new(),
            store: 0.0,
            nest_tiles: Vec::new(),
            nest_center: (0.0, 0.0),
            births: 0,
            deaths: 0,
            delivered_total: 0.0,
            floor_spawns: 0,
            last_floor_spawn: 0,
            hall_of_fame: Vec::new(),
            next_lineage_hint: 0,
            first_delivery_done: false,
            first_kill_done: false,
        }
    }

    pub fn record_death(&mut self, fitness: f32, lineage: u32, genome: &Genome, cap: usize) {
        self.deaths += 1;
        if self.hall_of_fame.len() >= cap {
            // Sorted descending, so the last entry is the weakest.
            //
            // A *tie* displaces it. Strict `>=` here would freeze the archive of
            // any colony that has never delivered food: every corpse scores 0.0,
            // every 0.0 is rejected by the 0.0 already sitting there, and the
            // colony breeds forever from the ten genomes that happened to die
            // first. Since the extinction floor draws from this archive, that
            // turns the only reproduction path most colonies have into a
            // memoryless resample. Accepting ties makes a flat archive a
            // drifting population instead, so neutral mutations accumulate and
            // the search can cross the plateau to its first delivered crumb.
            if self
                .hall_of_fame
                .last()
                .map_or(false, |(f, _, _)| *f > fitness)
            {
                return;
            }
            self.hall_of_fame.pop();
        }
        // `<=`, not `<`: the newcomer goes in *front* of everyone it ties with.
        // With `<` an all-zero archive pops its tail and pushes straight back
        // into the slot it just freed, so nine of ten entries stay frozen at the
        // colony's first nine corpses and only one slot ever drifts. Tying to
        // the front instead makes the tie group a sliding window of the most
        // recent corpses, which is what lets neutral mutations accumulate down a
        // lineage. Elites are untouched: a genome is only displaced by one that
        // delivered at least as much.
        let pos = self
            .hall_of_fame
            .iter()
            .position(|(f, _, _)| *f <= fitness)
            .unwrap_or(self.hall_of_fame.len());
        self.hall_of_fame
            .insert(pos, (fitness, lineage, genome.clone()));
    }

    /// Roulette-wheel over living ants **of this colony only**, weighted by
    /// shaped fitness (`food_delivered + harvest_weight · food_harvested`).
    /// Accumulates in ant-index order, so the draw is reproducible for a given
    /// rng state. `harvest_weight = 0` recovers pure delivery weighting.
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
        // Float rounding can leave `target` a hair above zero; fall back to the
        // last eligible ant rather than returning None.
        last
    }

    /// A genome from the archive and the lineage depth it was at when it died.
    ///
    /// Roulette-weighted by food delivered, exactly as `select_parent` weights
    /// the living. A uniform draw would discard the ordering the archive is
    /// maintained in: a colony holding `[8, 8, 5.8, 4, 2, 2, 2, 0, 0, 0]` would
    /// breed from a genome known to deliver nothing 30% of the time. `PARENT_EPS`
    /// keeps an all-zero archive — a colony that has yet to deliver anything —
    /// samplable, and keeps its drifting tail in play as explorers.
    pub fn archive_parent(&self, rng: &mut Pcg32) -> Option<(&Genome, u32)> {
        if self.hall_of_fame.is_empty() {
            return None;
        }
        let total: f32 = self
            .hall_of_fame
            .iter()
            .map(|(f, _, _)| f + PARENT_EPS)
            .sum();
        let mut target = rng.next_f32() * total;
        for (fitness, lineage, genome) in &self.hall_of_fame {
            target -= fitness + PARENT_EPS;
            if target <= 0.0 {
                return Some((genome, *lineage));
            }
        }
        // Float rounding can leave `target` a hair above zero; fall back to the
        // weakest rather than returning None on a non-empty archive.
        let (_, lineage, genome) = self.hall_of_fame.last().unwrap();
        Some((genome, *lineage))
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
            c.record_death(f, 0, &genome(f as u64), 3);
        }
        let fits: Vec<f32> = c.hall_of_fame.iter().map(|(f, _, _)| *f).collect();
        assert_eq!(fits, vec![9.0, 7.0, 5.0]);
    }

    #[test]
    fn hall_of_fame_ignores_a_worse_genome_when_full() {
        let mut c = ColonyState::new(0);
        c.record_death(10.0, 0, &genome(1), 1);
        c.record_death(2.0, 0, &genome(2), 1);
        assert_eq!(c.hall_of_fame.len(), 1);
        assert_eq!(c.hall_of_fame[0].0, 10.0);
    }

    /// The one that mattered. A colony that has never delivered food scores
    /// every corpse 0.0. If a tie cannot displace the incumbent, the archive
    /// freezes at that colony's first `cap` corpses and never changes again —
    /// and since the extinction floor breeds from the archive, the colony
    /// spends the rest of the run taking one mutation step away from ten fixed
    /// genomes and discarding the result. Measured: colonies 1, 3 and 5 of
    /// seed 1 took 138 deaths across 20,000 ticks without the archive moving
    /// once, and delivered exactly zero over 500,000.
    ///
    /// Asserts the archive holds *exactly* the last `cap` corpses, newest
    /// first. A weaker "did anything change?" check passes even when only the
    /// tail slot churns and the other nine stay frozen — which is the bug this
    /// began as, one layer down.
    #[test]
    fn a_zero_fitness_archive_becomes_a_sliding_window_of_recent_corpses() {
        let mut c = ColonyState::new(0);
        for i in 0..8u64 {
            c.record_death(0.0, 0, &genome(i), 3);
        }
        let held: Vec<f32> = c.hall_of_fame.iter().map(|(_, _, g)| g.params[0]).collect();
        let want: Vec<f32> = [7u64, 6, 5].iter().map(|i| genome(*i).params[0]).collect();

        assert_eq!(c.hall_of_fame.len(), 3, "the cap still holds");
        assert_eq!(held, want, "every slot must drift, not just the tail");
    }

    /// Drift must not cost a colony its elites: neutral churn belongs in the
    /// tail, not at the top.
    #[test]
    fn drift_never_displaces_a_genome_that_actually_delivered() {
        let mut c = ColonyState::new(0);
        c.record_death(12.0, 0, &genome(1), 3);
        for i in 0..20 {
            c.record_death(0.0, 0, &genome(i + 10), 3);
        }
        assert_eq!(
            c.hall_of_fame[0].0, 12.0,
            "the forager was evicted by drift"
        );
    }

    #[test]
    fn record_death_increments_the_death_counter() {
        let mut c = ColonyState::new(0);
        c.record_death(1.0, 0, &genome(1), 5);
        c.record_death(1.0, 0, &genome(2), 5);
        assert_eq!(c.deaths, 2);
    }

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

    #[test]
    fn select_parent_only_ever_returns_own_colony() {
        let c = ColonyState::new(1);
        let ants = ants_with(&[(1, 5.0), (2, 500.0), (1, 5.0)]);
        let mut r = Pcg32::new(1, 1);
        for _ in 0..200 {
            let p = c.select_parent(&ants, 0.0, &mut r).unwrap();
            assert_eq!(ants.colony[p], 1, "gene pools must never mix");
        }
    }

    #[test]
    fn select_parent_favours_higher_food_delivered() {
        let c = ColonyState::new(1);
        let ants = ants_with(&[(1, 0.0), (1, 1000.0)]);
        let mut r = Pcg32::new(2, 2);
        let wins = (0..1000)
            .filter(|_| c.select_parent(&ants, 0.0, &mut r) == Some(1))
            .count();
        assert!(wins > 900, "productive ant won only {wins}/1000");
    }

    #[test]
    fn select_parent_never_strictly_excludes_a_zero_fitness_ant() {
        let c = ColonyState::new(1);
        let ants = ants_with(&[(1, 0.0), (1, 0.0)]);
        let mut r = Pcg32::new(3, 3);
        let a = (0..500)
            .filter(|_| c.select_parent(&ants, 0.0, &mut r) == Some(0))
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
        assert_eq!(c.select_parent(&ants, 0.0, &mut r), Some(1));
    }

    #[test]
    fn select_parent_returns_none_for_an_empty_colony() {
        let c = ColonyState::new(9);
        let ants = ants_with(&[(1, 1.0)]);
        assert_eq!(c.select_parent(&ants, 0.0, &mut Pcg32::new(5, 5)), None);
    }

    #[test]
    fn select_parent_is_deterministic_for_a_given_rng() {
        let c = ColonyState::new(1);
        let ants = ants_with(&[(1, 3.0), (1, 4.0), (1, 5.0)]);
        let a: Vec<_> = (0..20)
            .scan(Pcg32::new(6, 6), |r, _| Some(c.select_parent(&ants, 0.0, r)))
            .collect();
        let b: Vec<_> = (0..20)
            .scan(Pcg32::new(6, 6), |r, _| Some(c.select_parent(&ants, 0.0, r)))
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
        c.record_death(1.0, 0, &genome(1), 5);
        assert!(c.archive_parent(&mut Pcg32::new(8, 8)).is_some());
    }

    /// The archive is sorted by food delivered and the floor breeds from it, so
    /// a uniform draw throws that ordering away. Colony 0 of seed 1 held
    /// `[8, 8, 5.8, 4, 2, 2, 2, 0, 0, 0]`: a 30% chance of breeding from a
    /// genome known to deliver nothing.
    #[test]
    fn archive_parent_favours_the_fittest_genome() {
        let mut c = ColonyState::new(0);
        c.record_death(0.0, 0, &genome(1), 5);
        c.record_death(1000.0, 0, &genome(2), 5);
        let target = c
            .hall_of_fame
            .iter()
            .position(|(f, _, _)| *f == 1000.0)
            .unwrap();
        let want = c.hall_of_fame[target].2.params[0];

        let mut r = Pcg32::new(11, 11);
        let wins = (0..1000)
            .filter(|_| c.archive_parent(&mut r).unwrap().0.params[0] == want)
            .count();
        assert!(wins > 900, "the productive genome won only {wins}/1000");
    }

    /// An all-zero archive still has to hand something back, or a colony that
    /// has never delivered can never be re-seeded at all.
    #[test]
    fn archive_parent_still_draws_from_an_all_zero_archive() {
        let mut c = ColonyState::new(0);
        for i in 0..4 {
            c.record_death(0.0, 0, &genome(i), 4);
        }
        let mut r = Pcg32::new(12, 12);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..200 {
            seen.insert(c.archive_parent(&mut r).unwrap().0.params[0].to_bits());
        }
        assert!(
            seen.len() > 1,
            "a flat archive must still be sampled broadly"
        );
    }

    #[test]
    fn the_archive_remembers_how_deep_a_lineage_was() {
        // A floor-spawned descendant needs its parent's depth, or the colony's
        // generation counter never advances.
        let mut c = ColonyState::new(0);
        c.record_death(5.0, 41, &genome(1), 5);
        let (_, lineage) = c.archive_parent(&mut Pcg32::new(8, 8)).unwrap();
        assert_eq!(lineage, 41);
    }
}
