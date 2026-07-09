use sim::config::Config;
use sim::snapshot::{load, save};
use sim::world::World;

fn small() -> Config {
    Config {
        width: 48,
        height: 48,
        num_colonies: 2,
        initial_ants_per_colony: 15,
        ..Config::default()
    }
}

#[test]
fn a_snapshot_round_trips_to_an_identical_world() {
    let mut w = World::new(&small(), 5);
    for _ in 0..200 {
        w.tick();
    }
    let bytes = save(&w).unwrap();
    let w2 = load(&bytes).unwrap();
    assert_eq!(w.state_hash(), w2.state_hash());
}

#[test]
fn a_loaded_world_ticks_identically_to_the_original() {
    let mut a = World::new(&small(), 6);
    for _ in 0..100 {
        a.tick();
    }
    let mut b = load(&save(&a).unwrap()).unwrap();
    for _ in 0..100 {
        a.tick();
        b.tick();
    }
    assert_eq!(
        a.state_hash(),
        b.state_hash(),
        "the rng or the spatial index did not survive"
    );
}

#[test]
fn garbage_bytes_are_an_error_not_a_panic() {
    assert!(load(&[0xde, 0xad, 0xbe, 0xef]).is_err());
}

#[test]
fn an_empty_buffer_is_an_error() {
    assert!(load(&[]).is_err());
}
