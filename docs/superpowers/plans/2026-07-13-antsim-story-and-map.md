# Story & Editable Map — Implementation Plan (Plan B of B)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn a growing simulation into a story you can read and shape — colony names, names for notable ants, an expandable chronicle of firsts, a right-click map editor, on-map labels, colorblind-friendly colony symbols, and a collapsible tuning menu.

**Architecture:** Names and chronicle events live in the **sim** (deterministic, serialized, survive save/load) and stream to the client over two new frames. Map edits are new client→server commands that mutate the `World` on the sim thread, exactly like reset/load. Labels, symbols, and the tuning menu are pure client concerns.

**Tech Stack:** Rust (`sim`, `server`), TypeScript (`web`). No new dependencies.

**Spec:** `docs/superpowers/specs/2026-07-13-antsim-growth-and-story.md` (Sections 2–5).
**Prerequisite:** Plan A (shaped fitness) complete and colonies observed to grow.

## Global Constraints

- **`sim` stays pure:** no I/O, no `println!`, no sockets. Names are generated from `(seed, id)`; map edits arrive as data through the existing command channel.
- **Determinism:** same seed + same config + same command stream ⇒ identical `state_hash`, any thread count. Names, chronicle, and detector flags are written only in the serial phase and are functions of sim state, never wall-clock.
- **The wire format is mirrored by hand** in `crates/server/src/protocol.rs` and `web/src/protocol.ts`; a layout mismatch is silent (garbled render, not an error). Every new frame gets a byte fixture in `crates/server/tests/fixtures.rs` decoded by `web/tests/protocol.test.ts`. **That cross-language test is the guard — a new frame is not done until it has one.**
- **A malformed client command is logged and dropped, never panics the sim** (existing `decode_command` contract).
- **Little-endian, one `u8` tag per message.** Strings are `u8 length` + UTF-8 bytes unless stated otherwise.
- **Tag budget:** server→client `0x01`–`0x08` are taken; new frames are `0x09` (ColonyMeta) and `0x0A` (Chronicle). Client→server `0x01`–`0x0A` are taken; new commands are `0x0B`–`0x0F`.
- **`delivered_total` on the wire stays the real objective.** Nothing in this plan changes `encode_stats`.
- Rust edition 2021. Every task ends green for the crates it touched, and a commit.

---

## Wire Format Additions

### Server → client

**`0x09` ColonyMeta** — sent on connect and after reset/load. Colony names.

| offset | type | field |
| --- | --- | --- |
| 0 | u8 | tag = 0x09 |
| 1 | u8 | count |
| then per colony | u8 | id |
| | u8 | name_len |
| | name_len × u8 | name (UTF-8) |

**`0x0A` Chronicle** — full capped snapshot, sent at the stats cadence (4 fps). The
client replaces its list wholesale (watch channels are latest-value-wins, so a
snapshot is simpler and more robust than incremental streaming).

| offset | type | field |
| --- | --- | --- |
| 0 | u8 | tag = 0x0A |
| 1 | u16 | count |
| then per event | u64 | tick |
| | u8 | colony |
| | u8 | kind (see `EventKind`) |
| | u8 | flags: bit0 = has_ant |
| | u64 | ant_id (0 if no ant) |
| | u8 | ant_name_len |
| | ant_name_len × u8 | ant_name (UTF-8) |
| | u16 | text_len |
| | text_len × u8 | text (UTF-8) |

**`0x05` AntDetail** gains a trailing name: after the existing 421-byte body,
append `u8 name_len` + UTF-8 name. `ANT_DETAIL_LEN` (421) becomes the *minimum*
length; existing fixed offsets are unchanged, so only the tail is variable.

### Client → server

| tag | const | payload | meaning |
| --- | --- | --- | --- |
| 0x0B | CMD_SET_FOOD | f32 x, f32 y, f32 amount | set standing food at the cell |
| 0x0C | CMD_SET_STONE | f32 x, f32 y, u8 solid | place/remove stone |
| 0x0D | CMD_SPAWN_ANT | f32 x, f32 y, u8 colony | spawn one ant of that colony |
| 0x0E | CMD_RENAME_COLONY | u8 colony, u8 len, len×u8 name | rename a colony |
| 0x0F | CMD_ADD_TO_STORE | u8 colony, f32 amount | add food to a colony's store |

Any unknown tag or short payload → `None` → logged and dropped. A non-finite
`f32` coordinate/amount is rejected at decode, as `SelectAt` already does.

---

# Phase B1 — Names and the Chronicle

## Task 1: Deterministic name generator

**Files:**
- Create: `crates/sim/src/names.rs`
- Modify: `crates/sim/src/lib.rs` (add `pub mod names;`)

**Interfaces:**
- Consumes: `Pcg32`.
- Produces: `names::colony_name(seed: u64, colony: u8) -> String`; `names::ant_name(id: u64) -> String`.

- [ ] **Step 1: Write the failing tests**

Create `crates/sim/src/names.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colony_name_is_deterministic_for_seed_and_id() {
        assert_eq!(colony_name(1, 0), colony_name(1, 0));
        assert_eq!(colony_name(42, 3), colony_name(42, 3));
    }

    #[test]
    fn different_colonies_get_different_names() {
        let names: Vec<String> = (0..8).map(|c| colony_name(1, c)).collect();
        let mut uniq = names.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), names.len(), "colony names collided: {names:?}");
    }

    #[test]
    fn ant_name_is_deterministic_for_id() {
        assert_eq!(ant_name(1234), ant_name(1234));
        assert_ne!(ant_name(1), ant_name(2));
    }

    #[test]
    fn names_are_non_empty_and_ascii() {
        let n = colony_name(7, 5);
        assert!(!n.is_empty() && n.is_ascii());
        assert!(!ant_name(99).is_empty());
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Add `pub mod names;` to `lib.rs`. Run: `cargo test -p sim --release names`
Expected: FAIL — `cannot find function colony_name`.

- [ ] **Step 3: Implement**

Prepend to `crates/sim/src/names.rs`:

```rust
use crate::rng::Pcg32;

const ADJECTIVES: [&str; 16] = [
    "Amber", "Iron", "Crimson", "Ashen", "Golden", "Shadow", "Verdant", "Pale",
    "Obsidian", "Copper", "Silent", "Restless", "Hollow", "Bitter", "Wandering", "Gilded",
];
const NOUNS: [&str; 16] = [
    "Host", "Legion", "Marsh", "Warren", "Vale", "Reach", "Hollow", "Expanse",
    "Court", "Swarm", "Colony", "Dominion", "Nest", "Coil", "Drift", "Span",
];
const GIVEN: [&str; 32] = [
    "Ada", "Bramble", "Cyrus", "Dot", "Ember", "Fen", "Gale", "Hazel",
    "Ivo", "Juno", "Kestrel", "Lark", "Moss", "Nim", "Orin", "Pike",
    "Quill", "Rune", "Sable", "Thorn", "Umber", "Vex", "Wren", "Xan",
    "Yarrow", "Zephyr", "Bryn", "Cinder", "Dusk", "Flint", "Grove", "Hollis",
];

/// Deterministic from `(seed, colony)` so a seed always tells the same story and
/// save/load preserves it. Two-part "Adjective Noun".
pub fn colony_name(seed: u64, colony: u8) -> String {
    let mut r = Pcg32::new(seed ^ 0x9E37_79B9_7F4A_7C15, colony as u64 + 1);
    let a = ADJECTIVES[r.next_below(ADJECTIVES.len() as u32) as usize];
    let n = NOUNS[r.next_below(NOUNS.len() as u32) as usize];
    format!("the {a} {n}")
}

/// Deterministic from the ant's id. A given name plus the id's own number keeps
/// two ants with the same given name distinguishable.
pub fn ant_name(id: u64) -> String {
    let mut r = Pcg32::new(id ^ 0xD1B5_4A32_D192_ED03, 1);
    let g = GIVEN[r.next_below(GIVEN.len() as u32) as usize];
    format!("{g}-{}", id % 1000)
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p sim --release names`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/sim/src/names.rs crates/sim/src/lib.rs
git commit -m "feat(sim): deterministic colony and ant name generator"
```

---

## Task 2: Colony names on `ColonyState`, set at worldgen

**Files:**
- Modify: `crates/sim/src/colony.rs` (add `name` field)
- Modify: `crates/sim/src/worldgen.rs` (assign names, thread the seed in)
- Modify: `crates/sim/src/world.rs` (pass the seed to `generate`)

**Interfaces:**
- Consumes: `names::colony_name` (Task 1).
- Produces: `ColonyState.name: String`, populated by `worldgen::generate` from the world seed.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/sim/src/worldgen.rs`:

```rust
    #[test]
    fn colonies_are_named_deterministically_from_the_seed() {
        let c = cfg();
        let (_, a) = generate(&c, 1, &mut Pcg32::new(1, 1));
        let (_, b) = generate(&c, 1, &mut Pcg32::new(1, 1));
        assert!(!a[0].name.is_empty());
        assert_eq!(a[0].name, b[0].name);
        assert_ne!(a[0].name, a[1].name);
    }
```

Note the new `generate(&c, 1, &mut rng)` arity — `generate` gains a `seed`
parameter. Update every existing `generate(&c, &mut ...)` call in this test
module to `generate(&c, 1, &mut ...)`.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sim --release colonies_are_named`
Expected: FAIL — arity mismatch / `no field name`.

- [ ] **Step 3: Implement**

In `crates/sim/src/colony.rs`, add to `ColonyState`:

```rust
    /// Display name, generated deterministically from `(seed, id)` at worldgen.
    /// Empty only for a `ColonyState::new` built outside worldgen (tests).
    pub name: String,
```

and in `ColonyState::new`, add `name: String::new(),` to the struct literal.

In `crates/sim/src/worldgen.rs`, change the signature and name each colony:

```rust
pub fn generate(cfg: &Config, seed: u64, rng: &mut Pcg32) -> (Grid, Vec<ColonyState>) {
```

and where each `ColonyState::new(id)` is created (the `let mut col = ...` line):

```rust
        let mut col = ColonyState::new(id);
        col.name = crate::names::colony_name(seed, id);
```

In `crates/sim/src/world.rs`, find the `World::new` call to `worldgen::generate`
and pass the seed it already holds:

```rust
        let (grid, colonies) = worldgen::generate(cfg, seed, &mut rng);
```

(Adjust the local variable name to whatever `World::new` calls its seed.)

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p sim --release colonies_are_named worldgen`
Expected: PASS.

- [ ] **Step 5: Regenerate the golden master**

`ColonyState` gained a serialized field. No physics changed, so this re-pins the
same trajectory:

Run: `REGENERATE_GOLDEN=1 cargo test -p sim --release --test golden`
Then: `cargo test -p sim --release`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/sim/src/colony.rs crates/sim/src/worldgen.rs crates/sim/src/world.rs crates/sim/tests/golden_master.bin
git commit -m "feat(sim): name every colony at worldgen from the seed"
```

---

## Task 3: The `Chronicle` and its event registry

**Files:**
- Create: `crates/sim/src/chronicle.rs`
- Modify: `crates/sim/src/lib.rs` (`pub mod chronicle;`)
- Modify: `crates/sim/src/world.rs` (own a `Chronicle`, run detectors each tick)
- Modify: `crates/sim/src/colony.rs` (per-colony "first" flags)

**Interfaces:**
- Consumes: `names` (Task 1), `Ants`, `ColonyState`.
- Produces: `chronicle::EventKind` (u8-tagged enum), `chronicle::ChronicleEvent`, `chronicle::Chronicle` with `record(...)` and a `CAP`. `World.chronicle: Chronicle`. `ColonyState` gains `first_delivery_done: bool`, `first_kill_done: bool`.

The detector registry is a list of functions checked once per tick against the
post-apply world. Adding a milestone is one new function plus one `EventKind` —
this is the expandability the spec requires.

- [ ] **Step 1: Write the failing tests**

Create `crates/sim/src/chronicle.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_delivery_fires_once_and_names_the_ant() {
        let mut ch = Chronicle::new();
        let mut fired = false;
        ch.record(&mut fired, ChronicleEvent {
            tick: 10, colony: 2, kind: EventKind::FirstDelivery,
            ant_id: Some(7), ant_name: Some(crate::names::ant_name(7)),
            text: "brought the first food home".into(),
        });
        assert_eq!(ch.events.len(), 1);
        assert!(fired, "the one-shot flag must latch");
    }

    #[test]
    fn the_chronicle_is_capped_newest_kept() {
        let mut ch = Chronicle::new();
        let mut flag = false;
        for t in 0..(Chronicle::CAP as u64 + 50) {
            ch.record(&mut flag, ChronicleEvent {
                tick: t, colony: 0, kind: EventKind::PopulationMilestone,
                ant_id: None, ant_name: None, text: "grew".into(),
            });
            flag = false; // rolling events do not latch
        }
        assert_eq!(ch.events.len(), Chronicle::CAP);
        assert_eq!(ch.events.last().unwrap().tick, Chronicle::CAP as u64 + 49,
            "the newest event must be retained");
    }

    #[test]
    fn event_kind_round_trips_through_its_wire_byte() {
        for k in [EventKind::FirstDelivery, EventKind::FirstKill,
                  EventKind::FirstTrailFollow, EventKind::PopulationMilestone,
                  EventKind::EldestAnt, EventKind::TopForager] {
            assert_eq!(EventKind::from_u8(k as u8), Some(k));
        }
        assert_eq!(EventKind::from_u8(200), None);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Add `pub mod chronicle;` to `lib.rs`. Run: `cargo test -p sim --release chronicle`
Expected: FAIL — `cannot find type Chronicle`.

- [ ] **Step 3: Implement the data model**

Prepend to `crates/sim/src/chronicle.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    FirstDelivery = 0,
    FirstKill = 1,
    FirstTrailFollow = 2,
    PopulationMilestone = 3,
    EldestAnt = 4,
    TopForager = 5,
}

impl EventKind {
    pub fn from_u8(v: u8) -> Option<EventKind> {
        Some(match v {
            0 => EventKind::FirstDelivery,
            1 => EventKind::FirstKill,
            2 => EventKind::FirstTrailFollow,
            3 => EventKind::PopulationMilestone,
            4 => EventKind::EldestAnt,
            5 => EventKind::TopForager,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChronicleEvent {
    pub tick: u64,
    pub colony: u8,
    pub kind: EventKind,
    pub ant_id: Option<u64>,
    pub ant_name: Option<String>,
    pub text: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Chronicle {
    pub events: Vec<ChronicleEvent>,
}

impl Chronicle {
    /// Keep every permanent "first" is impractical unbounded, so cap the whole
    /// log and drop the oldest. Permanent firsts are rare and near the front of
    /// a run, so in practice they survive; the cap protects a very long session.
    pub const CAP: usize = 200;

    pub fn new() -> Self {
        Self::default()
    }

    /// Append an event. `latch` is the caller's one-shot flag for a "first":
    /// pass a rolling event a throwaway `&mut false`. Set here so the detector
    /// site stays a single call.
    pub fn record(&mut self, latch: &mut bool, ev: ChronicleEvent) {
        *latch = true;
        self.events.push(ev);
        if self.events.len() > Self::CAP {
            let overflow = self.events.len() - Self::CAP;
            self.events.drain(0..overflow);
        }
    }
}
```

- [ ] **Step 4: Run the model tests**

Run: `cargo test -p sim --release chronicle`
Expected: PASS.

- [ ] **Step 5: Wire detectors into the tick**

In `crates/sim/src/colony.rs`, add the one-shot flags to `ColonyState` and its
`new`:

```rust
    pub first_delivery_done: bool,
    pub first_kill_done: bool,
```
```rust
            first_delivery_done: false,
            first_kill_done: false,
```

In `crates/sim/src/world.rs`, add the field to `World`, init it in `World::new`
(`chronicle: Chronicle::new()`), and call a detector pass at the end of `tick()`,
after the death sweep and reproduction, so it sees the settled state:

```rust
        self.run_chronicle_detectors();
```

Add the detector method to `impl World` (uses only already-tracked state —
`ColonyState::first_delivery_done`, `deaths`, population, `delivered_total`):

```rust
    /// The event registry. Each block is one detector; add a milestone by adding
    /// a block. Runs in the serial phase, so it cannot perturb determinism.
    fn run_chronicle_detectors(&mut self) {
        let tick = self.tick_count;
        for ci in 0..self.colonies.len() {
            // FirstDelivery: the colony's store has been fed for the first time.
            if !self.colonies[ci].first_delivery_done
                && self.colonies[ci].delivered_total > 0.0
            {
                let cid = self.colonies[ci].id;
                // Attribute to the living ant of this colony with the most
                // delivered — the likely deliverer this tick.
                let who = (0..self.ants.len())
                    .filter(|&i| self.ants.alive[i] && self.ants.colony[i] == cid)
                    .max_by(|&a, &b| {
                        self.ants.food_delivered[a]
                            .total_cmp(&self.ants.food_delivered[b])
                    });
                let (ant_id, ant_name) = match who {
                    Some(i) => (
                        Some(self.ants.id[i]),
                        Some(crate::names::ant_name(self.ants.id[i])),
                    ),
                    None => (None, None),
                };
                let cname = self.colonies[ci].name.clone();
                let mut done = self.colonies[ci].first_delivery_done;
                self.chronicle.record(&mut done, crate::chronicle::ChronicleEvent {
                    tick,
                    colony: cid,
                    kind: crate::chronicle::EventKind::FirstDelivery,
                    ant_id,
                    ant_name,
                    text: format!("{cname}: first food carried home"),
                });
                self.colonies[ci].first_delivery_done = done;
            }
        }
    }
```

> `FirstKill`, `FirstTrailFollow`, `PopulationMilestone`, `EldestAnt`, and
> `TopForager` are added as further blocks in this method in Task 8 of this
> phase (below). This task ships `FirstDelivery` end to end first so the whole
> pipeline (sim → wire → UI) is proven before adding detectors.

- [ ] **Step 6: Run the sim suite and regenerate the golden master**

`World` and `ColonyState` gained serialized fields; regenerate:

Run: `REGENERATE_GOLDEN=1 cargo test -p sim --release --test golden`
Then: `cargo test -p sim --release`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/sim/src/chronicle.rs crates/sim/src/lib.rs crates/sim/src/world.rs crates/sim/src/colony.rs crates/sim/tests/golden_master.bin
git commit -m "feat(sim): chronicle with a FirstDelivery detector"
```

---

## Task 4: Encode ColonyMeta and Chronicle; append name to AntDetail

**Files:**
- Modify: `crates/server/src/protocol.rs`
- Modify: `crates/server/src/sim_thread.rs` (publish the two frames; put name into AntDetail)

**Interfaces:**
- Consumes: `World.chronicle`, `ColonyState.name`, `names::ant_name`.
- Produces: `encode_colony_meta(out, &World)`, `encode_chronicle(out, &World)`, and an extended `encode_ant_detail` whose `AntDetail` struct gains `name: &str`.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `crates/server/src/protocol.rs`:

```rust
    #[test]
    fn colony_meta_encodes_tag_count_and_a_name() {
        let w = World::new(&Config { num_colonies: 2, ..tiny() }, 1);
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
        let w = World::new(&tiny(), 1);
        let mut b = Vec::new();
        encode_chronicle(&mut b, &w);
        assert_eq!(b[0], TAG_CHRONICLE);
        // A fresh world has an empty chronicle.
        assert_eq!(u16::from_le_bytes([b[1], b[2]]), 0);
    }

    #[test]
    fn ant_detail_appends_a_length_prefixed_name() {
        let act = Activations { inputs: [0.0; sim::N_INPUTS], h1: [0.0; sim::N_HIDDEN1],
            h2: [0.0; sim::N_HIDDEN2], outputs: [0.0; sim::N_OUTPUTS] };
        let mut b = Vec::new();
        encode_ant_detail(&mut b, &AntDetail {
            id: 5, colony: 0, alive: true, x: 1.0, y: 2.0, heading: 0.0,
            energy: 1.0, max_energy: 1.0, size: 1.0, carrying: 0.0,
            food_delivered: 0.0, age: 0, lineage: 0,
            traits: [0.0; 8], act: &act, name: "Wren-5",
        });
        assert_eq!(b[ANT_DETAIL_LEN], 6, "name length byte follows the fixed body");
        assert_eq!(b.len(), ANT_DETAIL_LEN + 1 + 6);
    }
```

> `tiny()` is a small `Config` helper; if the test module lacks one, add
> `fn tiny() -> Config { Config { width: 16, height: 16, num_colonies: 2, ..Config::default() } }`.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p server colony_meta chronicle ant_detail_appends`
Expected: FAIL — missing consts/functions and `AntDetail` has no `name`.

- [ ] **Step 3: Implement**

In `crates/server/src/protocol.rs`, add the tags near the others:

```rust
pub const TAG_COLONY_META: u8 = 0x09;
pub const TAG_CHRONICLE: u8 = 0x0A;
```

Add a string helper next to the `put_*` helpers:

```rust
#[inline]
fn put_str_u8(b: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let n = bytes.len().min(255);
    put_u8(b, n as u8);
    b.extend_from_slice(&bytes[..n]);
}
```

Add the encoders (follow the `encode_hello` style — `out.clear()` first):

```rust
pub fn encode_colony_meta(out: &mut Vec<u8>, w: &World) {
    out.clear();
    put_u8(out, TAG_COLONY_META);
    put_u8(out, w.colonies.len() as u8);
    for c in &w.colonies {
        put_u8(out, c.id);
        put_str_u8(out, &c.name);
    }
}

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
```

Extend the `AntDetail` struct (in `protocol.rs`) with `pub name: &'a str,` (add a
lifetime to the struct if it does not already have one — it borrows `act`, so it
does). In `encode_ant_detail`, after the existing 421-byte body is written, append:

```rust
    put_str_u8(out, d.name);
```

- [ ] **Step 4: Populate the name in `sim_thread.rs`**

In `crates/server/src/sim_thread.rs`, both `AntDetail` construction sites (the
dead-ant path and the live path in `publish_detail`) need a `name`. Dead path:

```rust
                name: "",
```

Live path:

```rust
            name: &sim::names::ant_name(w.ants.id[i]),
```

> Later (Phase B1 Task 8 / storytelling) this becomes the ant's *stored* notable
> name when it has one; for now every selected ant shows its deterministic name,
> which is correct and cheap.

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test -p server colony_meta chronicle ant_detail_appends`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/server/src/protocol.rs crates/server/src/sim_thread.rs
git commit -m "feat(protocol): encode colony meta, chronicle, and ant name"
```

---

## Task 5: Publish the new frames from the sim thread

**Files:**
- Modify: `crates/server/src/sim_thread.rs` (add `colony_meta` and `chronicle` channels)
- Modify: `crates/server/src/ws.rs` (fan the two new receivers out to the socket)

**Interfaces:**
- Consumes: Task 4's encoders.
- Produces: `Handles.colony_meta` and `Handles.chronicle` `watch::Receiver<Frame>`; both seeded in `spawn` and refreshed — `colony_meta` on connect/reset/load, `chronicle` at the stats cadence.

- [ ] **Step 1: Add the channels**

In `crates/server/src/sim_thread.rs`, add to `Handles` and `Publishers`:

```rust
    pub colony_meta: watch::Receiver<Frame>,
    pub chronicle: watch::Receiver<Frame>,
```
```rust
    colony_meta: watch::Sender<Frame>,
    chronicle: watch::Sender<Frame>,
```

In `spawn`, seed them and add to the `Publishers`/`Handles` literals:

```rust
    let (colony_meta_tx, colony_meta) =
        watch::channel(frame(|b| protocol::encode_colony_meta(b, &world)));
    let (chronicle_tx, chronicle) =
        watch::channel(frame(|b| protocol::encode_chronicle(b, &world)));
```

Add `colony_meta: colony_meta_tx, chronicle: chronicle_tx,` to `Publishers { .. }`
and `colony_meta, chronicle,` to `Handles { .. }`.

Add a helper next to `publish_hello`:

```rust
fn publish_colony_meta(pubs: &Publishers, st: &State, buf: &mut Vec<u8>) {
    protocol::encode_colony_meta(buf, &st.world);
    let _ = pubs.colony_meta.send(Arc::new(buf.clone()));
}
```

In the stats-cadence block in `run`, publish the chronicle alongside stats:

```rust
            protocol::encode_chronicle(&mut buf, &st.world);
            let _ = pubs.chronicle.send(Arc::new(buf.clone()));
```

In `apply_command`, call `publish_colony_meta(pubs, st, buf);` in the `Load` and
`Reset` arms (right after `publish_config`), since names change with the world.

- [ ] **Step 2: Update the `Publishers`/`Handles` test doubles**

In the `tests` module of `sim_thread.rs`, the `pubs()` helper builds a
`Publishers` literal; add `colony_meta: watch::channel(empty()).0,` and
`chronicle: watch::channel(empty()).0,`. Extend
`every_frame_channel_is_populated_the_instant_spawn_returns` with:

```rust
        let cm = h.colony_meta.borrow().clone();
        assert_eq!(cm[0], protocol::TAG_COLONY_META);
        let ch = h.chronicle.borrow().clone();
        assert_eq!(ch[0], protocol::TAG_CHRONICLE);
```

- [ ] **Step 3: Fan out in `ws.rs`**

In `crates/server/src/ws.rs`, the per-connection send task selects over the watch
receivers. Add the two new receivers to that select loop exactly as `stats` is
handled — clone the frame and write it to the socket when the receiver changes.
Follow the existing `stats`/`terrain` arms verbatim, substituting `colony_meta`
and `chronicle`.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p server`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/server/src/sim_thread.rs crates/server/src/ws.rs
git commit -m "feat(server): publish colony-meta and chronicle frames"
```

---

## Task 6: Cross-language fixtures for the new frames

**Files:**
- Modify: `crates/server/tests/fixtures.rs` (emit `.bin` for the new frames)
- Modify: `web/src/protocol.ts` (decode `0x09`, `0x0A`, extended `0x05`)
- Modify: `web/tests/protocol.test.ts` (assert the decode)

**Interfaces:**
- Produces: `ColonyMeta`, `Chronicle`, `ChronicleEvent` decoded types in `protocol.ts`; the `AntDetail` decode reads the trailing name.

- [ ] **Step 1: Emit the fixtures**

In `crates/server/tests/fixtures.rs`, follow the existing pattern (build a tiny
world, call an encoder, write `crates/server/tests/fixtures/<name>.bin`). Add
`colony_meta.bin` and `chronicle.bin`. For a non-empty chronicle, tick the world
until `FirstDelivery` fires, or hand-insert one event via
`w.chronicle.record(&mut false, ChronicleEvent { .. })` before encoding, so the
fixture exercises a populated event.

Run: `cargo test -p server --test fixtures`
Expected: the two `.bin` files appear under `crates/server/tests/fixtures/`.

- [ ] **Step 2: Write the failing web test**

In `web/tests/protocol.test.ts`, load `colony_meta.bin` and `chronicle.bin` (same
way the suite loads existing fixtures) and assert: colony meta decodes to the
expected count and a non-empty first name; chronicle decodes to the expected
event count with the right `kind` and text. Run: `cd web && npm test`
Expected: FAIL — `decode` returns unknown for tag 0x09/0x0A.

- [ ] **Step 3: Implement the decoders in `protocol.ts`**

Add to `web/src/protocol.ts`, mirroring the Rust layout exactly (a `DataView`
with `littleEndian = true`, a `u8`-length string reader). Add `ColonyMeta` and
`Chronicle` to the decoded union and to the `decode()` switch:

```ts
export interface ColonyMeta {
  kind: "colonyMeta";
  colonies: { id: number; name: string }[];
}

export interface ChronicleEvent {
  tick: number;
  colony: number;
  eventKind: number;
  antId: number | null;
  antName: string | null;
  text: string;
}
export interface Chronicle { kind: "chronicle"; events: ChronicleEvent[]; }
```

Read a `u8`-prefixed UTF-8 string with a shared helper (add it near the other
readers):

```ts
function readStrU8(v: DataView, o: { p: number }): string {
  const n = v.getUint8(o.p); o.p += 1;
  const bytes = new Uint8Array(v.buffer, v.byteOffset + o.p, n);
  o.p += n;
  return new TextDecoder().decode(bytes);
}
```

Extend the existing `AntDetail` decode to read the trailing name after the fixed
body (`detail.name = readStrU8(view, cursor)`), defaulting to `""` when the frame
is exactly `ANT_DETAIL_LEN` (an old server).

- [ ] **Step 4: Run to verify it passes**

Run: `cd web && npm test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/server/tests/fixtures web/src/protocol.ts web/tests/protocol.test.ts
git commit -m "test(protocol): cross-language fixtures for colony meta and chronicle"
```

---

## Task 7: Store the new frames and render names + chronicle

**Files:**
- Modify: `web/src/net.ts` (dispatch the two new kinds)
- Modify: `web/src/state.ts` (hold colony meta + chronicle; expose colony name lookup)
- Modify: `web/src/ui/colony.ts` (show the colony name on each card)
- Modify: `web/src/ui/inspector.ts` (show the selected ant's name)
- Create: `web/src/ui/chronicle.ts` (the story log panel)
- Modify: `web/src/main.ts` (mount the chronicle panel)

**Interfaces:**
- Consumes: Task 6's decoded types.
- Produces: `Store` methods `applyColonyMeta`, `applyChronicle`, `colonyName(id)`; a mounted chronicle panel.

- [ ] **Step 1: Dispatch and store**

In `web/src/net.ts`, add cases to the `dispatch` switch:

```ts
      case "colonyMeta":
        this.store.applyColonyMeta(f);
        break;
      case "chronicle":
        this.store.applyChronicle(f);
        break;
```

In `web/src/state.ts`, add `colonyMeta` and `chronicle` to the state shape and to
the initial state, plus:

```ts
  applyColonyMeta(m: ColonyMeta): void {
    this.state.colonyMeta = m;
    this.notify();
  }
  applyChronicle(c: Chronicle): void {
    this.state.chronicle = c;
    this.notify();
  }
  colonyName(id: number): string {
    return this.state.colonyMeta?.colonies.find((c) => c.id === id)?.name ?? `colony ${id}`;
  }
```

- [ ] **Step 2: Show names on the colony cards and inspector**

In `web/src/ui/colony.ts`, replace the `colony N` card title with
`store.colonyName(id)` (keep the id available for the symbol in Phase B3).

In `web/src/ui/inspector.ts`, render `store.state.detail?.name` next to the ant
id when a name is present.

- [ ] **Step 3: The chronicle panel**

Create `web/src/ui/chronicle.ts`:

```ts
import type { Store } from "../state.js";

/** The story log: newest first, colored by colony. */
export function mountChronicle(root: HTMLElement, store: Store): void {
  const panel = document.createElement("div");
  panel.className = "chronicle";
  const h = document.createElement("h2");
  h.textContent = "Chronicle";
  root.append(h, panel);

  const render = () => {
    const evs = store.state.chronicle?.events ?? [];
    panel.innerHTML = "";
    for (const e of [...evs].reverse().slice(0, 60)) {
      const row = document.createElement("div");
      row.className = "chron-row";
      const t = document.createElement("span");
      t.className = "chron-tick";
      t.textContent = `t${e.tick.toLocaleString()}`;
      const txt = document.createElement("span");
      txt.textContent = e.antName ? `${e.text} — ${e.antName}` : e.text;
      row.append(t, txt);
      panel.append(row);
    }
  };
  store.subscribe(render);
  render();
}
```

Mount it in `web/src/main.ts` under the right rail (append a container and call
`mountChronicle`). Add minimal styles for `.chronicle`, `.chron-row`,
`.chron-tick` in the existing stylesheet (`web/index.html`'s inline CSS or the
project's CSS file), matching the colony-card look.

- [ ] **Step 4: Verify in the browser**

Build and run:

```bash
(cd web && npm run build)
cargo run -p server --release -- --web web/dist --seed 1
```

Open `http://127.0.0.1:8080`, run at 100x, and confirm: colony cards show
generated names; when a colony first delivers, a "first food carried home" line
appears in the Chronicle; clicking an ant shows its name in the inspector.

- [ ] **Step 5: Commit**

```bash
git add web/src/net.ts web/src/state.ts web/src/ui/colony.ts web/src/ui/inspector.ts web/src/ui/chronicle.ts web/src/main.ts web/index.html
git commit -m "feat(web): colony names, ant names, and the chronicle panel"
```

---

## Task 8: The remaining detectors

**Files:**
- Modify: `crates/sim/src/world.rs` (`run_chronicle_detectors`)
- Modify: `crates/sim/src/apply.rs` (flag a kill for `FirstKill`)
- Modify: `crates/sim/src/colony.rs` (population-milestone + rolling-title state)

**Interfaces:**
- Consumes: the Task 3 registry.
- Produces: `FirstKill`, `PopulationMilestone`, `EldestAnt`, `TopForager` events.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `crates/sim/src/world.rs`:

```rust
    #[test]
    fn population_milestone_fires_when_a_colony_first_reaches_ten() {
        // Drive a small rich world until some colony passes 10, then assert a
        // PopulationMilestone event exists for it. Uses a generous tick budget.
        let mut w = World::new(&Config {
            width: 96, height: 96, num_colonies: 2,
            initial_ants_per_colony: 12, // start above the milestone
            ..Config::default()
        }, 1);
        w.tick();
        assert!(w.chronicle.events.iter().any(|e|
            matches!(e.kind, crate::chronicle::EventKind::PopulationMilestone)),
            "no population milestone fired for a 12-ant colony");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sim --release population_milestone`
Expected: FAIL — no `PopulationMilestone` detector yet.

- [ ] **Step 3: Implement the detectors**

Add `MILESTONES: [u32; 4] = [10, 25, 50, 100]` handling and rolling titles to
`run_chronicle_detectors`. Track per-colony `next_milestone_idx: usize`,
`eldest_seen: u64`, and `best_forager_delivered: f32` on `ColonyState` (add
fields + `new` inits, all serialized). For each colony:

```rust
        // PopulationMilestone: crossed the next threshold.
        {
            let cid = self.colonies[ci].id;
            let pop = self.ants.population(cid);
            while self.colonies[ci].next_milestone_idx < MILESTONES.len()
                && pop >= MILESTONES[self.colonies[ci].next_milestone_idx]
            {
                let m = MILESTONES[self.colonies[ci].next_milestone_idx];
                let cname = self.colonies[ci].name.clone();
                self.colonies[ci].next_milestone_idx += 1;
                let mut flag = false;
                self.chronicle.record(&mut flag, crate::chronicle::ChronicleEvent {
                    tick, colony: cid,
                    kind: crate::chronicle::EventKind::PopulationMilestone,
                    ant_id: None, ant_name: None,
                    text: format!("{cname} reached {m} ants"),
                });
            }
        }
```

Add `FirstKill` next to `FirstDelivery`, gated on `first_kill_done`. Detect a
kill by observing `ColonyState.deaths`/an attack flag: set a per-world
`kill_happened_by: Option<(u8, u64)>` in `apply_combat` when a killing blow lands
(colony of the killer + killer id), consumed and cleared by the detector. Emit
`FirstKill` for the killer's colony the first time.

`EldestAnt` / `TopForager`: rolling titles — compare the current max age /
`food_delivered` in the colony against `eldest_seen` / `best_forager_delivered`;
emit only when the record is broken.

`FirstTrailFollow`: when `first_delivery_done` becomes true, additionally check
whether the deliverer stood on a food-trail cell above a threshold
(`phero.food[cell] > TRAIL_THRESHOLD`) at delivery — record a separate
`FirstTrailFollow` if so. (Requires threading the delivering cell; if that is
awkward, defer `FirstTrailFollow` to a follow-up and log it as deferred here.)

- [ ] **Step 4: Run to verify it passes, regenerate golden**

Run: `cargo test -p sim --release`
Then: `REGENERATE_GOLDEN=1 cargo test -p sim --release --test golden`
Then: `cargo test -p sim --release`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/sim/src crates/sim/tests/golden_master.bin
git commit -m "feat(sim): first-kill, population, and rolling-title detectors"
```

---

# Phase B2 — Right-click map editor

## Task 9: Map-edit commands (decode + World mutators)

**Files:**
- Modify: `crates/sim/src/world.rs` (public mutators)
- Modify: `crates/server/src/protocol.rs` (command tags, `Command` variants, decode)

**Interfaces:**
- Produces: `World::set_food(x, y, amount)`, `World::set_stone(x, y, solid)`, `World::spawn_ant_at(x, y, colony)`, `World::rename_colony(colony, name)`, `World::add_to_store(colony, amount)`; `Command::{SetFood, SetStone, SpawnAnt, RenameColony, AddToStore}`; matching decode.

- [ ] **Step 1: Write the failing tests (sim mutators)**

Add to the `tests` module in `crates/sim/src/world.rs`:

```rust
    #[test]
    fn set_food_writes_the_cell_and_clamps_out_of_bounds() {
        let mut w = World::new(&Config { width: 32, height: 32, ..Config::default() }, 1);
        w.set_food(5.0, 5.0, 123.0);
        let i = w.grid.idx(5, 5);
        assert_eq!(w.grid.food[i], 123.0);
        w.set_food(-9.0, -9.0, 1.0); // must not panic
    }

    #[test]
    fn add_to_store_credits_the_named_colony() {
        let mut w = World::new(&Config { num_colonies: 2, ..Config::default() }, 1);
        let before = w.colonies[1].store;
        w.add_to_store(1, 50.0);
        assert_eq!(w.colonies[1].store, before + 50.0);
    }

    #[test]
    fn spawn_ant_at_adds_one_living_ant_of_that_colony() {
        let mut w = World::new(&Config { width: 32, height: 32, num_colonies: 2, ..Config::default() }, 1);
        let before = w.ants.population(0);
        w.spawn_ant_at(4.0, 4.0, 0);
        assert_eq!(w.ants.population(0), before + 1);
        assert!(w.ants.id.windows(2).all(|s| s[0] < s[1]), "ids stay sorted");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sim --release set_food add_to_store spawn_ant_at`
Expected: FAIL — methods missing.

- [ ] **Step 3: Implement the mutators**

Add to `impl World` (they run outside `tick`, applied by the sim thread between
ticks; they mutate state deterministically and must keep the derived index
consistent):

```rust
    pub fn set_food(&mut self, x: f32, y: f32, amount: f32) {
        if let Some(i) = self.cell_index(x, y) {
            self.grid.food[i] = amount.max(0.0);
        }
    }

    pub fn set_stone(&mut self, x: f32, y: f32, solid: bool) {
        if let Some(i) = self.cell_index(x, y) {
            // A nest tile is never stone; refuse to bury one.
            if self.grid.nest[i] == crate::grid::NO_NEST {
                self.grid.stone[i] = solid;
                if solid { self.grid.food[i] = 0.0; }
            }
        }
    }

    pub fn add_to_store(&mut self, colony: u8, amount: f32) {
        if let Some(c) = self.colonies.get_mut(colony as usize) {
            c.store = (c.store + amount).max(0.0);
        }
    }

    pub fn rename_colony(&mut self, colony: u8, name: String) {
        if let Some(c) = self.colonies.get_mut(colony as usize) {
            c.name = name;
        }
    }

    pub fn spawn_ant_at(&mut self, x: f32, y: f32, colony: u8) {
        let Some(_) = self.cell_index(x, y) else { return };
        if colony as usize >= self.colonies.len() { return; }
        let genome = match self.colonies[colony as usize].archive_parent(&mut self.rng) {
            Some((g, _)) => g.clone(),
            None => crate::genome::Genome::random(&mut self.rng),
        };
        let id = self.next_ant_id;
        self.next_ant_id += 1;
        let energy = crate::reproduce::NEWBORN_ENERGY_FRAC
            * genome.max_energy(&self.cfg, crate::reproduce::NEWBORN_SIZE);
        self.ants.push(crate::ants::Spawn {
            id, colony, x, y, heading: 0.0, energy,
            size: crate::reproduce::NEWBORN_SIZE, lineage: 0, genome,
            birth_tick: self.tick_count,
        });
        self.rebuild_index();
    }

    fn cell_index(&self, x: f32, y: f32) -> Option<usize> {
        if !x.is_finite() || !y.is_finite() { return None; }
        if !self.grid.in_bounds(x as i32, y as i32) { return None; }
        Some(self.grid.idx(x as u16, y as u16))
    }
```

> Adjust `self.next_ant_id` and `self.rebuild_index()` to the actual field/method
> names `World` uses to allocate ant ids and refresh the spatial index (grep
> `next_id`/`rebuild` in `world.rs`). `push` requires strictly increasing ids, so
> use the world's id counter, never a literal.

- [ ] **Step 4: Add the command tags and decode**

In `crates/server/src/protocol.rs`, add:

```rust
pub const CMD_SET_FOOD: u8 = 0x0B;
pub const CMD_SET_STONE: u8 = 0x0C;
pub const CMD_SPAWN_ANT: u8 = 0x0D;
pub const CMD_RENAME_COLONY: u8 = 0x0E;
pub const CMD_ADD_TO_STORE: u8 = 0x0F;
```

Add variants to `enum Command` (note `RenameColony` owns a `String`, so `Command`
can no longer be `Copy` — drop `Copy` from its derive, keep `Clone`):

```rust
    SetFood(f32, f32, f32),
    SetStone(f32, f32, bool),
    SpawnAnt(f32, f32, u8),
    RenameColony(u8, String),
    AddToStore(u8, f32),
```

Add decode arms (reuse the finite-check pattern from `SelectAt`):

```rust
        CMD_SET_FOOD => {
            let x = f32::from_le_bytes(rest.get(0..4)?.try_into().ok()?);
            let y = f32::from_le_bytes(rest.get(4..8)?.try_into().ok()?);
            let a = f32::from_le_bytes(rest.get(8..12)?.try_into().ok()?);
            if !(x.is_finite() && y.is_finite() && a.is_finite()) { return None; }
            Command::SetFood(x, y, a)
        }
        CMD_SET_STONE => {
            let x = f32::from_le_bytes(rest.get(0..4)?.try_into().ok()?);
            let y = f32::from_le_bytes(rest.get(4..8)?.try_into().ok()?);
            if !(x.is_finite() && y.is_finite()) { return None; }
            Command::SetStone(x, y, *rest.get(8)? != 0)
        }
        CMD_SPAWN_ANT => {
            let x = f32::from_le_bytes(rest.get(0..4)?.try_into().ok()?);
            let y = f32::from_le_bytes(rest.get(4..8)?.try_into().ok()?);
            if !(x.is_finite() && y.is_finite()) { return None; }
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
            if !a.is_finite() { return None; }
            Command::AddToStore(colony, a)
        }
```

Any existing `match cmd` on `Command` that relied on `Copy` (e.g. a test using
`assert_eq!` on a decoded value) still works via `Clone`/`PartialEq`; add
`#[derive(PartialEq)]` retention — `Command` keeps `Clone, Debug, PartialEq`.

- [ ] **Step 5: Write the failing decode test**

Add to the `tests` module in `protocol.rs`:

```rust
    #[test]
    fn decodes_the_map_edit_commands() {
        let mut b = vec![CMD_SET_FOOD];
        b.extend_from_slice(&1.0f32.to_le_bytes());
        b.extend_from_slice(&2.0f32.to_le_bytes());
        b.extend_from_slice(&50.0f32.to_le_bytes());
        assert_eq!(decode_command(&b), Some(Command::SetFood(1.0, 2.0, 50.0)));

        let mut r = vec![CMD_RENAME_COLONY, 3, 4];
        r.extend_from_slice(b"Ants");
        assert_eq!(decode_command(&r), Some(Command::RenameColony(3, "Ants".into())));

        assert_eq!(decode_command(&[CMD_ADD_TO_STORE, 0]), None); // truncated
    }
```

- [ ] **Step 6: Run to verify it passes**

Run: `cargo test -p sim --release; cargo test -p server decodes_the_map_edit`
Expected: PASS. Regenerate the golden master if `World` gained a serialized field
(`next_ant_id` may already exist; if you added one, regenerate).

- [ ] **Step 7: Commit**

```bash
git add crates/sim/src/world.rs crates/server/src/protocol.rs crates/sim/tests/golden_master.bin
git commit -m "feat: map-edit commands and World mutators"
```

---

## Task 10: Apply map-edit commands in the sim thread

**Files:**
- Modify: `crates/server/src/sim_thread.rs` (`apply_command`)

**Interfaces:**
- Consumes: Task 9's `Command` variants and `World` mutators.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `sim_thread.rs`:

```rust
    #[test]
    fn a_set_food_command_reaches_the_world() {
        let mut st = state();
        let mut buf = Vec::new();
        apply_command(&mut st, Command::SetFood(4.0, 4.0, 77.0), &pubs(), &mut buf);
        let i = st.world.grid.idx(4, 4);
        assert_eq!(st.world.grid.food[i], 77.0);
    }

    #[test]
    fn a_rename_reaches_the_colony_and_republishes_meta() {
        let mut st = state();
        let mut buf = Vec::new();
        apply_command(&mut st, Command::RenameColony(0, "Test Host".into()), &pubs(), &mut buf);
        assert_eq!(st.world.colonies[0].name, "Test Host");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p server a_set_food_command a_rename_reaches`
Expected: FAIL — non-exhaustive `match cmd`.

- [ ] **Step 3: Implement**

In `apply_command`, add arms (rename republishes colony meta so the UI updates):

```rust
        Command::SetFood(x, y, a) => st.world.set_food(x, y, a),
        Command::SetStone(x, y, solid) => st.world.set_stone(x, y, solid),
        Command::SpawnAnt(x, y, colony) => st.world.spawn_ant_at(x, y, colony),
        Command::AddToStore(colony, a) => st.world.add_to_store(colony, a),
        Command::RenameColony(colony, name) => {
            st.world.rename_colony(colony, name);
            publish_colony_meta(pubs, st, buf);
        }
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p server`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/server/src/sim_thread.rs
git commit -m "feat(server): apply map-edit commands on the sim thread"
```

---

## Task 11: Command encoders and the context menu (web)

**Files:**
- Modify: `web/src/protocol.ts` (command encoders)
- Create: `web/src/ui/contextmenu.ts`
- Modify: `web/src/main.ts` (wire `contextmenu` event)

**Interfaces:**
- Produces: `cmdSetFood`, `cmdSetStone`, `cmdSpawnAnt`, `cmdRenameColony`, `cmdAddToStore` byte encoders; a context menu opened on right-click whose items depend on what is under the cursor.

- [ ] **Step 1: Command encoders + test**

In `web/src/protocol.ts`, mirror the command tags and add encoders (follow the
existing `cmdReset`/`cmdSetConfig` pattern that writes a `Uint8Array`). Add to
`web/tests/protocol.test.ts` a round-trip length/þbyte check for `cmdSetFood`
(13 bytes: tag + 3×f32) and `cmdRenameColony` (tag + colony + len + bytes). Run
`cd web && npm test` — expect FAIL, then implement, then PASS.

```ts
export function cmdSetFood(x: number, y: number, amount: number): Uint8Array {
  const b = new Uint8Array(13);
  const v = new DataView(b.buffer);
  b[0] = 0x0b;
  v.setFloat32(1, x, true); v.setFloat32(5, y, true); v.setFloat32(9, amount, true);
  return b;
}
// cmdSetStone (0x0c): tag + f32 x + f32 y + u8 solid  (10 bytes)
// cmdSpawnAnt (0x0d): tag + f32 x + f32 y + u8 colony (10 bytes)
// cmdAddToStore (0x0f): tag + u8 colony + f32 amount   (6 bytes)
export function cmdRenameColony(colony: number, name: string): Uint8Array {
  const bytes = new TextEncoder().encode(name).slice(0, 255);
  const b = new Uint8Array(2 + bytes.length);
  b[0] = 0x0e; b[1] = colony; b.set(bytes, 2);
  return b;
}
```

- [ ] **Step 2: The context menu component**

Create `web/src/ui/contextmenu.ts`: a function `openContextMenu(x, y, items)`
that positions an absolutely-placed `<ul>` at screen `(x, y)`, renders one `<li>`
per item `{ label, onClick }`, closes on outside-click or `Escape`, and supports
an inline editor row (a number input + apply button) for "Set food amount" and
"Add to store" and a text input for "Rename". Keep it dependency-free DOM, in the
style of `controls.ts`.

- [ ] **Step 3: Wire the right-click in `main.ts`**

In `web/src/main.ts` `attachPointer`, add:

```ts
  canvas.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    const rect = canvas.getBoundingClientRect();
    const px = (e.clientX - rect.left) * r.dpr;
    const py = (e.clientY - rect.top) * r.dpr;
    const w = r.camera.screenToWorld(px, py, r.viewW, r.viewH);
    const target = hitTest(w, store); // ant | food | nest | stone | dirt
    openContextMenu(e.clientX, e.clientY, menuItemsFor(target, w, store, net));
  });
```

Implement `hitTest(worldPos, store)` locally in `main.ts`: nearest ant within a
small radius from the latest `ants` frame → `ant`; else read the latest `terrain`
frame's cell → `nest` / `stone` / `food` / `dirt`. `menuItemsFor` builds the
item list per the spec table (Section 3) and each item sends the matching command
(`net.send(cmdSetFood(...))`, etc.); the "Inspect" item sends `cmdSelectAt` (the
existing selection path).

- [ ] **Step 4: Verify in the browser**

Build, run, right-click food → set amount → watch the terrain layer update;
right-click dirt → spawn ant → watch population rise; right-click a nest → rename
→ watch the card and label update; right-click an ant → Inspect → the NN view
opens.

- [ ] **Step 5: Commit**

```bash
git add web/src/protocol.ts web/src/ui/contextmenu.ts web/src/main.ts web/tests/protocol.test.ts
git commit -m "feat(web): right-click context menu and map-edit commands"
```

---

# Phase B3 — Labels and colorblind symbols

## Task 12: Colony symbol glyphs

**Files:**
- Create: `web/src/symbols.ts`
- Modify: `web/src/ui/colony.ts` (draw the glyph on each card)
- Test: `web/tests/symbols.test.ts`

**Interfaces:**
- Produces: `symbolFor(colonyId: number): SymbolShape` (8 distinct shapes) and `drawSymbol(ctx, shape, x, y, size, color)` for canvas, plus `symbolSvg(shape, color)` for DOM cards.

- [ ] **Step 1: Write the failing test**

Create `web/tests/symbols.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { symbolFor, SHAPES } from "../src/symbols.js";

describe("colony symbols", () => {
  it("maps each of 8 colonies to a distinct shape", () => {
    const shapes = new Set(Array.from({ length: 8 }, (_, i) => symbolFor(i)));
    expect(shapes.size).toBe(8);
  });
  it("wraps past the shape count without throwing", () => {
    expect(symbolFor(9)).toBe(SHAPES[9 % SHAPES.length]);
  });
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd web && npm test symbols`
Expected: FAIL — module missing.

- [ ] **Step 3: Implement**

Create `web/src/symbols.ts`:

```ts
export const SHAPES = [
  "circle", "triangle", "square", "diamond", "plus", "star", "hexagon", "cross",
] as const;
export type SymbolShape = (typeof SHAPES)[number];

export function symbolFor(colonyId: number): SymbolShape {
  return SHAPES[colonyId % SHAPES.length];
}

/** Draw a filled glyph centered at (x, y). Canvas — used on the map + cards. */
export function drawSymbol(
  ctx: CanvasRenderingContext2D, shape: SymbolShape,
  x: number, y: number, r: number, color: string,
): void {
  ctx.fillStyle = color;
  ctx.strokeStyle = color;
  ctx.lineWidth = Math.max(1, r * 0.35);
  ctx.beginPath();
  switch (shape) {
    case "circle": ctx.arc(x, y, r, 0, Math.PI * 2); ctx.fill(); break;
    case "square": ctx.fillRect(x - r, y - r, 2 * r, 2 * r); break;
    case "triangle":
      ctx.moveTo(x, y - r); ctx.lineTo(x + r, y + r); ctx.lineTo(x - r, y + r);
      ctx.closePath(); ctx.fill(); break;
    case "diamond":
      ctx.moveTo(x, y - r); ctx.lineTo(x + r, y); ctx.lineTo(x, y + r);
      ctx.lineTo(x - r, y); ctx.closePath(); ctx.fill(); break;
    case "plus":
      ctx.fillRect(x - r * 0.35, y - r, r * 0.7, 2 * r);
      ctx.fillRect(x - r, y - r * 0.35, 2 * r, r * 0.7); break;
    case "cross":
      ctx.moveTo(x - r, y - r); ctx.lineTo(x + r, y + r);
      ctx.moveTo(x + r, y - r); ctx.lineTo(x - r, y + r); ctx.stroke(); break;
    case "hexagon":
      for (let k = 0; k < 6; k++) {
        const a = (Math.PI / 3) * k - Math.PI / 6;
        const px = x + r * Math.cos(a), py = y + r * Math.sin(a);
        k === 0 ? ctx.moveTo(px, py) : ctx.lineTo(px, py);
      }
      ctx.closePath(); ctx.fill(); break;
    case "star":
      for (let k = 0; k < 10; k++) {
        const rr = k % 2 === 0 ? r : r * 0.45;
        const a = (Math.PI / 5) * k - Math.PI / 2;
        const px = x + rr * Math.cos(a), py = y + rr * Math.sin(a);
        k === 0 ? ctx.moveTo(px, py) : ctx.lineTo(px, py);
      }
      ctx.closePath(); ctx.fill(); break;
  }
}
```

- [ ] **Step 4: Show the glyph on colony cards**

In `web/src/ui/colony.ts`, prepend a small `<canvas>` (or reuse `drawSymbol` onto
one) to each card header, colored by the existing colony color. Verify the test
passes: `cd web && npm test symbols`.

- [ ] **Step 5: Commit**

```bash
git add web/src/symbols.ts web/src/ui/colony.ts web/tests/symbols.test.ts
git commit -m "feat(web): distinct per-colony symbol glyphs for colorblind clarity"
```

---

## Task 13: Map label overlay

**Files:**
- Create: `web/src/ui/labels.ts`
- Modify: `web/src/main.ts` (update labels each frame; add a Labels toggle)
- Modify: `web/src/state.ts` (a `labels: boolean` flag + toggle)
- Modify: `web/src/ui/controls.ts` (the toggle checkbox)

**Interfaces:**
- Consumes: camera (world→screen), `store` (terrain, colony meta, selection), `symbols`.
- Produces: a DOM overlay that draws nest labels (symbol + name), food-patch labels ("Food"), and the selected ant's name, with right-then-down collision avoidance and a zoom-fade.

- [ ] **Step 1: Implement the overlay**

Create `web/src/ui/labels.ts` exporting `class LabelOverlay` with
`update(camera, viewW, viewH, store)` called each frame from the render loop.
It positions absolutely-placed `<div>` labels over the canvas:

- **Nests:** one per colony at its nest center (from `store.colonyMeta` +
  nest cells in the latest terrain frame, or the colony's known nest center),
  showing the symbol glyph + `store.colonyName(id)`.
- **Food patches:** cluster the terrain frame's food channel into connected
  blobs (a cheap flood-fill over downsampled cells) and label each centroid
  "Food".
- **Selected ant:** the ant's name at its screen position.

Layout: place each label at its anchor; if its rect overlaps an already-placed
label this frame, shift it right by its width, then (if still colliding) down by
its height — the requested left-to-right, then up-down fallback. Below a zoom
threshold (`camera.zoom < LABEL_MIN_ZOOM`), hide all labels (set container
`display:none`). Reuse label DOM nodes across frames keyed by a stable id to
avoid per-frame churn.

- [ ] **Step 2: Toggle + wiring**

Add `labels: boolean` (default true) to `state.ts` with a `toggleLabels()`; add a
"labels" checkbox to the Layers group in `controls.ts`; in `main.ts`'s `frame()`,
call `labelOverlay.update(...)` when `store.state.labels`, else hide it.

- [ ] **Step 3: Verify in the browser**

Run and confirm: nests show name + glyph; food patches show "Food"; labels nudge
apart instead of overlapping; zooming out hides them; the Labels toggle works.

- [ ] **Step 4: Commit**

```bash
git add web/src/ui/labels.ts web/src/main.ts web/src/state.ts web/src/ui/controls.ts
git commit -m "feat(web): on-map labels for nests, food, and the selected ant"
```

---

# Phase B4 — Collapsible tuning menu

## Task 14: Put the tuning sliders behind a collapsible section

**Files:**
- Modify: `web/src/ui/controls.ts`
- Modify: `web/index.html` (a little CSS for the collapsed state)

**Interfaces:**
- Produces: the "Tuning" section header toggles the slider stack; collapsed by default.

- [ ] **Step 1: Implement**

In `web/src/ui/controls.ts`, wrap the tuning sliders (the `sliders.forEach((s) =>
root.append(s.wrap))` block) in a container `<div class="tuning-body">` and make
the `section("Tuning")` header a button that toggles a `collapsed` class on that
container. Start collapsed:

```ts
  const tuningHead = document.createElement("button");
  tuningHead.className = "section-toggle";
  tuningHead.textContent = "Tuning ▸";
  const tuningBody = div("tuning-body collapsed");
  tuningHead.addEventListener("click", () => {
    const open = tuningBody.classList.toggle("collapsed") === false;
    tuningHead.textContent = open ? "Tuning ▾" : "Tuning ▸";
  });
  root.append(tuningHead);
  sliders.forEach((s) => tuningBody.append(s.wrap));
  root.append(tuningBody);
```

Add CSS: `.tuning-body.collapsed { display: none; }` and a button style matching
the existing section headers.

- [ ] **Step 2: Verify in the browser**

Run and confirm the rail is short by default (Playback, Layers, World visible;
Tuning collapsed), the save/load/reset row is no longer pushed off-screen, and
clicking "Tuning ▸" reveals the sliders — including the `harvest weight` slider
from Plan A.

- [ ] **Step 3: Commit**

```bash
git add web/src/ui/controls.ts web/index.html
git commit -m "feat(web): collapse the tuning sliders behind a section toggle"
```

---

## Final: full verification and docs

- [ ] **Step 1: Everything green**

```bash
cargo test --workspace --release
(cd web && npm test)
```
Expected: PASS everywhere, including the regenerated golden master and the new
cross-language fixtures.

- [ ] **Step 2: End-to-end browser smoke**

Build and run; confirm the whole story surface: named colonies with distinct
glyphs, on-map labels, a chronicle that fills with firsts as colonies grow, the
right-click editor across every target type, and the collapsed tuning menu.

- [ ] **Step 3: Update the README**

Document the new UI (context-menu editing, labels, symbols, chronicle, collapsed
tuning) in the "Running it" section, and add a short "Telling a story" paragraph.

```bash
git add README.md
git commit -m "docs: story, editable map, labels, and symbols in the README"
```

---

## Self-Review Notes

- **Spec coverage:** Section 2 names+chronicle (Tasks 1–8), Section 3 map editor (Tasks 9–11), Section 4 labels+symbols (Tasks 12–13), Section 5 tuning menu (Task 14). Every new frame (`0x09`, `0x0A`, extended `0x05`) has a cross-language fixture (Task 6). Every map-edit command has a decode test (Task 9) and a sim-thread apply test (Task 10). Determinism preserved: names/chronicle/detector flags written only in the serial phase; map edits applied between ticks with `rebuild_index`. `delivered_total` wire stat untouched.
- **Type consistency:** `generate(cfg, seed, rng)` (Task 2) is called with the new arity in `world.rs` and worldgen tests. `Command` loses `Copy` (Task 9) because `RenameColony(String)`; no code relies on `Copy` (verify by compiling — the tests use `Clone`/`PartialEq`). `AntDetail` gains `name: &str` (Task 4), supplied at both construction sites in `sim_thread.rs`. `encode_colony_meta`/`encode_chronicle` mirror `ColonyMeta`/`Chronicle` in `protocol.ts` (Task 6).
- **Deferred, logged not silent:** `FirstTrailFollow` may be deferred in Task 8 if threading the delivery cell is awkward — the task says to log it as deferred, not drop it silently.
- **Ordering:** Phase B1 ships `FirstDelivery` end-to-end (Tasks 1–7) before adding the other detectors (Task 8), so the sim→wire→UI pipeline is proven on one event before breadth is added.
