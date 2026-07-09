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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn small() -> Config {
        Config {
            width: 8,
            height: 4,
            ..Config::default()
        }
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
