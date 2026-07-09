use crate::ants::Ants;
use crate::config::Config;

pub const NO_OCCUPANT: u32 = u32::MAX;

/// Compressed-sparse-row index from cell to the ants standing in it, rebuilt
/// each tick by counting sort. Ants are scattered in index order, so
/// `cell_ants` is always sorted ascending — which is what makes the serial
/// apply phase's tie-breaks deterministic.
#[derive(Clone)]
pub struct Spatial {
    width: u16,
    height: u16,
    cell_start: Vec<u32>,
    items: Vec<u32>,
    occupant: Vec<u32>,
}

impl Default for Spatial {
    fn default() -> Self {
        Spatial {
            width: 0,
            height: 0,
            cell_start: vec![0],
            items: Vec::new(),
            occupant: Vec::new(),
        }
    }
}

impl Spatial {
    /// Re-shape an index for a given config. Used after loading a snapshot,
    /// where the index is rebuilt rather than serialised.
    pub fn resize(&mut self, cfg: &Config) {
        *self = Spatial::new(cfg);
    }

    pub fn cell_count(&self) -> usize {
        self.occupant.len()
    }

    pub fn new(cfg: &Config) -> Self {
        Spatial {
            width: cfg.width,
            height: cfg.height,
            cell_start: vec![0; cfg.cell_count() + 1],
            items: Vec::new(),
            occupant: vec![NO_OCCUPANT; cfg.cell_count()],
        }
    }

    #[inline]
    fn idx(&self, x: i32, y: i32) -> usize {
        y as usize * self.width as usize + x as usize
    }

    pub fn rebuild(&mut self, ants: &Ants) {
        let cells = self.cell_start.len() - 1;
        self.cell_start.iter_mut().for_each(|c| *c = 0);
        self.occupant.iter_mut().for_each(|o| *o = NO_OCCUPANT);

        let mut counts = vec![0u32; cells];
        for i in 0..ants.len() {
            if !ants.alive[i] {
                continue;
            }
            let (x, y) = ants.cell(i);
            counts[self.idx(x as i32, y as i32)] += 1;
        }

        let mut acc = 0u32;
        for c in 0..cells {
            self.cell_start[c] = acc;
            acc += counts[c];
        }
        self.cell_start[cells] = acc;

        self.items.clear();
        self.items.resize(acc as usize, 0);
        let mut cursor: Vec<u32> = self.cell_start[..cells].to_vec();
        for i in 0..ants.len() {
            if !ants.alive[i] {
                continue;
            }
            let (x, y) = ants.cell(i);
            let c = self.idx(x as i32, y as i32);
            self.items[cursor[c] as usize] = i as u32;
            cursor[c] += 1;
            if self.occupant[c] == NO_OCCUPANT {
                self.occupant[c] = i as u32;
            }
        }
    }

    pub fn cell_ants(&self, i: usize) -> &[u32] {
        let s = self.cell_start[i] as usize;
        let e = self.cell_start[i + 1] as usize;
        &self.items[s..e]
    }

    #[inline]
    pub fn occupant(&self, i: usize) -> Option<u32> {
        match self.occupant[i] {
            NO_OCCUPANT => None,
            v => Some(v),
        }
    }

    #[inline]
    pub fn set_occupant(&mut self, i: usize, ant: u32) {
        self.occupant[i] = ant;
    }

    #[inline]
    pub fn clear_occupant(&mut self, i: usize) {
        self.occupant[i] = NO_OCCUPANT;
    }

    /// Square neighbourhood of radius `r`, inclusive. Counts the querying ant
    /// itself among the friends, which the sensor normalises away.
    pub fn counts_in_radius(
        &self,
        ants: &Ants,
        cx: i32,
        cy: i32,
        r: i32,
        colony: u8,
    ) -> (u32, u32) {
        let (mut friends, mut foes) = (0, 0);
        for y in (cy - r).max(0)..=(cy + r).min(self.height as i32 - 1) {
            for x in (cx - r).max(0)..=(cx + r).min(self.width as i32 - 1) {
                for &a in self.cell_ants(self.idx(x, y)) {
                    let a = a as usize;
                    if !ants.alive[a] {
                        continue;
                    }
                    if ants.colony[a] == colony {
                        friends += 1;
                    } else {
                        foes += 1;
                    }
                }
            }
        }
        (friends, foes)
    }

    /// Lowest-indexed living ant of another colony in the 3x3 block centred on
    /// `(cx, cy)`. Deterministic by construction.
    pub fn first_adjacent_foe(&self, ants: &Ants, cx: i32, cy: i32, colony: u8) -> Option<u32> {
        let mut best: Option<u32> = None;
        for y in (cy - 1).max(0)..=(cy + 1).min(self.height as i32 - 1) {
            for x in (cx - 1).max(0)..=(cx + 1).min(self.width as i32 - 1) {
                for &a in self.cell_ants(self.idx(x, y)) {
                    let ai = a as usize;
                    if ants.alive[ai] && ants.colony[ai] != colony && best.map_or(true, |b| a < b) {
                        best = Some(a);
                    }
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ants::{Ants, Spawn};
    use crate::config::Config;
    use crate::genome::Genome;
    use crate::rng::Pcg32;

    fn cfg() -> Config {
        Config {
            width: 8,
            height: 8,
            ..Config::default()
        }
    }

    fn ants_at(positions: &[(f32, f32, u8)]) -> Ants {
        let mut a = Ants::new();
        for (i, (x, y, c)) in positions.iter().enumerate() {
            a.push(Spawn {
                id: i as u64,
                colony: *c,
                x: *x,
                y: *y,
                heading: 0.0,
                energy: 10.0,
                size: 1.0,
                lineage: 0,
                genome: Genome::random(&mut Pcg32::new(i as u64, 1)),
                birth_tick: 0,
            });
        }
        a
    }

    #[test]
    fn cell_ants_lists_occupants_of_that_cell() {
        let c = cfg();
        let ants = ants_at(&[(2.5, 3.5, 0), (2.1, 3.9, 1), (5.0, 5.0, 0)]);
        let mut s = Spatial::new(&c);
        s.rebuild(&ants);
        let cell = 3 * 8 + 2;
        assert_eq!(s.cell_ants(cell), &[0, 1]);
        assert_eq!(s.cell_ants(5 * 8 + 5), &[2]);
        assert!(s.cell_ants(0).is_empty());
    }

    #[test]
    fn cell_ants_are_sorted_by_ant_index() {
        let c = cfg();
        let ants = ants_at(&[(1.0, 1.0, 0), (1.5, 1.5, 0), (1.2, 1.2, 0)]);
        let mut s = Spatial::new(&c);
        s.rebuild(&ants);
        assert_eq!(s.cell_ants(8 + 1), &[0, 1, 2]);
    }

    #[test]
    fn occupant_defaults_to_the_lowest_index_in_the_cell() {
        let c = cfg();
        let ants = ants_at(&[(4.0, 4.0, 0), (4.5, 4.5, 0)]);
        let mut s = Spatial::new(&c);
        s.rebuild(&ants);
        assert_eq!(s.occupant(4 * 8 + 4), Some(0));
    }

    #[test]
    fn occupant_is_none_for_an_empty_cell() {
        let c = cfg();
        let mut s = Spatial::new(&c);
        s.rebuild(&ants_at(&[]));
        assert_eq!(s.occupant(0), None);
    }

    #[test]
    fn set_and_clear_occupant() {
        let c = cfg();
        let mut s = Spatial::new(&c);
        s.rebuild(&ants_at(&[]));
        s.set_occupant(9, 3);
        assert_eq!(s.occupant(9), Some(3));
        s.clear_occupant(9);
        assert_eq!(s.occupant(9), None);
    }

    #[test]
    fn counts_in_radius_splits_friend_from_foe() {
        let c = cfg();
        let ants = ants_at(&[(4.0, 4.0, 1), (5.0, 4.0, 1), (3.0, 4.0, 2), (0.0, 0.0, 2)]);
        let mut s = Spatial::new(&c);
        s.rebuild(&ants);
        let (friends, foes) = s.counts_in_radius(&ants, 4, 4, 1, 1);
        assert_eq!(friends, 2, "self plus the neighbour at (5,4)");
        assert_eq!(foes, 1, "the ant at (3,4); the one at (0,0) is out of range");
    }

    #[test]
    fn counts_in_radius_clips_at_the_border() {
        let c = cfg();
        let ants = ants_at(&[(0.0, 0.0, 1)]);
        let mut s = Spatial::new(&c);
        s.rebuild(&ants);
        let (friends, foes) = s.counts_in_radius(&ants, 0, 0, 2, 1);
        assert_eq!((friends, foes), (1, 0));
    }

    #[test]
    fn first_adjacent_foe_ignores_nestmates() {
        let c = cfg();
        let ants = ants_at(&[(4.0, 4.0, 1), (5.0, 4.0, 1)]);
        let mut s = Spatial::new(&c);
        s.rebuild(&ants);
        assert_eq!(s.first_adjacent_foe(&ants, 4, 4, 1), None);
    }

    #[test]
    fn first_adjacent_foe_picks_the_lowest_index() {
        let c = cfg();
        // Two foes adjacent; ant index 2 sits at (5,4), index 1 at (3,4).
        let ants = ants_at(&[(4.0, 4.0, 1), (3.0, 4.0, 2), (5.0, 4.0, 2)]);
        let mut s = Spatial::new(&c);
        s.rebuild(&ants);
        assert_eq!(s.first_adjacent_foe(&ants, 4, 4, 1), Some(1));
    }

    #[test]
    fn first_adjacent_foe_skips_the_dead() {
        let c = cfg();
        let mut ants = ants_at(&[(4.0, 4.0, 1), (3.0, 4.0, 2)]);
        ants.alive[1] = false;
        let mut s = Spatial::new(&c);
        s.rebuild(&ants);
        assert_eq!(s.first_adjacent_foe(&ants, 4, 4, 1), None);
    }
}
