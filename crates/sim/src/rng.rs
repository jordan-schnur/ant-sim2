use serde::{Deserialize, Serialize};

/// PCG-XSH-RR 64/32. Hand-rolled so that determinism cannot be broken by a
/// dependency bump: the golden-master fixture depends on this exact stream.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pcg32 {
    state: u64,
    inc: u64,
}

const MULT: u64 = 6_364_136_223_846_793_005;

impl Pcg32 {
    /// `seq` selects one of 2^63 distinct streams for the same `seed`.
    pub fn new(seed: u64, seq: u64) -> Self {
        let mut r = Pcg32 {
            state: 0,
            inc: (seq << 1) | 1,
        };
        r.next_u32();
        r.state = r.state.wrapping_add(seed);
        r.next_u32();
        r
    }

    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(MULT).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Uniform in `[0.0, 1.0)`. Uses the top 24 bits, which is exactly the
    /// f32 mantissa width, so every representable value is equally likely.
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Uniform in `[0, n)`, rejection-sampled so it is unbiased.
    /// Panics if `n == 0`.
    pub fn next_below(&mut self, n: u32) -> u32 {
        assert!(n > 0, "next_below requires n > 0");
        let threshold = n.wrapping_neg() % n;
        loop {
            let v = self.next_u32();
            if v >= threshold {
                return v % n;
            }
        }
    }

    /// Box-Muller. Discards the second variate rather than caching it, which
    /// keeps the struct's serialized state trivially reproducible.
    pub fn next_gaussian(&mut self) -> f32 {
        let mut u1 = self.next_f32();
        if u1 <= f32::EPSILON {
            u1 = f32::EPSILON;
        }
        let u2 = self.next_f32();
        (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream() {
        let mut a = Pcg32::new(42, 1);
        let mut b = Pcg32::new(42, 1);
        for _ in 0..1000 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn different_streams_diverge() {
        let mut a = Pcg32::new(42, 1);
        let mut b = Pcg32::new(42, 2);
        let diff = (0..100).filter(|_| a.next_u32() != b.next_u32()).count();
        assert!(diff > 90, "streams should differ, only {diff}/100 differed");
    }

    #[test]
    fn f32_is_in_unit_interval() {
        let mut r = Pcg32::new(7, 7);
        for _ in 0..10_000 {
            let v = r.next_f32();
            assert!((0.0..1.0).contains(&v), "out of range: {v}");
        }
    }

    #[test]
    fn next_below_respects_bound() {
        let mut r = Pcg32::new(9, 9);
        for _ in 0..10_000 {
            assert!(r.next_below(7) < 7);
        }
    }

    #[test]
    fn gaussian_has_roughly_unit_variance() {
        let mut r = Pcg32::new(3, 3);
        let n = 100_000;
        let xs: Vec<f32> = (0..n).map(|_| r.next_gaussian()).collect();
        let mean = xs.iter().sum::<f32>() / n as f32;
        let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / n as f32;
        assert!(mean.abs() < 0.02, "mean {mean}");
        assert!((var - 1.0).abs() < 0.05, "var {var}");
    }

    #[test]
    fn roundtrips_through_serde() {
        let mut a = Pcg32::new(11, 13);
        a.next_u32();
        let bytes = bincode::serialize(&a).unwrap();
        let mut b: Pcg32 = bincode::deserialize(&bytes).unwrap();
        assert_eq!(a.next_u32(), b.next_u32());
    }
}
