//! Diagnostics for the one signal homing depends on: the nest scent gradient.
//!
//! If these fail, no amount of evolution produces a forager, because the
//! information an ant would need is not in its inputs.

use sim::config::Config;
use sim::pheromone::Pheromones;
use sim::sense::squash_phero;

/// Emit nest scent from a 3x3 block at the centre, let it reach equilibrium,
/// and report what an ant's sensor would read at each distance.
fn equilibrium_profile(cfg: &Config, ticks: u32) -> Vec<(i32, f32)> {
    let mut p = Pheromones::new(cfg);
    let (w, h) = (cfg.width as i32, cfg.height as i32);
    let (cx, cy) = (w / 2, h / 2);
    let idx = |x: i32, y: i32| (y * w + x) as usize;

    for _ in 0..ticks {
        for dy in -1..=1 {
            for dx in -1..=1 {
                p.deposit_scent(idx(cx + dx, cy + dy), cfg.nest_scent_emission, 0);
            }
        }
        p.step(cfg);
    }

    [2, 4, 6, 9, 12, 16, 20]
        .iter()
        .map(|&d| {
            let (raw, _) = p.scent_for(idx(cx + d, cy), 0);
            (d, squash_phero(raw, cfg.phero_log_div))
        })
        .collect()
}

fn cfg() -> Config {
    Config {
        width: 96,
        height: 96,
        ..Config::default()
    }
}

#[test]
fn the_nest_gradient_decreases_monotonically_with_distance() {
    let profile = equilibrium_profile(&cfg(), 3_000);
    for w in profile.windows(2) {
        assert!(
            w[0].1 > w[1].1,
            "scent is not monotonically decreasing: {:?} then {:?}\nfull profile: {profile:?}",
            w[0],
            w[1]
        );
    }
}

#[test]
fn the_nest_gradient_is_not_saturated_near_the_nest() {
    // The failure mode a tanh squash produces: everything within ~20 cells of
    // the nest reads exactly 1.0, so the ant standing in it is gradient-blind.
    let profile = equilibrium_profile(&cfg(), 3_000);
    for (d, v) in &profile {
        assert!(
            *v < 1.0,
            "sensor saturated at distance {d}: {v}\nprofile: {profile:?}"
        );
    }
}

#[test]
fn the_nest_gradient_is_discriminable_at_foraging_range() {
    // An ant's whiskers sample a few cells apart. If two adjacent sample points
    // differ by less than a whisker's worth of f32 noise, the gradient carries
    // no usable information. Checks specifically around SEED_PATCH_DISTANCE.
    let profile = equilibrium_profile(&cfg(), 3_000);
    let at = |d: i32| profile.iter().find(|(x, _)| *x == d).unwrap().1;
    let near_far = at(9) - at(16);
    assert!(
        near_far > 0.01,
        "gradient between 9 and 16 cells is only {near_far}; an ant at foraging \
         range cannot tell which way is home. Lower scent_diffusion, raise \
         scent_evaporation's decay, or shorten SEED_PATCH_DISTANCE.\nprofile: {profile:?}"
    );
}

#[test]
fn scent_reaches_beyond_the_guaranteed_food_patch() {
    // SEED_PATCH_DISTANCE is 12. A laden ant standing on that patch must be
    // able to sense home from where it stands.
    let profile = equilibrium_profile(&cfg(), 3_000);
    let at_patch = profile.iter().find(|(d, _)| *d == 12).unwrap().1;
    assert!(
        at_patch > 0.0,
        "no scent at all at the food patch: an ant there is lost"
    );
}
