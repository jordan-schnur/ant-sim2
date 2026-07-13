use crate::genome::Genome;
use crate::rng::Pcg32;
use crate::N_MEMORY;
use serde::{Deserialize, Serialize};

pub struct Spawn {
    pub id: u64,
    pub colony: u8,
    pub x: f32,
    pub y: f32,
    pub heading: f32,
    pub energy: f32,
    pub size: f32,
    pub lineage: u32,
    pub genome: Genome,
    pub birth_tick: u64,
}

/// Struct-of-arrays. The think phase streams position/energy/size across all
/// ants; a `Vec<Ant>` would pull 4.5 KB genomes through cache to read an f32.
///
/// Invariant: `id` is strictly increasing. Iterating `0..len()` therefore
/// iterates in ant-id order, which is what makes the serial apply phase's
/// "lowest id wins" rule well defined.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Ants {
    pub id: Vec<u64>,
    pub colony: Vec<u8>,
    pub x: Vec<f32>,
    pub y: Vec<f32>,
    pub heading: Vec<f32>,
    pub energy: Vec<f32>,
    pub size: Vec<f32>,
    pub age: Vec<u32>,
    pub carrying: Vec<f32>,
    pub lineage: Vec<u32>,
    /// Lifetime food delivered to the nest. The real objective.
    pub food_delivered: Vec<f32>,
    /// Lifetime food grabbed into cargo, whether or not it was ever delivered.
    /// A dense fitness stepping stone: see `Config::fitness`. Serialized (unlike
    /// `attacking`) because fitness must survive save/load.
    pub food_harvested: Vec<f32>,
    pub memory: Vec<[f32; N_MEMORY]>,
    pub genome: Vec<Genome>,
    pub rng: Vec<Pcg32>,
    pub alive: Vec<bool>,

    /// Landed an attack this tick. Pure observation: nothing senses it and it
    /// never feeds an intent, so it cannot perturb the trajectory.
    ///
    /// Not serialised. It is derived per-tick, and keeping it out of the
    /// snapshot means adding it did not change the wire layout of a saved
    /// `World` — the golden master stayed valid. `World::rebuild_index`
    /// restores its length after a load; until then it is empty, which is why
    /// reads go through `is_attacking`.
    #[serde(skip)]
    pub attacking: Vec<bool>,
}

impl Ants {
    pub fn new() -> Self {
        Self::default()
    }

    /// Tolerates the empty vector left by deserialisation.
    #[inline]
    pub fn is_attacking(&self, i: usize) -> bool {
        self.attacking.get(i).copied().unwrap_or(false)
    }

    /// Called at the top of each tick. Also repairs the length after a load.
    pub fn clear_attacking(&mut self) {
        self.attacking.clear();
        self.attacking.resize(self.id.len(), false);
    }

    pub fn len(&self) -> usize {
        self.id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.id.is_empty()
    }

    pub fn push(&mut self, s: Spawn) {
        debug_assert!(
            self.id.last().map_or(true, |&last| s.id > last),
            "ant ids must be pushed in increasing order"
        );
        // Reserved, and currently unread. Every ant carries a private stream
        // seeded from (id, birth_tick) so that any future *stochastic* ant
        // behaviour — noisy sensors, probabilistic actions — stays independent
        // of thread scheduling. Today the think phase is fully deterministic
        // and all randomness lives in the serial reproduce phase, drawing from
        // `World::rng`. Keep this field: adding it later would change every
        // ant's stream and invalidate the golden master.
        self.rng
            .push(Pcg32::new(s.id, s.birth_tick.wrapping_add(1)));
        self.id.push(s.id);
        self.colony.push(s.colony);
        self.x.push(s.x);
        self.y.push(s.y);
        self.heading.push(s.heading);
        self.energy.push(s.energy);
        self.size.push(s.size);
        self.age.push(0);
        self.carrying.push(0.0);
        self.lineage.push(s.lineage);
        self.food_delivered.push(0.0);
        self.food_harvested.push(0.0);
        self.memory.push([0.0; N_MEMORY]);
        self.genome.push(s.genome);
        self.alive.push(true);
        self.attacking.push(false);
    }

    /// Floored cell.
    ///
    /// The upper bound is guaranteed by `apply_movement`, not re-checked here:
    /// a move that would leave the grid is rejected (`Grid::is_stone` reports
    /// out-of-bounds as stone), and a move that stays within the current cell
    /// cannot cross `width`. `World`'s `every_ant_stays_on_the_map` test pins
    /// that invariant. The `debug_assert` catches a non-finite position, which
    /// would otherwise cast to 0 and silently teleport the ant to the corner.
    #[inline]
    pub fn cell(&self, i: usize) -> (u16, u16) {
        debug_assert!(
            self.x[i].is_finite() && self.y[i].is_finite(),
            "ant {i} has a non-finite position: ({}, {})",
            self.x[i],
            self.y[i]
        );
        (self.x[i].max(0.0) as u16, self.y[i].max(0.0) as u16)
    }

    pub fn population(&self, colony: u8) -> u32 {
        self.colony
            .iter()
            .zip(&self.alive)
            .filter(|(c, a)| **c == colony && **a)
            .count() as u32
    }

    /// Order-preserving compaction. Order preservation is load-bearing: it is
    /// what keeps `id` sorted across ticks.
    pub fn retain_alive(&mut self) {
        let keep = self.alive.clone();
        let mut k = 0usize;
        retain(&mut self.id, &keep);
        retain(&mut self.colony, &keep);
        retain(&mut self.x, &keep);
        retain(&mut self.y, &keep);
        retain(&mut self.heading, &keep);
        retain(&mut self.energy, &keep);
        retain(&mut self.size, &keep);
        retain(&mut self.age, &keep);
        retain(&mut self.carrying, &keep);
        retain(&mut self.lineage, &keep);
        retain(&mut self.food_delivered, &keep);
        retain(&mut self.food_harvested, &keep);
        retain(&mut self.memory, &keep);
        retain(&mut self.genome, &keep);
        retain(&mut self.rng, &keep);
        if self.attacking.len() == keep.len() {
            retain(&mut self.attacking, &keep);
        }
        self.alive.retain(|_| {
            let v = keep[k];
            k += 1;
            v
        });
    }
}

fn retain<T>(v: &mut Vec<T>, keep: &[bool]) {
    let mut i = 0usize;
    v.retain(|_| {
        let k = keep[i];
        i += 1;
        k
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genome::Genome;
    use crate::rng::Pcg32;

    fn spawn(id: u64, colony: u8, x: f32, y: f32) -> Spawn {
        Spawn {
            id,
            colony,
            x,
            y,
            heading: 0.0,
            energy: 50.0,
            size: 1.0,
            lineage: 0,
            genome: Genome::random(&mut Pcg32::new(id, 1)),
            birth_tick: 0,
        }
    }

    #[test]
    fn push_appends_and_len_tracks() {
        let mut a = Ants::new();
        assert!(a.is_empty());
        a.push(spawn(0, 0, 1.0, 1.0));
        a.push(spawn(1, 0, 2.0, 2.0));
        assert_eq!(a.len(), 2);
        assert_eq!(a.id, vec![0, 1]);
    }

    #[test]
    fn every_parallel_vec_has_the_same_length() {
        let mut a = Ants::new();
        for i in 0..5 {
            a.push(spawn(i, 0, 0.0, 0.0));
        }
        let n = a.len();
        assert_eq!(a.colony.len(), n);
        assert_eq!(a.x.len(), n);
        assert_eq!(a.y.len(), n);
        assert_eq!(a.heading.len(), n);
        assert_eq!(a.energy.len(), n);
        assert_eq!(a.size.len(), n);
        assert_eq!(a.age.len(), n);
        assert_eq!(a.carrying.len(), n);
        assert_eq!(a.lineage.len(), n);
        assert_eq!(a.food_delivered.len(), n);
        assert_eq!(a.food_harvested.len(), n);
        assert_eq!(a.memory.len(), n);
        assert_eq!(a.genome.len(), n);
        assert_eq!(a.rng.len(), n);
        assert_eq!(a.alive.len(), n);
        assert_eq!(a.attacking.len(), n);
    }

    #[test]
    fn cell_floors_the_position() {
        let mut a = Ants::new();
        a.push(spawn(0, 0, 3.9, 7.1));
        assert_eq!(a.cell(0), (3, 7));
    }

    #[test]
    fn retain_alive_preserves_id_order() {
        let mut a = Ants::new();
        for i in 0..6 {
            a.push(spawn(i, 0, i as f32, 0.0));
        }
        a.alive[1] = false;
        a.alive[4] = false;
        a.retain_alive();
        assert_eq!(a.id, vec![0, 2, 3, 5]);
        assert_eq!(a.x, vec![0.0, 2.0, 3.0, 5.0]);
        assert!(a.id.windows(2).all(|w| w[0] < w[1]), "ids must stay sorted");
    }

    #[test]
    fn retain_alive_keeps_vecs_in_lockstep() {
        let mut a = Ants::new();
        for i in 0..4 {
            a.push(spawn(i, (i % 2) as u8, 0.0, 0.0));
        }
        a.alive[0] = false;
        a.retain_alive();
        assert_eq!(a.len(), 3);
        assert_eq!(a.colony.len(), 3);
        assert_eq!(a.genome.len(), 3);
    }

    #[test]
    fn population_counts_only_the_named_colony() {
        let mut a = Ants::new();
        a.push(spawn(0, 1, 0.0, 0.0));
        a.push(spawn(1, 1, 0.0, 0.0));
        a.push(spawn(2, 2, 0.0, 0.0));
        assert_eq!(a.population(1), 2);
        assert_eq!(a.population(2), 1);
        assert_eq!(a.population(3), 0);
    }

    #[test]
    fn newborn_food_harvested_starts_at_zero() {
        let mut a = Ants::new();
        a.push(spawn(0, 0, 0.0, 0.0));
        assert_eq!(a.food_harvested[0], 0.0);
    }

    #[test]
    fn newborn_memory_starts_at_zero() {
        let mut a = Ants::new();
        a.push(spawn(0, 0, 0.0, 0.0));
        assert_eq!(a.memory[0], [0.0; crate::N_MEMORY]);
    }
}
