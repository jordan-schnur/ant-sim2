# UI Pass: Explorer Tabs, Ant Popover, and Number Legibility — Design

**Date:** 2026-07-13
**Scope:** `web/` only. No server, sim, or wire-format changes.

## Goal

Make the simulation's numbers self-explanatory and turn the right rail into a
game-engine-style, context-sensitive inspector, plus a glance-level ant popover
in the world.

Two user-facing complaints motivate this:

1. The numbers aren't legible — "what does delivered mean?" A viewer can't tell
   `store` (a spendable bank) from `delivered` (a lifetime odometer), or why a
   colony at `store == 0` is still alive.
2. There's no fast way to inspect an individual ant (all its stats + its neural
   network), and the right rail is a fixed vertical stack rather than a
   context-specific tool.

## Background: what the numbers actually mean

These definitions are the source of truth for the tooltip copy in this spec.
Verified against `crates/sim/src/apply.rs`, `reproduce.rs`, `world.rs`.

- **store** — the colony's food on hand. Grows only when an ant banks carried
  food on its own nest (`apply_nest`). Spent on paid births (`birth_cost` each)
  and on refueling ants that stand on the nest. A bank balance: it goes down.
  A colony at `store == 0` is *not* dead — its ants live on personal `energy`
  reserves, the extinction floor mints free ants below `extinction_floor`, and
  any delivery makes the store briefly positive again.
- **delivered** (`deliveredTotal` per colony, `foodDelivered` per ant) — the
  cumulative food ever carried home. Monotonic; never spent down. An odometer.
  It is the fitness signal — "is this colony evolving to forage better."
- **energy** (per ant) — the ant's personal fuel. Spent on movement and upkeep;
  refilled *only* by refueling at its own nest (drawn from `store`), or scavenged
  from kills. There is no direct eating of ground food — harvested food is cargo
  (`carrying`) until banked.
- **generation** (`meanLineage` / `lineage`) — mean/individual lineage depth.
- **pop / store / delivered** together: pop = ants alive now; store = spendable
  fund; delivered = lifetime brought home.

## Current structure (for reference)

- **Left rail** (240px): tuning controls (`mountControls`).
- **Right rail** (300px): a vertical stack — colony cards (`mountColonies`),
  chronicle (`mountChronicle`), ant inspector (`mountInspector`).
- **World**: canvas + `LabelOverlay` + `ColonyPanel` (in-world colony popover).
- **Selection today is split**: `state.detail` / `state.genome` (selected ant,
  server-resolved via `cmdSelectAt`) and `state.selectedColony` (colony popover).
  There is no tile selection.

## Design

### 1. Unified selection model

Replace the two ad-hoc selection fields' *coordination* with one explicit
discriminated selection so the popover and the Explorer can never disagree:

```ts
type Selection =
  | { kind: "ant" }               // detail/genome already hold the payload
  | { kind: "colony"; id: number }
  | { kind: "tile"; x: number; y: number }  // world-cell coords (ints)
  | null;
```

`state.detail` / `state.genome` remain the ant payload (unchanged wire path).
`state.selectedColony` is subsumed by `selection.kind === "colony"`. Selecting
any entity sets `selection` and notifies; the Explorer renders off `selection`.
Selecting anything auto-switches the right rail to the Explorer tab.

Escape / out-of-bounds click / empty-ground deselect sets `selection = null`
(and clears detail/genome as today).

### 2. Right rail → two tabs

A tab strip at the top of the right rail:

- **Colonies** — existing colony cards + charts + chronicle, unchanged content,
  housed under the tab. (Chronicle stays here rather than a third tab: it's a
  low-interaction log and a third tab is not worth it.)
- **Explorer** — one context-sensitive panel that renders `selection`:
  - `ant` → full ant stats (name, id, colony, energy/maxEnergy, size, age,
    generation, carrying, delivered), Traits block, Outputs block, and the live
    NN (`nnview.draw`). This is today's `mountInspector` content, relocated.
  - `colony` → full colony stats (pop, store, births, deaths, floorSpawns,
    meanSize, generation, delivered) + its pop/store/delivered history chart
    (reuse the colony card's existing chart drawing).
  - `tile` → cell coords, food amount, stone coverage, nest owner (or "none"),
    and pheromone at that cell: food, alarm, scent, owning colony. All read
    client-side from `state.terrain` and `state.phero` (both already on the
    wire). No server round-trip.
  - `null` → world context: tick, total ants, and per-colony-summed totals
    (Σ pop, Σ store, Σ delivered), so the panel is never blank.

Tab state (`activeTab: "colonies" | "explorer"`) lives in the store so a
selection can switch it programmatically.

### 3. In-world ant popover (the glance)

Clicking an ant opens a compact popover anchored on the ant's world position
(`detail.x/y` projected through `projectToCss`), following it as it moves —
mirroring `ColonyPanel`. Contents: name/id, energy (`e / max`), carrying,
delivered. **Text-only — no NN thumbnail** (a live graph on a popover chasing a
moving ant is unreadable; the NN stays in the Explorer). Dismissed by the same
deselect paths as everything else. The full ant detail + NN is in the Explorer.

Clicking a nest still opens the existing `ColonyPanel` popover *and* now sets
`selection = {kind:"colony"}` so the Explorer fills too.

### 4. Tile inspection + hotkeys

- **Left-click** keeps today's behavior: nest tile → colony; otherwise the
  server's nearest ant.
- **Alt/Option + left-click** → `selection = {kind:"tile", x, y}` for the cell
  under the cursor. Fully client-side.
- **Right-click menu** gains "Inspect tile here" as an additional item.
- A small **hotkeys/legend affordance** in the UI documents: `Alt-click` = tile,
  `F` = fit view, `Space` = pause/resume, `Esc` = deselect. (User explicitly
  asked for hotkeys to be explained.) Simplest form: a `?`/"keys" line in a
  corner overlay or the left rail footer listing these.

### 5. Explain-the-numbers via hover tooltips

Every stat label across the colony cards, the ant view, and the tile view gets a
styled hover tooltip (not native `title`) with a one-line plain-English
definition. A shared helper maps a stat key → tooltip string so copy lives in
one place. Core copy:

- **store** — "Spendable food fund. Births and refueling draw from it. A colony
  at 0 survives on ant reserves + the extinction floor."
- **delivered** — "Lifetime food carried home. An odometer — never spent down.
  This is the fitness signal."
- **energy** — "The ant's personal fuel. Refilled only at its own nest."
- **generation** — "Lineage depth — how many ancestors deep this line is."
- **carrying** — "Food the ant is holding, not yet banked at a nest."
- **pop** — "Ants alive right now."
- Tile pheromone rows (food/alarm/scent/owner) get one-liners too.

## Non-goals (YAGNI)

- No server, sim, or wire-format changes. Tile data is derived from frames the
  client already receives.
- No new chart types — the Explorer's colony view reuses the colony card chart.
- No NN thumbnail in the popover.
- No draggable/resizable/pinnable windows — the popover and Explorer are enough.
- No multi-select.

## Testing strategy

Vitest, matching the existing `web/tests/*` style (pure logic + store, no DOM
GL). Concretely:

- **Selection model** (`state.test.ts`): selecting ant/colony/tile sets the
  right `selection`; deselect clears it; selecting flips `activeTab` to
  "explorer".
- **Tile readout** (new pure helper): given a `terrain` + `phero` frame and a
  cell, returns the correct food/stone/nest/pheromone values (including the
  factor/downsample mapping and the 255 = "no owner/no nest" sentinels).
- **World summary** (new pure helper): Σ pop / Σ store / Σ delivered over a
  `ColonyStat[]`.
- **Tooltip copy** (new pure map): every stat key used by the panels has a
  non-empty tooltip string (guards against a label with no definition).
- Keep the existing camera/label/terrain tests green.

## Open items resolved as defaults (revisit freely)

- Chronicle → under the Colonies tab (not its own tab).
- Ant popover → text-only, NN in Explorer only.

## File map (indicative; finalized in the plan)

- `web/src/state.ts` — `selection`, `activeTab`, select/deselect methods,
  world-summary helper.
- `web/src/tile.ts` (new) — pure `tileReadout(terrain, phero, x, y)`.
- `web/src/ui/tooltips.ts` (new) — stat-key → tooltip copy + a `withTooltip`
  DOM helper.
- `web/src/ui/rail.ts` (new) — tab strip + tab switching for the right rail.
- `web/src/ui/explorer.ts` (new) — context-sensitive panel dispatching on
  `selection` (reuses inspector/colony/tile renderers).
- `web/src/ui/antpopover.ts` (new) — in-world ant popover (mirrors
  `colonypanel.ts`).
- `web/src/ui/inspector.ts` — factor the ant-detail rendering so the Explorer
  can call it; add tooltips.
- `web/src/ui/colony.ts` / `colonypanel.ts` — tooltips; colony card chart made
  callable from the Explorer.
- `web/src/main.ts` — wire tabs, Alt-click tile selection, popover, hotkey line.
- `web/index.html` — tab strip, tooltip, popover, hotkey-line styles.
