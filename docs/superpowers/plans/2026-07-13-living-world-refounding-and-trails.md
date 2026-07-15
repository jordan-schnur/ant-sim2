# Living World: Refounding + Colony Trail — Implementation Plan

> **For agentic workers:** implement task-by-task; each task ends with its test suite green and a commit.

**Goal:** Make the world robustly alive (cross-colony refounding at true extinction) and give ants a dedicated recent-trail sense channel.

**Architecture:** Two independent subsystems sharing one coordinated wire bump. Part B (trail) first (sensing), Part A (refounding) second (reproduction).

**Tech Stack:** Rust workspace (`sim`, `server`, `headless`) + TypeScript/WebGL2 web.

## Global Constraints

- Determinism is sacred: all new RNG use is in id order; `state_hash` covers new persistent state.
- Wire targets: `N_INPUTS` 46→51, `N_PARAMS` 1160→1240, `ANT_DETAIL_LEN` 433→453, `CONFIG_FIELDS` 18→21, `CHANNELS_PER_WHISKER` 6→7.
- Invariant preserved: living reproduction never mixes gene pools; only refounding does.
- Nests beacon **scent only**, never the trail field.

---

## Part B — Colony trail pheromone

- **B1 — `OwnedField` refactor:** extract `{mag, owner}` with `deposit/read/diffuse`; back `scent` with it. Migrate external readers (`protocol.rs` phero-frame + reset test, `world.rs`/`apply.rs` tests). All existing pheromone/scent tests green.
- **B2 — trail field + config:** add `trail: OwnedField`; config `trail_emission=1.0`, `trail_evaporation=0.95`, `trail_diffusion=0.06`; `deposit_passive` lays trail; `step` diffuses it. Tests: ant lays trail, nest does not, trail decays faster than scent.
- **B3 — sense channel:** `CH_OWN_TRAIL`, `CHANNELS_PER_WHISKER=7`, shift `IN_*`, `N_INPUTS=51`, assert `N_PARAMS=1240`. Tests: layout, own-trail reads, foe doesn't bleed, bounded.
- **B4 — determinism:** `state_hash` folds trail mag+owner. Test.
- **B5 — server wire:** `ANT_DETAIL_LEN=453`; `CONFIG_FIELDS` +3 trail (ids 18–20) + read/write arms; phero frame carries trail (widen texel); guards. Tests.
- **B6 — headless:** `apply_override` +3 trail fields. Test.
- **B7 — web:** `protocol.ts` (dims, config fields, phero decode), `nnlabels.ts` (own-trail label), `tunables.ts` (3 sliders), `state.ts` (trail layer flag), renderer + `controls.ts` toggle, inspector. Vitest green.

## Part A — Cross-colony refounding

- **A1 — reservoir draw:** `world_reservoir_parent(colonies, rng) -> Option<(Genome,u32)>`, fitness-weighted union across all halls of fame, deterministic. Tests.
- **A2 — retire floor + telemetry:** remove `extinction_floor`/`floor_respawn_interval` (config, headless); `ColonyState`: drop `floor_spawns`/`last_floor_spawn`, add `refounds: u64`; remove the floor block in `reproduce.rs`; `stats.rs`/`protocol.rs` stats frame + web `ColonyStat`: `floorSpawns`→`refounds`; charts/inspector. Green.
- **A3 — refound at pop 0:** in `reproduce`, pop==0 → spawn `initial_ants_per_colony` founders from the reservoir (genesis-mirrored: full energy, size 1.0, nest tiles; lineage = parent+1), cold-start random fallback, chronicle event, `refounds += 1`. Tests: fires at 0, cohort size, gene-flow across colonies, cold-start, determinism, payoff behavior test.

## Verify & merge

Full `cargo test` + `npm test` (web) + wire-format guard green; commit; merge to `main`.
