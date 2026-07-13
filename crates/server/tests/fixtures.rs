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

    // Every field gets a DISTINCT, NON-ZERO value.
    //
    // A 50-tick world leaves `store`, `deaths`, `delivered_total`, and `lineage`
    // at exactly 0.0. A decoder reading any of them at the wrong offset would
    // still find a zero there and every `toBeCloseTo(0)` would pass — the test
    // would be blind to precisely the offset bugs it exists to catch. Distinct
    // values also mean a decoder that swaps two fields cannot accidentally agree.
    w.ants.heading[0] = 0.75;
    w.ants.energy[0] = 13.5;
    w.ants.size[0] = 1.25;
    w.ants.lineage[0] = 5;
    w.ants.food_delivered[0] = 9.75;
    w.ants.age[0] = 37;

    w.colonies[0].store = 123.5;
    w.colonies[0].births = 11;
    w.colonies[0].deaths = 7;
    w.colonies[0].floor_spawns = 3;
    w.colonies[0].delivered_total = 45.25;

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
    let mut w = fixture_world();
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

    encode_terrain(&mut b, &w, 2);
    write("terrain.bin", &b);
    assert_eq!(b.len(), 14 + 16 * 16 * 4);

    let stats = w.stats();
    encode_stats(&mut b, w.tick_count, &stats);
    write("stats.bin", &b);

    let act = w.activations(0);
    let traits = w.ants.genome[0].traits.as_array();
    let detail_name = sim::names::ant_name(w.ants.id[0]);
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
            name: &detail_name,
        },
    );
    write("detail.bin", &b);
    assert_eq!(b.len(), ANT_DETAIL_LEN + 1 + detail_name.len());

    let g = Genome::random(&mut Pcg32::new(7, 7));
    encode_ant_genome(&mut b, 42, &g);
    write("genome.bin", &b);

    encode_config(&mut b, &w.cfg);
    write("config.bin", &b);

    encode_colony_meta(&mut b, &w);
    write("colony_meta.bin", &b);
    let colony_count = w.colonies.len();
    let colony0_name = w.colonies[0].name.clone();

    // Exactly one hand-inserted event so the fixture exercises a populated
    // chronicle deterministically. The 50-tick world's detectors may have
    // already logged real events; clear them so the wire test pins a known one.
    let chron_ant = w.ants.id[0];
    w.chronicle.events.clear();
    w.chronicle.record(&mut false, sim::chronicle::ChronicleEvent {
        tick: 5,
        colony: 1,
        kind: sim::chronicle::EventKind::FirstDelivery,
        ant_id: Some(chron_ant),
        ant_name: Some(sim::names::ant_name(chron_ant)),
        text: "the first crumb".into(),
    });
    encode_chronicle(&mut b, &w);
    write("chronicle.bin", &b);

    // The values TypeScript must agree on. Hand-rolled so the server crate does
    // not take a serde_json dependency for one test.
    let a0 = 13;
    let expected = format!(
        concat!(
            "{{\n",
            "  \"hello\": {{ \"width\": {}, \"height\": {}, \"numColonies\": {}, \"pheroResLog2\": 8, \"tick\": {} }},\n",
            "  \"ants\": {{ \"tick\": {}, \"count\": {}, \"first\": {{ \"x\": {}, \"y\": {}, \"colony\": {}, \"size\": {}, \"flags\": {} }} }},\n",
            "  \"phero\": {{ \"w\": 16, \"h\": 16, \"factor\": 2, \"firstTexel\": [{}, {}, {}, {}], \"brightestScent\": {{ \"texel\": {}, \"value\": {}, \"owner\": {} }} }},\n",
            "  \"terrain\": {{ \"w\": 16, \"h\": 16, \"factor\": 2, \"stoneTexels\": {}, \"foodTexels\": {}, \"nestTexels\": {}, \"maxFood\": {}, \"maxStone\": {} }},\n",
            "  \"stats\": {{ \"count\": {}, \"first\": {{ \"id\": {}, \"population\": {}, \"store\": {}, \"births\": {}, \"deaths\": {}, \"floorSpawns\": {}, \"meanSize\": {}, \"meanLineage\": {}, \"deliveredTotal\": {} }} }},\n",
            "  \"detail\": {{ \"id\": {}, \"colony\": {}, \"alive\": true, \"x\": {}, \"y\": {}, \"age\": {}, \"lineage\": {}, \"trait0\": {}, \"trait7\": {}, \"input0\": {}, \"input43\": {}, \"h1_0\": {}, \"h1_15\": {}, \"h2_0\": {}, \"h2_15\": {}, \"output0\": {}, \"output7\": {} }},\n",
            "  \"genome\": {{ \"id\": 42, \"nParams\": {}, \"param0\": {} }},\n",
            "  \"config\": {{ \"count\": {}, \"field0\": {} }},\n",
            "  \"colonyMeta\": {{ \"count\": {}, \"name0\": \"{}\" }},\n",
            "  \"chronicle\": {{ \"count\": 1, \"tick0\": 5, \"colony0\": 1, \"kind0\": 0, \"text0\": \"the first crumb\" }}\n",
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
        terrain_summary(&w).0,
        terrain_summary(&w).1,
        terrain_summary(&w).2,
        terrain_summary(&w).3,
        terrain_summary(&w).4,
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
        traits[7],
        act.inputs[0],
        act.inputs[43],
        // Every layer's first AND last element. Pinning only inputs[0] and
        // outputs[0] let a shifted `h2` read into `h1` undetected: both are
        // tanh outputs, so the wrong values still looked entirely plausible.
        act.h1[0],
        act.h1[15],
        act.h2[0],
        act.h2[15],
        act.outputs[0],
        act.outputs[7],
        g.params.len(),
        g.params[0],
        CONFIG_FIELDS.len(),
        w.cfg.food_evaporation,
        colony_count,
        colony0_name,
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

    // A zero-valued field makes its cross-language assertion vacuous: a decoder
    // reading the wrong offset finds zero there too. Guard the guard.
    let s = &stats[0];
    for (name, v) in [
        ("store", s.store),
        ("mean_size", s.mean_size),
        ("mean_lineage", s.mean_lineage),
        ("delivered_total", s.delivered_total),
    ] {
        assert!(v != 0.0, "stats fixture field `{name}` is zero");
    }
    for (name, v) in [
        ("births", s.births),
        ("deaths", s.deaths),
        ("floor_spawns", s.floor_spawns),
    ] {
        assert!(v != 0, "stats fixture field `{name}` is zero");
    }
    // The terrain fixture must show a map, not a void.
    let (stone, food, nest, max_food, max_stone) = terrain_summary(&w);
    assert!(stone > 0, "terrain fixture has no stone");
    assert!(food > 0, "terrain fixture has no food");
    assert!(nest > 0, "terrain fixture has no nest tiles");
    assert!(max_food > 0 && max_stone > 0);

    assert_ne!(w.ants.lineage[0], 0, "detail fixture lineage is zero");
    assert_ne!(w.ants.age[0], 0, "detail fixture age is zero");
    assert_ne!(traits[0], 0.0, "detail fixture trait0 is zero");

    // The activation layers must be mutually distinguishable, or a decoder that
    // reads `h2` at `h1`'s offset finds equally plausible tanh values and the
    // cross-language test never notices.
    assert_ne!(act.h1[0], act.h2[0], "h1 and h2 are indistinguishable");
    assert_ne!(
        act.h1[15], act.h2[15],
        "h1 and h2 tails are indistinguishable"
    );
    assert_ne!(
        act.outputs[0], act.outputs[7],
        "output head equals its tail"
    );

    // Distinct, so a decoder that swaps two fields cannot coincidentally agree.
    let scalars = [
        s.store,
        s.mean_size,
        s.mean_lineage,
        s.delivered_total,
        w.ants.energy[0],
        w.ants.heading[0],
        w.ants.food_delivered[0],
    ];
    for i in 0..scalars.len() {
        for j in (i + 1)..scalars.len() {
            assert_ne!(
                scalars[i], scalars[j],
                "fixture scalars {i} and {j} collide"
            );
        }
    }
}

/// (stone texels, food texels, nest texels, max food byte, max stone byte).
/// The TypeScript side recomputes these from the same bytes; if either half
/// reads the channels in a different order, the counts disagree.
fn terrain_summary(w: &World) -> (usize, usize, usize, u8, u8) {
    let mut b = Vec::new();
    encode_terrain(&mut b, w, 2);
    let t = &b[14..];
    let n = t.len() / 4;
    let stone = (0..n).filter(|i| t[4 * i + 1] > 0).count();
    let food = (0..n).filter(|i| t[4 * i] > 0).count();
    let nest = (0..n).filter(|i| t[4 * i + 2] != 255).count();
    let max_food = (0..n).map(|i| t[4 * i]).max().unwrap();
    let max_stone = (0..n).map(|i| t[4 * i + 1]).max().unwrap();
    (stone, food, nest, max_food, max_stone)
}

fn phero_texel(w: &World, k: usize) -> u8 {
    let mut b = Vec::new();
    encode_phero(&mut b, w, 2);
    b[14 + k]
}
