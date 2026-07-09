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
        diffuse_decay(
            &mut self.food,
            self.width,
            self.height,
            cfg.food_diffusion,
            cfg.food_evaporation,
        );
        diffuse_decay(
            &mut self.alarm,
            self.width,
            self.height,
            cfg.alarm_diffusion,
            cfg.alarm_evaporation,
        );
        diffuse_decay(
            &mut self.scent,
            self.width,
            self.height,
            cfg.scent_diffusion,
            cfg.scent_evaporation,
        );
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
    // Diffusion must read the *previous* state everywhere, or the result
    // depends on iteration order.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn small() -> Config {
        Config {
            width: 8,
            height: 8,
            ..Config::default()
        }
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
        let cfg = Config {
            food_evaporation: 1.0,
            ..cfg
        };
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
