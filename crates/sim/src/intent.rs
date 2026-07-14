use crate::ants::Ants;
use crate::apply::wrap_angle;
use crate::brain::{Brain, OUT_ATTACK, OUT_GRAB, OUT_MEMORY, OUT_VX, OUT_VY};
use crate::config::Config;
use crate::grid::Grid;
use crate::pheromone::Pheromones;
use crate::sense::sense;
use crate::spatial::Spatial;
use crate::N_MEMORY;

/// Maximum heading change per tick, radians. The network commands a world-frame
/// direction, not a turn rate; this caps how fast the ant can rotate toward it,
/// so a reversed command becomes a gradual U-turn rather than an instant snap.
pub const MAX_TURN: f32 = 0.4;
/// Below this velocity magnitude the command is treated as "hold position":
/// steering off `atan2(0, 0)` would otherwise snap every idle ant to world-east.
const MIN_SPEED_CMD: f32 = 1e-4;
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

    // Outputs 0,1 are a world-frame desired-velocity vector. Steer the heading
    // toward its direction (capped) and move at its magnitude. A steady vector
    // holds a steady heading, so straight travel is the network's default rather
    // than the knife-edge that a turn-rate output made it.
    let (vx, vy) = (o[OUT_VX], o[OUT_VY]);
    let mag = (vx * vx + vy * vy).sqrt();
    let heading = if mag > MIN_SPEED_CMD {
        let desired = vy.atan2(vx);
        let delta = wrap_angle(desired - ants.heading[i]).clamp(-MAX_TURN, MAX_TURN);
        ants.heading[i] + delta
    } else {
        ants.heading[i]
    };
    let speed = mag.min(1.0) * ants.genome[i].traits.max_speed;

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
    fn speed_is_the_velocity_magnitude_capped_by_the_trait() {
        let (c, mut a, g, p, s) = world();
        // Zero velocity vector -> hold position.
        force_outputs(&mut a.genome[0], [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(think(0, &a, &g, &p, &s, &c).speed, 0.0, "idle should not move");

        // Full-magnitude command -> speed capped at the trait.
        force_outputs(
            &mut a.genome[0],
            [1.0 - 1e-6, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        );
        let sp = think(0, &a, &g, &p, &s, &c).speed;
        assert!(
            sp <= a.genome[0].traits.max_speed + 1e-4,
            "speed {sp} exceeded trait"
        );
        assert!(sp > 0.0);
    }

    #[test]
    fn turn_toward_the_command_is_capped_at_max_turn_per_tick() {
        let (c, mut a, g, p, s) = world();
        a.heading[0] = 0.0; // facing +x
        // Command the exact opposite direction (-x). The ant must not snap
        // around; it turns at most MAX_TURN this tick.
        force_outputs(
            &mut a.genome[0],
            [-1.0 + 1e-6, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        );
        let delta = think(0, &a, &g, &p, &s, &c).heading - a.heading[0];
        assert!(delta.abs() <= MAX_TURN + 1e-4, "turned {delta} in one tick");
        assert!(delta.abs() > 0.0, "should have started turning");
    }

    #[test]
    fn an_aligned_ant_stops_turning_instead_of_spinning() {
        let (c, mut a, g, p, s) = world();
        a.heading[0] = std::f32::consts::FRAC_PI_2; // facing +y
        // Command +y — the direction it already faces. A steady command on an
        // aligned heading must produce no further turn: this is the property
        // that makes straight travel stable rather than a knife-edge.
        force_outputs(
            &mut a.genome[0],
            [0.0, 1.0 - 1e-6, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        );
        let delta = think(0, &a, &g, &p, &s, &c).heading - a.heading[0];
        assert!(delta.abs() < 1e-3, "aligned ant kept turning by {delta}");
    }

    #[test]
    fn an_idle_command_holds_the_heading_rather_than_snapping_east() {
        let (c, mut a, g, p, s) = world();
        a.heading[0] = 1.0;
        force_outputs(&mut a.genome[0], [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        let out = think(0, &a, &g, &p, &s, &c);
        assert_eq!(out.heading, 1.0, "a zero command must not rotate the ant");
        assert_eq!(out.speed, 0.0);
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
