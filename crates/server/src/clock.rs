//! Tick pacing. Pure: `ticks_due` is handed the elapsed time and reads no wall
//! clock, so the whole thing is unit-testable without sleeping.
//!
//! Tick rate and frame rate are independent. Paused is zero ticks. 100x ticks
//! as fast as the CPU allows. Nothing here can change *what* a tick computes.

use std::time::Duration;

/// Bounds one call so the sim loop always returns to drain commands. Without
/// it, "100x" makes the pause button unresponsive: the loop would sit inside a
/// single `ticks_due` batch for seconds at a time.
pub const MAX_TICKS_PER_ITER: u32 = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speed {
    X1,
    X10,
    X100,
}

impl Speed {
    pub fn from_wire(v: u8) -> Speed {
        match v {
            1 => Speed::X10,
            2 => Speed::X100,
            _ => Speed::X1,
        }
    }

    /// `None` means unbounded — tick as fast as the CPU allows.
    pub fn target_tps(self) -> Option<f64> {
        match self {
            Speed::X1 => Some(60.0),
            Speed::X10 => Some(600.0),
            Speed::X100 => None,
        }
    }
}

pub struct Clock {
    pub paused: bool,
    pub speed: Speed,
    /// Fractional ticks carried between calls, so 60 tps does not lose the
    /// remainder every iteration and quietly run slow.
    accumulator: f64,
    pending_steps: u32,
}

impl Default for Clock {
    fn default() -> Self {
        Clock {
            paused: true,
            speed: Speed::X1,
            accumulator: 0.0,
            pending_steps: 0,
        }
    }
}

impl Clock {
    /// A single step implies pause: the operator wants to look at the result.
    pub fn step(&mut self) {
        self.paused = true;
        self.pending_steps += 1;
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        // Drop carried fractional time; resuming should not fire a burst of
        // ticks accumulated while the operator was reading the screen.
        self.accumulator = 0.0;
    }

    pub fn set_speed(&mut self, speed: Speed) {
        self.speed = speed;
        self.accumulator = 0.0;
    }

    pub fn ticks_due(&mut self, elapsed: Duration) -> u32 {
        if self.pending_steps > 0 {
            let n = self.pending_steps.min(MAX_TICKS_PER_ITER);
            self.pending_steps -= n;
            return n;
        }
        if self.paused {
            return 0;
        }
        match self.speed.target_tps() {
            None => MAX_TICKS_PER_ITER,
            Some(tps) => {
                self.accumulator += elapsed.as_secs_f64() * tps;
                let n = self.accumulator.floor();
                // Clamp before the cast: a huge elapsed (a suspended laptop)
                // would otherwise saturate to u32::MAX and stall the loop.
                let n = n.min(MAX_TICKS_PER_ITER as f64);
                self.accumulator -= n;
                n as u32
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn a_paused_clock_yields_nothing_however_long_it_waits() {
        let mut c = Clock::default();
        c.set_paused(true);
        assert_eq!(c.ticks_due(Duration::from_secs(10)), 0);
    }

    #[test]
    fn one_x_runs_at_about_sixty_ticks_per_second() {
        let mut c = Clock::default();
        c.set_paused(false);
        c.set_speed(Speed::X1);
        let total: u32 = (0..100).map(|_| c.ticks_due(ms(10))).sum();
        assert!((59..=61).contains(&total), "got {total} ticks in 1s");
    }

    #[test]
    fn ten_x_runs_at_about_six_hundred_ticks_per_second() {
        let mut c = Clock::default();
        c.set_paused(false);
        c.set_speed(Speed::X10);
        let total: u32 = (0..100).map(|_| c.ticks_due(ms(10))).sum();
        assert!((595..=605).contains(&total), "got {total}");
    }

    #[test]
    fn fractional_ticks_accumulate_rather_than_being_lost() {
        // At 60 tps a 1 ms slice is 0.06 ticks. Truncating each call would
        // yield zero forever and the sim would never advance at 1x.
        let mut c = Clock::default();
        c.set_paused(false);
        c.set_speed(Speed::X1);
        let total: u32 = (0..1000).map(|_| c.ticks_due(ms(1))).sum();
        assert!((59..=61).contains(&total), "got {total}");
    }

    #[test]
    fn a_hundred_x_is_unbounded_but_still_capped_per_call() {
        let mut c = Clock::default();
        c.set_paused(false);
        c.set_speed(Speed::X100);
        assert_eq!(c.ticks_due(Duration::ZERO), MAX_TICKS_PER_ITER);
    }

    #[test]
    fn an_enormous_elapsed_cannot_produce_more_than_the_cap() {
        // A suspended laptop hands us hours of elapsed time. Without the clamp
        // the f64 -> u32 cast saturates and the loop stops answering commands.
        let mut c = Clock::default();
        c.set_paused(false);
        c.set_speed(Speed::X10);
        assert_eq!(c.ticks_due(Duration::from_secs(86_400)), MAX_TICKS_PER_ITER);
    }

    #[test]
    fn a_step_yields_exactly_one_tick_and_then_stops() {
        let mut c = Clock::default();
        c.step();
        assert_eq!(c.ticks_due(Duration::ZERO), 1);
        assert_eq!(c.ticks_due(Duration::ZERO), 0);
    }

    #[test]
    fn a_step_pauses_a_running_clock() {
        let mut c = Clock::default();
        c.set_paused(false);
        c.step();
        assert!(c.paused);
        assert_eq!(c.ticks_due(Duration::ZERO), 1);
        assert_eq!(c.ticks_due(ms(100)), 0);
    }

    #[test]
    fn resuming_does_not_fire_a_burst_of_time_spent_paused() {
        let mut c = Clock::default();
        c.set_paused(false);
        c.set_speed(Speed::X1);
        let _ = c.ticks_due(ms(8)); // leaves ~0.48 in the accumulator
        c.set_paused(true);
        c.set_paused(false);
        assert_eq!(c.ticks_due(Duration::ZERO), 0);
    }

    #[test]
    fn speed_decodes_from_the_wire_and_defaults_to_one_x() {
        assert_eq!(Speed::from_wire(0), Speed::X1);
        assert_eq!(Speed::from_wire(1), Speed::X10);
        assert_eq!(Speed::from_wire(2), Speed::X100);
        assert_eq!(Speed::from_wire(200), Speed::X1);
    }
}
