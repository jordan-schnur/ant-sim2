//! Golden master: pins the exact physics of the tick.
//!
//! This test WILL fail whenever you intentionally change a simulation rule or a
//! default in `Config`. That is the point. To accept the new behaviour:
//!
//!     REGENERATE_GOLDEN=1 cargo test -p sim --test golden
//!
//! Then review the diff on `golden_master.bin` in your commit — a changed
//! fixture is a claim that you meant to change the simulation.
//!
//! # This fixture pins the platform, not just the code
//!
//! `tanh`, `ln`, `sin`, and `cos` are libm calls, and their final-ULP results
//! differ across operating systems and architectures. The determinism tests
//! guarantee that a given *machine* reproduces itself across thread counts;
//! nothing guarantees an aarch64 Mac and an x86_64 Linux box agree bit for bit.
//!
//! So: **regenerate this fixture when you move to a new platform**, and do not
//! read a failure on a fresh CI runner as a physics regression until you have
//! confirmed the same binary passes on the machine that generated it.

use sim::config::Config;
use sim::snapshot::{load, save};
use sim::world::World;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden_master.bin");

fn cfg() -> Config {
    Config {
        width: 32,
        height: 32,
        num_colonies: 2,
        initial_ants_per_colony: 8,
        food_patch_count: 3,
        ..Config::default()
    }
}

fn advanced_world() -> World {
    let mut w = World::new(&cfg(), 2024);
    for _ in 0..1_000 {
        w.tick();
    }
    w
}

#[test]
fn the_tick_still_produces_the_recorded_world() {
    let w = advanced_world();

    if std::env::var("REGENERATE_GOLDEN").is_ok() {
        std::fs::write(FIXTURE, save(&w).unwrap()).unwrap();
        eprintln!("regenerated {FIXTURE}");
        return;
    }

    let bytes = std::fs::read(FIXTURE)
        .unwrap_or_else(|e| panic!("missing fixture {FIXTURE}: {e}. Run with REGENERATE_GOLDEN=1"));
    let expected = load(&bytes).unwrap();

    assert_eq!(
        w.state_hash(),
        expected.state_hash(),
        "the simulation's physics changed. If intentional, regenerate with REGENERATE_GOLDEN=1"
    );
}
