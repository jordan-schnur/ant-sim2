//! Owns the `World` on a dedicated OS thread and publishes frames.
//!
//! Not a tokio task: `World::tick` is CPU-bound and drives its own rayon pool,
//! so parking it on an async executor would starve the runtime that has to keep
//! answering the WebSocket.
//!
//! Frames go out on `watch` channels, which are latest-value-wins. A slow or
//! backgrounded browser therefore *drops* frames rather than queueing them; an
//! unbounded queue would accumulate a gigabyte of pheromone textures behind a
//! tab nobody is looking at. Nobody wants to watch a 30-second-old ant.
//!
//! Cadences are wall-clock (20/10/4 fps), independent of tick rate. That is
//! what lets 100x fast-forward run without drowning the client.

use crate::clock::{Clock, Speed};
use crate::protocol::{self, AntDetail, Command};
use sim::config::Config;
use sim::genome::Traits;
use sim::snapshot;
use sim::world::World;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::watch;

const ANTS_PERIOD: Duration = Duration::from_millis(50); // 20 fps
const PHERO_PERIOD: Duration = Duration::from_millis(100); // 10 fps
const STATS_PERIOD: Duration = Duration::from_millis(250); // 4 fps
/// Terrain changes slowly — food regrows at 0.002/tick — and a harvested cell
/// is still visibly gone within a quarter second. No reason to pay a second
/// full-resolution texture at the pheromone cadence.
const TERRAIN_PERIOD: Duration = Duration::from_millis(250); // 4 fps

/// When there is nothing to tick and nothing to publish, yield rather than
/// spinning a core at 100% while paused.
const IDLE_SLEEP: Duration = Duration::from_millis(1);

/// Wall-clock ceiling on one batch of ticks.
///
/// `Clock::MAX_TICKS_PER_ITER` bounds the batch by *count*, which is not enough:
/// at 100x on a 512x512 world with 10k ants, 4096 ticks is many seconds during
/// which nothing publishes and no command is read. The operator sees a frozen
/// screen and an unresponsive pause button. Bounding by time as well means
/// "100x" honestly reads as "as fast as the CPU allows, still drawing at 20 fps".
///
/// Half the ant-frame period, so a batch cannot make us miss two frames running.
const TICK_BUDGET: Duration = Duration::from_millis(25);

pub type Frame = Arc<Vec<u8>>;

/// Unbounded on purpose. Commands are tiny and rare (a slider drag is ~20/sec
/// from a single local operator); a bounded channel would mean choosing between
/// blocking the async runtime and silently dropping the operator's input.
#[derive(Clone)]
pub struct Handles {
    pub commands: UnboundedSender<Command>,
    pub hello: watch::Receiver<Frame>,
    pub ants: watch::Receiver<Frame>,
    pub phero: watch::Receiver<Frame>,
    pub terrain: watch::Receiver<Frame>,
    pub stats: watch::Receiver<Frame>,
    pub detail: watch::Receiver<Frame>,
    pub genome: watch::Receiver<Frame>,
    pub config: watch::Receiver<Frame>,
}

struct Publishers {
    hello: watch::Sender<Frame>,
    ants: watch::Sender<Frame>,
    phero: watch::Sender<Frame>,
    terrain: watch::Sender<Frame>,
    stats: watch::Sender<Frame>,
    detail: watch::Sender<Frame>,
    genome: watch::Sender<Frame>,
    config: watch::Sender<Frame>,
}

fn empty() -> Frame {
    Arc::new(Vec::new())
}

fn frame(f: impl FnOnce(&mut Vec<u8>)) -> Frame {
    let mut b = Vec::new();
    f(&mut b);
    Arc::new(b)
}

/// The world is built here, on the caller's thread, so that every channel is
/// seeded with a real frame before `spawn` returns. A client that connects in
/// the first millisecond would otherwise be handed a zero-length `hello` and
/// have nothing to size its canvas from.
///
/// `detail` and `genome` are the exceptions: they are legitimately empty until
/// an ant is selected, and the client checks for that.
pub fn spawn(cfg: Config, seed: u64, save_path: PathBuf) -> Handles {
    let (ctx, crx) = tokio::sync::mpsc::unbounded_channel();

    let world = World::new(&cfg, seed);
    let phero_factor = 2u8;

    let (hello_tx, hello) = watch::channel(frame(|b| protocol::encode_hello(b, &world, 8)));
    let (config_tx, config) = watch::channel(frame(|b| protocol::encode_config(b, &world.cfg)));
    let (ants_tx, ants) = watch::channel(frame(|b| protocol::encode_ants(b, &world)));
    let (phero_tx, phero) =
        watch::channel(frame(|b| protocol::encode_phero(b, &world, phero_factor)));
    let (terrain_tx, terrain) =
        watch::channel(frame(|b| protocol::encode_terrain(b, &world, phero_factor)));
    let (stats_tx, stats) = watch::channel(frame(|b| {
        protocol::encode_stats(b, world.tick_count, &world.stats())
    }));
    let (detail_tx, detail) = watch::channel(empty());
    let (genome_tx, genome) = watch::channel(empty());

    let pubs = Publishers {
        hello: hello_tx,
        ants: ants_tx,
        phero: phero_tx,
        terrain: terrain_tx,
        stats: stats_tx,
        detail: detail_tx,
        genome: genome_tx,
        config: config_tx,
    };

    let st = State {
        world,
        clock: Clock::default(),
        selected: None,
        phero_factor,
        seed,
        save_path,
    };

    std::thread::Builder::new()
        .name("sim".into())
        .spawn(move || run(st, crx, pubs))
        .expect("spawn sim thread");

    Handles {
        commands: ctx,
        hello,
        ants,
        phero,
        terrain,
        stats,
        detail,
        genome,
        config,
    }
}

struct State {
    world: World,
    clock: Clock,
    selected: Option<u64>,
    phero_factor: u8,
    seed: u64,
    save_path: PathBuf,
}

impl State {
    /// Resolving a selection to an *index* every frame — rather than caching
    /// one — is deliberate. `retain_alive` compacts the SoA, so an index is
    /// only valid within the tick that produced it.
    fn selected_index(&self) -> Option<usize> {
        self.selected.and_then(|id| self.world.index_of(id))
    }
}

fn run(mut st: State, mut rx: UnboundedReceiver<Command>, pubs: Publishers) {
    // Reused across the whole run: a 1 MB pheromone frame at 10 fps would
    // otherwise allocate 10 MB/sec for nothing.
    let mut buf = Vec::new();

    let mut last = Instant::now();
    let (mut t_ants, mut t_phero, mut t_stats) = (Instant::now(), Instant::now(), Instant::now());
    let mut t_terrain = Instant::now();

    loop {
        // --- commands ---
        loop {
            match rx.try_recv() {
                Ok(cmd) => apply_command(&mut st, cmd, &pubs, &mut buf),
                Err(TryRecvError::Empty) => break,
                // Every client dropped AND the server is shutting down.
                Err(TryRecvError::Disconnected) => return,
            }
        }

        // --- ticks ---
        let now = Instant::now();
        let elapsed = now.duration_since(last);
        last = now;
        let due = st.clock.ticks_due(elapsed);
        let batch_start = Instant::now();
        for _ in 0..due {
            st.world.tick();
            // Abandon the rest of the batch rather than starve the UI. At 1x
            // and 10x this never trips; at 100x it is what keeps the frame
            // cadence and the pause button alive.
            if batch_start.elapsed() >= TICK_BUDGET {
                break;
            }
        }

        // --- frames, on wall-clock cadences independent of tick rate ---
        let mut published = false;
        if now.duration_since(t_ants) >= ANTS_PERIOD {
            t_ants = now;
            protocol::encode_ants(&mut buf, &st.world);
            let _ = pubs.ants.send(Arc::new(buf.clone()));
            published = true;
        }
        if now.duration_since(t_phero) >= PHERO_PERIOD {
            t_phero = now;
            protocol::encode_phero(&mut buf, &st.world, st.phero_factor);
            let _ = pubs.phero.send(Arc::new(buf.clone()));
            published = true;
        }
        if now.duration_since(t_terrain) >= TERRAIN_PERIOD {
            t_terrain = now;
            protocol::encode_terrain(&mut buf, &st.world, st.phero_factor);
            let _ = pubs.terrain.send(Arc::new(buf.clone()));
            published = true;
        }
        if now.duration_since(t_stats) >= STATS_PERIOD {
            t_stats = now;
            let stats = st.world.stats();
            protocol::encode_stats(&mut buf, st.world.tick_count, &stats);
            let _ = pubs.stats.send(Arc::new(buf.clone()));
            publish_detail(&pubs, &st, &mut buf);
            // Refreshed rather than sent once at startup: a client that
            // connects to an already-running sim would otherwise be told the
            // world is at tick 0. Fifteen bytes at 4 fps. `hello` is idempotent
            // world metadata, and the client treats a repeat as such.
            publish_hello(&pubs, &st, &mut buf);
            published = true;
        }

        if due == 0 && !published {
            std::thread::sleep(IDLE_SLEEP);
        }
    }
}

fn apply_command(st: &mut State, cmd: Command, pubs: &Publishers, buf: &mut Vec<u8>) {
    match cmd {
        Command::SetPaused(p) => st.clock.set_paused(p),
        Command::SetSpeed(s) => st.clock.set_speed(Speed::from_wire(s)),
        Command::Step => st.clock.step(),
        Command::SelectAt(x, y) => {
            st.selected = st.world.nearest_ant(x, y);
            publish_genome(pubs, st, buf);
            publish_detail(pubs, st, buf);
        }
        Command::ClearSelection => st.selected = None,
        Command::SetConfig(field, v) => {
            if protocol::apply_config_field(&mut st.world.cfg, field, v) {
                publish_config(pubs, st, buf);
            } else {
                tracing::warn!(field, "unknown config field id");
            }
        }
        Command::SetPheroRes(log2) => {
            // 8 -> 256x256 (factor 2), 9 -> 512x512 (factor 1). Anything else
            // is a client bug; keep the current value rather than dividing by
            // a nonsense factor.
            st.phero_factor = match log2 {
                9 => 1,
                8 => 2,
                _ => {
                    tracing::warn!(log2, "bad pheromone resolution");
                    st.phero_factor
                }
            };
        }
        Command::Save => match snapshot::save(&st.world) {
            Ok(bytes) => match std::fs::write(&st.save_path, bytes) {
                Ok(()) => tracing::info!(path = ?st.save_path, "saved"),
                Err(e) => tracing::error!(%e, "save failed"),
            },
            Err(e) => tracing::error!(%e, "encode failed"),
        },
        Command::Load => match std::fs::read(&st.save_path).map(|b| snapshot::load(&b)) {
            Ok(Ok(w)) => {
                st.world = w;
                // The loaded world has a different ant population; a stale
                // selection would point at someone else entirely.
                st.selected = None;
                publish_hello(pubs, st, buf);
                publish_config(pubs, st, buf);
                tracing::info!(path = ?st.save_path, "loaded");
            }
            Ok(Err(e)) => tracing::error!(%e, "decode failed"),
            Err(e) => tracing::error!(%e, "read failed"),
        },
        Command::Reset(seed) => {
            // Keep the live-tuned config; the operator reset the world, not
            // their afternoon of slider work.
            let cfg = st.world.cfg.clone();
            st.seed = seed;
            st.world = World::new(&cfg, seed);
            st.selected = None;
            publish_hello(pubs, st, buf);
            publish_config(pubs, st, buf);
        }
    }
}

fn publish_hello(pubs: &Publishers, st: &State, buf: &mut Vec<u8>) {
    let log2 = if st.phero_factor == 1 { 9 } else { 8 };
    protocol::encode_hello(buf, &st.world, log2);
    let _ = pubs.hello.send(Arc::new(buf.clone()));
}

fn publish_config(pubs: &Publishers, st: &State, buf: &mut Vec<u8>) {
    protocol::encode_config(buf, &st.world.cfg);
    let _ = pubs.config.send(Arc::new(buf.clone()));
}

fn publish_genome(pubs: &Publishers, st: &State, buf: &mut Vec<u8>) {
    let Some(i) = st.selected_index() else { return };
    protocol::encode_ant_genome(buf, st.world.ants.id[i], &st.world.ants.genome[i]);
    let _ = pubs.genome.send(Arc::new(buf.clone()));
}

fn publish_detail(pubs: &Publishers, st: &State, buf: &mut Vec<u8>) {
    let Some(id) = st.selected else { return };
    let w = &st.world;

    // A selected ant can die between frames. Say so, rather than freezing the
    // inspector on numbers that stopped being true.
    let Some(i) = st.selected_index().filter(|&i| w.ants.alive[i]) else {
        let act = sim::brain::Activations {
            inputs: [0.0; sim::N_INPUTS],
            h1: [0.0; sim::N_HIDDEN1],
            h2: [0.0; sim::N_HIDDEN2],
            outputs: [0.0; sim::N_OUTPUTS],
        };
        protocol::encode_ant_detail(
            buf,
            &AntDetail {
                id,
                colony: 0,
                alive: false,
                x: 0.0,
                y: 0.0,
                heading: 0.0,
                energy: 0.0,
                max_energy: 0.0,
                size: 0.0,
                carrying: 0.0,
                food_delivered: 0.0,
                age: 0,
                lineage: 0,
                traits: Traits::from_array([0.0; 8]).as_array(),
                act: &act,
                name: "",
            },
        );
        let _ = pubs.detail.send(Arc::new(buf.clone()));
        return;
    };

    let act = w.activations(i);
    protocol::encode_ant_detail(
        buf,
        &AntDetail {
            id,
            colony: w.ants.colony[i],
            alive: true,
            x: w.ants.x[i],
            y: w.ants.y[i],
            heading: w.ants.heading[i],
            energy: w.ants.energy[i],
            max_energy: w.ants.genome[i].max_energy(&w.cfg, w.ants.size[i]),
            size: w.ants.size[i],
            carrying: w.ants.carrying[i],
            food_delivered: w.ants.food_delivered[i],
            age: w.ants.age[i],
            lineage: w.ants.lineage[i],
            traits: w.ants.genome[i].traits.as_array(),
            act: &act,
            name: &sim::names::ant_name(w.ants.id[i]),
        },
    );
    let _ = pubs.detail.send(Arc::new(buf.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::decode_command;

    fn small() -> Config {
        Config {
            width: 32,
            height: 32,
            num_colonies: 2,
            initial_ants_per_colony: 4,
            food_patch_count: 2,
            ..Config::default()
        }
    }

    fn state() -> State {
        State {
            world: World::new(&small(), 1),
            clock: Clock::default(),
            selected: None,
            phero_factor: 2,
            seed: 1,
            save_path: std::env::temp_dir().join("antsim_test_snapshot.bin"),
        }
    }

    fn pubs() -> Publishers {
        Publishers {
            hello: watch::channel(empty()).0,
            ants: watch::channel(empty()).0,
            phero: watch::channel(empty()).0,
            terrain: watch::channel(empty()).0,
            stats: watch::channel(empty()).0,
            detail: watch::channel(empty()).0,
            genome: watch::channel(empty()).0,
            config: watch::channel(empty()).0,
        }
    }

    /// The whole contract of this module: driving `World` through a clock must
    /// not change what the simulation computes. If this ever fails, the server
    /// has leaked into the physics.
    #[test]
    fn ticking_through_the_clock_matches_ticking_the_world_directly() {
        let mut direct = World::new(&small(), 1);
        for _ in 0..1000 {
            direct.tick();
        }

        let mut st = state();
        st.clock.set_paused(false);
        st.clock.set_speed(Speed::X100);
        let mut done = 0;
        while done < 1000 {
            let due = st
                .clock
                .ticks_due(Duration::from_millis(16))
                .min(1000 - done);
            for _ in 0..due {
                st.world.tick();
            }
            done += due;
        }

        assert_eq!(direct.state_hash(), st.world.state_hash());
        assert_eq!(direct.tick_count, st.world.tick_count);
    }

    #[test]
    fn speed_does_not_change_the_trajectory_only_how_fast_we_get_there() {
        let mut a = state();
        let mut b = state();
        for _ in 0..200 {
            a.world.tick();
        }
        // Same tick count, reached through a different clock configuration.
        b.clock.set_paused(false);
        b.clock.set_speed(Speed::X1);
        let mut done = 0;
        while done < 200 {
            let due = b
                .clock
                .ticks_due(Duration::from_millis(100))
                .min(200 - done);
            for _ in 0..due {
                b.world.tick();
            }
            done += due;
        }
        assert_eq!(a.world.state_hash(), b.world.state_hash());
    }

    #[test]
    fn a_set_config_command_reaches_the_world() {
        let mut st = state();
        let mut buf = Vec::new();
        apply_command(&mut st, Command::SetConfig(10, 12.5), &pubs(), &mut buf);
        assert_eq!(st.world.cfg.birth_cost, 12.5);
    }

    #[test]
    fn an_unknown_config_field_is_dropped_not_fatal() {
        let mut st = state();
        let mut buf = Vec::new();
        apply_command(&mut st, Command::SetConfig(250, 1.0), &pubs(), &mut buf);
        assert_eq!(st.world.cfg.birth_cost, Config::default().birth_cost);
    }

    #[test]
    fn selecting_resolves_to_a_living_ant_and_survives_compaction() {
        // `retain_alive` compacts the SoA every tick, so a cached index would
        // silently start pointing at a different ant. Only the id is stable.
        let mut st = state();
        let mut buf = Vec::new();
        let (x, y) = (st.world.ants.x[3], st.world.ants.y[3]);
        apply_command(&mut st, Command::SelectAt(x, y), &pubs(), &mut buf);
        let picked = st.selected.expect("something should be selected");

        for _ in 0..50 {
            st.world.tick();
        }
        if let Some(i) = st.selected_index() {
            assert_eq!(st.world.ants.id[i], picked, "id must resolve to itself");
        }
    }

    #[test]
    fn a_detail_frame_for_a_dead_ant_reports_it_dead_rather_than_freezing() {
        let mut st = state();
        let mut buf = Vec::new();
        st.selected = Some(999_999); // never existed; same path as "died"
        let p = pubs();
        publish_detail(&p, &st, &mut buf);
        assert_eq!(buf[0], protocol::TAG_ANT_DETAIL);
        assert_eq!(buf[10], 0, "alive byte must be false");
        assert_eq!(buf.len(), protocol::ANT_DETAIL_LEN + 1, "fixed body plus an empty-name byte");
    }

    #[test]
    fn reset_keeps_the_tuned_config_but_rebuilds_the_world() {
        let mut st = state();
        let mut buf = Vec::new();
        apply_command(&mut st, Command::SetConfig(10, 7.0), &pubs(), &mut buf);
        for _ in 0..20 {
            st.world.tick();
        }
        apply_command(&mut st, Command::Reset(99), &pubs(), &mut buf);

        assert_eq!(st.world.tick_count, 0);
        assert_eq!(st.seed, 99);
        assert_eq!(
            st.world.cfg.birth_cost, 7.0,
            "a reset must not discard an afternoon of slider work"
        );
    }

    #[test]
    fn reset_with_the_same_seed_reproduces_the_same_world() {
        let mut st = state();
        let mut buf = Vec::new();
        let h0 = st.world.state_hash();
        for _ in 0..30 {
            st.world.tick();
        }
        apply_command(&mut st, Command::Reset(1), &pubs(), &mut buf);
        assert_eq!(st.world.state_hash(), h0);
    }

    #[test]
    fn save_then_load_restores_the_exact_world() {
        let mut st = state();
        let mut buf = Vec::new();
        for _ in 0..40 {
            st.world.tick();
        }
        apply_command(&mut st, Command::Save, &pubs(), &mut buf);
        let saved = st.world.state_hash();

        for _ in 0..40 {
            st.world.tick();
        }
        assert_ne!(st.world.state_hash(), saved);

        apply_command(&mut st, Command::Load, &pubs(), &mut buf);
        assert_eq!(st.world.state_hash(), saved);

        // And it keeps ticking correctly from there: the derived spatial index
        // was rebuilt, not left empty.
        let mut fresh = state();
        for _ in 0..40 {
            fresh.world.tick();
        }
        for _ in 0..25 {
            fresh.world.tick();
            st.world.tick();
        }
        assert_eq!(st.world.state_hash(), fresh.world.state_hash());
    }

    #[test]
    fn a_load_clears_a_selection_that_no_longer_means_anything() {
        let mut st = state();
        let mut buf = Vec::new();
        apply_command(&mut st, Command::Save, &pubs(), &mut buf);
        st.selected = Some(3);
        apply_command(&mut st, Command::Load, &pubs(), &mut buf);
        assert_eq!(st.selected, None);
    }

    #[test]
    fn a_bad_pheromone_resolution_keeps_the_current_one() {
        let mut st = state();
        let mut buf = Vec::new();
        apply_command(&mut st, Command::SetPheroRes(9), &pubs(), &mut buf);
        assert_eq!(st.phero_factor, 1);
        apply_command(&mut st, Command::SetPheroRes(77), &pubs(), &mut buf);
        assert_eq!(st.phero_factor, 1, "nonsense must not divide the grid");
    }

    #[test]
    fn a_malformed_command_never_reaches_the_world() {
        // The decode layer is the guard; this pins that the two agree.
        assert_eq!(decode_command(&[0xFF]), None);
        assert_eq!(decode_command(&[protocol::CMD_RESET, 1]), None);
    }

    /// A client can connect before the sim thread has run a single iteration.
    /// Every channel it reads on connect must already hold a real frame, not an
    /// empty buffer — `spawn` seeds them synchronously for exactly this reason.
    #[test]
    fn every_frame_channel_is_populated_the_instant_spawn_returns() {
        let h = spawn(
            small(),
            1,
            std::env::temp_dir().join("antsim_spawn_test.bin"),
        );

        let hello = h.hello.borrow().clone();
        assert_eq!(hello[0], protocol::TAG_HELLO);
        assert_eq!(u16::from_le_bytes([hello[1], hello[2]]), 32);

        let cfgf = h.config.borrow().clone();
        assert_eq!(cfgf[0], protocol::TAG_CONFIG);

        let ants = h.ants.borrow().clone();
        assert_eq!(ants[0], protocol::TAG_ANTS);
        assert_eq!(
            u32::from_le_bytes([ants[9], ants[10], ants[11], ants[12]]),
            8
        );

        let ph = h.phero.borrow().clone();
        assert_eq!(ph[0], protocol::TAG_PHERO);
        assert_eq!(ph.len(), 14 + 16 * 16 * 4);

        let st = h.stats.borrow().clone();
        assert_eq!(st[0], protocol::TAG_STATS);

        // Nothing is selected yet, so these two are legitimately empty.
        assert!(h.detail.borrow().is_empty());
        assert!(h.genome.borrow().is_empty());
    }

    #[test]
    fn the_thread_ticks_and_publishes_once_unpaused() {
        let h = spawn(
            small(),
            1,
            std::env::temp_dir().join("antsim_thread_test.bin"),
        );
        h.commands.send(Command::SetPaused(false)).unwrap();
        h.commands.send(Command::SetSpeed(2)).unwrap();

        // Poll rather than sleeping a fixed span: a loaded CI box is slow, and
        // a fixed sleep is how a test becomes flaky.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let ants = h.ants.borrow().clone();
            let tick = u64::from_le_bytes(ants[1..9].try_into().unwrap());
            if tick > 0 {
                let count = u32::from_le_bytes([ants[9], ants[10], ants[11], ants[12]]);
                assert!(count > 0, "no ants in the frame");
                assert_eq!(ants.len(), 13 + 8 * count as usize);
                return;
            }
            assert!(Instant::now() < deadline, "the world never advanced");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// At 100x a batch is bounded by wall-clock, not only by tick count. Without
    /// this, one batch on a big world runs for seconds while the socket goes
    /// silent and the pause button does nothing.
    #[test]
    fn fast_forward_still_publishes_frames_and_answers_commands() {
        let h = spawn(small(), 1, std::env::temp_dir().join("antsim_ff_test.bin"));
        h.commands.send(Command::SetPaused(false)).unwrap();
        h.commands.send(Command::SetSpeed(2)).unwrap();

        let tick = || u64::from_le_bytes(h.ants.borrow()[1..9].try_into().unwrap());

        // Two distinct published frames means the loop returned between them.
        let deadline = Instant::now() + Duration::from_secs(10);
        let first = loop {
            let t = tick();
            if t > 0 {
                break t;
            }
            assert!(Instant::now() < deadline, "never started ticking");
            std::thread::sleep(Duration::from_millis(5));
        };
        loop {
            if tick() > first {
                break;
            }
            assert!(Instant::now() < deadline, "only one batch ever published");
            std::thread::sleep(Duration::from_millis(5));
        }

        // And a command sent mid-fast-forward is honoured promptly.
        h.commands.send(Command::SetPaused(true)).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        let a = tick();
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(tick(), a, "pause did not take effect during fast-forward");
    }

    #[test]
    fn a_paused_thread_never_advances_the_world() {
        let h = spawn(
            small(),
            1,
            std::env::temp_dir().join("antsim_paused_test.bin"),
        );
        // Default is paused; give the loop plenty of chances to misbehave.
        std::thread::sleep(Duration::from_millis(150));
        let ants = h.ants.borrow().clone();
        assert_eq!(u64::from_le_bytes(ants[1..9].try_into().unwrap()), 0);
    }
}
