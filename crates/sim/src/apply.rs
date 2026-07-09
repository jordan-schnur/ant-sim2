use crate::ants::Ants;
use crate::colony::ColonyState;
use crate::config::Config;
use crate::grid::{Grid, NO_NEST};
use crate::intent::Intent;
use crate::pheromone::Pheromones;
use crate::spatial::Spatial;

/// Everything the serial apply phase is allowed to mutate.
pub struct ApplyCtx<'a> {
    pub cfg: &'a Config,
    pub grid: &'a mut Grid,
    pub phero: &'a mut Pheromones,
    pub spatial: &'a mut Spatial,
    /// Indexed by colony id.
    pub colonies: &'a mut [ColonyState],
}

/// Normalise to `[-PI, PI)` so headings cannot drift to a magnitude where f32
/// loses angular precision.
pub fn wrap_angle(a: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let mut r = (a + PI).rem_euclid(TAU);
    if r < 0.0 {
        r += TAU;
    }
    r - PI
}

/// Heading, then translation. One ant per cell, except on nest tiles: without
/// that exemption newborns could not spawn onto a busy nest and returning
/// foragers would jam in the doorway.
pub fn apply_movement(i: usize, intent: &Intent, ants: &mut Ants, ctx: &mut ApplyCtx) {
    ants.memory[i] = intent.memory;
    ants.age[i] += 1;
    ants.heading[i] = wrap_angle(intent.heading);

    if intent.speed <= 0.0 {
        return;
    }

    let (cx, cy) = ants.cell(i);
    let cur = ctx.grid.idx(cx, cy);
    let nx = ants.x[i] + ants.heading[i].cos() * intent.speed;
    let ny = ants.y[i] + ants.heading[i].sin() * intent.speed;
    let (tx, ty) = (nx.floor() as i32, ny.floor() as i32);

    // Staying inside the current cell needs no occupancy check.
    if tx == cx as i32 && ty == cy as i32 {
        ants.x[i] = nx;
        ants.y[i] = ny;
        ants.energy[i] -= ctx.cfg.move_cost * intent.speed;
        return;
    }

    if ctx.grid.is_stone(tx, ty) {
        return;
    }
    let target = ctx.grid.idx_clamped(tx, ty);
    let is_nest = ctx.grid.nest[target] != NO_NEST;
    if !is_nest && ctx.spatial.occupant(target).is_some() {
        return;
    }

    if ctx.spatial.occupant(cur) == Some(i as u32) {
        ctx.spatial.clear_occupant(cur);
    }
    if ctx.spatial.occupant(target).is_none() {
        ctx.spatial.set_occupant(target, i as u32);
    }
    ants.x[i] = nx;
    ants.y[i] = ny;
    ants.energy[i] -= ctx.cfg.move_cost * intent.speed;
}

/// Grab from the ground, or drop onto it. Nest tiles are handled by
/// `apply_nest`, so releasing on one is a no-op rather than a food pile.
pub fn apply_food(i: usize, intent: &Intent, ants: &mut Ants, ctx: &mut ApplyCtx) {
    let (cx, cy) = ants.cell(i);
    let c = ctx.grid.idx(cx, cy);
    let capacity = ants.genome[i].traits.carry_capacity;

    if intent.grab && ants.carrying[i] < capacity {
        let want = ctx.cfg.harvest_rate.min(capacity - ants.carrying[i]);
        ants.carrying[i] += ctx.grid.harvest(c, want);
    } else if intent.release && ants.carrying[i] > 0.0 && ctx.grid.nest[c] == NO_NEST {
        ctx.grid.food[c] += ants.carrying[i];
        ants.carrying[i] = 0.0;
    }
}

/// Standing on your own nest banks your load and refuels you. Both are
/// automatic; the network must only evolve to *go there*.
pub fn apply_nest(i: usize, ants: &mut Ants, ctx: &mut ApplyCtx) {
    let (cx, cy) = ants.cell(i);
    let c = ctx.grid.idx(cx, cy);
    if ctx.grid.nest[c] != ants.colony[i] {
        return;
    }
    let colony = &mut ctx.colonies[ants.colony[i] as usize];

    let load = ants.carrying[i];
    if load > 0.0 {
        colony.store += load;
        ants.food_delivered[i] += load;
        ants.carrying[i] = 0.0;
    }

    let max_e = ants.genome[i].max_energy(ctx.cfg, ants.size[i]);
    let want = (max_e - ants.energy[i]).max(0.0).min(ctx.cfg.refuel_rate);
    let taken = want.min(colony.store);
    colony.store -= taken;
    ants.energy[i] += taken;
}

/// Passive chemical leakage. No `Intent` field gates this: ants leak because
/// they are ants. Food-trail is proportional to the load, so only a laden ant
/// marks a path — which is why trails run from food back toward the nest.
///
/// Takes loose fields rather than `&Ants` so the caller can hold `&mut Ants`
/// across the call without cloning the whole store every iteration.
pub fn deposit_passive(cell: usize, carrying: f32, colony: u8, ctx: &mut ApplyCtx) {
    if carrying > 0.0 {
        ctx.phero
            .deposit_food(cell, ctx.cfg.food_trail_emission * carrying);
    }
    ctx.phero
        .deposit_scent(cell, ctx.cfg.ant_scent_emission, colony);
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
    use crate::N_MEMORY;

    struct Fixture {
        cfg: Config,
        ants: Ants,
        grid: Grid,
        phero: Pheromones,
        spatial: Spatial,
        colonies: Vec<ColonyState>,
    }

    impl Fixture {
        /// Hands back `&mut Ants` and the context as *disjoint* borrows of
        /// separate fields. A method returning only `ApplyCtx` would borrow the
        /// whole fixture and make `apply_movement(.., &mut f.ants, &mut f.ctx())`
        /// a double mutable borrow.
        fn split(&mut self) -> (&mut Ants, ApplyCtx<'_>) {
            (
                &mut self.ants,
                ApplyCtx {
                    cfg: &self.cfg,
                    grid: &mut self.grid,
                    phero: &mut self.phero,
                    spatial: &mut self.spatial,
                    colonies: &mut self.colonies,
                },
            )
        }
        fn ctx(&mut self) -> ApplyCtx<'_> {
            self.split().1
        }
        fn rebuild(&mut self) {
            self.spatial.rebuild(&self.ants);
        }
    }

    fn fixture(positions: &[(f32, f32, u8)]) -> Fixture {
        let cfg = Config {
            width: 16,
            height: 16,
            ..Config::default()
        };
        let mut ants = Ants::new();
        for (i, (x, y, c)) in positions.iter().enumerate() {
            let mut g = Genome::random(&mut Pcg32::new(i as u64, 1));
            g.traits = Traits::from_array([1.0, 0.5, 0.5, 3.0, 10.0, 2.0, 1.0, 10000.0]);
            ants.push(Spawn {
                id: i as u64,
                colony: *c,
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
        let grid = Grid::new(&cfg);
        let phero = Pheromones::new(&cfg);
        let mut spatial = Spatial::new(&cfg);
        spatial.rebuild(&ants);
        let colonies = (0..4).map(ColonyState::new).collect();
        Fixture {
            cfg,
            ants,
            grid,
            phero,
            spatial,
            colonies,
        }
    }

    fn intent() -> Intent {
        Intent {
            heading: 0.0,
            speed: 0.0,
            attack: false,
            grab: false,
            release: false,
            memory: [0.0; N_MEMORY],
        }
    }

    #[test]
    fn wrap_angle_keeps_headings_bounded() {
        for a in [-100.0f32, -3.5, 0.0, 3.5, 100.0] {
            let w = wrap_angle(a);
            assert!(
                w >= -std::f32::consts::PI && w < std::f32::consts::PI,
                "{a} -> {w}"
            );
        }
    }

    #[test]
    fn an_ant_moves_along_its_heading() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let i = Intent {
            heading: 0.0,
            speed: 1.0,
            ..intent()
        };
        let (ants, mut ctx) = f.split();
        apply_movement(0, &i, ants, &mut ctx);
        assert!((f.ants.x[0] - 9.5).abs() < 1e-5);
        assert!((f.ants.y[0] - 8.5).abs() < 1e-5);
    }

    #[test]
    fn movement_costs_energy_proportional_to_distance() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let before = f.ants.energy[0];
        let i = Intent {
            heading: 0.0,
            speed: 1.0,
            ..intent()
        };
        let (ants, mut ctx) = f.split();
        apply_movement(0, &i, ants, &mut ctx);
        let expected = before - f.cfg.move_cost * 1.0;
        assert!((f.ants.energy[0] - expected).abs() < 1e-4);
    }

    #[test]
    fn stone_blocks_movement_and_costs_nothing() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let s = f.grid.idx(9, 8);
        f.grid.stone[s] = true;
        let before = f.ants.energy[0];
        let i = Intent {
            heading: 0.0,
            speed: 1.0,
            ..intent()
        };
        let (ants, mut ctx) = f.split();
        apply_movement(0, &i, ants, &mut ctx);
        assert_eq!(f.ants.x[0], 8.5, "should not have entered stone");
        assert_eq!(f.ants.energy[0], before, "a blocked ant pays no move cost");
    }

    #[test]
    fn the_map_border_blocks_movement() {
        let mut f = fixture(&[(0.5, 8.5, 1)]);
        let i = Intent {
            heading: std::f32::consts::PI,
            speed: 1.0,
            ..intent()
        };
        let (ants, mut ctx) = f.split();
        apply_movement(0, &i, ants, &mut ctx);
        assert_eq!(f.ants.x[0], 0.5);
    }

    #[test]
    fn moving_within_the_same_cell_is_always_allowed() {
        let mut f = fixture(&[(8.1, 8.5, 1)]);
        let i = Intent {
            heading: 0.0,
            speed: 0.2,
            ..intent()
        };
        let (ants, mut ctx) = f.split();
        apply_movement(0, &i, ants, &mut ctx);
        assert!((f.ants.x[0] - 8.3).abs() < 1e-5);
    }

    #[test]
    fn an_occupied_cell_blocks_the_higher_id_ant() {
        let mut f = fixture(&[(9.5, 8.5, 1), (8.5, 8.5, 1)]);
        f.rebuild();
        // Ant 1 tries to walk into ant 0's cell.
        let i = Intent {
            heading: 0.0,
            speed: 1.0,
            ..intent()
        };
        let (ants, mut ctx) = f.split();
        apply_movement(1, &i, ants, &mut ctx);
        assert_eq!(f.ants.x[1], 8.5, "blocked by the incumbent");
    }

    #[test]
    fn nest_tiles_are_exempt_from_cell_exclusion() {
        let mut f = fixture(&[(9.5, 8.5, 1), (8.5, 8.5, 1)]);
        let n = f.grid.idx(9, 8);
        f.grid.nest[n] = 1;
        f.rebuild();
        let i = Intent {
            heading: 0.0,
            speed: 1.0,
            ..intent()
        };
        let (ants, mut ctx) = f.split();
        apply_movement(1, &i, ants, &mut ctx);
        assert!((f.ants.x[1] - 9.5).abs() < 1e-5, "should stack on the nest");
    }

    #[test]
    fn grab_harvests_food_up_to_carry_capacity() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let c = f.grid.idx(8, 8);
        f.grid.food[c] = 100.0;
        f.ants.carrying[0] = 9.7; // capacity is 10.0
        let i = Intent {
            grab: true,
            ..intent()
        };
        let (ants, mut ctx) = f.split();
        apply_food(0, &i, ants, &mut ctx);
        assert!((f.ants.carrying[0] - 10.0).abs() < 1e-5);
        assert!((f.grid.food[c] - 99.7).abs() < 1e-4);
    }

    #[test]
    fn grab_takes_nothing_from_an_empty_cell() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let i = Intent {
            grab: true,
            ..intent()
        };
        let (ants, mut ctx) = f.split();
        apply_food(0, &i, ants, &mut ctx);
        assert_eq!(f.ants.carrying[0], 0.0);
    }

    #[test]
    fn release_drops_the_load_back_onto_the_ground() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        f.ants.carrying[0] = 4.0;
        let i = Intent {
            release: true,
            ..intent()
        };
        let (ants, mut ctx) = f.split();
        apply_food(0, &i, ants, &mut ctx);
        assert_eq!(f.ants.carrying[0], 0.0);
        assert_eq!(f.grid.food[f.grid.idx(8, 8)], 4.0);
    }

    #[test]
    fn standing_on_your_own_nest_deposits_the_load_into_the_store() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let n = f.grid.idx(8, 8);
        f.grid.nest[n] = 1;
        f.ants.carrying[0] = 6.0;
        let (ants, mut ctx) = f.split();
        apply_nest(0, ants, &mut ctx);
        assert_eq!(f.ants.carrying[0], 0.0);
        assert_eq!(f.colonies[1].store, 6.0);
    }

    #[test]
    fn depositing_credits_food_delivered_which_is_the_only_fitness_signal() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let n = f.grid.idx(8, 8);
        f.grid.nest[n] = 1;
        f.ants.carrying[0] = 6.0;
        let (ants, mut ctx) = f.split();
        apply_nest(0, ants, &mut ctx);
        assert_eq!(f.ants.food_delivered[0], 6.0);
    }

    #[test]
    fn a_foreign_nest_accepts_nothing() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let n = f.grid.idx(8, 8);
        f.grid.nest[n] = 2;
        f.ants.carrying[0] = 6.0;
        let (ants, mut ctx) = f.split();
        apply_nest(0, ants, &mut ctx);
        assert_eq!(f.ants.carrying[0], 6.0);
        assert_eq!(f.colonies[2].store, 0.0);
    }

    #[test]
    fn refuelling_draws_from_the_store_and_is_capped_by_max_energy() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let n = f.grid.idx(8, 8);
        f.grid.nest[n] = 1;
        f.colonies[1].store = 1000.0;
        let max_e = f.ants.genome[0].max_energy(&f.cfg, f.ants.size[0]);
        f.ants.energy[0] = max_e - 1.0;
        let (ants, mut ctx) = f.split();
        apply_nest(0, ants, &mut ctx);
        assert!((f.ants.energy[0] - max_e).abs() < 1e-4);
        assert!(
            (f.colonies[1].store - 999.0).abs() < 1e-3,
            "took only what it needed"
        );
    }

    #[test]
    fn an_empty_store_cannot_refuel_anyone() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let n = f.grid.idx(8, 8);
        f.grid.nest[n] = 1;
        f.colonies[1].store = 0.0;
        f.ants.energy[0] = 1.0;
        let (ants, mut ctx) = f.split();
        apply_nest(0, ants, &mut ctx);
        assert_eq!(f.ants.energy[0], 1.0);
    }

    #[test]
    fn every_ant_leaks_colony_scent_unconditionally() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let c = f.grid.idx(8, 8);
        deposit_passive(c, 0.0, 1, &mut f.ctx());
        assert_eq!(f.phero.scent[c], f.cfg.ant_scent_emission);
        assert_eq!(f.phero.owner[c], 1);
    }

    #[test]
    fn only_a_laden_ant_lays_food_trail() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let c = f.grid.idx(8, 8);

        deposit_passive(c, 0.0, 1, &mut f.ctx());
        assert_eq!(f.phero.food[c], 0.0, "an empty-handed ant lays no trail");

        deposit_passive(c, 3.0, 1, &mut f.ctx());
        assert!((f.phero.food[c] - 3.0 * f.cfg.food_trail_emission).abs() < 1e-4);
    }

    #[test]
    fn release_onto_a_nest_tile_is_ignored_so_food_cannot_be_dumped_at_the_door() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let n = f.grid.idx(8, 8);
        f.grid.nest[n] = 1;
        f.ants.carrying[0] = 5.0;
        let i = Intent {
            release: true,
            ..intent()
        };
        let (ants, mut ctx) = f.split();
        apply_food(0, &i, ants, &mut ctx);
        assert_eq!(f.grid.food[f.grid.idx(8, 8)], 0.0);
        assert_eq!(
            f.ants.carrying[0], 5.0,
            "apply_nest handles nest deposits, not apply_food"
        );
    }
}
