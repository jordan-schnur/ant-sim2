//! Searches for a genome that can forage, and checks it in as a fixture.
//!
//!     cargo test -p sim --release --test known_good -- --ignored --nocapture
//!
//! This is a tool, not a guard: the search is `#[ignore]`d so CI never runs it.
//! Its output, `known_good_forager.bin`, separates "the world is broken" from
//! "evolution has not found it yet".
//!
//! # Read the fixture honestly
//!
//! This genome is *a* forager, not a general one. Foraging skill varies a lot by
//! map: on some seeds an evolved colony carries thousands of food home, and on
//! others the same genome carries food to within a few cells of its nest and
//! never quite lands on it. Scoring on a single seed therefore measures the map
//! as much as the genome, which is why `mean_score` averages several. An earlier
//! two-seed search overfit badly for exactly this reason: one of its two maps
//! scored zero for every genome tried, so all selection pressure came from the
//! other one.

use sim::config::Config;
use sim::genome::Genome;
use sim::rng::Pcg32;
use sim::world::World;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/known_good_forager.bin");

/// Maps the search and the guard both evaluate on. More than one, because a
/// single map rewards memorising a direction rather than following a gradient.
const EVAL_SEEDS: [u64; 3] = [1, 2, 3];

fn cfg() -> Config {
    Config {
        width: 48,
        height: 48,
        num_colonies: 1,
        initial_ants_per_colony: 30,
        food_patch_count: 4,
        ..Config::default()
    }
}

/// Food actually carried to the nest by a colony seeded entirely with `g`.
///
/// Deliberately excludes `colonies[0].store`, which starts at
/// `initial_food_store`: including it would score a genome that never forages at
/// 600 just for existing.
fn score(g: &Genome, seed: u64, ticks: u32) -> f32 {
    let c = cfg();
    let mut w = World::new(&c, seed);
    for i in 0..w.ants.len() {
        w.ants.genome[i] = g.clone();
    }

    // Ants die and are compacted out of the arrays, taking their lifetime
    // `food_delivered` with them, so bank a dead ant's total before it is lost.
    // Match on id, not index: `retain_alive` shifts indices.
    let snapshot = |w: &World| -> Vec<(u64, f32)> {
        w.ants
            .id
            .iter()
            .copied()
            .zip(w.ants.food_delivered.iter().copied())
            .collect()
    };

    let mut banked = 0.0f32;
    let mut prev = snapshot(&w);
    for _ in 0..ticks {
        w.tick();
        let cur = snapshot(&w);
        // Both are sorted ascending by id: ids in `prev` missing from `cur` died.
        let (mut i, mut j) = (0usize, 0usize);
        while i < prev.len() {
            if j < cur.len() && cur[j].0 == prev[i].0 {
                i += 1;
                j += 1;
            } else if j < cur.len() && cur[j].0 < prev[i].0 {
                j += 1; // a newborn
            } else {
                banked += prev[i].1;
                i += 1;
            }
        }
        prev = cur;
    }
    banked + prev.iter().map(|p| p.1).sum::<f32>()
}

fn mean_score(g: &Genome, ticks: u32) -> f32 {
    EVAL_SEEDS.iter().map(|&s| score(g, s, ticks)).sum::<f32>() / EVAL_SEEDS.len() as f32
}

/// The genome the search starts from. The guard measures against it, so the
/// fixture has to beat where it began rather than merely be nonzero.
fn search_start() -> Genome {
    Genome::random(&mut Pcg32::new(0xF00D, 1))
}

#[test]
#[ignore = "offline tool; regenerates the known-good forager fixture"]
fn search_for_a_forager() {
    let c = cfg();
    let mut rng = Pcg32::new(0xF00D, 1);
    let mut best = Genome::random(&mut rng);
    let mut best_score = mean_score(&best, 3_000);
    println!("baseline {best_score:.1}");

    for gen in 0..300 {
        let candidate = best.mutated(&c, &mut rng);
        let s = mean_score(&candidate, 3_000);
        if s > best_score {
            best = candidate;
            best_score = s;
            println!("gen {gen}: new best {best_score:.1}");
        }
    }

    println!("final mean score {best_score:.1}");
    for &seed in &EVAL_SEEDS {
        println!("  seed {seed}: {:.1}", score(&best, seed, 3_000));
    }
    std::fs::write(FIXTURE, bincode::serialize(&best).unwrap()).unwrap();
}

#[test]
fn the_known_good_forager_beats_the_genome_it_evolved_from() {
    let Ok(bytes) = std::fs::read(FIXTURE) else {
        eprintln!("no fixture yet; run the ignored `search_for_a_forager` first");
        return;
    };
    let evolved: Genome = bincode::deserialize(&bytes).unwrap();

    let baseline = mean_score(&search_start(), 3_000);
    let evolved_score = mean_score(&evolved, 3_000);

    assert!(
        evolved_score > baseline.max(1.0) * 1.5,
        "the checked-in forager ({evolved_score:.1}) no longer clearly beats the random \
         genome it evolved from ({baseline:.1}). Either a simulation rule changed under \
         it, or the fixture was generated from a failed search."
    );
}
