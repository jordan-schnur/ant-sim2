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
        Genome {
            params,
            traits: Traits::from_array(traits_arr),
        }
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
        let cfg = Config {
            mutation_sigma: 10.0,
            big_jump_chance: 1.0,
            ..Config::default()
        };
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
