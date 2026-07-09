use crate::ants::Ants;
use crate::config::Config;
use crate::grid::Grid;
use crate::pheromone::Pheromones;
use crate::spatial::Spatial;
use crate::{N_INPUTS, N_MEMORY};

// --- Input vector layout. These indices are the contract with `brain.rs`. ---
pub const IN_WHISKERS: usize = 0; // 5 whiskers x 6 channels = 30
pub const IN_UNDERFOOT: usize = 30; // food, food-pheromone, alarm
pub const IN_COUNTS: usize = 33; // friends, foes
pub const IN_PROPRIO: usize = 35; // energy, size, carrying, age
pub const IN_BIAS: usize = 39;
pub const IN_MEMORY: usize = 40; // N_MEMORY recurrent values

/// Radians relative to the ant's heading. Antennae, not eyes.
pub const WHISKER_ANGLES: [f32; 5] = [-1.2, -0.6, 0.0, 0.6, 1.2];
pub const CHANNELS_PER_WHISKER: usize = 6;

pub const CH_FOOD: usize = 0;
pub const CH_FOOD_PHERO: usize = 1;
pub const CH_ALARM: usize = 2;
pub const CH_OWN_SCENT: usize = 3;
pub const CH_FOE_SCENT: usize = 4;
pub const CH_BLOCKED: usize = 5;

/// Square radius, in cells, for the friend/foe counters.
pub const NEIGHBOUR_RADIUS: i32 = 2;
/// Count at which the friend/foe inputs saturate.
const CROWD_SATURATION: f32 = 8.0;

/// Compress an unbounded pheromone magnitude into `[0, 1]`.
///
/// Logarithmic, deliberately. Scent near a nest is ~10^4; a faint trail is
/// ~10^0. A `tanh` squash saturates at 1.0 across the whole nest neighbourhood,
/// flattening the gradient an ant must climb to get home. `ln` keeps every
/// decade discriminable, and `ln(1 + 0) == 0` pins the empty case to zero.
#[inline]
pub fn squash_phero(v: f32, log_div: f32) -> f32 {
    ((v.max(0.0) + 1.0).ln() / log_div).min(1.0)
}

/// Build one ant's sensory vector. **Read-only by contract** — this runs in the
/// parallel think phase, and a single write here would destroy determinism.
pub fn sense(
    i: usize,
    ants: &Ants,
    grid: &Grid,
    phero: &Pheromones,
    spatial: &Spatial,
    cfg: &Config,
) -> [f32; N_INPUTS] {
    let mut inputs = [0.0f32; N_INPUTS];

    let colony = ants.colony[i];
    let (px, py) = (ants.x[i], ants.y[i]);
    let heading = ants.heading[i];
    let traits = &ants.genome[i].traits;

    // --- Whiskers ---
    for (w, angle) in WHISKER_ANGLES.iter().enumerate() {
        let a = heading + angle;
        let sx = px + a.cos() * traits.vision;
        let sy = py + a.sin() * traits.vision;
        let (ix, iy) = (sx.floor() as i32, sy.floor() as i32);
        let base = IN_WHISKERS + w * CHANNELS_PER_WHISKER;

        if !grid.in_bounds(ix, iy) {
            inputs[base + CH_BLOCKED] = 1.0;
            continue;
        }
        let c = grid.idx_clamped(ix, iy);
        let (own, foe) = phero.scent_for(c, colony);
        let d = cfg.phero_log_div;
        inputs[base + CH_FOOD] = (grid.food[c] / cfg.food_patch_max).min(1.0);
        inputs[base + CH_FOOD_PHERO] = squash_phero(phero.food[c], d);
        inputs[base + CH_ALARM] = squash_phero(phero.alarm[c], d);
        inputs[base + CH_OWN_SCENT] = squash_phero(own, d);
        inputs[base + CH_FOE_SCENT] = squash_phero(foe, d);
        inputs[base + CH_BLOCKED] = if grid.stone[c] { 1.0 } else { 0.0 };
    }

    // --- Underfoot ---
    let (cx, cy) = ants.cell(i);
    let here = grid.idx(cx, cy);
    inputs[IN_UNDERFOOT] = (grid.food[here] / cfg.food_patch_max).min(1.0);
    inputs[IN_UNDERFOOT + 1] = squash_phero(phero.food[here], cfg.phero_log_div);
    inputs[IN_UNDERFOOT + 2] = squash_phero(phero.alarm[here], cfg.phero_log_div);

    // --- Crowding ---
    let (friends, foes) =
        spatial.counts_in_radius(ants, cx as i32, cy as i32, NEIGHBOUR_RADIUS, colony);
    // `friends` includes this ant, so subtract it before normalising.
    inputs[IN_COUNTS] = (friends.saturating_sub(1) as f32 / CROWD_SATURATION).min(1.0);
    inputs[IN_COUNTS + 1] = (foes as f32 / CROWD_SATURATION).min(1.0);

    // --- Proprioception ---
    let max_e = ants.genome[i].max_energy(cfg, ants.size[i]);
    inputs[IN_PROPRIO] = (ants.energy[i] / max_e).clamp(0.0, 1.0);
    inputs[IN_PROPRIO + 1] = (ants.size[i] / traits.max_size).clamp(0.0, 1.0);
    inputs[IN_PROPRIO + 2] = (ants.carrying[i] / traits.carry_capacity).clamp(0.0, 1.0);
    inputs[IN_PROPRIO + 3] = (ants.age[i] as f32 / traits.lifespan).clamp(0.0, 1.0);

    inputs[IN_BIAS] = 1.0;

    inputs[IN_MEMORY..IN_MEMORY + N_MEMORY].copy_from_slice(&ants.memory[i]);

    inputs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ants::{Ants, Spawn};
    use crate::config::Config;
    use crate::genome::{Genome, Traits};
    use crate::grid::Grid;
    use crate::pheromone::Pheromones;
    use crate::rng::Pcg32;
    use crate::spatial::Spatial;
    use crate::N_INPUTS;

    fn cfg() -> Config {
        Config {
            width: 16,
            height: 16,
            ..Config::default()
        }
    }

    /// An ant at (8,8) facing +x, vision 3, all traits mid-range.
    fn setup() -> (Config, Ants, Grid, Pheromones, Spatial) {
        let c = cfg();
        let mut a = Ants::new();
        let mut g = Genome::random(&mut Pcg32::new(1, 1));
        g.traits = Traits::from_array([0.5, 0.5, 0.5, 3.0, 10.0, 2.0, 1.0, 10000.0]);
        a.push(Spawn {
            id: 0,
            colony: 1,
            x: 8.5,
            y: 8.5,
            heading: 0.0,
            energy: 100.0,
            size: 1.0,
            lineage: 0,
            genome: g,
            birth_tick: 0,
        });
        let grid = Grid::new(&c);
        let phero = Pheromones::new(&c);
        let mut s = Spatial::new(&c);
        s.rebuild(&a);
        (c, a, grid, phero, s)
    }

    fn whisker(inputs: &[f32; N_INPUTS], w: usize, ch: usize) -> f32 {
        inputs[IN_WHISKERS + w * CHANNELS_PER_WHISKER + ch]
    }

    #[test]
    fn layout_constants_sum_to_the_input_count() {
        assert_eq!(IN_UNDERFOOT, WHISKER_ANGLES.len() * CHANNELS_PER_WHISKER);
        assert_eq!(IN_MEMORY + crate::N_MEMORY, N_INPUTS);
    }

    #[test]
    fn bias_input_is_always_one() {
        let (c, a, g, p, s) = setup();
        assert_eq!(sense(0, &a, &g, &p, &s, &c)[IN_BIAS], 1.0);
    }

    #[test]
    fn every_input_is_finite_and_bounded() {
        let (c, a, g, p, s) = setup();
        for (i, v) in sense(0, &a, &g, &p, &s, &c).iter().enumerate() {
            assert!(v.is_finite(), "input {i} is not finite");
            assert!((-1.0..=1.0).contains(v), "input {i} = {v} out of [-1,1]");
        }
    }

    #[test]
    fn memory_inputs_mirror_the_ants_memory() {
        let (c, mut a, g, p, s) = setup();
        a.memory[0] = [0.1, -0.2, 0.3, -0.4];
        let inputs = sense(0, &a, &g, &p, &s, &c);
        assert_eq!(&inputs[IN_MEMORY..], &[0.1, -0.2, 0.3, -0.4]);
    }

    #[test]
    fn the_forward_whisker_sees_food_placed_ahead() {
        let (c, a, mut g, p, s) = setup();
        // vision = 3, heading = 0 (+x), so the forward whisker samples ~(11,8).
        let i = g.idx(11, 8);
        g.food[i] = c.food_patch_max;
        let inputs = sense(0, &a, &g, &p, &s, &c);
        assert!(
            whisker(&inputs, 2, CH_FOOD) > 0.9,
            "forward whisker should see it"
        );
        assert_eq!(whisker(&inputs, 0, CH_FOOD), 0.0, "hard-left should not");
    }

    #[test]
    fn whiskers_rotate_with_heading() {
        let (c, mut a, mut g, p, s) = setup();
        let i = g.idx(8, 11); // directly +y of the ant
        g.food[i] = c.food_patch_max;
        // Facing +y, the forward whisker should now find it.
        a.heading[0] = std::f32::consts::FRAC_PI_2;
        let inputs = sense(0, &a, &g, &p, &s, &c);
        assert!(whisker(&inputs, 2, CH_FOOD) > 0.9);
    }

    #[test]
    fn stone_reads_as_blocked() {
        let (c, a, mut g, p, s) = setup();
        let i = g.idx(11, 8);
        g.stone[i] = true;
        assert_eq!(whisker(&sense(0, &a, &g, &p, &s, &c), 2, CH_BLOCKED), 1.0);
    }

    #[test]
    fn off_grid_reads_as_blocked() {
        let (c, mut a, g, p, s) = setup();
        a.x[0] = 0.5; // vision 3 to the left is off the map
        a.heading[0] = std::f32::consts::PI;
        assert_eq!(whisker(&sense(0, &a, &g, &p, &s, &c), 2, CH_BLOCKED), 1.0);
    }

    #[test]
    fn own_and_foreign_scent_land_in_different_channels() {
        let (c, a, g, mut p, s) = setup();
        let ahead = g.idx(11, 8);
        p.deposit_scent(ahead, 10.0, 1); // ant's own colony
        let inputs = sense(0, &a, &g, &p, &s, &c);
        assert!(whisker(&inputs, 2, CH_OWN_SCENT) > 0.0);
        assert_eq!(whisker(&inputs, 2, CH_FOE_SCENT), 0.0);

        let mut p2 = Pheromones::new(&c);
        p2.deposit_scent(ahead, 10.0, 7); // a foreign colony
        let inputs = sense(0, &a, &g, &p2, &s, &c);
        assert_eq!(whisker(&inputs, 2, CH_OWN_SCENT), 0.0);
        assert!(whisker(&inputs, 2, CH_FOE_SCENT) > 0.0);
    }

    #[test]
    fn underfoot_channels_read_the_ants_own_cell() {
        let (c, a, mut g, mut p, s) = setup();
        let here = g.idx(8, 8);
        g.food[here] = c.food_patch_max;
        p.deposit_food(here, 100.0);
        p.deposit_alarm(here, 100.0);
        let inputs = sense(0, &a, &g, &p, &s, &c);
        assert!(inputs[IN_UNDERFOOT] > 0.9);
        assert!(inputs[IN_UNDERFOOT + 1] > 0.0);
        assert!(inputs[IN_UNDERFOOT + 2] > 0.0);
    }

    #[test]
    fn friend_and_foe_counts_are_normalised() {
        let c = cfg();
        let mut a = Ants::new();
        for (i, (x, y, col)) in [(8.5, 8.5, 1u8), (9.5, 8.5, 1), (7.5, 8.5, 2)]
            .iter()
            .enumerate()
        {
            let mut g = Genome::random(&mut Pcg32::new(i as u64, 1));
            g.traits = Traits::from_array([0.5, 0.5, 0.5, 3.0, 10.0, 2.0, 1.0, 10000.0]);
            a.push(Spawn {
                id: i as u64,
                colony: *col,
                x: *x,
                y: *y,
                heading: 0.0,
                energy: 100.0,
                size: 1.0,
                lineage: 0,
                genome: g,
                birth_tick: 0,
            });
        }
        let grid = Grid::new(&c);
        let phero = Pheromones::new(&c);
        let mut s = Spatial::new(&c);
        s.rebuild(&a);
        let inputs = sense(0, &a, &grid, &phero, &s, &c);
        assert!(inputs[IN_COUNTS] > 0.0, "should see one friend besides itself");
        assert!(inputs[IN_COUNTS + 1] > 0.0, "should see one foe");
    }

    #[test]
    fn proprioception_reports_fullness_not_raw_energy() {
        let (c, mut a, g, p, s) = setup();
        a.energy[0] = a.genome[0].max_energy(&c, a.size[0]);
        assert_eq!(sense(0, &a, &g, &p, &s, &c)[IN_PROPRIO], 1.0);
        a.energy[0] = 0.0;
        assert_eq!(sense(0, &a, &g, &p, &s, &c)[IN_PROPRIO], 0.0);
    }

    #[test]
    fn carrying_input_saturates_at_capacity() {
        let (c, mut a, g, p, s) = setup();
        a.carrying[0] = a.genome[0].traits.carry_capacity;
        assert_eq!(sense(0, &a, &g, &p, &s, &c)[IN_PROPRIO + 2], 1.0);
    }

    #[test]
    fn squash_phero_maps_nothing_to_zero() {
        assert_eq!(squash_phero(0.0, 12.0), 0.0);
    }

    #[test]
    fn squash_phero_stays_in_range_for_absurd_inputs() {
        for v in [0.0, 1.0, 1e3, 1e6, 1e30] {
            let s = squash_phero(v, 12.0);
            assert!((0.0..=1.0).contains(&s), "{v} -> {s}");
        }
    }

    #[test]
    fn squash_phero_stays_discriminable_across_four_decades() {
        // This is the property that makes homing possible. A tanh squash would
        // return 1.0 for every one of these, and the ant would see no gradient.
        let d = 12.0;
        let samples: Vec<f32> = [1.0, 10.0, 100.0, 1_000.0, 10_000.0]
            .iter()
            .map(|v| squash_phero(*v, d))
            .collect();
        for w in samples.windows(2) {
            assert!(w[1] - w[0] > 0.05, "adjacent decades too close: {w:?}");
        }
        assert!(*samples.last().unwrap() < 1.0, "saturated at the top decade");
    }

    #[test]
    fn a_stronger_scent_always_reads_higher() {
        let d = 12.0;
        let mut prev = -1.0;
        for v in [0.0, 0.5, 2.0, 50.0, 5_000.0, 50_000.0] {
            let s = squash_phero(v, d);
            assert!(s > prev, "not monotone at {v}");
            prev = s;
        }
    }
}
