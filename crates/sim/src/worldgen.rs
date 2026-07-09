use crate::colony::ColonyState;
use crate::config::Config;
use crate::grid::Grid;
use crate::rng::Pcg32;

/// Nests are 3x3 blocks: big enough that returning foragers do not queue,
/// small enough to be a real place on the map.
pub const NEST_RADIUS: i32 = 1;
/// Each colony gets one guaranteed food patch this far from its nest, so no
/// colony starts in a barren corner. The rest are scattered.
///
/// Kept short deliberately: the round trip must pay for itself at mean traits
/// (see the break-even note on `Config`), and the nest scent gradient has to
/// still be readable at this range (see `tests/gradient.rs`).
pub const SEED_PATCH_DISTANCE: f32 = 12.0;
/// Colonies are placed on a circle at this fraction of the map's half-width.
const NEST_RING_FRAC: f32 = 0.72;

/// How many stone blobs to stamp for a given map, from a target coverage.
///
/// A fixed blob *count* does not survive changing the map size: 60 blobs is 3%
/// of a 512x512 map and more than 100% of the 48x48 worlds the tests use, which
/// would bury every test colony in solid rock and make the behavioural tests
/// fail for terrain reasons while pointing at the economy.
///
/// Mean blob radius is `radius * (0.4 + E[U(0,1)]) = 0.9 * radius`, so mean
/// area is `PI * (0.9r)^2 ~= 2.54 r^2`. Overlap means realised coverage lands a
/// little under the target, which is fine.
fn stone_blob_count(cfg: &Config) -> u32 {
    let mean_blob_area = 2.54 * cfg.stone_blob_radius * cfg.stone_blob_radius;
    let target_cells = cfg.stone_density * cfg.cell_count() as f32;
    ((target_cells / mean_blob_area).round() as u32).max(1)
}

pub fn generate(cfg: &Config, rng: &mut Pcg32) -> (Grid, Vec<ColonyState>) {
    let mut grid = Grid::new(cfg);
    let w = cfg.width as f32;
    let h = cfg.height as f32;
    let (cxm, cym) = (w * 0.5, h * 0.5);

    // --- Stone blobs: chokepoints, so different regions reward different bets.
    for _ in 0..stone_blob_count(cfg) {
        let bx = rng.next_f32() * w;
        let by = rng.next_f32() * h;
        let r = cfg.stone_blob_radius * (0.4 + rng.next_f32());
        stamp(&mut grid, bx, by, r, |g, i| g.stone[i] = true);
    }

    // --- Colonies on a ring, evenly spaced. ---
    let mut colonies = Vec::with_capacity(cfg.num_colonies as usize);
    let ring = cxm.min(cym) * NEST_RING_FRAC;
    for id in 0..cfg.num_colonies {
        let theta = std::f32::consts::TAU * id as f32 / cfg.num_colonies as f32;
        let nx = cxm + ring * theta.cos();
        let ny = cym + ring * theta.sin();

        let mut col = ColonyState::new(id);
        col.store = cfg.initial_food_store;
        col.nest_center = (nx, ny);

        let (ix, iy) = (nx as i32, ny as i32);
        for dy in -NEST_RADIUS..=NEST_RADIUS {
            for dx in -NEST_RADIUS..=NEST_RADIUS {
                let (x, y) = (ix + dx, iy + dy);
                if !grid.in_bounds(x, y) {
                    continue;
                }
                let i = grid.idx_clamped(x, y);
                // A nest is never stone, and never steals another colony's tile.
                if grid.nest[i] == crate::grid::NO_NEST {
                    grid.stone[i] = false;
                    grid.food[i] = 0.0;
                    grid.nest[i] = id;
                    col.nest_tiles.push(i);
                }
            }
        }

        // One guaranteed patch within foraging reach of this nest.
        let a = rng.next_f32() * std::f32::consts::TAU;
        let px = (nx + SEED_PATCH_DISTANCE * a.cos()).clamp(1.0, w - 2.0);
        let py = (ny + SEED_PATCH_DISTANCE * a.sin()).clamp(1.0, h - 2.0);
        food_patch(&mut grid, px, py, cfg);

        colonies.push(col);
    }

    // --- Scattered patches at varied distances. ---
    for _ in 0..cfg.food_patch_count {
        let px = rng.next_f32() * w;
        let py = rng.next_f32() * h;
        food_patch(&mut grid, px, py, cfg);
    }

    (grid, colonies)
}

fn food_patch(grid: &mut Grid, px: f32, py: f32, cfg: &Config) {
    let r = cfg.food_patch_radius;
    let maxf = cfg.food_patch_max;
    stamp(grid, px, py, r, |g, i| {
        if !g.stone[i] && g.nest[i] == crate::grid::NO_NEST {
            g.food[i] = maxf;
        }
    });
}

fn stamp(grid: &mut Grid, cx: f32, cy: f32, r: f32, mut f: impl FnMut(&mut Grid, usize)) {
    let (x0, x1) = ((cx - r) as i32, (cx + r) as i32);
    let (y0, y1) = ((cy - r) as i32, (cy + r) as i32);
    for y in y0..=y1 {
        for x in x0..=x1 {
            if !grid.in_bounds(x, y) {
                continue;
            }
            if (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy) <= r {
                let i = grid.idx_clamped(x, y);
                f(grid, i);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::rng::Pcg32;

    fn cfg() -> Config {
        Config {
            width: 128,
            height: 128,
            num_colonies: 4,
            food_patch_count: 8,
            ..Config::default()
        }
    }

    #[test]
    fn generation_is_deterministic_for_a_seed() {
        let c = cfg();
        let (g1, _) = generate(&c, &mut Pcg32::new(1, 1));
        let (g2, _) = generate(&c, &mut Pcg32::new(1, 1));
        assert_eq!(g1.stone, g2.stone);
        assert_eq!(g1.food, g2.food);
        assert_eq!(g1.nest, g2.nest);
    }

    #[test]
    fn different_seeds_give_different_maps() {
        let c = cfg();
        let (g1, _) = generate(&c, &mut Pcg32::new(1, 1));
        let (g2, _) = generate(&c, &mut Pcg32::new(2, 2));
        assert_ne!(g1.stone, g2.stone);
    }

    #[test]
    fn one_colony_state_per_configured_colony() {
        let c = cfg();
        let (_, colonies) = generate(&c, &mut Pcg32::new(1, 1));
        assert_eq!(colonies.len(), c.num_colonies as usize);
        for (i, col) in colonies.iter().enumerate() {
            assert_eq!(col.id, i as u8);
        }
    }

    #[test]
    fn every_colony_starts_with_a_full_store() {
        let c = cfg();
        let (_, colonies) = generate(&c, &mut Pcg32::new(1, 1));
        assert!(colonies.iter().all(|col| col.store == c.initial_food_store));
    }

    #[test]
    fn every_colony_has_nest_tiles_and_they_are_tagged_on_the_grid() {
        let c = cfg();
        let (grid, colonies) = generate(&c, &mut Pcg32::new(1, 1));
        for col in &colonies {
            assert!(!col.nest_tiles.is_empty());
            for &t in &col.nest_tiles {
                assert_eq!(grid.nest[t], col.id);
            }
        }
    }

    #[test]
    fn nests_are_never_stone() {
        let c = cfg();
        let (grid, colonies) = generate(&c, &mut Pcg32::new(1, 1));
        for col in &colonies {
            for &t in &col.nest_tiles {
                assert!(!grid.stone[t], "a nest tile was buried in stone");
            }
        }
    }

    #[test]
    fn nests_do_not_overlap() {
        let c = cfg();
        let (_, colonies) = generate(&c, &mut Pcg32::new(1, 1));
        let mut all: Vec<usize> = colonies.iter().flat_map(|c| c.nest_tiles.clone()).collect();
        let before = all.len();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), before);
    }

    #[test]
    fn some_food_exists_and_none_sits_on_stone() {
        let c = cfg();
        let (grid, _) = generate(&c, &mut Pcg32::new(1, 1));
        let total: f32 = grid.food.iter().sum();
        assert!(total > 0.0, "map has no food at all");
        for i in 0..grid.food.len() {
            if grid.stone[i] {
                assert_eq!(grid.food[i], 0.0);
            }
        }
    }

    #[test]
    fn food_never_exceeds_the_patch_maximum() {
        let c = cfg();
        let (grid, _) = generate(&c, &mut Pcg32::new(1, 1));
        assert!(grid.food.iter().all(|f| *f <= c.food_patch_max + 1e-3));
    }

    #[test]
    fn each_colony_has_food_within_reach_of_its_nest() {
        // Guards the "every colony dies in the first minute" failure mode.
        let c = cfg();
        let (grid, colonies) = generate(&c, &mut Pcg32::new(1, 1));
        for col in &colonies {
            let (nx, ny) = col.nest_center;
            let near: f32 = (0..grid.food.len())
                .filter(|&i| {
                    let (x, y) = (
                        (i % grid.width as usize) as f32,
                        (i / grid.width as usize) as f32,
                    );
                    (x - nx).hypot(y - ny) < SEED_PATCH_DISTANCE + c.food_patch_radius
                })
                .map(|i| grid.food[i])
                .sum();
            assert!(near > 0.0, "colony {} has no food near its nest", col.id);
        }
    }

    #[test]
    fn the_map_has_some_stone_but_is_not_a_wall() {
        let c = cfg();
        let (grid, _) = generate(&c, &mut Pcg32::new(1, 1));
        let stones = grid.stone.iter().filter(|s| **s).count();
        let frac = stones as f32 / grid.stone.len() as f32;
        assert!(frac > 0.01, "no terrain variety: {frac}");
        assert!(frac < 0.30, "map is mostly wall: {frac}");
    }

    #[test]
    fn stone_coverage_is_independent_of_map_size() {
        // A fixed blob count would bury the small worlds the tests use while
        // barely speckling the real 512x512 map.
        let frac_at = |side: u16| {
            let c = Config {
                width: side,
                height: side,
                num_colonies: 2,
                ..cfg()
            };
            let (grid, _) = generate(&c, &mut Pcg32::new(1, 1));
            grid.stone.iter().filter(|s| **s).count() as f32 / grid.stone.len() as f32
        };
        for side in [64u16, 128, 256] {
            let f = frac_at(side);
            assert!(
                (0.01..0.30).contains(&f),
                "{side}x{side} map has {f} stone coverage, outside the workable band"
            );
        }
    }

    #[test]
    fn a_tiny_test_world_still_gets_at_least_one_blob() {
        let c = Config {
            width: 32,
            height: 32,
            num_colonies: 1,
            ..cfg()
        };
        let (grid, _) = generate(&c, &mut Pcg32::new(1, 1));
        assert!(grid.stone.iter().any(|s| *s));
    }
}
