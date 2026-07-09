use crate::ants::Ants;
use crate::brain::{Brain, OUT_ATTACK, OUT_GRAB, OUT_MEMORY, OUT_THROTTLE, OUT_TURN};
use crate::config::Config;
use crate::grid::Grid;
use crate::pheromone::Pheromones;
use crate::sense::sense;
use crate::spatial::Spatial;
use crate::N_MEMORY;

/// Maximum heading change per tick, radians. Caps how sharply an ant can turn
/// regardless of what its network asks for.
pub const MAX_TURN: f32 = 0.4;
pub const ATTACK_THRESHOLD: f32 = 0.5;
pub const GRAB_THRESHOLD: f32 = 0.3;

/// What one ant wants to do this tick. Produced by the read-only parallel
/// phase, consumed by the serial apply phase.
#[derive(Clone, Debug)]
pub struct Intent {
    pub heading: f32,
    /// Cells per tick, always >= 0.
    pub speed: f32,
    pub attack: bool,
    pub grab: bool,
    pub release: bool,
    pub memory: [f32; N_MEMORY],
}

/// The entire parallel phase. Borrows everything immutably and returns a value;
/// it structurally cannot race, which is the whole determinism argument.
pub fn think(
    i: usize,
    ants: &Ants,
    grid: &Grid,
    phero: &Pheromones,
    spatial: &Spatial,
    cfg: &Config,
) -> Intent {
    let inputs = sense(i, ants, grid, phero, spatial, cfg);
    let act = ants.genome[i].forward(&inputs);
    let o = act.outputs;

    let heading = ants.heading[i] + o[OUT_TURN] * MAX_TURN;
    // Backwards is not modelled; a negative throttle simply means "stop".
    let speed = o[OUT_THROTTLE].max(0.0) * ants.genome[i].traits.max_speed;

    let mut memory = [0.0f32; N_MEMORY];
    memory.copy_from_slice(&o[OUT_MEMORY..OUT_MEMORY + N_MEMORY]);

    Intent {
        heading,
        speed,
        attack: o[OUT_ATTACK] > ATTACK_THRESHOLD,
        grab: o[OUT_GRAB] > GRAB_THRESHOLD,
        release: o[OUT_GRAB] < -GRAB_THRESHOLD,
        memory,
    }
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

    fn world() -> (Config, Ants, Grid, Pheromones, Spatial) {
        let c = Config {
            width: 16,
            height: 16,
            ..Config::default()
        };
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
        let p = Pheromones::new(&c);
        let mut s = Spatial::new(&c);
        s.rebuild(&a);
        (c, a, grid, p, s)
    }

    /// Force every output to a chosen constant by zeroing the net and setting
    /// the final biases. tanh(atanh(v)) = v.
    fn force_outputs(g: &mut Genome, values: [f32; crate::N_OUTPUTS]) {
        g.params.iter_mut().for_each(|p| *p = 0.0);
        let bias_start = crate::N_PARAMS - crate::N_OUTPUTS;
        for (j, v) in values.iter().enumerate() {
            g.params[bias_start + j] = v.atanh();
        }
    }

    #[test]
    fn think_is_pure() {
        let (c, a, g, p, s) = world();
        let x = think(0, &a, &g, &p, &s, &c);
        let y = think(0, &a, &g, &p, &s, &c);
        assert_eq!(x.heading, y.heading);
        assert_eq!(x.speed, y.speed);
    }

    #[test]
    fn speed_is_never_negative_and_is_capped_by_the_trait() {
        let (c, mut a, g, p, s) = world();
        force_outputs(
            &mut a.genome[0],
            [0.0, -1.0 + 1e-6, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        );
        assert_eq!(
            think(0, &a, &g, &p, &s, &c).speed,
            0.0,
            "reverse is not a thing"
        );

        force_outputs(
            &mut a.genome[0],
            [0.0, 1.0 - 1e-6, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        );
        let sp = think(0, &a, &g, &p, &s, &c).speed;
        assert!(
            sp <= a.genome[0].traits.max_speed + 1e-4,
            "speed {sp} exceeded trait"
        );
        assert!(sp > 0.0);
    }

    #[test]
    fn turn_is_capped_at_max_turn_per_tick() {
        let (c, mut a, g, p, s) = world();
        force_outputs(
            &mut a.genome[0],
            [1.0 - 1e-6, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        );
        let delta = think(0, &a, &g, &p, &s, &c).heading - a.heading[0];
        assert!(delta.abs() <= MAX_TURN + 1e-4, "turned {delta} in one tick");
    }

    #[test]
    fn attack_fires_only_above_threshold() {
        let (c, mut a, g, p, s) = world();
        force_outputs(&mut a.genome[0], [0.0, 0.0, 0.9, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert!(think(0, &a, &g, &p, &s, &c).attack);
        force_outputs(&mut a.genome[0], [0.0, 0.0, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert!(!think(0, &a, &g, &p, &s, &c).attack);
    }

    #[test]
    fn grab_and_release_are_opposite_signs_and_never_both() {
        let (c, mut a, g, p, s) = world();
        force_outputs(&mut a.genome[0], [0.0, 0.0, 0.0, 0.9, 0.0, 0.0, 0.0, 0.0]);
        let i = think(0, &a, &g, &p, &s, &c);
        assert!(i.grab && !i.release);

        force_outputs(&mut a.genome[0], [0.0, 0.0, 0.0, -0.9, 0.0, 0.0, 0.0, 0.0]);
        let i = think(0, &a, &g, &p, &s, &c);
        assert!(i.release && !i.grab);

        force_outputs(&mut a.genome[0], [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        let i = think(0, &a, &g, &p, &s, &c);
        assert!(!i.release && !i.grab);
    }

    #[test]
    fn memory_outputs_are_carried_on_the_intent() {
        let (c, mut a, g, p, s) = world();
        force_outputs(
            &mut a.genome[0],
            [0.0, 0.0, 0.0, 0.0, 0.5, -0.5, 0.25, -0.25],
        );
        let i = think(0, &a, &g, &p, &s, &c);
        for (got, want) in i.memory.iter().zip([0.5, -0.5, 0.25, -0.25]) {
            assert!((got - want).abs() < 1e-5, "{got} != {want}");
        }
    }
}
