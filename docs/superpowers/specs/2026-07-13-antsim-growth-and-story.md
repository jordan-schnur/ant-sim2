# antsim2 — Growth, Story, and the Editable Map — Design

**Date:** 2026-07-13
**Status:** Approved (design); implementation plan to follow.
**Prior specs:** [`2026-07-09-antsim-design.md`](2026-07-09-antsim-design.md)
**Prior plans:** sim-core and server-web (both complete).

## Goal

Make colonies actually grow so the simulation tells a story, and give the
operator the tools to watch and shape that story:

1. **Shaped fitness** so evolution climbs from *harvest* to *deliver* instead of
   failing at a sparse-reward cliff. This is what unsticks colonies from the
   5-ant extinction floor.
2. **Names and a chronicle of firsts** — colony names, names for notable ants,
   and an expandable event log ("first to bring food home", "first to kill").
3. **A right-click context menu** that edits the map (food, stone, ants, colony)
   and inspects ants.
4. **Map labels** and **colorblind-friendly colony symbols**.
5. **Tuning sliders behind a collapsible menu** so the rail stays short.

## The problem this fixes (diagnosis)

The economy is closed and its only inflow is delivery:

- Ants gain energy exactly one way that lasts: refuelling at their own nest,
  which **draws down the colony food store** (`apply.rs:112`, `taken =
  want.min(colony.store)`).
- The store is filled **only** by food delivered home (`apply.rs:104`).
- A paid birth costs `birth_cost` (default 40) out of that store; the only other
  birth path is the free "extinction floor" trickle, rate-limited to one ant per
  `floor_respawn_interval` and capped at `extinction_floor` (default 5).

So a colony that never delivers has a store that only drains, never funds a paid
birth, and sits pinned at 5 ants forever. "15–20 generations, still 5 ants" is
the exact signature. The generation counter still advances because floor spawns
inherit their archived parent's lineage depth.

The reason delivery never starts is that fitness is **only** food delivered
home, and a random generation-0 brain must chain *leave nest → find food → grab
→ turn around → carry home → drop on nest* before it scores a single point. That
is a needle in a haystack; every early ant scores 0.0 and selection is blind.

## Intentional thesis change

The founding spec and README state in several places that **food delivered home
is the only fitness signal**. This design **deliberately changes that.** It is a
chosen pivot from "does foraging emerge from nothing" toward "the sim reliably
grows so it can be watched and narrated." The change is gated behind a tunable
weight that defaults to *on* but can be set to zero to recover the original
thesis exactly. The README and the original design note will be updated to
record this as intentional, not drift.

---

## Section 1 — Shaped fitness

### Mechanism

Replace the single `food_delivered` selection weight with a `fitness()` value
used everywhere a genome is ranked (living-parent roulette in
`ColonyState::select_parent`, and hall-of-fame ranking in
`ColonyState::record_death`):

```
fitness = food_delivered + harvest_weight · food_harvested
```

- `food_delivered` keeps weight **1.0** — it is the unit and the real goal.
- `food_harvested` is a **new per-ant lifetime accumulator**: total food grabbed
  into cargo over the ant's life (summed at `apply_food`, the `intent.grab`
  branch, `apply.rs:85`). It is credited when food enters `carrying`, whether or
  not it is ever delivered.
- `harvest_weight` is a **new tunable `Config` field** (default small — see
  calibration). `harvest_weight = 0` reproduces the original delivery-only
  behavior bit-for-bit.

### Why `food_harvested` and not longevity

Grabbing food already requires leaving the nest, navigating onto a food tile,
and firing "grab" — roughly the whole foraging loop except the final "carry it
home". It is therefore a **dense, honest gradient toward delivery**.

Longevity/age is explicitly **rejected** as a fitness term: because ants refuel
by draining the shared store, a long-lived non-forager is a *parasite* on its
colony. Rewarding survival-for-its-own-sake would select for nest-campers that
drain the store and deliver nothing — the opposite of what we want.

### Anti-reward-hacking calibration

`harvest_weight` must be small enough that **any real delivery outweighs a whole
lifetime of harvesting-without-delivering**, so evolution is nudged, not
diverted:

- A busy forager might harvest hundreds of food units over its life while
  delivering tens. To keep delivery dominant, the default `harvest_weight` is set
  so that a lifetime of pure harvesting (no delivery) scores below a single
  successful delivery run. Concretely we start at **`harvest_weight = 0.02`**
  (≈ one delivered unit is worth ~50 harvested-but-not-delivered units) and tune
  during implementation against a real run. The exact number is a starting
  guess, not a tuned equilibrium; the slider exists precisely to sweep it.

Once a lineage cracks delivery, `food_delivered` dominates and the harvest term
fades into noise — the shaped gradient is a scaffold that removes itself.

### What stays unchanged

- `delivered_total` (the wire stat and the colony charts) still tracks **only**
  delivery, so the operator's "is it evolving" curve measures the real
  objective, never the shaped proxy.
- Gene pools still never mix; parent selection is still per-colony roulette; the
  hall-of-fame drift fix is preserved (ties still displace, so a flat archive
  still drifts).

### Cost and invariants

- New SoA column `food_harvested: Vec<f32>` in `Ants`, serialized like every
  other column → **regenerates the golden-master fixture** (documented pattern,
  done four times before).
- Determinism preserved: the new accumulator is written only in the serial apply
  phase, never read during the parallel think phase.

### Secondary lever (noted, not default)

Even with good foragers, growth speed is bounded by `birth_cost` and
`refuel_rate`, which are already on sliders. Defaults are left alone; the
operator can retune them live. No new economy default is changed by this design.

---

## Section 2 — Names and the chronicle of firsts

All of this lives in the **sim** (deterministic, serialized, survives save/load),
not the client, so a seed reproduces the same story and it can be retold.

### a) Colony names

A deterministic generator produces one name per colony from `(seed, colony_id)`.
Same seed ⇒ same names; reset re-derives; save/load preserves. Stored on
`ColonyState` as `name: String`. Evocative style (e.g. "the Amber Host",
"Blackmarsh"). Shown on nest labels, colony cards, chronicle entries, inspector.

### b) Notable-ant names

Ants are anonymous until a chronicle detector marks one notable, at which point
it is assigned a deterministic name (from its `id`) stored on the ant
(`Ants::name: Vec<Option<String>>`, or an id→name side map on the world). Shown
in the inspector and in the chronicle. Most ants never get a name; that is the
point.

### c) The chronicle — an expandable event registry

A `Chronicle` on the `World` holds an append-only `Vec<ChronicleEvent>`:

```
struct ChronicleEvent {
    tick: u64,
    colony: u8,
    ant_id: Option<u64>,   // the responsible ant, if any
    ant_name: Option<String>,
    kind: EventKind,
    text: String,          // rendered, human-readable
}
```

Events are produced by a **registry of detectors**, each a small rule checked
once per tick (or on the relevant apply event) against sim state. Adding a new
milestone is **one new detector** — this is the expandability requested.

Initial detectors:

- `FirstDelivery` — first ant of a colony ever to bank food. Names that ant.
- `FirstKill` — first ant of a colony ever to land a killing blow. Names it.
- `FirstTrailFollow` — first delivery made while standing on an existing food
  trail above a threshold (distinguishes trail-following from lone discovery).
- `PopulationMilestone` — colony first reaches 10 / 25 / 50 / 100 ants.
- `EldestAnt` / `TopForager` — rolling titles reassigned when beaten (emit an
  event only on change, not every tick).

Detector state that must persist (per-colony "has FirstDelivery fired yet")
lives on `ColonyState` so it serializes and cannot re-fire after load.

The chronicle is **capped** (e.g. keep all permanent "firsts" plus the most
recent N rolling events) so it cannot grow without bound; the cap is logged, not
silent.

### Wire additions (server → client)

- **`0x09` ColonyMeta** — sent on connect and after reset/load. Per colony: id +
  name (length-prefixed UTF-8). Colony *symbols* are derived client-side from id
  (see Section 4), so they are not on the wire.
- **`0x0A` Chronicle** — the event log. Sent as a snapshot on connect, then
  incrementally as events fire. Each entry: tick, colony, optional ant id, kind,
  length-prefixed text (and ant name when present).
- **AntDetail `0x05`** gains the ant's name (length-prefixed, may be empty). This
  changes its fixed layout → fixtures regenerate.

---

## Section 3 — Right-click context menu + map editing

A right-click resolves what is under the cursor server-side (reusing the
nearest-thing logic) and opens a context menu. **Full set** in scope:

| Target | Menu items |
| --- | --- |
| Ant | **Inspect** (opens inspector + neural-net view — the same action as left-click, now also on right-click) |
| Food tile / patch | **Set food amount…** (inline slider/number → sets standing food) |
| Empty dirt | **Add food here**, **Place stone**, **Spawn ant ▸** (colony submenu) |
| Stone | **Remove stone** |
| Nest | **Rename colony…**, **Add to food store…** |

Map edits are **new client → server commands** that mutate the `World` on the sim
thread, exactly like reset/load — explicit operator interventions that perturb
the run from that point on (acceptable and expected, same as reset):

- **`0x0B` SetFood** — `f32 x, f32 y, f32 amount` (sets standing food at the cell)
- **`0x0C` SetStone** — `f32 x, f32 y, u8 solid` (place/remove stone). Placing
  stone under a standing ant is allowed; the existing collision rules govern its
  next move, so no special "un-wall" handling is needed.
- **`0x0D` SpawnAnt** — `f32 x, f32 y, u8 colony` (spawns one ant of that colony
  at the cell, random genome or archive-drawn — archive-drawn if available)
- **`0x0E` RenameColony** — `u8 colony, name` (length-prefixed UTF-8)
- **`0x0F` AddToStore** — `u8 colony, f32 amount`

All validate and clamp; a malformed edit is logged and dropped, never panics the
sim (same contract as existing commands). "Inspect" reuses the existing
`SelectAt` command — no new tag.

### Client

A small context-menu component (`web/src/ui/contextmenu.ts`) opens on
`contextmenu` event, positioned at the cursor, its items chosen from what the
server reports (or from a local hit-test against the latest terrain/ant frames).
Editing widgets (food amount, rename) are inline in the menu.

---

## Section 4 — Labels and colorblind colony symbols

### Labels (Prison Architect style)

A **DOM overlay** layer over the canvas, anchored to world coordinates and moved
by the camera transform each frame (crisp text; canvas2d text blurs under zoom).
Labeled entities:

- **Nests** — colony symbol + name
- **Food patches** — "Food" (one label per patch, computed by clustering food
  cells, not one per cell)
- **Notable / selected ants** — the ant's name

Layout: horizontal text; on collision a label nudges **right first, then stacks
downward** (the requested left-to-right, up-and-down fallback). Labels fade out
below a zoom threshold to avoid clutter, and there is a **Labels** toggle in the
left rail next to the layer toggles.

### Colorblind colony symbols (shapez 2 style)

Each colony gets a **distinct shape glyph** derived from its id — circle,
triangle, square, diamond, plus, star, hexagon, cross (8 shapes for the default 8
colonies). The glyph is drawn **alongside the color** everywhere identity
matters:

- Colony cards (right rail)
- Nest labels (Section 4 labels)
- Chronicle entries

Swarming ants stay color-only (a glyph on a ~3px dot is invisible); the nest and
card glyphs are the reliable anchor. Stamping the glyph on ants at high zoom is a
possible later addition, out of scope here.

Symbols are a pure client concern, derived from colony id — nothing on the wire.

---

## Section 5 — Tuning behind a collapsible menu

The Tuning slider stack — now including the new **`harvest_weight`** slider from
Section 1 — becomes a **collapsible section, collapsed by default**. Clicking
"Tuning ▸" expands it. Playback, Layers, and World (save/load/reset) stay
always-visible so the rail is short and the save/load/reset row is no longer
pushed off-screen. Matches the existing rail-collapse pattern.

---

## Config additions

| id | field | notes |
| --- | --- | --- |
| 16 | `harvest_weight` | fitness weight on lifetime food harvested; default 0.02; 0 = original delivery-only thesis |

(Existing tunable ids 0–15 are unchanged.)

## Protocol additions (summary)

Server → client: `0x09` ColonyMeta, `0x0A` Chronicle, and a name field appended
to `0x05` AntDetail.

Client → server: `0x0B` SetFood, `0x0C` SetStone, `0x0D` SpawnAnt, `0x0E`
RenameColony, `0x0F` AddToStore.

The cross-language fixture guard (`crates/server/tests/fixtures.rs` ↔
`web/tests/protocol.test.ts`) is extended to cover every new frame — a reordered
field must fail a test, not render as garbage.

## Testing

- **Fitness:** with `harvest_weight = 0`, selection distribution is identical to
  today (regression guard for the thesis toggle). With it positive, a
  harvest-heavy zero-delivery ant beats a do-nothing ant in the roulette, and a
  single delivering ant still beats a lifetime harvester (anti-hacking bound).
- **Chronicle:** each detector fires exactly once for a true "first" and never
  re-fires after save/load; a rolling title emits only on change.
- **Names:** deterministic for a `(seed, id)`; stable across save/load.
- **Map edits:** each command mutates the `World` as specified; a malformed
  payload is dropped without panicking; determinism holds when no edits are sent.
- **Protocol:** byte-exact fixtures for `0x09`, `0x0A`, the extended `0x05`, and
  every new client command; the web decoder round-trips them.
- **Golden master** regenerated once for the new `food_harvested` column and the
  chronicle/name state, and pinned.
- **Web:** context-menu item selection by target type; label collision layout;
  symbol-per-colony mapping; collapsible tuning section.

## Determinism and purity invariants (unchanged contracts)

- `sim` stays pure: no I/O, no sockets, no printing. Map edits arrive as data
  through the existing command channel.
- The parallel think phase stays read-only; every new field (`food_harvested`,
  chronicle, names, detector flags) is written only in the serial phase.
- Same seed + same config + same command stream ⇒ identical state hash,
  regardless of thread count.

## Out of scope

- Lifetime/reinforcement learning (the brain stays a fixed-weight forward pass;
  this was explicitly considered and set aside in favor of shaped evolutionary
  fitness).
- Glyphs stamped on individual ants at high zoom.
- Naming every ant (only notable ants are named).
