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
/// Can ant `i`, currently in cell `(cx, cy)`, occupy the cell that holds world
/// point `(px, py)`? Staying inside the current cell is always fine; otherwise
/// the target must be on the map, not stone, and either a nest tile or a cell
/// that is empty (or already this ant's).
fn can_enter(ctx: &ApplyCtx, i: usize, px: f32, py: f32, cx: u16, cy: u16) -> bool {
    let (tx, ty) = (px.floor() as i32, py.floor() as i32);
    if tx == cx as i32 && ty == cy as i32 {
        return true;
    }
    if ctx.grid.is_stone(tx, ty) {
        return false;
    }
    let target = ctx.grid.idx_clamped(tx, ty);
    if ctx.grid.nest[target] != NO_NEST {
        return true;
    }
    match ctx.spatial.occupant(target) {
        None => true,
        Some(o) => o as usize == i,
    }
}

pub fn apply_movement(i: usize, intent: &Intent, ants: &mut Ants, ctx: &mut ApplyCtx) {
    ants.memory[i] = intent.memory;
    ants.age[i] += 1;
    ants.heading[i] = wrap_angle(intent.heading);

    if intent.speed <= 0.0 {
        return;
    }

    let (cx, cy) = ants.cell(i);
    let cur = ctx.grid.idx(cx, cy);
    let (x0, y0) = (ants.x[i], ants.y[i]);
    let dx = ants.heading[i].cos() * intent.speed;
    let dy = ants.heading[i].sin() * intent.speed;

    // Thigmotaxis: try the full move, then slide along whichever single axis is
    // open. Without this, a blocked ant freezes against the wall forever — and
    // since a naive brain issues a near-constant command, it re-aims into the
    // same wall every tick and never moves again. Sliding lets it follow the
    // obstacle and keep exploring, the way a real ant traces an edge.
    let (nx, ny, slid) = if can_enter(ctx, i, x0 + dx, y0 + dy, cx, cy) {
        (x0 + dx, y0 + dy, false)
    } else if !ctx
        .grid
        .is_stone((x0 + dx).floor() as i32, (y0 + dy).floor() as i32)
    {
        // Blocked by another ant, not terrain: wait for the lane to clear.
        // Sliding around transient traffic scatters ants queued at the nest
        // mouth or a food cell and collapses throughput under congestion.
        return;
    } else if dx != 0.0 && can_enter(ctx, i, x0 + dx, y0, cx, cy) {
        (x0 + dx, y0, true)
    } else if dy != 0.0 && can_enter(ctx, i, x0, y0 + dy, cx, cy) {
        (x0, y0 + dy, true)
    } else {
        return; // boxed in against terrain on both axes: nothing to do
    };

    // When it slides, face where it actually moved, not where it wished to, so
    // the body and whiskers stay aligned with travel along the wall. A full move
    // already travels along `heading`, so leave that case untouched.
    let (mdx, mdy) = (nx - x0, ny - y0);
    if slid && (mdx != 0.0 || mdy != 0.0) {
        ants.heading[i] = wrap_angle(mdy.atan2(mdx));
    }

    let target = ctx.grid.idx_clamped(nx.floor() as i32, ny.floor() as i32);
    if target != cur {
        if ctx.spatial.occupant(cur) == Some(i as u32) {
            ctx.spatial.clear_occupant(cur);
        }
        if ctx.spatial.occupant(target).is_none() {
            ctx.spatial.set_occupant(target, i as u32);
        }
    }
    ants.x[i] = nx;
    ants.y[i] = ny;
    // Pay for distance actually covered, not the commanded speed: a slide moves
    // along one axis only, so it should cost less than the full diagonal.
    ants.energy[i] -= ctx.cfg.move_cost * (mdx * mdx + mdy * mdy).sqrt();

    // Homing credit: while carrying food, bank the net progress made back toward
    // the ant's own nest. This is the fitness gradient the delivery-only signal
    // lacks — an ant hauling food homeward is closer to a forager than one that
    // never heads back — and it is what lets a colony bootstrap from random
    // genomes. Clamped at zero so wandering away never pushes fitness negative.
    if ants.carrying[i] > 0.0 {
        let (ncx, ncy) = ctx.colonies[ants.colony[i] as usize].nest_center;
        let d_before = ((x0 - ncx).powi(2) + (y0 - ncy).powi(2)).sqrt();
        let d_after = ((nx - ncx).powi(2) + (ny - ncy).powi(2)).sqrt();
        ants.food_homing[i] = (ants.food_homing[i] + (d_before - d_after)).max(0.0);
    }
}

/// A food-trail reading at or above this counts as "a trail led here": enough
/// to mean a nestmate recently walked this cell laden, not mere evaporated
/// residue. A laden ant deposits `food_trail_emission * carrying` per tick, so
/// at the defaults (emission 2.0) this is roughly one recent passage carrying a
/// few units of food. It gates the `FirstTrailFollow` chronicle beat, which is
/// narrative, not physics — a heuristic threshold is appropriate.
pub const TRAIL_FOLLOW_THRESHOLD: f32 = 5.0;

/// Grab from the ground, or drop onto it. Nest tiles are handled by
/// `apply_nest`, so releasing on one is a no-op rather than a food pile.
pub fn apply_food(i: usize, intent: &Intent, ants: &mut Ants, ctx: &mut ApplyCtx) {
    let (cx, cy) = ants.cell(i);
    let c = ctx.grid.idx(cx, cy);
    let capacity = ants.genome[i].traits.carry_capacity;

    if intent.grab && ants.carrying[i] < capacity {
        // Read the trail *before* this ant's own deposit for the tick, which
        // lands later in `deposit_passive`: a positive reading here is a trail
        // laid by others, so grabbing on it means this ant followed a trail to
        // food rather than stumbling onto it.
        let on_trail = ctx.phero.food[c] >= TRAIL_FOLLOW_THRESHOLD;
        let want = ctx.cfg.harvest_rate.min(capacity - ants.carrying[i]);
        let taken = ctx.grid.harvest(c, want);
        ants.carrying[i] += taken;
        ants.food_harvested[i] += taken;
        if taken > 0.0 && on_trail {
            if let Some(f) = ants.followed_trail.get_mut(i) {
                *f = true;
            }
        }
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
        colony.delivered_total += load;
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
    // Dedicated fast-fading colony trail: "a colony-mate was here recently",
    // separate from the persistent nest beacon above. Nests never lay this.
    ctx.phero
        .deposit_trail(cell, ctx.cfg.trail_emission, colony);
}

/// An ant may not shrink below this, however starved.
pub const MIN_SIZE: f32 = 0.2;

/// Attack the lowest-indexed adjacent foe. Damage is `size x strength`,
/// negated in proportion to the target's armor. Aggression is never free:
/// it costs energy up front, and only pays if the corpse is worth more.
///
/// Energy is health, so this simply drains the victim. `sweep_deaths` decides
/// who actually died — one code path for death bookkeeping.
pub fn apply_combat(i: usize, intent: &Intent, ants: &mut Ants, ctx: &mut ApplyCtx) {
    if !intent.attack || ants.energy[i] < ctx.cfg.attack_cost {
        return;
    }
    let (cx, cy) = ants.cell(i);
    let Some(v) = ctx
        .spatial
        .first_adjacent_foe(ants, cx as i32, cy as i32, ants.colony[i])
    else {
        return;
    };
    let v = v as usize;
    if let Some(f) = ants.attacking.get_mut(i) {
        *f = true;
    }

    let damage = ctx.cfg.attack_damage
        * ants.size[i]
        * ants.genome[i].traits.strength
        * (1.0 - ants.genome[v].traits.armor);

    ants.energy[i] -= ctx.cfg.attack_cost;
    let victim_energy_before = ants.energy[v];
    ants.energy[v] -= damage;

    // Alarm is leaked involuntarily by both parties, as in real ants.
    let (vx, vy) = ants.cell(v);
    let here = ctx.grid.idx(cx, cy);
    let there = ctx.grid.idx(vx, vy);
    ctx.phero.deposit_alarm(here, ctx.cfg.alarm_emission);
    ctx.phero.deposit_alarm(there, ctx.cfg.alarm_emission);

    // Only the blow that *crosses* zero scavenges. Deaths are flagged by the
    // end-of-tick sweep, so a victim already at or below zero stays a valid
    // target for the rest of the serial phase — without this guard, every ant
    // in a mob would "kill" the same corpse and each mint a full kill bonus
    // from nothing.
    let killing_blow = victim_energy_before > 0.0 && ants.energy[v] <= 0.0;
    if killing_blow {
        let scavenged = ctx.cfg.kill_energy_frac * ctx.cfg.max_energy_per_size * ants.size[v];
        let max_e = ants.genome[i].max_energy(ctx.cfg, ants.size[i]);
        ants.energy[i] = (ants.energy[i] + scavenged).min(max_e);
        if let Some(f) = ants.killed.get_mut(i) {
            *f = true;
        }
    }
}

/// Upkeep, then growth or famine-shrink. Size multiplies both what an ant can
/// do and what it costs to be.
pub fn apply_metabolism(i: usize, ants: &mut Ants, cfg: &Config) {
    ants.energy[i] -= ants.genome[i].upkeep(cfg, ants.size[i]);

    let max_e = ants.genome[i].max_energy(cfg, ants.size[i]);
    let max_size = ants.genome[i].traits.max_size;

    if ants.energy[i] > cfg.growth_threshold * max_e && ants.size[i] < max_size {
        let grow = cfg.growth_rate.min(max_size - ants.size[i]);
        ants.size[i] += grow;
        ants.energy[i] -= grow * cfg.max_energy_per_size;
    } else if ants.energy[i] <= 0.0 && ants.size[i] > MIN_SIZE {
        let shrink = cfg.shrink_rate.min(ants.size[i] - MIN_SIZE);
        ants.size[i] -= shrink;
        ants.energy[i] += shrink * cfg.max_energy_per_size;
    }
}

/// The single place an ant dies. Runs after every ant has acted, so an ant
/// driven to zero energy by a lower-id attacker may still have taken its own
/// turn this tick. That is deterministic, and cheaper than a mid-tick recheck.
pub fn sweep_deaths(ants: &mut Ants, ctx: &mut ApplyCtx) {
    for i in 0..ants.len() {
        if !ants.alive[i] {
            continue;
        }
        let starved = ants.energy[i] <= 0.0;
        let elderly = ants.age[i] as f32 > ants.genome[i].traits.lifespan;
        if !starved && !elderly {
            continue;
        }

        ants.alive[i] = false;

        let (cx, cy) = ants.cell(i);
        let c = ctx.grid.idx(cx, cy);
        if ants.carrying[i] > 0.0 {
            ctx.grid.food[c] += ants.carrying[i];
            ants.carrying[i] = 0.0;
        }
        if ctx.spatial.occupant(c) == Some(i as u32) {
            ctx.spatial.clear_occupant(c);
        }

        let colony = &mut ctx.colonies[ants.colony[i] as usize];
        colony.record_death(
            // TODO(task 4): use recent_productivity
            ctx.cfg.fitness(ants.food_delivered[i], ants.food_harvested[i], ants.food_homing[i], 0.0),
            ants.lineage[i],
            &ants.genome[i],
            ctx.cfg.hall_of_fame_size,
        );
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
    fn grabbing_food_credits_food_harvested() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let c = f.grid.idx(8, 8);
        f.grid.food[c] = 100.0;
        f.ants.carrying[0] = 0.0;
        f.ants.food_harvested[0] = 0.0;
        let i = Intent {
            grab: true,
            ..intent()
        };
        let (ants, mut ctx) = f.split();
        apply_food(0, &i, ants, &mut ctx);
        assert!(f.ants.food_harvested[0] > 0.0, "grab must credit harvest");
        assert_eq!(
            f.ants.food_harvested[0], f.ants.carrying[0],
            "harvested equals what entered cargo this grab"
        );
    }

    #[test]
    fn grabbing_on_a_trail_cell_flags_a_trail_follow() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let c = f.grid.idx(8, 8);
        f.grid.food[c] = 100.0;
        f.phero.food[c] = TRAIL_FOLLOW_THRESHOLD + 1.0;
        let i = Intent {
            grab: true,
            ..intent()
        };
        let (ants, mut ctx) = f.split();
        apply_food(0, &i, ants, &mut ctx);
        assert!(
            f.ants.followed_trail_this_tick(0),
            "grabbing on an established trail is a trail-follow"
        );
    }

    #[test]
    fn grabbing_without_a_trail_is_not_a_follow() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let c = f.grid.idx(8, 8);
        f.grid.food[c] = 100.0; // no food pheromone: virgin food
        let i = Intent {
            grab: true,
            ..intent()
        };
        let (ants, mut ctx) = f.split();
        apply_food(0, &i, ants, &mut ctx);
        assert!(!f.ants.followed_trail_this_tick(0), "the trailblazer follows nothing");
    }

    #[test]
    fn a_trail_over_empty_ground_is_not_a_follow() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let c = f.grid.idx(8, 8);
        f.phero.food[c] = TRAIL_FOLLOW_THRESHOLD + 1.0; // a trail, but no food to grab
        let i = Intent {
            grab: true,
            ..intent()
        };
        let (ants, mut ctx) = f.split();
        apply_food(0, &i, ants, &mut ctx);
        assert!(
            !f.ants.followed_trail_this_tick(0),
            "no food taken means no follow, however strong the trail"
        );
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
    fn depositing_credits_the_colonys_lifetime_total() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let n = f.grid.idx(8, 8);
        f.grid.nest[n] = 1;
        f.ants.carrying[0] = 6.0;
        let (ants, mut ctx) = f.split();
        apply_nest(0, ants, &mut ctx);
        assert_eq!(f.colonies[1].delivered_total, 6.0);
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
        // Deficit below refuel_rate, so the cap under test is max_energy (the
        // ant's need), not the per-tick refuel rate.
        f.ants.energy[0] = max_e - 0.5;
        let (ants, mut ctx) = f.split();
        apply_nest(0, ants, &mut ctx);
        assert!((f.ants.energy[0] - max_e).abs() < 1e-4);
        assert!(
            (f.colonies[1].store - 999.5).abs() < 1e-3,
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
        assert_eq!(f.phero.scent.mag[c], f.cfg.ant_scent_emission);
        assert_eq!(f.phero.scent.owner[c], 1);
    }

    #[test]
    fn every_ant_lays_colony_trail_unconditionally() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let c = f.grid.idx(8, 8);
        deposit_passive(c, 0.0, 1, &mut f.ctx());
        assert_eq!(f.phero.trail.mag[c], f.cfg.trail_emission);
        assert_eq!(f.phero.trail.owner[c], 1);
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

    #[test]
    fn attacking_costs_energy_and_damages_the_foe() {
        let mut f = fixture(&[(8.5, 8.5, 1), (9.5, 8.5, 2)]);
        f.rebuild();
        let att_before = f.ants.energy[0];
        let def_before = f.ants.energy[1];
        let i = Intent {
            attack: true,
            ..intent()
        };
        let (ants, mut ctx) = f.split();
        apply_combat(0, &i, ants, &mut ctx);
        assert!(f.ants.energy[0] < att_before, "attacker pays");
        assert!(f.ants.energy[1] < def_before, "defender bleeds");
    }

    #[test]
    fn a_nestmate_is_never_attacked() {
        let mut f = fixture(&[(8.5, 8.5, 1), (9.5, 8.5, 1)]);
        f.rebuild();
        let before = f.ants.energy[1];
        let i = Intent {
            attack: true,
            ..intent()
        };
        let (ants, mut ctx) = f.split();
        apply_combat(0, &i, ants, &mut ctx);
        assert_eq!(f.ants.energy[1], before);
    }

    #[test]
    fn a_distant_foe_is_out_of_reach() {
        let mut f = fixture(&[(2.5, 2.5, 1), (12.5, 12.5, 2)]);
        f.rebuild();
        let before = f.ants.energy[1];
        let i = Intent {
            attack: true,
            ..intent()
        };
        let (ants, mut ctx) = f.split();
        apply_combat(0, &i, ants, &mut ctx);
        assert_eq!(f.ants.energy[1], before);
    }

    #[test]
    fn damage_scales_with_size_and_strength_and_is_reduced_by_armor() {
        let base = |att: (f32, f32), def_armor: f32| {
            let mut f = fixture(&[(8.5, 8.5, 1), (9.5, 8.5, 2)]);
            f.ants.size[0] = att.0;
            f.ants.genome[0].traits.strength = att.1;
            f.ants.genome[1].traits.armor = def_armor;
            f.ants.energy[1] = 1000.0;
            f.rebuild();
            let i = Intent {
                attack: true,
                ..intent()
            };
            let (ants, mut ctx) = f.split();
            apply_combat(0, &i, ants, &mut ctx);
            1000.0 - f.ants.energy[1]
        };
        assert!(
            base((2.0, 1.0), 0.0) > base((1.0, 1.0), 0.0),
            "size raises damage"
        );
        assert!(
            base((1.0, 1.0), 0.0) > base((1.0, 0.2), 0.0),
            "strength raises damage"
        );
        assert!(
            base((1.0, 1.0), 0.9) < base((1.0, 1.0), 0.0),
            "armor cuts damage"
        );
    }

    #[test]
    fn attacking_raises_the_alarm_pheromone() {
        let mut f = fixture(&[(8.5, 8.5, 1), (9.5, 8.5, 2)]);
        f.rebuild();
        let i = Intent {
            attack: true,
            ..intent()
        };
        let (ants, mut ctx) = f.split();
        apply_combat(0, &i, ants, &mut ctx);
        let victim_cell = f.grid.idx(9, 8);
        assert!(
            f.phero.alarm[victim_cell] > 0.0,
            "alarm marks the victim's cell"
        );
    }

    #[test]
    fn a_killer_scavenges_energy_from_the_body() {
        let mut f = fixture(&[(8.5, 8.5, 1), (9.5, 8.5, 2)]);
        f.ants.energy[0] = 10.0;
        f.ants.energy[1] = 0.01; // one hit from death
        f.ants.genome[0].traits.strength = 1.0;
        f.rebuild();
        let i = Intent {
            attack: true,
            ..intent()
        };
        let (ants, mut ctx) = f.split();
        apply_combat(0, &i, ants, &mut ctx);
        assert!(
            f.ants.energy[1] <= 0.0,
            "victim is dead by the sweep's reckoning"
        );
        assert!(
            f.ants.energy[0] > 10.0 - f.cfg.attack_cost,
            "killer absorbed the corpse"
        );
    }

    #[test]
    fn combat_does_not_mark_the_dead_the_sweep_does() {
        let mut f = fixture(&[(8.5, 8.5, 1), (9.5, 8.5, 2)]);
        f.ants.energy[1] = 0.01;
        f.rebuild();
        let i = Intent {
            attack: true,
            ..intent()
        };
        let (ants, mut ctx) = f.split();
        apply_combat(0, &i, ants, &mut ctx);
        assert!(f.ants.alive[1], "still flagged alive until the sweep runs");
    }

    #[test]
    fn only_the_killing_blow_scavenges_so_a_mob_cannot_mint_energy() {
        // Three attackers, one nearly-dead victim. Because deaths are flagged
        // by the sweep and not by combat, the corpse stays a legal target all
        // tick. Exactly one attacker may collect the bounty.
        let mut f = fixture(&[(8.5, 8.5, 1), (9.5, 8.5, 1), (8.5, 9.5, 1), (9.5, 9.5, 2)]);
        f.ants.energy[3] = 0.01;
        for a in 0..3 {
            f.ants.energy[a] = 10.0;
            f.ants.genome[a].traits.strength = 1.0;
        }
        f.ants.genome[3].traits.armor = 0.0;
        f.rebuild();

        let i = Intent {
            attack: true,
            ..intent()
        };
        for a in 0..3 {
            let (ants, mut ctx) = f.split();
            apply_combat(a, &i, ants, &mut ctx);
        }

        let bounty = f.cfg.kill_energy_frac * f.cfg.max_energy_per_size * f.ants.size[3];
        let gained: f32 = (0..3)
            .map(|a| f.ants.energy[a] - (10.0 - f.cfg.attack_cost))
            .sum();
        assert!(
            (gained - bounty).abs() < 1e-3,
            "mob scavenged {gained} from a corpse worth {bounty}: energy was created"
        );
    }

    #[test]
    fn hitting_an_already_dead_victim_yields_nothing() {
        let mut f = fixture(&[(8.5, 8.5, 1), (9.5, 8.5, 2)]);
        f.ants.energy[0] = 10.0;
        f.ants.energy[1] = -5.0; // already below zero, sweep has not run
        f.rebuild();
        let i = Intent {
            attack: true,
            ..intent()
        };
        let (ants, mut ctx) = f.split();
        apply_combat(0, &i, ants, &mut ctx);
        assert!(
            (f.ants.energy[0] - (10.0 - f.cfg.attack_cost)).abs() < 1e-4,
            "attacker gained energy from a corpse it did not kill"
        );
    }

    #[test]
    fn an_exhausted_ant_cannot_afford_to_attack() {
        let mut f = fixture(&[(8.5, 8.5, 1), (9.5, 8.5, 2)]);
        f.ants.energy[0] = f.cfg.attack_cost * 0.5;
        let before = f.ants.energy[1];
        f.rebuild();
        let i = Intent {
            attack: true,
            ..intent()
        };
        let (ants, mut ctx) = f.split();
        apply_combat(0, &i, ants, &mut ctx);
        assert_eq!(f.ants.energy[1], before);
    }

    #[test]
    fn metabolism_drains_energy_every_tick() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let before = f.ants.energy[0];
        apply_metabolism(0, &mut f.ants, &f.cfg);
        assert!(f.ants.energy[0] < before);
    }

    #[test]
    fn a_well_fed_ant_grows_and_pays_for_the_tissue() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        f.ants.size[0] = 1.0;
        f.ants.energy[0] = f.ants.genome[0].max_energy(&f.cfg, 1.0);
        let size_before = f.ants.size[0];
        apply_metabolism(0, &mut f.ants, &f.cfg);
        assert!(f.ants.size[0] > size_before, "should grow when nearly full");
    }

    #[test]
    fn growth_stops_at_the_genetic_max_size() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        let max = f.ants.genome[0].traits.max_size;
        f.ants.size[0] = max;
        f.ants.energy[0] = f.ants.genome[0].max_energy(&f.cfg, max);
        apply_metabolism(0, &mut f.ants, &f.cfg);
        assert!(f.ants.size[0] <= max + 1e-6);
    }

    #[test]
    fn a_starving_ant_burns_its_own_body_for_energy() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        f.ants.size[0] = 2.0;
        f.ants.energy[0] = 0.0;
        apply_metabolism(0, &mut f.ants, &f.cfg);
        assert!(f.ants.size[0] < 2.0, "fat is a famine buffer");
        assert!(f.ants.energy[0] > 0.0, "and it buys another tick");
    }

    #[test]
    fn shrinking_bottoms_out_at_min_size() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        f.ants.size[0] = MIN_SIZE;
        f.ants.energy[0] = 0.0;
        apply_metabolism(0, &mut f.ants, &f.cfg);
        assert!(f.ants.size[0] >= MIN_SIZE - 1e-6);
    }

    #[test]
    fn the_sweep_kills_the_starved_and_records_their_fitness() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        f.ants.energy[0] = 0.0;
        f.ants.food_delivered[0] = 12.0;
        let (ants, mut ctx) = f.split();
        sweep_deaths(ants, &mut ctx);
        assert!(!f.ants.alive[0]);
        assert_eq!(f.colonies[1].deaths, 1);
        assert_eq!(f.colonies[1].hall_of_fame[0].0, 12.0);
    }

    #[test]
    fn the_sweep_kills_ants_that_outlive_their_genetic_lifespan() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        f.ants.age[0] = f.ants.genome[0].traits.lifespan as u32 + 1;
        let (ants, mut ctx) = f.split();
        sweep_deaths(ants, &mut ctx);
        assert!(!f.ants.alive[0], "nobody lives forever");
    }

    #[test]
    fn a_corpse_drops_the_food_it_was_carrying() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        f.ants.energy[0] = 0.0;
        f.ants.carrying[0] = 7.0;
        let (ants, mut ctx) = f.split();
        sweep_deaths(ants, &mut ctx);
        let here = f.grid.idx(8, 8);
        assert_eq!(f.grid.food[here], 7.0);
    }

    #[test]
    fn the_sweep_leaves_the_living_alone() {
        let mut f = fixture(&[(8.5, 8.5, 1)]);
        f.ants.energy[0] = 5.0;
        let (ants, mut ctx) = f.split();
        sweep_deaths(ants, &mut ctx);
        assert!(f.ants.alive[0]);
        assert_eq!(f.colonies[1].deaths, 0);
    }
}
