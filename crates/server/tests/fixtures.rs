//! Emits the byte fixtures that `web/tests/protocol.test.ts` decodes.
//!
//! The wire format lives in two places — `protocol.rs` and `protocol.ts` — and
//! nothing in either language can see the other. A field reordered on one side
//! renders as ants in a diagonal line three weeks later, with no error anywhere.
//! These fixtures are the joint: Rust writes bytes and asserts what it wrote,
//! TypeScript reads the same bytes and asserts it agrees.
//!
//! Regenerate (after an intentional format change) with:
//!
//!     cargo test -p server --test fixtures
//!
//! then re-run `npm test` in `web/` and expect it to fail until `protocol.ts`
//! is updated to match. That failure is the point.

use server::protocol::*;
use sim::brain::Brain;
use sim::config::Config;
use sim::genome::Genome;
use sim::rng::Pcg32;
use sim::world::World;
use std::path::PathBuf;

fn dir() -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(name: &str, bytes: &[u8]) {
    std::fs::write(dir().join(name), bytes).unwrap();
}

fn fixture_world() -> World {
    let cfg = Config {
        width: 32,
        height: 32,
        num_colonies: 2,
        initial_ants_per_colony: 4,
        food_patch_count: 2,
        ..Config::default()
    };
    let mut w = World::new(&cfg, 2024);
    for _ in 0..50 {
        w.tick();
    }
    // Pin the flag bits and a known position rather than hoping the simulation
    // happens to produce them.
    w.ants.clear_attacking();
    w.ants.x[0] = 17.5;
    w.ants.y[0] = 4.25;
    w.ants.carrying[0] = 2.0;
    w.ants.attacking[0] = true;
    // The ant moved, so its sensed neighbour counts must be recomputed against
    // where it now is, not where it was.
    w.rebuild_index();
    w
}

/// Index of the brightest scent texel, and its owner. `expected.json` pins a
/// texel that is actually non-zero: a spot check on an empty corner would pass
/// against a completely broken encoder.
fn brightest_scent_texel(w: &World) -> (usize, u8, u8) {
    let mut b = Vec::new();
    encode_phero(&mut b, w, 2);
    let texels = &b[14..];
    let n = texels.len() / 4;
    let best = (0..n).max_by_key(|i| texels[4 * i + 2]).unwrap();
    (best, texels[4 * best + 2], texels[4 * best + 3])
}

#[test]
fn emit_protocol_fixtures() {
    let w = fixture_world();
    let mut b = Vec::new();

    encode_hello(&mut b, &w, 8);
    write("hello.bin", &b);

    encode_ants(&mut b, &w);
    write("ants.bin", &b);
    let live = w.ants.alive.iter().filter(|a| **a).count();
    assert_eq!(b.len(), 13 + BYTES_PER_ANT * live);

    encode_phero(&mut b, &w, 2);
    write("phero.bin", &b);
    assert_eq!(b.len(), 14 + 16 * 16 * 4);

    let stats = w.stats();
    encode_stats(&mut b, w.tick_count, &stats);
    write("stats.bin", &b);

    let act = w.activations(0);
    let traits = w.ants.genome[0].traits.as_array();
    encode_ant_detail(
        &mut b,
        &AntDetail {
            id: w.ants.id[0],
            colony: w.ants.colony[0],
            alive: true,
            x: w.ants.x[0],
            y: w.ants.y[0],
            heading: w.ants.heading[0],
            energy: w.ants.energy[0],
            max_energy: w.ants.genome[0].max_energy(&w.cfg, w.ants.size[0]),
            size: w.ants.size[0],
            carrying: w.ants.carrying[0],
            food_delivered: w.ants.food_delivered[0],
            age: w.ants.age[0],
            lineage: w.ants.lineage[0],
            traits,
            act: &act,
        },
    );
    write("detail.bin", &b);
    assert_eq!(b.len(), ANT_DETAIL_LEN);

    let g = Genome::random(&mut Pcg32::new(7, 7));
    encode_ant_genome(&mut b, 42, &g);
    write("genome.bin", &b);

    encode_config(&mut b, &w.cfg);
    write("config.bin", &b);

    // The values TypeScript must agree on. Hand-rolled so the server crate does
    // not take a serde_json dependency for one test.
    let a0 = 13;
    let expected = format!(
        concat!(
            "{{\n",
            "  \"hello\": {{ \"width\": {}, \"height\": {}, \"numColonies\": {}, \"pheroResLog2\": 8, \"tick\": {} }},\n",
            "  \"ants\": {{ \"tick\": {}, \"count\": {}, \"first\": {{ \"x\": {}, \"y\": {}, \"colony\": {}, \"size\": {}, \"flags\": {} }} }},\n",
            "  \"phero\": {{ \"w\": 16, \"h\": 16, \"factor\": 2, \"firstTexel\": [{}, {}, {}, {}], \"brightestScent\": {{ \"texel\": {}, \"value\": {}, \"owner\": {} }} }},\n",
            "  \"stats\": {{ \"count\": {}, \"first\": {{ \"id\": {}, \"population\": {}, \"store\": {}, \"births\": {}, \"deaths\": {}, \"floorSpawns\": {}, \"meanSize\": {}, \"meanLineage\": {}, \"deliveredTotal\": {} }} }},\n",
            "  \"detail\": {{ \"id\": {}, \"colony\": {}, \"alive\": true, \"x\": {}, \"y\": {}, \"age\": {}, \"lineage\": {}, \"trait0\": {}, \"input0\": {}, \"output0\": {} }},\n",
            "  \"genome\": {{ \"id\": 42, \"nParams\": {}, \"param0\": {} }},\n",
            "  \"config\": {{ \"count\": {}, \"field0\": {} }}\n",
            "}}\n"
        ),
        w.cfg.width,
        w.cfg.height,
        w.cfg.num_colonies,
        w.tick_count,
        w.tick_count,
        live,
        (w.ants.x[0] * 128.0) as u16,
        (w.ants.y[0] * 128.0) as u16,
        w.ants.colony[0],
        (w.ants.size[0] / 3.0 * 255.0) as u8,
        FLAG_CARRYING | FLAG_ATTACKING,
        phero_texel(&w, 0),
        phero_texel(&w, 1),
        phero_texel(&w, 2),
        phero_texel(&w, 3),
        brightest_scent_texel(&w).0,
        brightest_scent_texel(&w).1,
        brightest_scent_texel(&w).2,
        stats.len(),
        stats[0].id,
        stats[0].population,
        stats[0].store,
        stats[0].births,
        stats[0].deaths,
        stats[0].floor_spawns,
        stats[0].mean_size,
        stats[0].mean_lineage,
        stats[0].delivered_total,
        w.ants.id[0],
        w.ants.colony[0],
        w.ants.x[0],
        w.ants.y[0],
        w.ants.age[0],
        w.ants.lineage[0],
        traits[0],
        act.inputs[0],
        act.outputs[0],
        g.params.len(),
        g.params[0],
        CONFIG_FIELDS.len(),
        w.cfg.food_evaporation,
    );
    std::fs::write(dir().join("expected.json"), expected).unwrap();

    // Sanity: the ant we pinned really did land in the frame where we say.
    encode_ants(&mut b, &w);
    assert_eq!(
        u16::from_le_bytes([b[a0], b[a0 + 1]]),
        (17.5 * 128.0) as u16
    );
    assert_eq!(b[a0 + 6], FLAG_CARRYING | FLAG_ATTACKING);

    // And the detail frame's outputs really are a forward pass, so a TS decoder
    // that agrees with these bytes agrees with the network.
    let fwd = w.ants.genome[0].forward(&act.inputs);
    assert_eq!(fwd.outputs[0], act.outputs[0]);

    // The pheromone fixture must contain signal. An all-zero texture would let
    // a broken encoder pass every downstream assertion.
    let (_, value, owner) = brightest_scent_texel(&w);
    assert!(value > 32, "phero fixture is nearly black (peak {value})");
    assert!(owner < w.cfg.num_colonies, "peak scent has no owner");
}

fn phero_texel(w: &World, k: usize) -> u8 {
    let mut b = Vec::new();
    encode_phero(&mut b, w, 2);
    b[14 + k]
}
