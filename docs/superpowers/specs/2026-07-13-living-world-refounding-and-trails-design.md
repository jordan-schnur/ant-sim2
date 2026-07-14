# Living World: Cross-Colony Refounding + Colony Trail Pheromone — Design

**Date:** 2026-07-13
**Status:** Approved (brainstorm), pending implementation-plan

## Goal

Make the world robustly alive instead of lottery-dominated. Today a random
genome rarely learns to forage, so typically 1–2 of 8 colonies ignite and the
rest sit sterile at the extinction floor forever, with no way for the one
success to help the others. Two independent changes fix this:

- **Part A — Cross-colony refounding ("nuptial flight").** Let colonies truly
  die, and refound a dead nest from a *blend* of the best proven genomes across
  the whole world. One ignition then seeds every future refound — the world
  can't stay mostly sterile.
- **Part B — Colony trail pheromone.** Give ants a dedicated, fast-decaying
  "colony-mates were here recently" signal, separated from the persistent nest
  homing beacon, as a new NN sense channel. Evolution decides whether to follow
  it (recruit/exploit) or avoid it (disperse/explore).

The two parts are independent subsystems (reproduction vs. sensing). They share
one coordinated wire-format bump and are implemented and tested separately.

## Background

- Evolution here is **neuroevolution**, not gradient learning: each ant is a
  fixed feedforward NN, weights frozen at birth; the only optimization channel
  is fitness → reproduction (roulette selection + Gaussian mutation +
  per-colony hall-of-fame archive).
- **Per-colony sealed gene pools.** `ColonyState::select_parent` and paid births
  (`reproduce.rs`) draw only from the colony's own living ants; the hall-of-fame
  floor draws only from the colony's own archive. Nothing crosses colonies.
  Tests `gene_pools_never_mix` and `select_parent_only_ever_returns_own_colony`
  lock this in.
- **The extinction floor is a zombie lifeline.** `reproduce.rs` tops any colony
  below `extinction_floor` back up with one free ant per interval, drawn from
  its *own* archive. `a_colony_can_never_be_permanently_extinct` enforces it.
  This is exactly what prevents population from ever reaching zero, so a sterile
  colony persists forever instead of dying and being replaced by better stock.
- **Scent is already laid by ants but fused with the nest beacon.** Every ant
  leaks colony scent every tick (`deposit_passive` → `deposit_scent`,
  `ant_scent_emission = 0.5`; test `every_ant_leaks_colony_scent_unconditionally`),
  and ants sense own/foe scent via whisker channels `CH_OWN_SCENT`/`CH_FOE_SCENT`.
  But nests blast the *same* field (`nest_scent_emission = 50.0`) and it barely
  evaporates (`scent_evaporation = 0.999`), so the channel means "distance to my
  nest / whose territory," not "colony-mates were here recently." The recent-path
  signal is drowned. Part B un-fuses it into its own channel.

---

## Part A — Cross-colony refounding

### The governing invariant

**While a colony is alive, it breeds pure — paid births still draw only from its
own living ants. Only death opens the gene pool.** `gene_pools_never_mix` and
`select_parent_only_ever_returns_own_colony` stay true for everyday
reproduction. Cross-colony gene flow happens at exactly one moment: refounding.

### Collapse trigger

In the `reproduce` pass (per colony, id order — already deterministic), if a
colony's living population is **0**, refound it this same tick. Population zero
is the sole trigger (chosen over stagnation timers or store bankruptcy).

### The world reservoir

"Best of other generations, across the world" = the **union of every colony's
`hall_of_fame`**, fitness-weighted with the same `PARENT_EPS` roulette that
`ColonyState::archive_parent` already uses. No new persistent state — the
per-colony archives already exist and are maintained (`record_death`).

Add a function that draws one `(genome, lineage)` from the union across all
colonies, fitness-weighted. Signature (final names decided in the plan), e.g.:

```rust
/// Draw one archived genome from the union of all colonies' halls of fame,
/// fitness-weighted (PARENT_EPS keeps flat archives samplable). None only when
/// every colony's archive is empty (true cold start).
fn world_reservoir_parent(colonies: &[ColonyState], rng: &mut Pcg32)
    -> Option<(Genome, u32)>;
```

Determinism: iterate colonies in id order, then hall-of-fame order, accumulating
the roulette total exactly as `archive_parent` does.

### Refound = a fresh genesis founding for that nest

A refound reproduces genesis founding (`world.rs::new`, lines ~49–72) exactly,
with genes from the reservoir instead of `Genome::random`:

- Spawn `initial_ants_per_colony` founders (same count as genesis).
- Each founder is an **independent** fitness-weighted draw from the world
  reservoir, then `.mutated(cfg, rng)`. Independent draws + mutation make the
  cohort a *hybrid* of what's working across the map — a new competing lineage,
  not a photocopy of the current winner.
- Founder attributes mirror genesis founders exactly: **full energy**
  (`genome.max_energy(cfg, 1.0)`), **size 1.0**, random heading, spawned on the
  colony's nest tiles.
- Lineage = drawn parent's archived depth + 1 (a descendant of a proven queen);
  update `next_lineage_hint` accordingly.
- **No** starter food store and **no** grace period (instant cohort, per
  decision). See Risks.

### The extinction floor is retired

Remove the `below_floor` free-ant block from `reproduce.rs` entirely. Its role
(keep a colony alive from its own dead-end archive) is replaced by refounding
from the world reservoir at population 0. `a_colony_can_never_be_permanently_extinct`
inverts in spirit: a colony *can* hit zero, but refounds the same tick, so it is
still never *permanently* extinct — just reseeded from better stock.

Retire:
- Config fields `extinction_floor`, `floor_respawn_interval` (not in
  `CONFIG_FIELDS`; only struct + headless overrides + tests).
- `ColonyState` fields `floor_spawns`, `last_floor_spawn`.
- `ColonyStats.floor_spawns` in the stats wire frame (`protocol.rs:440`) and web
  `ColonyStat.floorSpawns`.

Add:
- `ColonyState.refounds: u64` — the new honest signal ("this colony collapsed
  and was reseeded N times"). Same wire slot/width the stats frame's
  `floor_spawns` u64 occupied — a rename, not a size change. Surface in
  `ColonyStats`, the stats frame, and the web charts/inspector where
  `floorSpawns` was shown.
- Chronicle event on refound, e.g. *"Colony Redwood collapsed — refounded from
  the world's proven lines."* (Reuse the existing `Chronicle` mechanism; exact
  copy decided in the plan.)

### Cold start

If the entire world reservoir is empty (the first ticks, before any colony has
archived anything), founders fall back to `Genome::random` — identical to
genesis. After the first colony ever ignites, its genes are in the union, so
every future refound anywhere pulls from proven stock.

### Part A tests

- Refound fires precisely at population 0; cohort size == `initial_ants_per_colony`.
- **Gene-flow test** (the scoped inverse of `gene_pools_never_mix`): a superstar
  colony's genome appears in a *different*, dead colony's refound cohort.
- Living reproduction still never mixes pools (existing invariant tests stay
  green).
- Cold-start fallback to random when the whole reservoir is empty.
- Founders mirror genesis: full energy, size 1.0, on nest tiles.
- Determinism / `state_hash` unchanged in shape (see wire section).
- **Payoff behavior test:** a world seeded so only one colony can ignite lifts
  the others via refounding — world-wide `delivered_total` climbs, not just the
  lucky colony's. This is the test that says the bootstrap problem is solved.

---

## Part B — Colony trail pheromone

### The field

A new owned field `trail` in `Pheromones`, structurally a twin of `scent`
(magnitude + colony owner, contested identically). Its differences from scent
are only tuning and who lays it:

- **Fast evaporation** (`trail_evaporation`, default ~0.95 vs scent's 0.999) →
  a trail means *recent*; it fades in tens of ticks instead of staining the map.
- **Only ants lay it.** In `deposit_passive`, every ant deposits
  `trail_emission` on its current cell every tick. **Nests never touch the trail
  field** — no beacon. This is what un-fuses "recent path" from "homing."

### DRY refactor (justified by the new field)

`scent` currently has bespoke deposit / owner-read / contested-diffuse code
(`deposit_scent`, `scent_for`, `diffuse_scent`). Rather than copy-paste it for
`trail`, extract a small shared `OwnedField` abstraction: a `{ mag: Vec<f32>,
owner: Vec<u8> }` with `deposit(i, amount, colony)`, `read(i, colony) ->
(own, foreign)`, and `diffuse(w, h, diffusion, evaporation)` (the contested
diffusion with owner tags). Make both `scent` and `trail` instances of it. This
is a targeted refactor in service of the work, not a drive-by. All existing
scent tests must continue to pass against the refactored type.

### Sensing

One new whisker channel `CH_OWN_TRAIL` = own-colony recent-trail intensity,
log-squashed via `squash_phero` like the other pheromone channels. **Foe-trail
is omitted** (YAGNI — `CH_FOE_SCENT` already carries enemy-territory info).

This grows `CHANNELS_PER_WHISKER` 6 → 7, i.e. **+5 NN inputs** (one per whisker).
All `IN_*` offsets after the whisker block shift by 5. No underfoot trail
reading (whisker-only keeps the change minimal; movement decisions use whiskers).

The NN gets a genuine new signal; evolution alone decides follow-vs-avoid. No
hard-coded behavior.

### Visualization

A minimal `trail` render layer + toggle in the web view, so the trails are
watchable on the map. Mirror the existing scent/food layer plumbing
(`state.ts` layers, renderer, layer toggle UI).

### Part B tests

- Every ant deposits trail unconditionally; **nests do not** touch the trail
  field.
- Trail evaporates fast (crisp/recent): an isolated trail deposit is gone within
  N ticks, in contrast to scent which persists far longer under the same steps.
- New whisker channel reads own-colony trail; a foreign colony's trail does not
  bleed into `CH_OWN_TRAIL`.
- `OwnedField` refactor: all pre-existing scent behavior tests pass unchanged.
- Every input stays finite and in [-1, 1] (existing `every_input_is_finite_and_bounded`
  extended to the new dimension).
- Determinism / `state_hash` includes `trail` magnitude + owner.

---

## Wire-format accounting (one coordinated pass)

This is the homing class of change; re-run the wire-format guard after.

| Constant | Before | After | Cause |
|---|---|---|---|
| `N_INPUTS` (`lib.rs`) | 46 | 51 | +1 whisker channel × 5 whiskers |
| `N_PARAMS` (`lib.rs` assert) | 1160 | 1240 | +5 inputs × 16 hidden-1 |
| `ANT_DETAIL_LEN` (`protocol.rs`) | 433 | 453 | +5 input floats × 4 bytes |
| `CONFIG_FIELDS` len (`protocol.rs`) | 18 | 21 | +3 trail config fields |
| `CHANNELS_PER_WHISKER` (`sense.rs`) | 6 | 7 | new `CH_OWN_TRAIL` |

Touch list:
- **sim:** `lib.rs` (constants + assert), `sense.rs` (`CH_OWN_TRAIL`,
  `CHANNELS_PER_WHISKER`, shifted `IN_*` offsets, read trail), `pheromone.rs`
  (`OwnedField`, `trail`, `step`), `apply.rs` (`deposit_passive` lays trail),
  `world.rs` (refounding, nest beacon still scent-only, `state_hash`),
  `reproduce.rs` (remove floor block), `colony.rs` (`refounds`, drop
  `floor_spawns`/`last_floor_spawn`; reservoir draw may live here or in
  `reproduce.rs` — decided in plan), `config.rs` (3 new trail fields; remove
  `extinction_floor`/`floor_respawn_interval`), `stats.rs` (`refounds`).
- **server:** `protocol.rs` — `ANT_DETAIL_LEN`, `CONFIG_FIELDS` (+3 trail
  entries, indices 18–20), `read_config_field`/`write` arms for trail, stats
  frame `floor_spawns` → `refounds`, detail-frame offset guards.
- **headless:** `main.rs` `apply_override` — add `trail_emission`,
  `trail_evaporation`, `trail_diffusion`; remove `extinction_floor` (and any
  floor override) arms.
- **web:** `protocol.ts` (`N_INPUTS`, `ANT_DETAIL_LEN`, computed `N_PARAMS`,
  h1/h2/output offsets, `CONFIG_FIELDS` + 3 trail entries, `ColonyStat.floorSpawns`
  → `refounds`), `nnlabels.ts` (new channel label "own trail", input groups),
  `tunables.ts` (3 trail sliders), `state.ts` (`refounds`, trail layer), renderer
  (trail layer), layer-toggle UI, and all dimension-hardcoded tests
  (`protocol.test.ts`, `nnview.test.ts`, `state.test.ts`, `nnlabels.test.ts`).

New config defaults (starting guesses, to be swept):
- `trail_emission: 1.0`
- `trail_evaporation: 0.95`
- `trail_diffusion: 0.06`

New tunable sliders (`tunables.ts`), ids 18–20:
- trail emission (linear, 0–5)
- trail evaporation (decay scale, 0.9–0.9999)
- trail diffusion (linear, 0–0.4)

---

## Risks and caveats

- **Death thrash (Part A).** With no grace period and no starter store, a
  refounded colony can re-die before its founders deliver, and refound again.
  Accepted for now (instant-cohort decision). If runs show pathological thrash,
  the fix is a short post-refound grace / small starter store — deferred, not
  built now. The `refounds` counter makes thrash observable.
- **Monoculture creep (Part A).** Fitness-weighting means a runaway superstar
  dominates the reservoir, so refounds lean toward its genes at that instant.
  Mitigated because each refounded colony then evolves in its own pure box and
  drifts, and its own archive later joins the union. If runs collapse to a true
  monoculture, the knob is "cap any single colony's share of the reservoir" —
  left out for now (YAGNI); measure first.
- **Trail tuning (Part B).** `trail_emission`/`trail_evaporation` defaults are
  guesses; the point of the sliders is to sweep them. Too-slow evaporation
  re-creates the scent-stain problem; too-fast makes the trail invisible.

## Out of scope (explicit)

- Post-refound grace period / starter store (deferred fix for death thrash).
- Reservoir monoculture cap.
- Foe-trail sense channel (`CH_FOE_TRAIL`).
- Underfoot trail reading.
- Migrant-trickle gene flow (rejected in favor of true extinction).
- Any change to the world-frame velocity control or homing-fitness shaping
  already shipped.
