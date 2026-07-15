//! Is the world winnable at all?
//!
//! These tests drive `apply_*` with a *scripted* controller — a plain Rust
//! function implementing the forager policy — bypassing the neural network
//! entirely. If a hand-written forager cannot profit here, the world's physics
//! are broken and no amount of evolution will help. Nothing in this file can
//! fail for neural-network reasons.

use sim::ants::Ants;
use sim::apply::{apply_food, apply_metabolism, apply_movement, apply_nest, ApplyCtx};
use sim::config::Config;
use sim::grid::NO_NEST;
use sim::intent::Intent;
use sim::spatial::Spatial;
use sim::world::World;
use sim::N_MEMORY;

fn cfg() -> Config {
    Config {
        width: 64,
        height: 64,
        num_colonies: 1,
        initial_ants_per_colony: 20,
        food_patch_count: 4,
        ..Config::default()
    }
}

/// Can ant `i` step into `(tx, ty)`? Mirrors `apply_movement`'s rules exactly:
/// stone and the map edge block, and one ant per cell except on nest tiles.
fn walkable(i: usize, w: &World, spatial: &Spatial, tx: i32, ty: i32) -> bool {
    if w.grid.is_stone(tx, ty) {
        return false;
    }
    let c = w.grid.idx_clamped(tx, ty);
    if w.grid.nest[c] != NO_NEST {
        return true;
    }
    match spatial.occupant(c) {
        None => true,
        Some(o) => o as usize == i,
    }
}

/// Nearest food cell nobody is standing on. Targeting an occupied cell would
/// queue every ant behind whoever arrived first. O(cells), which is fine here.
fn nearest_free_food(i: usize, ants: &Ants, w: &World, spatial: &Spatial) -> Option<(f32, f32)> {
    let (ax, ay) = (ants.x[i], ants.y[i]);
    let mut best: Option<(f32, (f32, f32))> = None;
    for c in 0..w.grid.food.len() {
        if w.grid.food[c] <= 0.0 {
            continue;
        }
        if let Some(o) = spatial.occupant(c) {
            if o as usize != i {
                continue;
            }
        }
        let width = w.grid.width as usize;
        let (fx, fy) = ((c % width) as f32, (c / width) as f32);
        let d = (fx - ax).hypot(fy - ay);
        if best.map_or(true, |(bd, _)| d < bd) {
            best = Some((d, (fx + 0.5, fy + 0.5)));
        }
    }
    best.map(|(_, t)| t)
}

/// Candidate headings, nearest-to-desired first, sweeping the **full circle**.
///
/// The sweep must be able to point backwards. Capped at +-90 degrees, an ant
/// that walks into a concave pocket of stone finds every candidate heading
/// blocked, stops, recomputes the same blocked heading next tick, and never
/// moves again. Measured: 19 of 20 ants frozen by tick 1500, with 67k food
/// still on the map.
const SIDESTEPS: [f32; 17] = {
    use std::f32::consts::PI;
    // Eight evenly spaced offsets each way, out to a full half-turn.
    const S: f32 = PI / 8.0;
    [
        0.0,
        S,
        -S,
        2.0 * S,
        -2.0 * S,
        3.0 * S,
        -3.0 * S,
        4.0 * S,
        -4.0 * S,
        5.0 * S,
        -5.0 * S,
        6.0 * S,
        -6.0 * S,
        7.0 * S,
        -7.0 * S,
        PI,
        -PI,
    ]
};

/// The forager policy, written by hand: harvest where you stand until half
/// loaded, then carry it home. Steer at the goal, stepping around whatever is
/// in the way.
fn scripted_intent(i: usize, ants: &Ants, w: &World, spatial: &Spatial) -> Intent {
    let (ax, ay) = (ants.x[i], ants.y[i]);
    let (cx, cy) = ants.cell(i);
    let laden = ants.carrying[i] >= ants.genome[i].traits.carry_capacity * 0.5;
    let speed = ants.genome[i].traits.max_speed;
    let on_food = w.grid.food[w.grid.idx(cx, cy)] > 0.0;
    let grab = on_food && !laden;

    let idle = Intent {
        heading: ants.heading[i],
        speed: 0.0,
        attack: false,
        grab,
        release: false,
        memory: [0.0; N_MEMORY],
    };

    // Standing on the goal: harvest, do not wander off it.
    if grab {
        return idle;
    }

    let target = if laden {
        w.colonies[0].nest_center
    } else {
        match nearest_free_food(i, ants, w, spatial) {
            Some(t) => t,
            None => return idle,
        }
    };

    let desired = (target.1 - ay).atan2(target.0 - ax);
    for off in SIDESTEPS {
        let h = desired + off;
        let (nx, ny) = (ax + h.cos() * speed, ay + h.sin() * speed);
        let (tx, ty) = (nx.floor() as i32, ny.floor() as i32);
        let same_cell = tx == cx as i32 && ty == cy as i32;
        if same_cell || walkable(i, w, spatial, tx, ty) {
            return Intent {
                heading: h,
                speed,
                attack: false,
                grab,
                release: false,
                memory: [0.0; N_MEMORY],
            };
        }
    }
    idle
}

/// Run the scripted colony. Deliberately does *not* call `sweep_deaths` or
/// `reproduce`: this measures the foraging economy alone.
fn run_scripted(seed: u64, ticks: u32) -> World {
    run_scripted_with(cfg(), seed, ticks)
}

fn run_scripted_with(c: Config, seed: u64, ticks: u32) -> World {
    let mut w = World::new(&c, seed);
    let mut spatial = Spatial::new(&c);
    for _ in 0..ticks {
        spatial.rebuild(&w.ants);
        let intents: Vec<Intent> = (0..w.ants.len())
            .map(|i| scripted_intent(i, &w.ants, &w, &spatial))
            .collect();
        let mut ctx = ApplyCtx {
            cfg: &w.cfg,
            grid: &mut w.grid,
            phero: &mut w.phero,
            spatial: &mut spatial,
            colonies: &mut w.colonies,
        };
        for i in 0..w.ants.len() {
            apply_movement(i, &intents[i], &mut w.ants, &mut ctx);
            apply_food(i, &intents[i], &mut w.ants, &mut ctx);
            apply_nest(i, &mut w.ants, &mut ctx);
            apply_metabolism(i, &mut w.ants, ctx.cfg);
        }
    }
    w
}

#[test]
fn a_scripted_forager_grows_the_colony_food_store() {
    let start_store = cfg().initial_food_store;
    let w = run_scripted(11, 4_000);
    assert!(
        w.colonies[0].store > start_store,
        "a hand-written forager could not profit: the world's economy is unwinnable. \
         store {} -> {}. Check harvest_rate, refuel_rate, birth_cost, and the trait taxes.",
        start_store,
        w.colonies[0].store
    );
}

#[test]
fn a_scripted_forager_actually_delivers_food() {
    let w = run_scripted(12, 4_000);
    let delivered: f32 = w.ants.food_delivered.iter().sum();
    assert!(delivered > 0.0, "no ant reached the nest with a load");
}

/// The stall this guards against is subtle: ants that deliver for a while and
/// then freeze look, in aggregate, exactly like a colony that has run out of
/// nearby food.
#[test]
fn scripted_foragers_keep_delivering_and_do_not_deadlock() {
    let early = run_scripted(11, 1_000);
    let late = run_scripted(11, 4_000);
    let d_early: f32 = early.ants.food_delivered.iter().sum();
    let d_late: f32 = late.ants.food_delivered.iter().sum();
    assert!(
        d_late > d_early * 1.5,
        "delivery stalled: {d_early} by tick 1000 but only {d_late} by tick 4000. \
         The foragers are stuck against terrain or each other."
    );
    let food_left: f32 = late.grid.food.iter().sum();
    assert!(
        food_left > 0.0,
        "the map was stripped bare; test is inconclusive"
    );
}

#[test]
fn a_colony_with_no_reachable_food_collapses_and_refounds_repeatedly() {
    let c = Config {
        width: 64,
        height: 64,
        num_colonies: 1,
        initial_ants_per_colony: 40,
        food_patch_count: 0,
        initial_food_store: 10.0,
        ..Config::default()
    };
    let mut w = World::new(&c, 13);
    // worldgen still seeds one guaranteed patch per colony; remove all food.
    w.grid.food.iter_mut().for_each(|f| *f = 0.0);

    for _ in 0..20_000 {
        w.tick();
    }
    // With the extinction floor retired, a foodless colony truly dies — and the
    // same tick refounds a fresh cohort, which then starves too. So it thrashes:
    // `refounds` climbs, and with no food it never affords a paid birth or grows
    // past a single founding cohort.
    assert!(
        w.colonies[0].refounds > 0,
        "a starving colony should have collapsed and refounded"
    );
    assert!(
        w.ants.population(0) <= w.cfg.initial_ants_per_colony,
        "with no food it cannot grow past a fresh cohort, got {}",
        w.ants.population(0)
    );
    assert!(
        w.colonies[0].births == 0,
        "a starving colony must not afford paid births"
    );
}

#[test]
fn a_random_colony_does_not_immediately_explode_in_population() {
    let mut w = World::new(&cfg(), 14);
    for _ in 0..5_000 {
        w.tick();
    }
    assert!(
        w.ants.len() < 5_000,
        "population ran away: birth_cost is too cheap"
    );
}

/// The maps are not the problem; the search is.
///
/// `tests/known_good.rs` hill-climbs a genome that delivers thousands of food on
/// map seed 2 and **exactly zero** on seeds 1 and 3 — for every genome in its
/// 300-generation lineage, including the random one it started from. That looks
/// like a broken map until you run a competent controller over the same three
/// maps, which is what this test does. All three are richly forageable. The
/// zeroes are a sparse-reward problem in the search, not terrain.
#[test]
fn the_scripted_forager_profits_on_every_map_the_genome_search_uses() {
    let search_cfg = || Config {
        width: 48,
        height: 48,
        num_colonies: 1,
        initial_ants_per_colony: 30,
        food_patch_count: 4,
        ..Config::default()
    };
    for seed in [1u64, 2, 3] {
        let w = run_scripted_with(search_cfg(), seed, 3_000);
        let delivered: f32 = w.ants.food_delivered.iter().sum();
        assert!(
            delivered > 1_000.0,
            "seed {seed}: a scripted forager delivered only {delivered:.0}. If this map \
             really is barren, the genome search's zero scores on it mean nothing."
        );
    }
}
