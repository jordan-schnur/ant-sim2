use crate::config::Config;
use serde::{Deserialize, Serialize};

/// Sentinel in an `OwnedField::owner` meaning "no colony has marked this cell".
pub const NO_OWNER: u8 = 255;

/// A contested, colony-owned scalar field: each cell holds a magnitude and the
/// id of the colony that owns it. Deposits from the owner reinforce; a different
/// colony erodes the incumbent and seizes the cell once it erodes past zero.
/// Diffusion carries ownership with the magnitude. Both `scent` (persistent
/// homing/territory beacon) and `trail` (fast-fading recent-path signal) are
/// instances — they differ only in tuning and who deposits.
#[derive(Clone, Serialize, Deserialize)]
pub struct OwnedField {
    /// Strength of the *owning* colony's mark. Never negative.
    pub mag: Vec<f32>,
    pub owner: Vec<u8>,
}

impl OwnedField {
    fn new(n: usize) -> Self {
        OwnedField {
            mag: vec![0.0; n],
            owner: vec![NO_OWNER; n],
        }
    }

    /// Same colony reinforces. A different colony erodes, and takes ownership
    /// if it erodes the incumbent past zero. This is why territory is a
    /// contested field rather than eight independent maps.
    pub fn deposit(&mut self, i: usize, amount: f32, colony: u8) {
        if self.owner[i] == colony {
            self.mag[i] += amount;
        } else if self.owner[i] == NO_OWNER || self.mag[i] <= amount {
            self.mag[i] = amount - self.mag[i];
            self.owner[i] = colony;
        } else {
            self.mag[i] -= amount;
        }
    }

    /// `(own, foreign)` as seen by `colony`. Exactly one is nonzero.
    #[inline]
    pub fn read(&self, i: usize, colony: u8) -> (f32, f32) {
        if self.owner[i] == colony {
            (self.mag[i], 0.0)
        } else if self.owner[i] == NO_OWNER {
            (0.0, 0.0)
        } else {
            (0.0, self.mag[i])
        }
    }

    fn diffuse(&mut self, w: u16, h: u16, diffusion: f32, evaporation: f32) {
        diffuse_owned(&mut self.mag, &mut self.owner, w, h, diffusion, evaporation);
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Pheromones {
    pub width: u16,
    pub height: u16,
    pub food: Vec<f32>,
    pub alarm: Vec<f32>,
    /// The exploration / home trail: laid by *unladen* ants, so it is densest
    /// near the nest and along outbound routes. Climbing its gradient trends
    /// homeward. Shared across colonies (no owner) — the colony-correct
    /// direction comes from the home vector, not this field. See
    /// `docs/superpowers/specs/2026-07-15-home-vector-and-exploration-trail-design.md`.
    pub home: Vec<f32>,
    /// Persistent colony beacon: nest homing signal fused with ant territory.
    pub scent: OwnedField,
    /// Fast-fading "colony-mates were here recently" signal. Only ants lay it;
    /// nests never touch it. This is the recruit/explore channel un-fused from
    /// the homing beacon.
    pub trail: OwnedField,
}

impl Pheromones {
    pub fn new(cfg: &Config) -> Self {
        let n = cfg.cell_count();
        Pheromones {
            width: cfg.width,
            height: cfg.height,
            food: vec![0.0; n],
            alarm: vec![0.0; n],
            home: vec![0.0; n],
            scent: OwnedField::new(n),
            trail: OwnedField::new(n),
        }
    }

    #[inline]
    pub fn deposit_food(&mut self, i: usize, amount: f32) {
        self.food[i] += amount;
    }

    #[inline]
    pub fn deposit_home(&mut self, i: usize, amount: f32) {
        self.home[i] += amount;
    }

    #[inline]
    pub fn deposit_alarm(&mut self, i: usize, amount: f32) {
        self.alarm[i] += amount;
    }

    #[inline]
    pub fn deposit_scent(&mut self, i: usize, amount: f32, colony: u8) {
        self.scent.deposit(i, amount, colony);
    }

    /// `(own_scent, foreign_scent)` as seen by `colony`. Exactly one is nonzero.
    #[inline]
    pub fn scent_for(&self, i: usize, colony: u8) -> (f32, f32) {
        self.scent.read(i, colony)
    }

    #[inline]
    pub fn deposit_trail(&mut self, i: usize, amount: f32, colony: u8) {
        self.trail.deposit(i, amount, colony);
    }

    /// `(own_trail, foreign_trail)` as seen by `colony`. Exactly one is nonzero.
    #[inline]
    pub fn trail_for(&self, i: usize, colony: u8) -> (f32, f32) {
        self.trail.read(i, colony)
    }

    /// Evaporate then diffuse every layer. Diffusion is a 4-point blend toward
    /// the neighbour average; out-of-bounds neighbours read as the cell itself,
    /// so nothing leaks off the border.
    ///
    /// The owned layers carry an owner, so they cannot use the same blend: they
    /// must spread *with* their ownership, or a nest's beacon diffuses into
    /// cells that hold a magnitude nobody owns, which `read` reports as nothing.
    /// See `diffuse_owned`.
    pub fn step(&mut self, cfg: &Config) {
        diffuse_decay(
            &mut self.food,
            self.width,
            self.height,
            cfg.food_diffusion,
            cfg.food_evaporation,
        );
        diffuse_decay(
            &mut self.alarm,
            self.width,
            self.height,
            cfg.alarm_diffusion,
            cfg.alarm_evaporation,
        );
        diffuse_decay(
            &mut self.home,
            self.width,
            self.height,
            cfg.home_diffusion,
            cfg.home_evaporation,
        );
        self.scent
            .diffuse(self.width, self.height, cfg.scent_diffusion, cfg.scent_evaporation);
        self.trail
            .diffuse(self.width, self.height, cfg.trail_diffusion, cfg.trail_evaporation);
    }
}

/// Fold one owner's contribution into a small per-owner tally. At most five
/// distinct owners can reach a cell in one step (itself plus four neighbours).
#[inline]
fn accumulate(ids: &mut [u8; 5], amts: &mut [f32; 5], n: &mut usize, owner: u8, amount: f32) {
    if owner == NO_OWNER || amount <= 0.0 {
        return;
    }
    for k in 0..*n {
        if ids[k] == owner {
            amts[k] += amount;
            return;
        }
    }
    ids[*n] = owner;
    amts[*n] = amount;
    *n += 1;
}

/// Diffusion for a contested colony-owned field.
///
/// Each cell keeps `1 - diffusion` of its own magnitude and receives
/// `diffusion/4` from each neighbour, but every contribution arrives *tagged
/// with its owner*. The strongest owner takes the cell and the rest erode it,
/// exactly as `OwnedField::deposit` does — so territory contests resolve
/// identically whether the magnitude arrived by an ant's feet or by diffusion.
///
/// When every contribution shares one owner this reduces to the plain 4-point
/// blend, which is the common case in a colony's own territory.
///
/// Deterministic: neighbours are visited in a fixed order and ties go to the
/// lower colony id.
fn diffuse_owned(
    mag: &mut [f32],
    owner: &mut [u8],
    w: u16,
    h: u16,
    diffusion: f32,
    evaporation: f32,
) {
    let w = w as usize;
    let h = h as usize;
    let src_v = mag.to_vec();
    let src_o = owner.to_vec();

    let keep = 1.0 - diffusion;
    let share = diffusion * 0.25;

    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let (vc, oc) = (src_v[i], src_o[i]);

            let mut ids = [NO_OWNER; 5];
            let mut amts = [0.0f32; 5];
            let mut n = 0usize;
            accumulate(&mut ids, &mut amts, &mut n, oc, vc * keep);

            // Out-of-bounds neighbours read as the cell itself, so nothing
            // leaks off the border.
            let neighbours = [
                if x > 0 { i - 1 } else { i },
                if x + 1 < w { i + 1 } else { i },
                if y > 0 { i - w } else { i },
                if y + 1 < h { i + w } else { i },
            ];
            for j in neighbours {
                accumulate(&mut ids, &mut amts, &mut n, src_o[j], src_v[j] * share);
            }

            if n == 0 {
                mag[i] = 0.0;
                owner[i] = NO_OWNER;
                continue;
            }

            let mut best = 0usize;
            for k in 1..n {
                if amts[k] > amts[best] || (amts[k] == amts[best] && ids[k] < ids[best]) {
                    best = k;
                }
            }
            let total: f32 = amts[..n].iter().sum();
            // The winner's margin over everyone else combined.
            let net = (2.0 * amts[best] - total) * evaporation;

            if net < 1e-6 {
                mag[i] = 0.0;
                owner[i] = NO_OWNER;
            } else {
                mag[i] = net;
                owner[i] = ids[best];
            }
        }
    }
}

fn diffuse_decay(layer: &mut [f32], w: u16, h: u16, diffusion: f32, evaporation: f32) {
    let w = w as usize;
    let h = h as usize;
    // Diffusion must read the *previous* state everywhere, or the result
    // depends on iteration order.
    let src = layer.to_vec();
    let at = |x: usize, y: usize| src[y * w + x];
    for y in 0..h {
        for x in 0..w {
            let v = at(x, y);
            let l = if x > 0 { at(x - 1, y) } else { v };
            let r = if x + 1 < w { at(x + 1, y) } else { v };
            let u = if y > 0 { at(x, y - 1) } else { v };
            let d = if y + 1 < h { at(x, y + 1) } else { v };
            let avg = 0.25 * (l + r + u + d);
            layer[y * w + x] = (v + diffusion * (avg - v)) * evaporation;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn small() -> Config {
        Config {
            width: 8,
            height: 8,
            ..Config::default()
        }
    }

    #[test]
    fn deposit_then_read_own_and_foreign() {
        let mut p = Pheromones::new(&small());
        p.deposit_scent(10, 4.0, 3);
        assert_eq!(p.scent_for(10, 3), (4.0, 0.0));
        assert_eq!(p.scent_for(10, 5), (0.0, 4.0));
    }

    #[test]
    fn same_colony_scent_accumulates() {
        let mut p = Pheromones::new(&small());
        p.deposit_scent(10, 2.0, 1);
        p.deposit_scent(10, 3.0, 1);
        assert_eq!(p.scent.mag[10], 5.0);
        assert_eq!(p.scent.owner[10], 1);
    }

    #[test]
    fn foreign_scent_erodes_the_incumbent() {
        let mut p = Pheromones::new(&small());
        p.deposit_scent(10, 5.0, 1);
        p.deposit_scent(10, 2.0, 2);
        assert_eq!(p.scent.owner[10], 1, "incumbent holds while strength remains");
        assert_eq!(p.scent.mag[10], 3.0);
    }

    #[test]
    fn overwhelming_foreign_scent_flips_ownership() {
        let mut p = Pheromones::new(&small());
        p.deposit_scent(10, 2.0, 1);
        p.deposit_scent(10, 5.0, 2);
        assert_eq!(p.scent.owner[10], 2);
        assert_eq!(p.scent.mag[10], 3.0);
    }

    #[test]
    fn unowned_cell_takes_the_depositor_as_owner() {
        let mut p = Pheromones::new(&small());
        assert_eq!(p.scent.owner[10], NO_OWNER);
        p.deposit_scent(10, 1.0, 6);
        assert_eq!(p.scent.owner[10], 6);
    }

    #[test]
    fn evaporation_decays_an_isolated_deposit() {
        let cfg = small();
        let mut p = Pheromones::new(&cfg);
        p.deposit_food(0, 100.0);
        let before = p.food[0];
        p.step(&cfg);
        assert!(p.food[0] < before, "food should decay");
    }

    #[test]
    fn diffusion_spreads_to_neighbours() {
        let cfg = small();
        let mut p = Pheromones::new(&cfg);
        let center = 8 * 4 + 4;
        p.deposit_food(center, 100.0);
        p.step(&cfg);
        assert!(p.food[center - 1] > 0.0, "should spread left");
        assert!(p.food[center + 1] > 0.0, "should spread right");
        assert!(p.food[center - 8] > 0.0, "should spread up");
        assert!(p.food[center + 8] > 0.0, "should spread down");
    }

    #[test]
    fn diffusion_does_not_leak_off_the_border() {
        let cfg = small();
        let mut p = Pheromones::new(&cfg);
        // Fill uniformly, disable evaporation: total must be conserved.
        let cfg = Config {
            food_evaporation: 1.0,
            ..cfg
        };
        for i in 0..cfg.cell_count() {
            p.food[i] = 1.0;
        }
        let before: f32 = p.food.iter().sum();
        p.step(&cfg);
        let after: f32 = p.food.iter().sum();
        assert!((before - after).abs() < 1e-3, "{before} vs {after}");
    }

    #[test]
    fn diffused_scent_carries_its_ownership_with_it() {
        // Without this, a nest's beacon spreads as a magnitude that `scent_for`
        // reports as zero to everyone, and homing becomes impossible.
        let cfg = small();
        let mut p = Pheromones::new(&cfg);
        let center = 8 * 4 + 4;
        p.deposit_scent(center, 1000.0, 2);
        p.step(&cfg);
        let (own, foreign) = p.scent_for(center + 1, 2);
        assert!(own > 0.0, "the neighbour cell holds no readable own-scent");
        assert_eq!(foreign, 0.0);
        assert_eq!(p.scent.owner[center + 1], 2);
    }

    #[test]
    fn a_lone_colonys_scent_diffuses_exactly_like_a_plain_field() {
        // With one owner everywhere, contested diffusion must reduce to the
        // ordinary 4-point blend.
        let cfg = small();
        let mut p = Pheromones::new(&cfg);
        let mut plain = vec![0.0f32; cfg.cell_count()];
        let center = 8 * 4 + 4;
        p.deposit_scent(center, 1000.0, 1);
        plain[center] = 1000.0;

        p.step(&cfg);
        diffuse_decay(
            &mut plain,
            cfg.width,
            cfg.height,
            cfg.scent_diffusion,
            cfg.scent_evaporation,
        );
        for i in 0..plain.len() {
            if plain[i] >= 1e-6 {
                assert!((p.scent.mag[i] - plain[i]).abs() < 1e-3, "cell {i}");
            }
        }
    }

    #[test]
    fn two_colonies_diffusing_into_one_cell_leave_only_the_stronger() {
        let cfg = small();
        let mut p = Pheromones::new(&cfg);
        let mid = 8 * 4 + 4;
        p.deposit_scent(mid - 1, 1000.0, 1);
        p.deposit_scent(mid + 1, 10.0, 3);
        p.step(&cfg);
        assert_eq!(p.scent.owner[mid], 1, "the stronger colony should hold the ground");
    }

    #[test]
    fn evenly_contested_ground_belongs_to_nobody() {
        let cfg = small();
        let mut p = Pheromones::new(&cfg);
        let mid = 8 * 4 + 4;
        p.deposit_scent(mid - 1, 500.0, 1);
        p.deposit_scent(mid + 1, 500.0, 2);
        p.step(&cfg);
        assert_eq!(p.scent.mag[mid], 0.0);
        assert_eq!(p.scent.owner[mid], NO_OWNER);
    }

    #[test]
    fn a_trail_fades_to_nothing_eventually() {
        let cfg = small();
        let mut p = Pheromones::new(&cfg);
        p.deposit_food(30, 1000.0);
        for _ in 0..5000 {
            p.step(&cfg);
        }
        let total: f32 = p.food.iter().sum();
        assert!(total < 1.0, "stale trail should evaporate, total={total}");
    }

    #[test]
    fn trail_reads_own_and_foreign_like_scent() {
        let mut p = Pheromones::new(&small());
        p.deposit_trail(10, 4.0, 3);
        assert_eq!(p.trail_for(10, 3), (4.0, 0.0));
        assert_eq!(p.trail_for(10, 5), (0.0, 4.0));
    }

    #[test]
    fn trail_evaporates_faster_than_scent() {
        // Same deposit into both fields; after a handful of steps the trail
        // (fast evaporation) must have collapsed far more than the scent.
        let cfg = small();
        let mut p = Pheromones::new(&cfg);
        let cell = 8 * 4 + 4;
        p.deposit_scent(cell, 1000.0, 1);
        p.deposit_trail(cell, 1000.0, 1);
        for _ in 0..30 {
            p.step(&cfg);
        }
        let scent_total: f32 = p.scent.mag.iter().sum();
        let trail_total: f32 = p.trail.mag.iter().sum();
        assert!(
            trail_total < scent_total * 0.5,
            "trail should be much fainter: trail={trail_total} scent={scent_total}"
        );
    }
}
