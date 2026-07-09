use sim::config::Config;
use sim::world::World;

fn small() -> Config {
    Config {
        width: 64,
        height: 64,
        num_colonies: 3,
        initial_ants_per_colony: 20,
        ..Config::default()
    }
}

fn run(seed: u64, ticks: u32) -> u64 {
    let mut w = World::new(&small(), seed);
    for _ in 0..ticks {
        w.tick();
    }
    w.state_hash()
}

fn run_on(threads: usize, seed: u64, ticks: u32) -> u64 {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap()
        .install(|| run(seed, ticks))
}

#[test]
fn same_seed_same_state() {
    assert_eq!(run(42, 500), run(42, 500));
}

#[test]
fn different_seeds_diverge() {
    assert_ne!(run(42, 500), run(43, 500));
}

#[test]
fn thread_count_does_not_change_the_outcome() {
    let one = run_on(1, 7, 1000);
    let many = run_on(16, 7, 1000);
    assert_eq!(
        one, many,
        "the parallel think phase is writing somewhere it must not"
    );
}

#[test]
fn a_long_run_stays_deterministic() {
    assert_eq!(run_on(2, 99, 10_000), run_on(13, 99, 10_000));
}
