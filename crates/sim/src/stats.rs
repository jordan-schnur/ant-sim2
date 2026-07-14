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
    /// Times this colony collapsed to zero and was refounded from the world
    /// reservoir. A colony refounding repeatedly is thrashing, not thriving.
    /// Reported so the simulation never silently flatters a losing colony.
    pub refounds: u64,
    pub mean_size: f32,
    pub mean_lineage: f32,
    /// Lifetime food delivered by the ants alive *right now*. Falls when a
    /// productive ant dies, so it is a poor progress signal on its own.
    pub food_delivered: f32,
    /// Food delivered by every ant this colony has ever had. Monotonic. This is
    /// the curve to watch for "is evolution working".
    pub delivered_total: f32,
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
                refounds: c.refounds,
                mean_size: if population == 0 { 0.0 } else { size_sum / n },
                mean_lineage: if population == 0 {
                    0.0
                } else {
                    lineage_sum / n
                },
                food_delivered: delivered,
                delivered_total: c.delivered_total,
            }
        })
        .collect()
}

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
                id: i as u64,
                colony: *c,
                x: 0.0,
                y: 0.0,
                heading: 0.0,
                energy: 1.0,
                size: *size,
                lineage: *lineage,
                genome: Genome::random(&mut Pcg32::new(i as u64, 1)),
                birth_tick: 0,
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
    fn delivered_total_survives_the_death_of_the_ant_that_earned_it() {
        // `food_delivered` counts the living; `delivered_total` counts history.
        let mut ants = ants_with(&[(0, 1.0, 0, 40.0)]);
        let mut cols: Vec<ColonyState> = (0..1).map(ColonyState::new).collect();
        cols[0].delivered_total = 40.0;
        ants.alive[0] = false;
        let s = &colony_stats(&ants, &cols)[0];
        assert_eq!(s.food_delivered, 0.0, "the earner is dead");
        assert_eq!(s.delivered_total, 40.0, "the food still came home");
    }

    #[test]
    fn refounds_are_reported_so_collapse_thrash_is_visible() {
        let ants = ants_with(&[(0, 1.0, 0, 0.0)]);
        let mut cols: Vec<ColonyState> = (0..1).map(ColonyState::new).collect();
        cols[0].refounds = 17;
        assert_eq!(colony_stats(&ants, &cols)[0].refounds, 17);
    }
}
