# Fullscreen NN Viewer + Explain-Everything Design

**Date:** 2026-07-15
**Status:** Approved for planning

## Goal

Make the simulation legible to a human watching it. Two deliverables sharing one
foundation:

1. A fullscreen neural-network viewer for the selected ant — like the fullscreen
   graph — where you can pause, single-step, watch every activation move, and
   read every one of the 60 inputs with an explanation of *how it is computed*.
2. A reusable "explain this" affordance (a small `ⓘ`) applied systematically
   across the whole UI, so nothing on screen is unexplained.

## Background: what exists today

- **Explanation copy is scattered across four places:** `web/src/ui/tooltips.ts`
  (`TOOLTIPS` dict for tile/colony/ant stat rows, revealed by a `data-tip`
  hover), the `hint` field on tuning sliders (`web/src/ui/tunables.ts`, shown as
  native `title`), `OUTPUT_DESC` in `web/src/nnlabels.ts`, and `nodeInfo()` in
  the same file (the NN node hover popover). Many controls have no explanation at
  all.
- **The NN is a canvas2d panel** (`web/src/ui/nnview.ts`, `draw`/`hitTest`/
  `layout`) rendered into a persistent `#nn` canvas in the right-hand Explorer
  pane (`web/src/ui/inspector.ts` → `renderAntDetail`, `attachNNPopover`). At
  260px tall it cannot show all 60 inputs legibly.
- **The fullscreen graph is the pattern to copy:** `web/src/ui/graphmodal.ts`
  `openGraph()` — a singleton modal with a `.graph-backdrop` + `.graph-panel`,
  Esc/✕ to close, backdrop-click to dismiss, and `store.subscribe()` for live
  updates. Opened from the compact stats charts (`web/src/ui/stats.ts`).
- **Single-step already exists:** `cmdStep()` and `cmdSetPaused()` in
  `web/src/protocol.ts`, wired to the `⏭`/`▶` buttons in
  `web/src/ui/controls.ts`. Stepping drives the sim one tick; the server sends a
  fresh ant-detail frame; `store.state.detail` (inputs/h1/h2/outputs) updates.
  No new protocol is needed for "watch every step."
- **Input layout is the contract in `crates/sim/src/sense.rs`,** hand-mirrored in
  `web/src/nnlabels.ts` (`INPUT_GROUPS`, `inputLabel`). 60 inputs:
  - `0..40` whiskers: 5 directions × 8 channels
    (food, food-pheromone, alarm, own-scent, foe-scent, wall, home-trail,
    own-trail)
  - `40..44` underfoot (food, food-pheromone, alarm, home-trail)
  - `44..46` crowd (friends near, foes near)
  - `46..50` body (energy, size, carrying, age)
  - `50` bias
  - `51..55` memory 0..3
  - `55..58` home vector (unit x, unit y, distance)
  - `58..60` facing (sin, cos)

## Design

### Piece 1 — Central explanation registry + `infoDot` affordance

**New module `web/src/ui/explain.ts`.** One keyed record of *all* explanatory
copy, becoming the single source of truth. It absorbs the existing `TOOLTIPS`
entries verbatim (same keys) so current call sites keep working, and adds copy
for everything currently unexplained (playback buttons, layer toggles, phero-res
button, save/load/reset, every tuning slider, the stats-chart metrics, the
world/colony/tile rows that lack a key today).

```ts
// web/src/ui/explain.ts
export const EXPLAIN: Record<string, string> = { /* ...all copy, one place... */ };
export function explainText(key: string): string | undefined { return EXPLAIN[key]; }
/** A small ⓘ that reveals its copy on hover and pins it on click. */
export function infoDot(key: string): HTMLElement { /* ... */ }
```

**`infoDot(key)`** renders a `<span class="info-dot" tabindex="0">ⓘ</span>`
carrying the copy. Behavior:
- Hover → shows a floating popover with the copy (reuses the visual style of the
  existing `.tip` / `.nn-pop`).
- Click (or Enter/Space when focused) → pins the popover open until the next
  outside click or Esc, so long "how it's computed" text can be read without
  holding the mouse still. Only one pinned popover at a time.
- Copy sourced by `key` from `EXPLAIN`; an unknown key renders nothing (fail
  soft, logged once to console in dev) so a typo never throws mid-render.

**`tooltips.ts` becomes a thin adapter:** `TOOLTIPS` is re-exported from / backed
by `EXPLAIN` (same keys) so `tipLabel`/`tipText` keep working unchanged. No call
site that uses `tipLabel` today needs to change; we *add* `infoDot`s where there
is currently nothing.

**Systematic sweep — where dots/labels go:**
- `controls.ts`: an `infoDot` on each Playback button row, each layer checkbox,
  the phero-res button, and the save/load/reset row.
- `controls.ts` sliders + `tunables.ts`: promote each slider's `hint` into
  `EXPLAIN` (richer copy) and add an `infoDot` beside the slider label. Keep the
  `hint` field working for any that still set it, but the dot is the affordance.
- `stats.ts`: an `infoDot` beside each chart title.
- `explorer.ts`: the colony, tile, and world rows already pass a key to
  `tipLabel`; add `infoDot`s for the rows that pass `""` today (id, age, deaths,
  etc.) by giving them keys in `EXPLAIN`.
- `inspector.ts`: `infoDot`s on the Traits/Inputs/Outputs section headings and on
  the fitness headline.

**Whisker-grid decision (YAGNI):** the 5×8 grid does **not** get 40 dots. The
`ⓘ` goes on the 8 channel headers (what each channel means + how squashed) and
the 5 direction labels (which antenna). Every *individual* input's full
explanation lives in the fullscreen list (Piece 3).

### Piece 2 — Per-input explanations (meaning + computation)

Author copy for all 60 inputs, read directly from `sense.rs`, each stating what
the number means *and* how it is computed. Examples (literal-expression style,
approved):

- whisker · food → "Food seen along this antenna. `grid.food[cell] ÷
  food_patch_max`, capped at 1. Cell sampled is `vision` steps out at the
  whisker's angle relative to your heading."
- whisker · food-pheromone → "Food-trail scent along this antenna, log-squashed:
  `ln(1 + value) ÷ phero_log_div`, capped at 1."
- whisker · wall → "1 if the sampled cell is stone or off the map, else 0."
- underfoot · home-trail → "Shared exploration/home trail on your own cell,
  log-squashed."
- crowd · friends near → "Same-colony ants within 2 cells (excluding you), ÷ 8,
  capped at 1."
- body · energy → "Fuel fraction: `energy ÷ max_energy`, clamped 0–1."
- body · age → "`age ÷ lifespan`, clamped 0–1."
- bias → "Constant 1. A learnable offset every neuron can weight."
- memory 0..3 → "Recurrent memory: this input is whatever the brain wrote to
  memory output N on the previous tick."
- home vector x/y → "World-frame unit vector toward your nest, X/Y component.
  `(nest − pos) ÷ distance`; zero on the nest."
- home distance → "Distance to your nest ÷ map diagonal, capped at 1."
- facing sin/cos → "`sin`/`cos` of your heading, so the network reads its own
  facing without the ±π wrap a raw angle would jump at."

**Extend `web/src/nnlabels.ts`** with `inputInfo(i: number): { label: string;
desc: string }`, computed from the group + channel the same way `inputLabel`
already is. `nodeInfo()` (the existing hover popover source) starts returning the
`desc` for input nodes too, so the small popover gains the computation. The
fullscreen list (Piece 3) consumes `inputInfo` directly.

Keep the file's existing "hand-kept mirror of `sense.rs`" warning and the
`INPUT_GROUPS.reduce(...) === N_INPUTS` guard; add an analogous guard that
`inputInfo` returns a non-empty `desc` for every `i in 0..N_INPUTS`.

### Piece 3 — Fullscreen NN modal

**New module `web/src/ui/nnmodal.ts`, `openNN(store, net)`** mirroring
`graphmodal.ts` structure: a singleton, `.nn-backdrop` + `.nn-modal-panel`,
Esc/✕/backdrop-click to close, `store.subscribe()` for live updates,
`window` resize handling, full teardown on close.

Layout inside the panel (flex row):
- **Left — the network, large.** Reuse `nnview.draw(ctx, w, h, act, params)`
  unchanged, painting into a big canvas sized to the panel. Attach the same
  hover popover logic used in the pane (factor the popover attach out of
  `inspector.ts` so both call sites share it, or reuse `attachNNPopover`
  directly against the modal canvas). Because the node popover now carries
  `desc`, hovering any node reads its computation.
- **Right — the full input/output list.** A scrollable column, grouped by
  `INPUT_GROUPS`, one row per input: name, live value (`store.state.detail`),
  a small activation-color chip (`activationColor(v)`), and an `infoDot` whose
  copy is `inputInfo(i).desc`. Below, the 8 outputs with `OUTPUT_DESC`. A filter
  text box at the top narrows rows by label substring.
- **Top bar — embedded playback.** `▶/❚❚ pause` and `⏭ step` buttons wired to
  `cmdSetPaused`/`cmdStep` via the passed-in `net`, plus the live tick number.
  Pause, step, and both the canvas and the list update in place. Reflects
  `store.state.paused` like `controls.ts` does.

**Opening it:** an expand button (`⤢`) overlaid on the `#nn` canvas in the
Explorer pane (added in `inspector.ts` / `explorer.ts`), plus it is a no-op with
a "select an ant" message when `store.state.detail` is null. Esc closes.

**No new protocol, no history buffer.** "Watch every step" is pause + step
driving the existing per-tick ant-detail frame. Scrubbing backward through past
ticks is explicitly out of scope (was offered, not chosen).

## Files

- **Create:** `web/src/ui/explain.ts` (registry + `infoDot`), `web/src/ui/
  nnmodal.ts` (fullscreen modal).
- **Modify:** `web/src/nnlabels.ts` (add `inputInfo`, feed `nodeInfo`),
  `web/src/ui/tooltips.ts` (back `TOOLTIPS` with `EXPLAIN`), `web/src/ui/
  inspector.ts` (share popover attach, add expand button, section-heading dots),
  `web/src/ui/explorer.ts` (row dots), `web/src/ui/controls.ts` (playback/layer/
  slider dots, mount expand handler), `web/src/ui/tunables.ts` (slider copy keys),
  `web/src/ui/stats.ts` (chart-title dots), `web/index.html` (CSS for
  `.info-dot`, its popover, `.nn-backdrop`/`.nn-modal-panel`/list).
- **Tests:** `web/tests/explain.test.ts` (every key resolves; `infoDot` renders,
  hover/pin behavior), `web/tests/nnlabels.test.ts` (extend: `inputInfo` covers
  all 60 with non-empty `desc`; `nodeInfo` returns desc for inputs).

## Testing

- **Unit (Vitest, jsdom):** `inputInfo(i)` returns a non-empty `desc` for every
  `i` in `0..N_INPUTS` and a label matching `inputLabel(i)`; `EXPLAIN` contains
  every key referenced by the sweep (a test enumerates the keys the modules use
  and asserts each exists); `infoDot` mounts, shows copy on hover, pins on click,
  unpins on outside click/Esc; unknown key renders empty and does not throw.
- **Guards stay:** the existing `INPUT_GROUPS` sum-to-`N_INPUTS` and
  `OUTPUT_LABELS` length checks in `nnlabels.ts` remain; add the `inputInfo`
  coverage guard alongside.
- **Manual smoke:** build web, run the server, select an ant, open the fullscreen
  NN, pause, step, confirm activations + list values advance one tick at a time
  and every input's `ⓘ` reads its formula. Confirm dots appear across controls,
  tunables, stats, and the readout panels.

## Non-goals

- No history/scrubbing of past ticks (offered, not chosen).
- No change to the sim, protocol, or wire format — this is web-only.
- No restyle of existing panels beyond adding the affordance.
- Not forcing a dot onto all 40 whisker cells (headers + fullscreen list cover
  them).
