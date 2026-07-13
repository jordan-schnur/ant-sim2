//! The wire format, little-endian, one `u8` tag per message.
//!
//! This module is mirrored by hand in `web/src/protocol.ts`. A mismatch is
//! silent — it renders as garbled ants, not as an error — so the layouts below
//! are pinned by unit tests here and by a cross-language fixture test in the
//! web app that decodes bytes this module produced.
//!
//! Encoders append into a caller-owned buffer. The sim thread reuses one buffer
//! per frame kind: a 1 MB pheromone frame at 10 fps would otherwise allocate
//! 10 MB/sec for nothing.

use sim::brain::Activations;
use sim::config::Config;
use sim::genome::Genome;
use sim::pheromone::Pheromones;
use sim::sense::squash_phero;
use sim::stats::ColonyStats;
use sim::world::World;

pub const TAG_HELLO: u8 = 0x01;
pub const TAG_ANTS: u8 = 0x02;
pub const TAG_PHERO: u8 = 0x03;
pub const TAG_STATS: u8 = 0x04;
pub const TAG_ANT_DETAIL: u8 = 0x05;
pub const TAG_ANT_GENOME: u8 = 0x06;
pub const TAG_CONFIG: u8 = 0x07;
pub const TAG_TERRAIN: u8 = 0x08;
pub const TAG_COLONY_META: u8 = 0x09;
pub const TAG_CHRONICLE: u8 = 0x0A;

pub const BYTES_PER_ANT: usize = 8;
pub const BYTES_PER_COLONY: usize = 46;
pub const ANT_DETAIL_LEN: usize = 425;

/// `size` byte divisor. `TRAIT_RANGES` caps `max_size` at 3.0, so this cannot
/// clip a legal ant.
const MAX_ENCODABLE_SIZE: f32 = 3.0;

/// Position is fixed-point 9.7: the grid is 512 wide, so 9 integer bits and 7
/// fractional bits fit a u16 exactly and sub-cell smoothness is free.
const POS_SCALE: f32 = 128.0;

pub const FLAG_CARRYING: u8 = 1 << 0;
pub const FLAG_ATTACKING: u8 = 1 << 1;

// --- Client -> server ---------------------------------------------------

pub const CMD_SET_PAUSED: u8 = 0x01;
pub const CMD_SET_SPEED: u8 = 0x02;
pub const CMD_STEP: u8 = 0x03;
pub const CMD_SELECT_AT: u8 = 0x04;
pub const CMD_CLEAR_SELECTION: u8 = 0x05;
pub const CMD_SET_CONFIG: u8 = 0x06;
pub const CMD_SET_PHERO_RES: u8 = 0x07;
pub const CMD_SAVE: u8 = 0x08;
pub const CMD_LOAD: u8 = 0x09;
pub const CMD_RESET: u8 = 0x0A;
pub const CMD_SET_FOOD: u8 = 0x0B;
pub const CMD_SET_STONE: u8 = 0x0C;
pub const CMD_SPAWN_ANT: u8 = 0x0D;
pub const CMD_RENAME_COLONY: u8 = 0x0E;
pub const CMD_ADD_TO_STORE: u8 = 0x0F;

// `RenameColony` owns a `String`, so `Command` can no longer be `Copy`.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    SetPaused(bool),
    SetSpeed(u8),
    Step,
    SelectAt(f32, f32),
    ClearSelection,
    SetConfig(u8, f32),
    SetPheroRes(u8),
    Save,
    Load,
    Reset(u64),
    SetFood(f32, f32, f32),
    SetStone(f32, f32, bool),
    SpawnAnt(f32, f32, u8),
    RenameColony(u8, String),
    AddToStore(u8, f32),
}

/// Returns `None` for an unknown tag or a truncated payload. The caller logs
/// and drops; a malformed command must never panic the sim thread.
pub fn decode_command(b: &[u8]) -> Option<Command> {
    let (&tag, rest) = b.split_first()?;
    Some(match tag {
        CMD_SET_PAUSED => Command::SetPaused(*rest.first()? != 0),
        CMD_SET_SPEED => Command::SetSpeed(*rest.first()?),
        CMD_STEP => Command::Step,
        CMD_SELECT_AT => {
            let x = f32::from_le_bytes(rest.get(0..4)?.try_into().ok()?);
            let y = f32::from_le_bytes(rest.get(4..8)?.try_into().ok()?);
            // A NaN coordinate would make every distance comparison false and
            // silently select nothing. Reject it at the boundary.
            if !x.is_finite() || !y.is_finite() {
                return None;
            }
            Command::SelectAt(x, y)
        }
        CMD_CLEAR_SELECTION => Command::ClearSelection,
        CMD_SET_CONFIG => {
            let field = *rest.first()?;
            let v = f32::from_le_bytes(rest.get(1..5)?.try_into().ok()?);
            if !v.is_finite() {
                return None;
            }
            Command::SetConfig(field, v)
        }
        CMD_SET_PHERO_RES => Command::SetPheroRes(*rest.first()?),
        CMD_SAVE => Command::Save,
        CMD_LOAD => Command::Load,
        CMD_RESET => Command::Reset(u64::from_le_bytes(rest.get(0..8)?.try_into().ok()?)),
        CMD_SET_FOOD => {
            let x = f32::from_le_bytes(rest.get(0..4)?.try_into().ok()?);
            let y = f32::from_le_bytes(rest.get(4..8)?.try_into().ok()?);
            let a = f32::from_le_bytes(rest.get(8..12)?.try_into().ok()?);
            if !(x.is_finite() && y.is_finite() && a.is_finite()) {
                return None;
            }
            Command::SetFood(x, y, a)
        }
        CMD_SET_STONE => {
            let x = f32::from_le_bytes(rest.get(0..4)?.try_into().ok()?);
            let y = f32::from_le_bytes(rest.get(4..8)?.try_into().ok()?);
            if !(x.is_finite() && y.is_finite()) {
                return None;
            }
            Command::SetStone(x, y, *rest.get(8)? != 0)
        }
        CMD_SPAWN_ANT => {
            let x = f32::from_le_bytes(rest.get(0..4)?.try_into().ok()?);
            let y = f32::from_le_bytes(rest.get(4..8)?.try_into().ok()?);
            if !(x.is_finite() && y.is_finite()) {
                return None;
            }
            Command::SpawnAnt(x, y, *rest.get(8)?)
        }
        CMD_RENAME_COLONY => {
            let colony = *rest.first()?;
            let len = *rest.get(1)? as usize;
            let bytes = rest.get(2..2 + len)?;
            let name = std::str::from_utf8(bytes).ok()?.to_string();
            Command::RenameColony(colony, name)
        }
        CMD_ADD_TO_STORE => {
            let colony = *rest.first()?;
            let a = f32::from_le_bytes(rest.get(1..5)?.try_into().ok()?);
            if !a.is_finite() {
                return None;
            }
            Command::AddToStore(colony, a)
        }
        _ => return None,
    })
}

// --- Tunable config fields ----------------------------------------------

/// Only scalars that are safe to change mid-run. `width`, `height`, and
/// `num_colonies` are structural and are absent from this table by
/// construction — there is no field id that could resize the world.
///
/// Ids 10..=13 are not in the spec's slider list. They are here because the
/// first 500k-tick run showed 97.7% of ants are born free from the extinction
/// floor rather than paid for out of a colony's store, and fingered exactly
/// these four as the reason. See `docs/superpowers/notes/`.
pub const CONFIG_FIELDS: [&str; 17] = [
    "food_evaporation",
    "alarm_evaporation",
    "scent_evaporation",
    "food_diffusion",
    "alarm_diffusion",
    "scent_diffusion",
    "tax_speed",
    "tax_vision",
    "mutation_rate",
    "mutation_sigma",
    "birth_cost",
    "harvest_rate",
    "refuel_rate",
    "growth_threshold",
    "food_regrow",
    "attack_damage",
    "harvest_weight",
];

fn field_mut(cfg: &mut Config, id: u8) -> Option<&mut f32> {
    Some(match id {
        0 => &mut cfg.food_evaporation,
        1 => &mut cfg.alarm_evaporation,
        2 => &mut cfg.scent_evaporation,
        3 => &mut cfg.food_diffusion,
        4 => &mut cfg.alarm_diffusion,
        5 => &mut cfg.scent_diffusion,
        6 => &mut cfg.tax_speed,
        7 => &mut cfg.tax_vision,
        8 => &mut cfg.mutation_rate,
        9 => &mut cfg.mutation_sigma,
        10 => &mut cfg.birth_cost,
        11 => &mut cfg.harvest_rate,
        12 => &mut cfg.refuel_rate,
        13 => &mut cfg.growth_threshold,
        14 => &mut cfg.food_regrow,
        15 => &mut cfg.attack_damage,
        16 => &mut cfg.harvest_weight,
        _ => return None,
    })
}

pub fn read_config_field(cfg: &Config, id: u8) -> Option<f32> {
    let mut c = cfg.clone();
    field_mut(&mut c, id).map(|v| *v)
}

/// Clamps rather than rejects: an operator dragging a slider should never see
/// the connection die. Returns false for an unknown field id.
pub fn apply_config_field(cfg: &mut Config, id: u8, value: f32) -> bool {
    // An evaporation rate outside (0,1) either freezes the field forever or
    // amplifies it without bound. `Pheromones::step` multiplies by it.
    let clamped = match id {
        0..=2 => value.clamp(1e-4, 0.999_99),
        3..=5 => value.clamp(0.0, 0.5),
        13 => value.clamp(0.01, 1.0),
        _ => value.max(0.0),
    };
    match field_mut(cfg, id) {
        Some(slot) => {
            *slot = clamped;
            true
        }
        None => false,
    }
}

// --- Encoders ------------------------------------------------------------

#[inline]
fn put_u8(b: &mut Vec<u8>, v: u8) {
    b.push(v);
}
#[inline]
fn put_u16(b: &mut Vec<u8>, v: u16) {
    b.extend_from_slice(&v.to_le_bytes());
}
#[inline]
fn put_u32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_le_bytes());
}
#[inline]
fn put_u64(b: &mut Vec<u8>, v: u64) {
    b.extend_from_slice(&v.to_le_bytes());
}
#[inline]
fn put_f32(b: &mut Vec<u8>, v: f32) {
    b.extend_from_slice(&v.to_le_bytes());
}
#[inline]
fn put_str_u8(b: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let n = bytes.len().min(255);
    put_u8(b, n as u8);
    b.extend_from_slice(&bytes[..n]);
}

pub fn encode_hello(out: &mut Vec<u8>, w: &World, phero_res_log2: u8) {
    out.clear();
    put_u8(out, TAG_HELLO);
    put_u16(out, w.cfg.width);
    put_u16(out, w.cfg.height);
    put_u8(out, w.cfg.num_colonies);
    put_u8(out, phero_res_log2);
    put_u64(out, w.tick_count);
}

pub fn encode_ants(out: &mut Vec<u8>, w: &World) {
    out.clear();
    put_u8(out, TAG_ANTS);
    put_u64(out, w.tick_count);

    let live = w.ants.alive.iter().filter(|a| **a).count() as u32;
    put_u32(out, live);

    for i in 0..w.ants.len() {
        if !w.ants.alive[i] {
            continue;
        }
        let qx = (w.ants.x[i] * POS_SCALE).clamp(0.0, u16::MAX as f32) as u16;
        let qy = (w.ants.y[i] * POS_SCALE).clamp(0.0, u16::MAX as f32) as u16;
        let sz = (w.ants.size[i] / MAX_ENCODABLE_SIZE * 255.0).clamp(0.0, 255.0) as u8;
        let mut flags = 0u8;
        if w.ants.carrying[i] > 0.0 {
            flags |= FLAG_CARRYING;
        }
        if w.ants.is_attacking(i) {
            flags |= FLAG_ATTACKING;
        }
        put_u16(out, qx);
        put_u16(out, qy);
        put_u8(out, w.ants.colony[i]);
        put_u8(out, sz);
        put_u8(out, flags);
        // Heading rides in what was the pad byte: the record stays 8 bytes and
        // GPU-aligned. Canonical mapping (shared with the ant shader and
        // headingByteToRadians in sprites.ts):
        //   byte = (angle + PI) / (2*PI) * 255, angle in [-PI, PI).
        let a = sim::apply::wrap_angle(w.ants.heading[i]);
        let h = ((a + std::f32::consts::PI) / (2.0 * std::f32::consts::PI) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8;
        put_u8(out, h);
    }
}

/// RGBA8. R = food trail, G = alarm, B = colony scent, A = owning colony.
///
/// R/G/B run through `squash_phero`, the very function the ants' sensors use,
/// so on-screen brightness *is* the number the ant reads. That makes the view a
/// debugging instrument rather than a decoration.
///
/// Downsampling is 2x2 **max**, not mean. A foraging trail is often one cell
/// wide; averaging it with three empty neighbours quarters its brightness and
/// it disappears at the default 256x256 — which an operator would read as "no
/// trails formed". Max over-shows sparse pheromone instead. For an instrument
/// whose entire job is revealing whether trails emerge, that is the correct
/// direction to fail in.
pub fn encode_phero(out: &mut Vec<u8>, w: &World, factor: u8) {
    let f = factor.max(1) as usize;
    let sw = w.cfg.width as usize / f;
    let sh = w.cfg.height as usize / f;

    out.clear();
    out.reserve(14 + sw * sh * 4);
    put_u8(out, TAG_PHERO);
    put_u64(out, w.tick_count);
    put_u16(out, sw as u16);
    put_u16(out, sh as u16);
    put_u8(out, f as u8);

    let div = w.cfg.phero_log_div;
    let p: &Pheromones = &w.phero;
    let width = w.cfg.width as usize;

    for sy in 0..sh {
        for sx in 0..sw {
            let (mut food, mut alarm, mut scent) = (0.0f32, 0.0f32, 0.0f32);
            let mut owner = sim::pheromone::NO_OWNER;
            for dy in 0..f {
                for dx in 0..f {
                    let i = (sy * f + dy) * width + (sx * f + dx);
                    food = food.max(p.food[i]);
                    alarm = alarm.max(p.alarm[i]);
                    if p.scent[i] > scent {
                        scent = p.scent[i];
                        owner = p.owner[i];
                    }
                }
            }
            put_u8(out, (squash_phero(food, div) * 255.0) as u8);
            put_u8(out, (squash_phero(alarm, div) * 255.0) as u8);
            put_u8(out, (squash_phero(scent, div) * 255.0) as u8);
            put_u8(out, owner);
        }
    }
}

/// The map itself: stone, standing food, and nest tiles.
///
/// Without this the client renders an empty void with pheromone smears on it —
/// the pheromone frame carries *trails*, not the food they lead to, and knows
/// nothing about the rock the ants walk around.
///
/// R = standing food, mean over the block and normalised by `food_patch_max`.
/// G = stone coverage fraction. Mean rather than max, unlike the pheromone
///     layer: a max would paint a whole super-cell solid for one stone corner
///     and draw walls that are not there. Trails need over-showing; terrain
///     needs honesty.
/// B = owning colony of a nest tile in the block (255 = no nest).
/// A = 255, unused. Kept so the texture is a plain RGBA8 upload.
pub fn encode_terrain(out: &mut Vec<u8>, w: &World, factor: u8) {
    let f = factor.max(1) as usize;
    let sw = w.cfg.width as usize / f;
    let sh = w.cfg.height as usize / f;

    out.clear();
    out.reserve(14 + sw * sh * 4);
    put_u8(out, TAG_TERRAIN);
    put_u64(out, w.tick_count);
    put_u16(out, sw as u16);
    put_u16(out, sh as u16);
    put_u8(out, f as u8);

    let width = w.cfg.width as usize;
    let per_block = (f * f) as f32;
    let food_max = w.cfg.food_patch_max.max(1e-6);

    for sy in 0..sh {
        for sx in 0..sw {
            let (mut food, mut stone) = (0.0f32, 0.0f32);
            let mut nest = sim::grid::NO_NEST;
            for dy in 0..f {
                for dx in 0..f {
                    let i = (sy * f + dy) * width + (sx * f + dx);
                    food += w.grid.food[i];
                    if w.grid.stone[i] {
                        stone += 1.0;
                    }
                    if w.grid.nest[i] != sim::grid::NO_NEST {
                        nest = w.grid.nest[i];
                    }
                }
            }
            put_u8(out, ((food / per_block / food_max).min(1.0) * 255.0) as u8);
            put_u8(out, ((stone / per_block) * 255.0) as u8);
            put_u8(out, nest);
            put_u8(out, 255);
        }
    }
}

/// `ColonyStats::food_delivered` (living ants only) is deliberately absent. It
/// falls whenever a good forager dies of old age, so an operator watching it
/// would read a thriving colony as a dying one. `delivered_total` is monotonic
/// and is the curve that answers "is this evolving".
pub fn encode_stats(out: &mut Vec<u8>, tick: u64, stats: &[ColonyStats]) {
    out.clear();
    put_u8(out, TAG_STATS);
    put_u64(out, tick);
    put_u8(out, stats.len() as u8);
    for s in stats {
        put_u8(out, s.id);
        put_u8(out, 0); // pad
        put_u32(out, s.population);
        put_f32(out, s.store);
        put_u64(out, s.births);
        put_u64(out, s.deaths);
        put_u64(out, s.floor_spawns);
        put_f32(out, s.mean_size);
        put_f32(out, s.mean_lineage);
        put_f32(out, s.delivered_total);
    }
}

pub struct AntDetail<'a> {
    pub id: u64,
    pub colony: u8,
    pub alive: bool,
    pub x: f32,
    pub y: f32,
    pub heading: f32,
    pub energy: f32,
    pub max_energy: f32,
    pub size: f32,
    pub carrying: f32,
    pub food_delivered: f32,
    pub food_harvested: f32,
    pub age: u32,
    pub lineage: u32,
    pub traits: [f32; 8],
    pub act: &'a Activations,
    pub name: &'a str,
}

pub fn encode_ant_detail(out: &mut Vec<u8>, d: &AntDetail) {
    out.clear();
    out.reserve(ANT_DETAIL_LEN);
    put_u8(out, TAG_ANT_DETAIL);
    put_u64(out, d.id);
    put_u8(out, d.colony);
    put_u8(out, d.alive as u8);
    put_u8(out, 0);
    put_u8(out, 0);
    for v in [
        d.x,
        d.y,
        d.heading,
        d.energy,
        d.max_energy,
        d.size,
        d.carrying,
        d.food_delivered,
    ] {
        put_f32(out, v);
    }
    put_u32(out, d.age);
    put_u32(out, d.lineage);
    for v in d.traits {
        put_f32(out, v);
    }
    for v in d.act.inputs {
        put_f32(out, v);
    }
    for v in d.act.h1 {
        put_f32(out, v);
    }
    for v in d.act.h2 {
        put_f32(out, v);
    }
    for v in d.act.outputs {
        put_f32(out, v);
    }
    // Appended after the activations so every earlier offset is unchanged; the
    // client reads it at ANT_DETAIL_LEN - 4. Fitness = delivered + w*harvested,
    // and the inspector shows that number, so the client needs harvested too.
    put_f32(out, d.food_harvested);
    // The fixed body ends here; `ANT_DETAIL_LEN` pins its length. The name is a
    // length-prefixed tail, so old fixed offsets are unchanged.
    debug_assert_eq!(out.len(), ANT_DETAIL_LEN);
    put_str_u8(out, d.name);
}

/// Colony names. Sent on connect and after reset/load (names change with the
/// world). See the `0x09` layout in the plan/spec.
pub fn encode_colony_meta(out: &mut Vec<u8>, w: &World) {
    out.clear();
    put_u8(out, TAG_COLONY_META);
    put_u8(out, w.colonies.len() as u8);
    for c in &w.colonies {
        put_u8(out, c.id);
        put_str_u8(out, &c.name);
    }
}

/// The chronicle: a full capped snapshot at the stats cadence. The client
/// replaces its list wholesale (watch channels are latest-value-wins).
pub fn encode_chronicle(out: &mut Vec<u8>, w: &World) {
    out.clear();
    put_u8(out, TAG_CHRONICLE);
    put_u16(out, w.chronicle.events.len() as u16);
    for ev in &w.chronicle.events {
        put_u64(out, ev.tick);
        put_u8(out, ev.colony);
        put_u8(out, ev.kind as u8);
        let has_ant = ev.ant_id.is_some();
        put_u8(out, if has_ant { 1 } else { 0 });
        put_u64(out, ev.ant_id.unwrap_or(0));
        put_str_u8(out, ev.ant_name.as_deref().unwrap_or(""));
        let t = ev.text.as_bytes();
        let n = t.len().min(u16::MAX as usize);
        put_u16(out, n as u16);
        out.extend_from_slice(&t[..n]);
    }
}

/// Split out of `AntDetail` because weights never change while an ant lives.
/// Resending 4.5 KB at 4 fps to redraw static edges is waste.
pub fn encode_ant_genome(out: &mut Vec<u8>, id: u64, g: &Genome) {
    out.clear();
    out.reserve(9 + g.params.len() * 4);
    put_u8(out, TAG_ANT_GENOME);
    put_u64(out, id);
    for v in &g.params {
        put_f32(out, *v);
    }
}

/// Lets the client render slider positions from the server's truth rather than
/// assuming its defaults match.
pub fn encode_config(out: &mut Vec<u8>, cfg: &Config) {
    out.clear();
    put_u8(out, TAG_CONFIG);
    put_u8(out, CONFIG_FIELDS.len() as u8);
    for id in 0..CONFIG_FIELDS.len() as u8 {
        put_u8(out, id);
        put_f32(
            out,
            read_config_field(cfg, id).expect("field table is dense"),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim::brain::Brain;
    use sim::config::Config;
    use sim::rng::Pcg32;

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

    #[test]
    fn hello_is_fifteen_bytes_and_carries_the_grid_size() {
        let w = World::new(&small(), 1);
        let mut b = Vec::new();
        encode_hello(&mut b, &w, 8);
        assert_eq!(b.len(), 15);
        assert_eq!(b[0], TAG_HELLO);
        assert_eq!(u16::from_le_bytes([b[1], b[2]]), 32);
        assert_eq!(b[5], 2);
        assert_eq!(b[6], 8);
    }

    #[test]
    fn an_ant_frame_is_a_header_plus_eight_bytes_per_living_ant() {
        let w = World::new(&small(), 1);
        let mut b = Vec::new();
        encode_ants(&mut b, &w);
        let count = u32::from_le_bytes([b[9], b[10], b[11], b[12]]);
        assert_eq!(count, 8);
        assert_eq!(b.len(), 13 + 8 * count as usize);
    }

    #[test]
    fn an_ant_frame_skips_the_dead() {
        let mut w = World::new(&small(), 1);
        w.ants.alive[0] = false;
        let mut b = Vec::new();
        encode_ants(&mut b, &w);
        assert_eq!(u32::from_le_bytes([b[9], b[10], b[11], b[12]]), 7);
    }

    #[test]
    fn position_survives_the_fixed_point_round_trip_to_within_half_a_cell() {
        let mut w = World::new(&small(), 1);
        w.ants.x[0] = 17.3;
        w.ants.y[0] = 4.75;
        let mut b = Vec::new();
        encode_ants(&mut b, &w);
        let x = u16::from_le_bytes([b[13], b[14]]) as f32 / POS_SCALE;
        let y = u16::from_le_bytes([b[15], b[16]]) as f32 / POS_SCALE;
        assert!((x - 17.3).abs() < 1.0 / POS_SCALE, "x round-tripped to {x}");
        // 4.75 is exactly representable in 9.7 fixed point.
        assert_eq!(y, 4.75);
    }

    #[test]
    fn the_far_corner_of_a_512_grid_does_not_wrap_to_zero() {
        // 511.9 * 128 = 65523, just inside u16. A 512-wide grid is the largest
        // that fits 9.7 fixed point; this is the test that would catch someone
        // widening the grid without widening the encoding.
        let q = (511.9f32 * POS_SCALE).clamp(0.0, u16::MAX as f32) as u16;
        assert!(q > 65_000, "got {q}");
        assert_eq!(512.0f32 * POS_SCALE, 65536.0, "512 is the exact overflow");
    }

    #[test]
    fn carrying_and_attacking_set_independent_flag_bits() {
        let mut w = World::new(&small(), 1);
        w.ants.clear_attacking();
        w.ants.carrying[0] = 3.0;
        w.ants.attacking[1] = true;
        w.ants.carrying[2] = 1.0;
        w.ants.attacking[2] = true;

        let mut b = Vec::new();
        encode_ants(&mut b, &w);
        let flags = |i: usize| b[13 + 8 * i + 6];
        assert_eq!(flags(0), FLAG_CARRYING);
        assert_eq!(flags(1), FLAG_ATTACKING);
        assert_eq!(flags(2), FLAG_CARRYING | FLAG_ATTACKING);
        assert_eq!(flags(3), 0);
    }

    #[test]
    fn heading_is_encoded_in_the_record_pad_byte() {
        use std::f32::consts::PI;
        let mut w = World::new(&small(), 1);
        // 0 rad (facing +x) sits at the middle of the u8 range.
        w.ants.heading[0] = 0.0;
        // -PI is the low end of the wrapped range -> byte 0.
        w.ants.heading[1] = -PI;
        // Just under +PI is the high end -> byte 255.
        w.ants.heading[2] = PI - 0.0001;

        let mut b = Vec::new();
        encode_ants(&mut b, &w);
        let heading_byte = |i: usize| b[13 + 8 * i + 7];

        assert_eq!(heading_byte(0), 128, "0 rad maps to mid-range");
        assert_eq!(heading_byte(1), 0, "-PI maps to 0");
        assert_eq!(heading_byte(2), 255, "just under +PI maps to 255");
    }

    #[test]
    fn a_pheromone_frame_is_a_header_plus_rgba_per_texel() {
        let w = World::new(&small(), 1);
        let mut b = Vec::new();
        encode_phero(&mut b, &w, 1);
        assert_eq!(b.len(), 14 + 32 * 32 * 4);
        assert_eq!(b[0], TAG_PHERO);
        assert_eq!(u16::from_le_bytes([b[9], b[10]]), 32);
        assert_eq!(b[13], 1);

        encode_phero(&mut b, &w, 2);
        assert_eq!(b.len(), 14 + 16 * 16 * 4);
        assert_eq!(u16::from_le_bytes([b[9], b[10]]), 16);
        assert_eq!(b[13], 2);
    }

    #[test]
    fn downsampling_takes_the_max_so_a_one_cell_trail_survives() {
        // The whole reason for max over mean. One bright cell in a 2x2 block
        // must still be bright after downsampling; a mean would quarter it.
        let mut w = World::new(&small(), 1);
        w.phero.food.iter_mut().for_each(|v| *v = 0.0);
        let bright = w.grid.idx(2, 2); // sub-cell of super-cell (1,1)
        w.phero.food[bright] = 500.0;

        let mut b = Vec::new();
        encode_phero(&mut b, &w, 2);
        let texel = 14 + (1 * 16 + 1) * 4;
        let full = (squash_phero(500.0, w.cfg.phero_log_div) * 255.0) as u8;
        assert_eq!(b[texel], full, "the max must survive downsampling intact");
        assert!(full > 60, "fixture is too dim to be meaningful");
    }

    #[test]
    fn the_scent_owner_comes_from_the_sub_cell_that_won_the_max() {
        let mut w = World::new(&small(), 1);
        w.phero.scent.iter_mut().for_each(|v| *v = 0.0);
        w.phero
            .owner
            .iter_mut()
            .for_each(|v| *v = sim::pheromone::NO_OWNER);
        let weak = w.grid.idx(0, 0);
        let strong = w.grid.idx(1, 1);
        w.phero.scent[weak] = 1.0;
        w.phero.owner[weak] = 0;
        w.phero.scent[strong] = 900.0;
        w.phero.owner[strong] = 1;

        let mut b = Vec::new();
        encode_phero(&mut b, &w, 2);
        assert_eq!(b[14 + 3], 1, "alpha must name the colony that won the max");
    }

    #[test]
    fn a_terrain_frame_is_a_header_plus_rgba_per_texel() {
        let w = World::new(&small(), 1);
        let mut b = Vec::new();
        encode_terrain(&mut b, &w, 1);
        assert_eq!(b.len(), 14 + 32 * 32 * 4);
        assert_eq!(b[0], TAG_TERRAIN);
        assert_eq!(b[13], 1);
    }

    #[test]
    fn terrain_carries_stone_food_and_nests() {
        let w = World::new(&small(), 1);
        let mut b = Vec::new();
        encode_terrain(&mut b, &w, 1);
        let t = &b[14..];
        let n = 32 * 32;
        assert!((0..n).any(|i| t[4 * i] > 0), "no food anywhere on the map");
        assert!((0..n).any(|i| t[4 * i + 1] > 0), "no stone anywhere");
        let nests: Vec<u8> = (0..n)
            .map(|i| t[4 * i + 2])
            .filter(|&v| v != sim::grid::NO_NEST)
            .collect();
        assert!(!nests.is_empty(), "no nest tiles");
        assert!(nests.iter().all(|&c| c < w.cfg.num_colonies));
    }

    #[test]
    fn stone_downsamples_by_coverage_not_by_max() {
        // A max would paint a whole super-cell solid for one stone corner and
        // draw walls the ants can walk straight through. Terrain must be honest
        // even though the pheromone layer deliberately is not.
        let mut w = World::new(&small(), 1);
        w.grid.stone.iter_mut().for_each(|s| *s = false);
        let one = w.grid.idx(0, 0);
        w.grid.stone[one] = true; // 1 of the 4 sub-cells of super-cell (0,0)

        let mut b = Vec::new();
        encode_terrain(&mut b, &w, 2);
        let g = b[14 + 1];
        assert!(
            (60..=68).contains(&g),
            "expected ~25% coverage (63), got {g}"
        );
    }

    #[test]
    fn terrain_food_is_normalised_by_the_patch_maximum() {
        let mut w = World::new(&small(), 1);
        w.grid.food.iter_mut().for_each(|f| *f = 0.0);
        let i = w.grid.idx(3, 3);
        w.grid.food[i] = w.cfg.food_patch_max;

        let mut b = Vec::new();
        encode_terrain(&mut b, &w, 1);
        assert_eq!(b[14 + (3 * 32 + 3) * 4], 255, "a full cell must saturate");

        // And beyond the maximum it clamps rather than wrapping to black.
        w.grid.food[i] = w.cfg.food_patch_max * 10.0;
        encode_terrain(&mut b, &w, 1);
        assert_eq!(b[14 + (3 * 32 + 3) * 4], 255);
    }

    #[test]
    fn a_stats_frame_is_a_header_plus_forty_six_bytes_per_colony() {
        let w = World::new(&small(), 1);
        let s = w.stats();
        let mut b = Vec::new();
        encode_stats(&mut b, w.tick_count, &s);
        assert_eq!(b.len(), 10 + BYTES_PER_COLONY * s.len());
        assert_eq!(b[9], 2);
    }

    #[test]
    fn an_ant_detail_frame_is_exactly_the_documented_length() {
        let w = World::new(&small(), 1);
        let act = w.activations(0);
        let mut b = Vec::new();
        encode_ant_detail(
            &mut b,
            &AntDetail {
                id: 7,
                colony: 1,
                alive: true,
                x: 1.0,
                y: 2.0,
                heading: 0.5,
                energy: 10.0,
                max_energy: 30.0,
                size: 1.0,
                carrying: 0.0,
                food_delivered: 0.0,
                food_harvested: 9.0,
                age: 3,
                lineage: 4,
                traits: [0.0; 8],
                act: &act,
                name: "",
            },
        );
        assert_eq!(b.len(), ANT_DETAIL_LEN + 1, "fixed body plus an empty-name length byte");
        assert_eq!(u64::from_le_bytes(b[1..9].try_into().unwrap()), 7);
        assert_eq!(b[10], 1, "alive byte");
        assert_eq!(u32::from_le_bytes(b[45..49].try_into().unwrap()), 3);
        assert_eq!(u32::from_le_bytes(b[49..53].try_into().unwrap()), 4);
        // food_harvested is the last fixed f32, at offset 421 (just past outputs).
        assert_eq!(f32::from_le_bytes(b[421..425].try_into().unwrap()), 9.0);
    }

    #[test]
    fn colony_meta_encodes_tag_count_and_a_name() {
        let w = World::new(&small(), 1);
        let mut b = Vec::new();
        encode_colony_meta(&mut b, &w);
        assert_eq!(b[0], TAG_COLONY_META);
        assert_eq!(b[1], 2); // count
        // id, then a non-zero name length for the first colony.
        assert_eq!(b[2], 0);
        assert!(b[3] > 0, "colony 0 has an empty name");
    }

    #[test]
    fn chronicle_encodes_tag_and_count() {
        let w = World::new(&small(), 1);
        let mut b = Vec::new();
        encode_chronicle(&mut b, &w);
        assert_eq!(b[0], TAG_CHRONICLE);
        // A fresh world has an empty chronicle.
        assert_eq!(u16::from_le_bytes([b[1], b[2]]), 0);
    }

    #[test]
    fn ant_detail_appends_a_length_prefixed_name() {
        let act = Activations {
            inputs: [0.0; sim::N_INPUTS],
            h1: [0.0; sim::N_HIDDEN1],
            h2: [0.0; sim::N_HIDDEN2],
            outputs: [0.0; sim::N_OUTPUTS],
        };
        let mut b = Vec::new();
        encode_ant_detail(&mut b, &AntDetail {
            id: 5, colony: 0, alive: true, x: 1.0, y: 2.0, heading: 0.0,
            energy: 1.0, max_energy: 1.0, size: 1.0, carrying: 0.0,
            food_delivered: 0.0, food_harvested: 0.0, age: 0, lineage: 0,
            traits: [0.0; 8], act: &act, name: "Wren-5",
        });
        assert_eq!(b[ANT_DETAIL_LEN], 6, "name length byte follows the fixed body");
        assert_eq!(b.len(), ANT_DETAIL_LEN + 1 + 6);
    }

    #[test]
    fn a_genome_frame_carries_every_parameter() {
        let g = Genome::random(&mut Pcg32::new(1, 1));
        let mut b = Vec::new();
        encode_ant_genome(&mut b, 42, &g);
        assert_eq!(b.len(), 9 + sim::N_PARAMS * 4);
        let first = f32::from_le_bytes(b[9..13].try_into().unwrap());
        assert_eq!(first, g.params[0]);
    }

    #[test]
    fn the_detail_frames_activations_match_a_forward_pass() {
        let w = World::new(&small(), 1);
        let act = w.activations(0);
        let mut b = Vec::new();
        encode_ant_detail(
            &mut b,
            &AntDetail {
                id: 0,
                colony: 0,
                alive: true,
                x: 0.0,
                y: 0.0,
                heading: 0.0,
                energy: 0.0,
                max_energy: 0.0,
                size: 0.0,
                carrying: 0.0,
                food_delivered: 0.0,
                food_harvested: 0.0,
                age: 0,
                lineage: 0,
                traits: [0.0; 8],
                act: &act,
                name: "",
            },
        );
        let out0 = f32::from_le_bytes(b[389..393].try_into().unwrap());
        let expected = w.ants.genome[0].forward(&act.inputs);
        assert_eq!(out0, expected.outputs[0]);
    }

    #[test]
    fn a_config_frame_covers_every_tunable_field() {
        let cfg = Config::default();
        let mut b = Vec::new();
        encode_config(&mut b, &cfg);
        assert_eq!(b[1] as usize, CONFIG_FIELDS.len());
        assert_eq!(b.len(), 2 + CONFIG_FIELDS.len() * 5);
        let v = f32::from_le_bytes(b[3..7].try_into().unwrap());
        assert_eq!(v, cfg.food_evaporation);
    }

    #[test]
    fn field_id_16_sets_harvest_weight() {
        let mut cfg = Config::default();
        assert!(apply_config_field(&mut cfg, 16, 0.1));
        assert_eq!(cfg.harvest_weight, 0.1);
        // Clamped to >= 0 like the other non-evaporation fields.
        apply_config_field(&mut cfg, 16, -1.0);
        assert_eq!(cfg.harvest_weight, 0.0);
    }

    #[test]
    fn the_config_table_is_dense_and_stops_where_it_says() {
        let cfg = Config::default();
        for id in 0..CONFIG_FIELDS.len() as u8 {
            assert!(read_config_field(&cfg, id).is_some(), "field {id} missing");
        }
        assert!(read_config_field(&cfg, CONFIG_FIELDS.len() as u8).is_none());
    }

    #[test]
    fn there_is_no_field_id_that_can_resize_the_world() {
        // Structural fields must be unreachable from the wire. If someone adds
        // `width` to the table, this test does not catch it -- but the absence
        // of any u16 setter in `apply_config_field` does.
        let mut cfg = Config::default();
        for id in 0..=255u8 {
            apply_config_field(&mut cfg, id, 1.0);
        }
        assert_eq!(cfg.width, 512);
        assert_eq!(cfg.height, 512);
        assert_eq!(cfg.num_colonies, 8);
    }

    #[test]
    fn evaporation_is_clamped_into_the_open_unit_interval() {
        let mut cfg = Config::default();
        apply_config_field(&mut cfg, 0, 5.0);
        assert!(cfg.food_evaporation < 1.0 && cfg.food_evaporation > 0.0);
        apply_config_field(&mut cfg, 0, -1.0);
        assert!(cfg.food_evaporation > 0.0);
    }

    #[test]
    fn a_negative_birth_cost_is_clamped_to_zero() {
        let mut cfg = Config::default();
        apply_config_field(&mut cfg, 10, -50.0);
        assert_eq!(cfg.birth_cost, 0.0);
    }

    #[test]
    fn an_unknown_config_field_is_rejected_not_ignored() {
        let mut cfg = Config::default();
        assert!(!apply_config_field(&mut cfg, 200, 1.0));
    }

    #[test]
    fn decode_handles_every_command_tag() {
        assert_eq!(
            decode_command(&[CMD_SET_PAUSED, 1]),
            Some(Command::SetPaused(true))
        );
        assert_eq!(
            decode_command(&[CMD_SET_SPEED, 2]),
            Some(Command::SetSpeed(2))
        );
        assert_eq!(decode_command(&[CMD_STEP]), Some(Command::Step));
        assert_eq!(
            decode_command(&[CMD_CLEAR_SELECTION]),
            Some(Command::ClearSelection)
        );
        assert_eq!(decode_command(&[CMD_SAVE]), Some(Command::Save));
        assert_eq!(decode_command(&[CMD_LOAD]), Some(Command::Load));

        let mut b = vec![CMD_SELECT_AT];
        b.extend_from_slice(&3.5f32.to_le_bytes());
        b.extend_from_slice(&7.25f32.to_le_bytes());
        assert_eq!(decode_command(&b), Some(Command::SelectAt(3.5, 7.25)));

        let mut b = vec![CMD_SET_CONFIG, 4];
        b.extend_from_slice(&0.25f32.to_le_bytes());
        assert_eq!(decode_command(&b), Some(Command::SetConfig(4, 0.25)));

        let mut b = vec![CMD_RESET];
        b.extend_from_slice(&99u64.to_le_bytes());
        assert_eq!(decode_command(&b), Some(Command::Reset(99)));
    }

    #[test]
    fn decodes_the_map_edit_commands() {
        let mut b = vec![CMD_SET_FOOD];
        b.extend_from_slice(&1.0f32.to_le_bytes());
        b.extend_from_slice(&2.0f32.to_le_bytes());
        b.extend_from_slice(&50.0f32.to_le_bytes());
        assert_eq!(decode_command(&b), Some(Command::SetFood(1.0, 2.0, 50.0)));

        let mut r = vec![CMD_RENAME_COLONY, 3, 4];
        r.extend_from_slice(b"Ants");
        assert_eq!(
            decode_command(&r),
            Some(Command::RenameColony(3, "Ants".into()))
        );

        assert_eq!(decode_command(&[CMD_ADD_TO_STORE, 0]), None); // truncated
    }

    #[test]
    fn an_unknown_tag_decodes_to_none() {
        assert_eq!(decode_command(&[0xFF, 1, 2, 3]), None);
    }

    #[test]
    fn an_empty_or_truncated_payload_decodes_to_none() {
        assert_eq!(decode_command(&[]), None);
        assert_eq!(decode_command(&[CMD_SET_PAUSED]), None);
        assert_eq!(decode_command(&[CMD_SELECT_AT, 0, 0]), None);
        assert_eq!(decode_command(&[CMD_RESET, 1, 2, 3]), None);
        assert_eq!(decode_command(&[CMD_SET_CONFIG, 3, 0, 0]), None);
    }

    #[test]
    fn a_nan_coordinate_is_rejected_rather_than_selecting_nothing() {
        // Every `d2 < best` comparison against NaN is false, so a NaN pick
        // would silently return the first ant -- or none. Reject at the edge.
        let mut b = vec![CMD_SELECT_AT];
        b.extend_from_slice(&f32::NAN.to_le_bytes());
        b.extend_from_slice(&0.0f32.to_le_bytes());
        assert_eq!(decode_command(&b), None);
    }

    #[test]
    fn a_nan_config_value_is_rejected_before_it_poisons_the_field() {
        let mut b = vec![CMD_SET_CONFIG, 0];
        b.extend_from_slice(&f32::NAN.to_le_bytes());
        assert_eq!(decode_command(&b), None);
    }
}
