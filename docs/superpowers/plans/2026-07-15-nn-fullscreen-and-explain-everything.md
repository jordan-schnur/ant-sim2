# Fullscreen NN Viewer + Explain-Everything Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A fullscreen neural-network viewer for the selected ant (pause, single-step, read every input with its computation) plus a reusable `ⓘ` "explain this" affordance swept across the whole web UI.

**Architecture:** Web-only (TypeScript, no Rust/protocol changes). One central copy registry (`explain.ts`) with an `infoDot` element becomes the single source of explanation text; `nnlabels.ts` gains per-input "how it's computed" copy; a new `nnmodal.ts` mirrors the existing `graphmodal.ts` modal, reusing `nnview.draw` and the existing `cmdStep`/`cmdSetPaused` protocol for stepping. No new wire format, no history buffer.

**Tech Stack:** TypeScript, Vite, Vitest (jsdom), plain DOM (no framework). Spec: `docs/superpowers/specs/2026-07-15-nn-fullscreen-and-explain-everything-design.md`.

## Global Constraints

- Web-only: do not touch `crates/**`, `web/src/protocol.ts` wire encoders/decoders, or any wire format. No sim, server, or protocol changes.
- All work happens in `web/`. Commands run from `web/` unless noted.
- `web/src/nnlabels.ts` is a hand-kept mirror of `crates/sim/src/sense.rs` — keep its existing warning comment and the `INPUT_GROUPS.reduce(...) === N_INPUTS` and `OUTPUT_LABELS.length === N_OUTPUTS` guards intact.
- The 60-input layout (verbatim from `sense.rs`): whiskers `0..40` (5 dirs × 8 channels: food, food-pheromone, alarm, own-scent, foe-scent, wall, home-trail, own-trail), underfoot `40..44`, crowd `44..46`, body `46..50` (energy, size, carrying, age), bias `50`, memory `51..55`, home vector `55..58` (unit x, unit y, distance), facing `58..60` (sin, cos).
- `squash_phero(v, div) = ln(1 + max(v,0)) / div`, capped at 1 (`phero_log_div`). Whisker/underfoot pheromone channels use it; food channels use `grid.food / food_patch_max` capped at 1.
- Tests: Vitest, files in `web/tests/*.test.ts`, run with `npm test` (or a single file via `npx vitest run tests/<file>`). Import app modules with the `.js` extension (e.g. `../src/ui/explain.js`) as the existing tests do.
- No emoji in code or commit messages beyond the UI glyphs the design specifies (`ⓘ`, `⤢`). Commit messages: short imperative subject.
- Match existing file idioms: block comments explaining *why*, `document.createElement` DOM building, CSS added to the `<style>` block in `web/index.html`.

---

### Task 1: Central explanation registry + `infoDot` affordance

**Files:**
- Create: `web/src/ui/explain.ts`
- Modify: `web/src/ui/tooltips.ts` (back `TOOLTIPS` with `EXPLAIN`)
- Modify: `web/index.html` (CSS for `.info-dot` and its popover)
- Test: `web/tests/explain.test.ts`

**Interfaces:**
- Produces:
  - `EXPLAIN: Record<string, string>` — all explanation copy, keyed. Includes every key currently in `TOOLTIPS` (`pop, store, delivered, energy, generation, carrying, fitness, harvested, recentProductivity, size, "paid births", free, phFood, phAlarm, phScent, phOwner, phHome, nest, stone, food`) with identical or improved copy, plus new keys used by later tasks.
  - `explainText(key: string): string | undefined`
  - `infoDot(key: string): HTMLElement` — a focusable `ⓘ` span; hover shows a floating popover, click/Enter/Space pins it, outside-click/Esc unpins. Unknown key → returns an empty-but-valid span that shows nothing (no throw).
- Consumes: nothing from other tasks.

- [ ] **Step 1: Write the failing test**

Create `web/tests/explain.test.ts`:

```ts
import { describe, expect, it, beforeEach } from "vitest";
import { EXPLAIN, explainText, infoDot } from "../src/ui/explain.js";

// Keys that existing panels already rely on via TOOLTIPS; must survive the move.
const LEGACY_KEYS = [
  "pop", "store", "delivered", "energy", "generation", "carrying",
  "fitness", "harvested", "recentProductivity", "size", "paid births",
  "free", "phFood", "phAlarm", "phScent", "phOwner", "nest", "stone", "food",
];

describe("EXPLAIN registry", () => {
  it("keeps non-empty copy for every legacy tooltip key", () => {
    for (const k of LEGACY_KEYS) {
      expect(EXPLAIN[k], k).toBeTruthy();
      expect(EXPLAIN[k].length, k).toBeGreaterThan(8);
    }
  });

  it("explainText returns the copy or undefined for an unknown key", () => {
    expect(explainText("store")).toBe(EXPLAIN["store"]);
    expect(explainText("nope_not_a_key")).toBeUndefined();
  });
});

describe("infoDot", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("renders an ⓘ span carrying the copy for a known key", () => {
    const el = infoDot("store");
    expect(el.textContent).toContain("ⓘ");
    expect(el.getAttribute("data-info")).toBe(EXPLAIN["store"]);
  });

  it("shows a popover on click and removes it on Escape", () => {
    document.body.append(infoDot("store"));
    const dot = document.querySelector(".info-dot") as HTMLElement;
    dot.click();
    expect(document.querySelector(".info-pop")).not.toBeNull();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(document.querySelector(".info-pop")).toBeNull();
  });

  it("does not throw and shows nothing for an unknown key", () => {
    const el = infoDot("nope_not_a_key");
    document.body.append(el);
    el.click();
    expect(document.querySelector(".info-pop")).toBeNull();
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd web && npx vitest run tests/explain.test.ts`
Expected: FAIL — cannot resolve `../src/ui/explain.js`.

- [ ] **Step 3: Implement `explain.ts`**

Create `web/src/ui/explain.ts`. Move the full contents of the current `TOOLTIPS` dict here as the seed of `EXPLAIN` (copy every key/value from `web/src/ui/tooltips.ts` verbatim), then add the new keys the sweep needs (Tasks 4–5 list them; adding them now is fine). Implement a single shared popover element and pin logic:

```ts
/**
 * One place for every scrap of explanatory copy in the UI, and the ⓘ control
 * that surfaces it. Splitting this copy across tooltips, slider hints, and the
 * NN labels is how half the panels ended up unexplained; this is the single
 * source of truth. `tooltips.ts` re-exports this map so existing `tipLabel`
 * call sites keep working unchanged.
 */

export const EXPLAIN: Record<string, string> = {
  // --- moved verbatim from tooltips.ts (keep keys identical) ---
  pop: "Ants alive right now.",
  // ...every other current TOOLTIPS entry, unchanged...

  // --- new: playback / layers / world controls (Task 4) ---
  "ctl.pause": "Play or pause the simulation. Space also toggles it.",
  "ctl.step": "Advance exactly one tick, then stay paused. The way to watch the world — and any ant's brain — change one step at a time.",
  "ctl.speed": "Ticks per animation frame: 1×, 10×, or 100×. Higher burns through generations faster but you see less.",
  "layer.food": "Overlay the food-trail pheromone: laid by laden ants, points back to food.",
  "layer.alarm": "Overlay the alarm pheromone: spikes where ants were attacked.",
  "layer.scent": "Overlay territory scent: each colony's claim on the ground, tinted by owner.",
  "layer.home": "Overlay the shared home/exploration trail every ant lays and reads.",
  "layer.trail": "Overlay the fast-fading colony recent-path trail (own colony).",
  "ctl.labels": "Show colony name labels over each nest.",
  "ctl.pheroRes": "Pheromone texture resolution. 512² is sharper but heavier than 256².",
  "ctl.save": "Save the current world to the server's slot.",
  "ctl.load": "Reload the last saved world.",
  "ctl.reset": "Restart the world from the given seed. Same seed → same world.",

  // --- new: stats chart titles (Task 4) ---
  "stat.delivered": "Lifetime food carried home, summed across colonies. The core fitness signal.",
  "stat.population": "Ants alive per colony over time.",
  "stat.generation": "Mean lineage depth — how many births deep the living ants are, on average.",
  "stat.distinct": "How many distinct lineage depths are alive at once — a spread of generations.",
  "stat.refounds": "How many times a colony collapsed to zero and re-seeded from its hall of fame.",
  "stat.store": "Spendable colony food fund over time (births and refueling draw it down).",

  // --- new: readout rows without a key today (Task 4) ---
  "id": "Stable per-ant id, assigned at birth.",
  "age": "Ticks this ant has been alive.",
  "deaths": "Ants in this colony that have died, lifetime.",
  "name": "This ant's given name (cosmetic).",
  "colony": "Which colony this ant belongs to.",

  // --- new: section headings (Task 4) ---
  "sec.traits": "Fixed, heritable body/brain parameters set at birth — never change during life, only across generations.",
  "sec.inputs": "The 60 numbers the ant's network senses this tick.",
  "sec.outputs": "The 8 numbers the network produces each tick: a velocity command, attack, grab, and 4 recurrent memory values.",
};

// Slider copy (Task 5) is keyed `tune.<id>` and added in that task.

let pinned: HTMLElement | null = null;

function removePop(): void {
  document.querySelector(".info-pop")?.remove();
  pinned = null;
}

function showPop(anchor: HTMLElement, text: string, pin: boolean): void {
  removePop();
  const pop = document.createElement("div");
  pop.className = "info-pop";
  pop.textContent = text;
  document.body.append(pop);
  const r = anchor.getBoundingClientRect();
  // Measure then flip so it never clips the viewport edge.
  let left = r.left;
  let top = r.bottom + 6;
  if (left + pop.offsetWidth > window.innerWidth - 6) {
    left = window.innerWidth - pop.offsetWidth - 6;
  }
  if (top + pop.offsetHeight > window.innerHeight - 6) {
    top = r.top - pop.offsetHeight - 6;
  }
  pop.style.left = `${Math.max(6, left)}px`;
  pop.style.top = `${Math.max(6, top)}px`;
  if (pin) pinned = pop;
}

export function explainText(key: string): string | undefined {
  return EXPLAIN[key];
}

export function infoDot(key: string): HTMLElement {
  const el = document.createElement("span");
  el.className = "info-dot";
  el.textContent = "ⓘ";
  el.setAttribute("tabindex", "0");
  const text = EXPLAIN[key];
  if (!text) {
    // Fail soft: an unknown key is a dead, silent dot, never a thrown render.
    if (import.meta.env?.DEV) console.warn(`infoDot: no copy for "${key}"`);
    el.classList.add("info-dot-empty");
    return el;
  }
  el.setAttribute("data-info", text);

  el.addEventListener("mouseenter", () => {
    if (!pinned) showPop(el, text, false);
  });
  el.addEventListener("mouseleave", () => {
    if (!pinned) removePop();
  });
  const toggle = (e: Event) => {
    e.stopPropagation();
    if (pinned) removePop();
    else showPop(el, text, true);
  };
  el.addEventListener("click", toggle);
  el.addEventListener("keydown", (e) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      toggle(e);
    }
  });
  return el;
}

// A pinned popover closes on the next outside click or Escape.
document.addEventListener("click", () => {
  if (pinned) removePop();
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && pinned) removePop();
});
```

When copying the `TOOLTIPS` entries, include **every** key currently in `web/src/ui/tooltips.ts` (`pop, store, delivered, energy, generation, carrying, fitness, harvested, recentProductivity, size, "paid births", free, phFood, phAlarm, phScent, phOwner, phHome, nest, stone, food`). Note `phHome` — the tile readout in `explorer.ts` passes `"phHome"` to `tipLabel`, so it must exist even though the current `tooltips.ts` snapshot may not list it; if it is missing there, add copy: `"Shared home/exploration trail strength on this cell."`

- [ ] **Step 4: Back `TOOLTIPS` with `EXPLAIN` in `tooltips.ts`**

Edit `web/src/ui/tooltips.ts` so `TOOLTIPS` is the registry, not a second copy:

```ts
import { EXPLAIN } from "./explain.js";

/** @deprecated Use `explain.ts` / `infoDot` directly. Kept so existing
 *  `tipLabel(text, key)` call sites resolve against the one registry. */
export const TOOLTIPS: Record<string, string> = EXPLAIN;
```

Keep `tipLabel` and `tipText` exactly as they are (they read `TOOLTIPS[key]`). Delete the old inline `TOOLTIPS` object literal (its entries now live in `EXPLAIN`).

- [ ] **Step 5: Add CSS to `web/index.html`**

In the `<style>` block (near the existing `.tip` / `.nn-pop` rules), add:

```css
.info-dot { display: inline-flex; align-items: center; justify-content: center;
  width: 13px; height: 13px; margin-left: 4px; border-radius: 50%;
  font-size: 10px; line-height: 1; color: var(--dim); cursor: help;
  user-select: none; vertical-align: middle; }
.info-dot:hover, .info-dot:focus { color: var(--accent); outline: none; }
.info-dot-empty { display: none; }
.info-pop { position: fixed; z-index: 60; max-width: 260px;
  background: #06070a; color: var(--text); border: 1px solid var(--line);
  border-radius: 5px; padding: 6px 8px; font-size: 11px; line-height: 1.4;
  box-shadow: 0 6px 18px rgba(0,0,0,0.6); }
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cd web && npx vitest run tests/explain.test.ts tests/tooltips.test.ts`
Expected: PASS — both the new registry test and the existing tooltip test (now reading `EXPLAIN`) are green.

- [ ] **Step 7: Typecheck**

Run: `cd web && npm run typecheck`
Expected: exit 0.

- [ ] **Step 8: Commit**

```bash
git add web/src/ui/explain.ts web/src/ui/tooltips.ts web/index.html web/tests/explain.test.ts
git commit -m "feat(web): central explanation registry and infoDot affordance"
```

---

### Task 2: Per-input explanations (`inputInfo`) with computation

**Files:**
- Modify: `web/src/nnlabels.ts` (add `inputInfo`, feed `nodeInfo`)
- Test: `web/tests/nnlabels.test.ts` (extend)

**Interfaces:**
- Consumes: `inputLabel(i)`, `INPUT_GROUPS`, `OUTPUT_DESC` (already in the file); `N_INPUTS` from `protocol.js`.
- Produces:
  - `inputInfo(i: number): { label: string; desc: string }` — `label === inputLabel(i)`, `desc` is a non-empty meaning-plus-computation string for every `i` in `0..N_INPUTS`.
  - `nodeInfo(layer, index)` updated so an input node (`layer === 0`) returns `{ label, desc }` from `inputInfo` (outputs and hidden unchanged).

- [ ] **Step 1: Write the failing test**

Append to `web/tests/nnlabels.test.ts`:

```ts
import { inputInfo } from "../src/nnlabels.js";

describe("inputInfo", () => {
  it("gives every input a label matching inputLabel and a non-empty computation", () => {
    for (let i = 0; i < N_INPUTS; i++) {
      const info = inputInfo(i);
      expect(info.label, `label ${i}`).toBe(inputLabel(i));
      expect(info.desc.length, `desc ${i}`).toBeGreaterThan(12);
    }
  });

  it("explains how a whisker food channel is computed", () => {
    // whisker 2 (ahead), channel 0 (food) = index 16.
    expect(inputInfo(16).desc.toLowerCase()).toContain("food_patch_max");
  });

  it("explains the home vector and facing math", () => {
    expect(inputInfo(55).desc.toLowerCase()).toContain("nest"); // home vector x
    expect(inputInfo(58).desc.toLowerCase()).toContain("sin"); // facing (sin)
  });

  it("nodeInfo returns a description for input nodes now", () => {
    const info = nodeInfo(0, 46); // energy
    expect(info.label).toBe("energy");
    expect(info.desc).toBeTruthy();
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd web && npx vitest run tests/nnlabels.test.ts`
Expected: FAIL — `inputInfo` is not exported; `nodeInfo(0, 46).desc` is undefined.

- [ ] **Step 3: Implement `inputInfo` and update `nodeInfo`**

In `web/src/nnlabels.ts`, add channel-level and group-level computation copy, then `inputInfo`. Derive the channel/group the same way `inputLabel` does (whiskers by `i % CH` and `Math.floor(i / CH)`, non-whiskers by group start). Full copy:

```ts
/** How each whisker channel is computed. Indexed by CH_* order (see sense.rs). */
const CHANNEL_DESC = [
  "Food seen along this antenna: grid food on the sampled cell ÷ food_patch_max, capped at 1. The cell is `vision` steps out at the whisker's angle from your heading.",
  "Food-trail pheromone along this antenna, log-squashed: ln(1 + value) ÷ phero_log_div, capped at 1.",
  "Alarm pheromone along this antenna, log-squashed the same way (spikes where ants were attacked).",
  "Your own colony's territory scent along this antenna, log-squashed.",
  "Rival colonies' scent along this antenna, log-squashed — enemy territory.",
  "1 if the sampled cell is stone or off the map, else 0.",
  "Shared home/exploration trail along this antenna, log-squashed — the trail every ant lays and reads.",
  "Your own colony's fast-fading recent-path trail along this antenna, log-squashed.",
] as const;

/** How each non-whisker input is computed, keyed by its inputLabel(). */
const INPUT_DESC: Record<string, string> = {
  "underfoot food": "Food on the cell you stand on ÷ food_patch_max, capped at 1.",
  "underfoot trail": "Food-trail pheromone on your cell, log-squashed.",
  "underfoot alarm": "Alarm pheromone on your cell, log-squashed.",
  "underfoot home trail": "Shared home/exploration trail on your cell, log-squashed.",
  "friends near": "Same-colony ants within 2 cells, not counting you, ÷ 8, capped at 1.",
  "foes near": "Other-colony ants within 2 cells ÷ 8, capped at 1.",
  "energy": "Fuel fraction: energy ÷ max_energy, clamped 0–1.",
  "size": "Body size ÷ your max_size trait, clamped 0–1.",
  "carrying": "Food in hand ÷ your carry_capacity trait, clamped 0–1.",
  "age": "Ticks alive ÷ your lifespan trait, clamped 0–1.",
  "bias": "Constant 1. A fixed input the network can weight as a learnable offset.",
  "memory 0": "Recurrent memory: whatever the brain wrote to memory output 0 last tick.",
  "memory 1": "Recurrent memory: whatever the brain wrote to memory output 1 last tick.",
  "memory 2": "Recurrent memory: whatever the brain wrote to memory output 2 last tick.",
  "memory 3": "Recurrent memory: whatever the brain wrote to memory output 3 last tick.",
  "home vector x": "World-frame unit vector toward your nest, X component: (nest_x − x) ÷ distance. Zero on the nest.",
  "home vector y": "World-frame unit vector toward your nest, Y component: (nest_y − y) ÷ distance. Zero on the nest.",
  "home distance": "Distance to your nest ÷ the map diagonal, capped at 1.",
  "facing (sin)": "sin of your heading — lets the network read its own facing without the ±π wrap a raw angle jumps at.",
  "facing (cos)": "cos of your heading — the other half of the facing signal.",
};

const CH_COUNT = CHANNELS.length; // 8

/** Meaning + computation for input index `i` (0..N_INPUTS). */
export function inputInfo(i: number): { label: string; desc: string } {
  const label = inputLabel(i);
  if (i < CH_COUNT * WHISKER_DIRS.length) {
    return { label, desc: CHANNEL_DESC[i % CH_COUNT] };
  }
  return { label, desc: INPUT_DESC[label] ?? "" };
}
```

Then change `nodeInfo` so input nodes carry the desc:

```ts
export function nodeInfo(layer: number, index: number): { label: string; desc?: string } {
  if (layer === 0) return inputInfo(index);
  if (layer === 3) return { label: `output · ${OUTPUT_LABELS[index]}`, desc: OUTPUT_DESC[index] };
  return { label: `hidden ${layer} · neuron ${index}` };
}
```

Add a guard next to the existing ones at the bottom of the file:

```ts
// Fail loudly if any input lacks a computation string.
for (let i = 0; i < N_INPUTS; i++) {
  if (!inputInfo(i).desc) throw new Error(`nnlabels: input ${i} has no computation copy`);
}
```

(`CHANNELS`, `WHISKER_DIRS`, `inputLabel`, `OUTPUT_LABELS`, `OUTPUT_DESC`, `N_INPUTS` are already imported/defined in the file.)

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd web && npx vitest run tests/nnlabels.test.ts`
Expected: PASS.

- [ ] **Step 5: Typecheck**

Run: `cd web && npm run typecheck`
Expected: exit 0.

- [ ] **Step 6: Commit**

```bash
git add web/src/nnlabels.ts web/tests/nnlabels.test.ts
git commit -m "feat(web): per-input explanations with computation (inputInfo)"
```

---

### Task 3: Fullscreen NN modal (`nnmodal.ts`)

**Files:**
- Create: `web/src/ui/nnmodal.ts`
- Modify: `web/index.html` (CSS for `.nn-backdrop`, `.nn-modal-panel`, the input list)
- Test: `web/tests/nnmodal.test.ts`

**Interfaces:**
- Consumes: `Store` from `../state.js`; `Net` from `../net.js`; `cmdSetPaused`, `cmdStep` from `../protocol.js`; `draw`, `activationColor` from `./nnview.js`; `attachNNPopover` from `./inspector.js`; `INPUT_GROUPS`, `inputInfo`, `OUTPUT_LABELS`, `OUTPUT_DESC` from `../nnlabels.js`; `infoDot` from `./explain.js`.
- Produces: `openNN(store: Store, net: Net): void` — singleton modal; a second call replaces the first. No-op-safe when `store.state.detail` is null (shows a "select an ant" message rather than a network).

- [ ] **Step 1: Write the failing test**

Create `web/tests/nnmodal.test.ts`. jsdom has no canvas 2d context, so the test asserts the modal's DOM scaffolding and teardown, not the pixels (mirrors how `graphmodal` is exercised indirectly). Build a minimal fake store/net:

```ts
import { describe, expect, it, beforeEach, vi } from "vitest";
import { openNN } from "../src/ui/nnmodal.js";
import { N_INPUTS } from "../src/protocol.js";

function fakeStore(detail: unknown) {
  const subs: Array<() => void> = [];
  return {
    state: { detail, genome: { params: new Float32Array(1) }, paused: true, tick: 42 },
    subscribe(fn: () => void) { subs.push(fn); return () => {}; },
    _emit() { for (const f of subs) f(); },
  } as any;
}
function fakeDetail() {
  return {
    alive: true, id: 1,
    inputs: new Float32Array(N_INPUTS).fill(0.5),
    h1: new Float32Array(16), h2: new Float32Array(16),
    outputs: new Float32Array(8),
  };
}

describe("openNN", () => {
  beforeEach(() => { document.body.innerHTML = ""; });

  it("mounts a backdrop with a panel and closes on Escape", () => {
    const net = { send: vi.fn() } as any;
    openNN(fakeStore(fakeDetail()), net);
    expect(document.querySelector(".nn-backdrop")).not.toBeNull();
    expect(document.querySelector(".nn-modal-panel")).not.toBeNull();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(document.querySelector(".nn-backdrop")).toBeNull();
  });

  it("lists a row per input", () => {
    openNN(fakeStore(fakeDetail()), { send: vi.fn() } as any);
    expect(document.querySelectorAll(".nnm-row").length).toBe(N_INPUTS + 8);
  });

  it("step button sends cmdStep and pauses", () => {
    const net = { send: vi.fn() } as any;
    openNN(fakeStore(fakeDetail()), net);
    (document.querySelector(".nnm-step") as HTMLElement).click();
    // cmdSetPaused(true) + cmdStep() → two sends.
    expect(net.send).toHaveBeenCalled();
  });

  it("is safe with no ant selected", () => {
    openNN(fakeStore(null), { send: vi.fn() } as any);
    expect(document.querySelector(".nn-backdrop")).not.toBeNull();
    expect(document.body.textContent).toContain("select an ant");
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd web && npx vitest run tests/nnmodal.test.ts`
Expected: FAIL — cannot resolve `../src/ui/nnmodal.js`.

- [ ] **Step 3: Implement `nnmodal.ts`**

Create `web/src/ui/nnmodal.ts`, modeled on `graphmodal.ts` (singleton `close`, backdrop, Esc, resize, `store.subscribe` render, full teardown):

```ts
/**
 * The selected ant's brain, fullscreen — the NN counterpart to the graph modal.
 * The pane view is 260px tall and cannot show 60 inputs legibly; here the
 * network is large, every input is listed with its value and how it is
 * computed, and the playback controls are in reach so you can pause, step one
 * tick, and watch the activations move. Reads store.state.detail, which the
 * server refreshes every tick — no history buffer, no new protocol.
 */

import { cmdSetPaused, cmdStep } from "../protocol.js";
import type { Net } from "../net.js";
import type { Store } from "../state.js";
import { draw as drawNet, activationColor } from "./nnview.js";
import { attachNNPopover } from "./inspector.js";
import { INPUT_GROUPS, OUTPUT_DESC, OUTPUT_LABELS, inputInfo } from "../nnlabels.js";
import { infoDot } from "./explain.js";

let close: (() => void) | null = null;

export function openNN(store: Store, net: Net): void {
  if (close) close();

  const backdrop = document.createElement("div");
  backdrop.className = "nn-backdrop";
  const panel = document.createElement("div");
  panel.className = "nn-modal-panel";
  backdrop.append(panel);

  // --- top bar: playback + tick ---
  const bar = document.createElement("div");
  bar.className = "nnm-bar";
  const pauseBtn = document.createElement("button");
  pauseBtn.className = "nnm-pause";
  pauseBtn.addEventListener("click", () => {
    const next = !store.state.paused;
    store.setPaused(next);
    net.send(cmdSetPaused(next));
  });
  const stepBtn = document.createElement("button");
  stepBtn.className = "nnm-step";
  stepBtn.textContent = "⏭ step";
  stepBtn.addEventListener("click", () => {
    store.setPaused(true);
    net.send(cmdSetPaused(true));
    net.send(cmdStep());
  });
  const tick = document.createElement("span");
  tick.className = "nnm-tick muted";
  const spacer = document.createElement("span");
  spacer.style.flex = "1";
  const closeBtn = document.createElement("button");
  closeBtn.textContent = "✕";
  closeBtn.title = "close (Esc)";
  closeBtn.addEventListener("click", () => close?.());
  bar.append(pauseBtn, stepBtn, tick, spacer, closeBtn);

  // --- body: canvas left, list right ---
  const bodyWrap = document.createElement("div");
  bodyWrap.className = "nnm-body";
  const canvasWrap = document.createElement("div");
  canvasWrap.className = "nnm-canvas-wrap";
  const canvas = document.createElement("canvas");
  canvas.className = "nnm-canvas";
  attachNNPopover(canvas, store); // node hover popover, now carrying desc
  canvasWrap.append(canvas);

  const side = document.createElement("div");
  side.className = "nnm-side";
  const filter = document.createElement("input");
  filter.className = "nnm-filter";
  filter.type = "text";
  filter.placeholder = "filter inputs…";
  const list = document.createElement("div");
  list.className = "nnm-list";
  side.append(filter, list);

  bodyWrap.append(canvasWrap, side);
  panel.append(bar, bodyWrap);
  document.body.append(backdrop);

  // Build the list rows once; update values on each render.
  interface Row { el: HTMLElement; chip: HTMLElement; val: HTMLElement; label: string; get: () => number; }
  const rows: Row[] = [];
  const addRow = (label: string, desc: string, get: () => number) => {
    const el = document.createElement("div");
    el.className = "nnm-row";
    const chip = document.createElement("span");
    chip.className = "nnm-chip";
    const name = document.createElement("span");
    name.className = "nnm-name";
    name.textContent = label;
    if (desc) name.append(infoDot("")); // placeholder replaced below
    const val = document.createElement("b");
    val.className = "nnm-val";
    el.append(chip, name, val);
    list.append(el);
    rows.push({ el, chip, val, label, get });
    return { el, name, desc };
  };

  const detail = () => store.state.detail;
  // Inputs, grouped.
  for (const g of INPUT_GROUPS) {
    const gh = document.createElement("div");
    gh.className = "nnm-group";
    gh.textContent = g.name;
    list.append(gh);
    for (let k = 0; k < g.len; k++) {
      const idx = g.start + k;
      const info = inputInfo(idx);
      const built = addRow(info.label, info.desc, () => detail()?.inputs[idx] ?? 0);
      built.name.append(infoDotFor(info.desc));
    }
  }
  // Outputs.
  const oh = document.createElement("div");
  oh.className = "nnm-group";
  oh.textContent = "outputs";
  list.append(oh);
  OUTPUT_LABELS.forEach((label, i) => {
    const built = addRow(label, OUTPUT_DESC[i], () => detail()?.outputs[i] ?? 0);
    built.name.append(infoDotFor(OUTPUT_DESC[i]));
  });

  const applyFilter = () => {
    const q = filter.value.trim().toLowerCase();
    for (const r of rows) r.el.style.display = !q || r.label.toLowerCase().includes(q) ? "" : "none";
  };
  filter.addEventListener("input", applyFilter);

  const sizeCanvas = () => {
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const w = Math.max(1, Math.round(canvas.clientWidth * dpr));
    const h = Math.max(1, Math.round(canvas.clientHeight * dpr));
    if (canvas.width !== w || canvas.height !== h) { canvas.width = w; canvas.height = h; }
    return { w, h };
  };

  const render = () => {
    const d = detail();
    const st = store.state;
    pauseBtn.textContent = st.paused ? "▶ run" : "❚❚ pause";
    pauseBtn.classList.toggle("on", !st.paused);
    tick.textContent = `tick ${st.tick.toLocaleString()}`;

    const ctx = canvas.getContext("2d");
    const { w, h } = sizeCanvas();
    if (!d || !d.alive) {
      if (ctx) drawNet(ctx, w, h, null, null);
      for (const r of rows) { r.val.textContent = "—"; r.chip.style.background = "#222"; }
      if (!list.querySelector(".nnm-empty")) {
        const m = document.createElement("div");
        m.className = "nnm-empty muted";
        m.textContent = "select an ant to inspect its brain";
        list.prepend(m);
      }
      return;
    }
    list.querySelector(".nnm-empty")?.remove();
    if (ctx) drawNet(ctx, w, h, d, st.genome?.params ?? null);
    for (const r of rows) {
      const v = r.get();
      r.val.textContent = `${v >= 0 ? "+" : ""}${v.toFixed(2)}`;
      r.chip.style.background = activationColor(v);
    }
  };

  const onResize = () => render();
  window.addEventListener("resize", onResize);
  const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") close?.(); };
  window.addEventListener("keydown", onKey);
  backdrop.addEventListener("click", (e) => { if (e.target === backdrop) close?.(); });

  const unsub = store.subscribe(render);
  render();

  close = () => {
    unsub();
    window.removeEventListener("resize", onResize);
    window.removeEventListener("keydown", onKey);
    backdrop.remove();
    close = null;
  };
}

/** A dot that shows arbitrary copy (not a registry key). */
function infoDotFor(desc: string): HTMLElement {
  const el = document.createElement("span");
  el.className = "info-dot";
  el.textContent = "ⓘ";
  el.title = desc; // native fallback; the modal is dense enough that title is fine here
  return el;
}
```

Note: remove the stray `name.append(infoDot(""))` placeholder line inside `addRow` — rows get their dot via `infoDotFor(desc)` at the call sites. `addRow` should only build chip/name/val. (Keep `addRow` returning `{ el, name, desc }` so callers can append the dot.)

If `Store` lacks a `setPaused` method, match whatever `controls.ts` calls (it uses `store.setPaused(next)`), so it exists.

- [ ] **Step 4: Add CSS to `web/index.html`**

```css
/* Fullscreen NN modal. */
.nn-backdrop { position: fixed; inset: 0; background: rgba(0,0,0,0.55); z-index: 50;
  display: flex; align-items: center; justify-content: center; }
.nn-modal-panel { background: var(--panel); border: 1px solid var(--line); border-radius: 8px;
  width: min(1200px, 94vw); height: min(820px, 90vh); display: flex; flex-direction: column;
  padding: 12px 14px; box-shadow: 0 12px 40px rgba(0,0,0,0.5); }
.nnm-bar { display: flex; align-items: center; gap: 8px; margin-bottom: 10px; }
.nnm-bar button { font-size: 11px; padding: 3px 8px; background: var(--panel-2); color: var(--text);
  border: 1px solid var(--line); border-radius: 4px; cursor: pointer; }
.nnm-bar button.on { background: var(--accent); color: #06121f; border-color: var(--accent); font-weight: 600; }
.nnm-tick { font-size: 11px; }
.nnm-body { flex: 1; min-height: 0; display: flex; gap: 12px; }
.nnm-canvas-wrap { flex: 1; min-width: 0; }
.nnm-canvas { width: 100%; height: 100%; display: block; background: #0a0a0c;
  border: 1px solid var(--line); border-radius: 5px; }
.nnm-side { width: 340px; flex-shrink: 0; display: flex; flex-direction: column; min-height: 0; }
.nnm-filter { background: var(--panel-2); color: var(--text); border: 1px solid var(--line);
  border-radius: 4px; padding: 4px 6px; font: inherit; font-size: 11px; margin-bottom: 6px; }
.nnm-list { flex: 1; overflow-y: auto; }
.nnm-group { font-size: 10px; text-transform: uppercase; letter-spacing: 0.08em; color: var(--dim);
  margin: 8px 0 3px; }
.nnm-row { display: flex; align-items: center; gap: 6px; padding: 1px 0; font-size: 11px; }
.nnm-chip { width: 10px; height: 10px; border-radius: 2px; flex-shrink: 0; }
.nnm-name { flex: 1; min-width: 0; color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.nnm-val { font-variant-numeric: tabular-nums; color: var(--dim); }
.nnm-empty { padding: 12px 0; }
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cd web && npx vitest run tests/nnmodal.test.ts`
Expected: PASS.

- [ ] **Step 6: Typecheck**

Run: `cd web && npm run typecheck`
Expected: exit 0.

- [ ] **Step 7: Commit**

```bash
git add web/src/ui/nnmodal.ts web/index.html web/tests/nnmodal.test.ts
git commit -m "feat(web): fullscreen NN modal with step-through and per-input list"
```

---

### Task 4: Wire the modal's expand button + sweep dots across panels

**Files:**
- Modify: `web/src/ui/inspector.ts` (expand button on the `#nn` canvas; section-heading dots)
- Modify: `web/src/ui/explorer.ts` (pass `net`/opener through; row dots)
- Modify: `web/src/main.ts` (thread `net` to the explorer so the modal can send commands)
- Modify: `web/src/ui/stats.ts` (chart-title dots)
- Modify: `web/src/ui/controls.ts` (playback/layer/world dots)
- Test: none new (covered by Tasks 1–3 unit tests + the manual smoke in Task 6). Run the full suite to catch regressions.

**Interfaces:**
- Consumes: `openNN` (Task 3), `infoDot` (Task 1).
- Produces: an `⤢` button overlaid on the Explorer NN canvas that calls `openNN(store, net)`; `ⓘ` dots on section headings, stat titles, and control rows.

- [ ] **Step 1: Thread `net` to the Explorer and add the expand button**

`renderAntDetail` / `mountExplorer` currently take only `store`. The modal needs `net`. In `web/src/main.ts`, find where `mountExplorer(pane, store)` is called and change it to pass `net` (the same `Net` already constructed there and handed to `mountControls`). Update `mountExplorer(pane: HTMLElement, store: Store, net: Net)` and `renderAntDetail(body, canvas, store, net)` signatures accordingly.

In `inspector.ts`, wrap the `#nn` canvas append so an expand button sits over it. Where `renderAntDetail` does `body.append(canvas)`, instead:

```ts
import { openNN } from "./nnmodal.js";
import { infoDot } from "./explain.js";
// ...
const nnWrap = document.createElement("div");
nnWrap.className = "nn-wrap";
const expand = document.createElement("button");
expand.className = "nn-expand";
expand.textContent = "⤢";
expand.title = "fullscreen brain";
expand.addEventListener("click", () => openNN(store, net));
nnWrap.append(canvas, expand);
body.append(nnWrap);
```

Add CSS to `web/index.html`:

```css
.nn-wrap { position: relative; }
.nn-expand { position: absolute; top: 12px; right: 6px; z-index: 2;
  font-size: 12px; line-height: 1; padding: 2px 5px; background: var(--panel-2);
  color: var(--text); border: 1px solid var(--line); border-radius: 4px; cursor: pointer; }
.nn-expand:hover { color: var(--accent); border-color: var(--accent); }
```

(The `#nn` rule already has `margin-top: 8px`; keeping the canvas inside `.nn-wrap` preserves layout. If the margin now doubles, move `margin-top` from `#nn` to `.nn-wrap`.)

- [ ] **Step 2: Section-heading dots in `inspector.ts`**

The `heading(text)` helper builds an `<h2>`. Add an optional key: `heading(text: string, key?: string)` that appends `infoDot(key)` when a key is given. Use it for the three sections:
- `heading("Traits", "sec.traits")`
- Inside `inputsSection`, `heading("Inputs", "sec.inputs")`
- `heading("Outputs", "sec.outputs")`

- [ ] **Step 3: Stat-title dots in `stats.ts`**

Where each chart's `title` div is built (`title.textContent = m.label`), append a dot keyed by metric. The metric keys are `delivered, population, generation, distinct, refounds, store`; the `EXPLAIN` keys are `stat.<key>`. Append `infoDot(\`stat.${m.key}\`)` to the title element. Stop click-through opening the graph when the dot is used: the dot's own click handler already calls `e.stopPropagation()`, so the chart's open-graph click won't fire.

- [ ] **Step 4: Control dots in `controls.ts`**

Add `infoDot`s (import from `../ui/explain.js`):
- After the Playback `section("Playback")` header, or beside the pause/step buttons: append `infoDot("ctl.step")` to the play row (the step control is the one worth explaining).
- Beside each layer checkbox label: the `checkbox(key, ...)` builds a `<label>`; append `infoDot(\`layer.${key}\`)` to it for `food/alarm/scent/home/trail`, and `infoDot("ctl.labels")` to the labels checkbox.
- Beside the phero-res button row: `infoDot("ctl.pheroRes")`.
- Beside the World save/load/reset row: `infoDot("ctl.save")` (one dot for the row is enough).

Keep changes minimal and additive — do not restructure `mountControls`.

- [ ] **Step 5: Explorer row dots for keyless rows**

In `explorer.ts`, the `kvRow(kv, label, key, value)` helper appends `tipLabel(label, key)`. For rows currently passing `""` (id, age, deaths, name, colony), give them real keys now present in `EXPLAIN` (`id, age, deaths, name, colony`) so their labels gain hover copy. This is a one-word change per row (swap `""` for the key). No new dot needed — `tipLabel` already reveals copy — but the keys must resolve, which Task 1 guarantees.

- [ ] **Step 6: Run the full suite + typecheck**

Run: `cd web && npm run typecheck && npm test`
Expected: typecheck exit 0; all test files pass (no regressions in `tooltips`, `nnlabels`, `explain`, `nnmodal`, and the rest).

- [ ] **Step 7: Commit**

```bash
git add web/src/ui/inspector.ts web/src/ui/explorer.ts web/src/main.ts web/src/ui/stats.ts web/src/ui/controls.ts web/index.html
git commit -m "feat(web): fullscreen brain button and explanation dots across panels"
```

---

### Task 5: Slider explanations (tunables)

**Files:**
- Modify: `web/src/ui/explain.ts` (add `tune.<id>` keys)
- Modify: `web/src/ui/controls.ts` (dot beside each slider label)
- Test: `web/tests/explain.test.ts` (extend: every tunable id has copy)

**Interfaces:**
- Consumes: `TUNABLES` from `./tunables.js` (ids 0..23), `infoDot`/`EXPLAIN` from `./explain.js`.
- Produces: `EXPLAIN["tune.<id>"]` for every id `0..23`.

- [ ] **Step 1: Write the failing test**

Append to `web/tests/explain.test.ts`:

```ts
import { TUNABLES } from "../src/ui/tunables.js";

describe("slider copy", () => {
  it("has non-empty copy for every tunable id", () => {
    for (const t of TUNABLES) {
      const copy = EXPLAIN[`tune.${t.id}`];
      expect(copy, `tune.${t.id} (${t.label})`).toBeTruthy();
      expect(copy.length, `tune.${t.id}`).toBeGreaterThan(12);
    }
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd web && npx vitest run tests/explain.test.ts`
Expected: FAIL — `tune.*` keys are missing.

- [ ] **Step 3: Add the `tune.<id>` copy to `EXPLAIN`**

In `web/src/ui/explain.ts`, add (ids and labels mirror `tunables.ts`):

```ts
  // --- tuning sliders (ids mirror CONFIG_FIELDS / tunables.ts) ---
  "tune.0": "Food-trail evaporation per tick. Nearer 1 = trails linger; the last decimal matters most.",
  "tune.1": "Alarm evaporation per tick. Nearer 1 = alarm lingers.",
  "tune.2": "Territory-scent evaporation per tick. Nearer 1 = claims persist.",
  "tune.3": "Food-trail diffusion: how much pheromone bleeds to neighboring cells each tick.",
  "tune.4": "Alarm diffusion per tick.",
  "tune.5": "Scent diffusion per tick.",
  "tune.6": "Energy tax per unit speed — the cost of moving fast.",
  "tune.7": "Energy tax per unit vision (×8: the trait ranges to 8). Seeing farther costs upkeep.",
  "tune.8": "Mutation rate: the chance each brain parameter is perturbed at birth.",
  "tune.9": "Mutation sigma: how large a perturbation is when it happens.",
  "tune.10": "Birth cost: food drawn from the store per paid birth. One foraging trip yields ~10.",
  "tune.11": "Harvest rate: food picked up per tick while standing on food.",
  "tune.12": "Refuel rate: energy restored per tick at the nest. High values let loiterers drain the store.",
  "tune.13": "Growth threshold: energy fraction an ant must hold before it spends any on growing.",
  "tune.14": "Ticks between food-relocation passes: how often depleted patches move elsewhere.",
  "tune.15": "Attack damage per successful bite.",
  "tune.16": "Harvest weight in fitness: 0 = deliver-only; nudging it up rewards picking food up at all.",
  "tune.17": "Homing weight: a fitness credit for carrying food toward home. Helps bootstrap foraging.",
  "tune.18": "Colony recent-path trail an ant lays each tick.",
  "tune.19": "Colony-trail evaporation. Fast decay = the trail means 'recent', not 'ever'.",
  "tune.20": "Colony-trail diffusion per tick.",
  "tune.21": "Productivity weight in fitness: rewards recent harvest/deliver/kills. 0 = cumulative only.",
  "tune.22": "Productivity decay: how fast 'recent' fades. 0.99 ≈ a 69-tick half-life.",
  "tune.23": "How many live food patches the world keeps on the map.",
```

- [ ] **Step 4: Add a dot beside each slider label in `controls.ts`**

In the `sliders = TUNABLES.map(...)` block, where `name.textContent = t.label` is set inside `head`, append `head.append(infoDot(\`tune.${t.id}\`))` (after the `name`/`val` append, so the dot sits at the end of the slider header). Keep the existing `t.hint` → `title` behavior untouched.

- [ ] **Step 5: Run the test + typecheck**

Run: `cd web && npx vitest run tests/explain.test.ts && npm run typecheck`
Expected: PASS; typecheck exit 0.

- [ ] **Step 6: Commit**

```bash
git add web/src/ui/explain.ts web/src/ui/controls.ts web/tests/explain.test.ts
git commit -m "feat(web): explanation copy and dots for all tuning sliders"
```

---

### Task 6: Full build + manual smoke

**Files:** none (verification only).

- [ ] **Step 1: Full test suite**

Run: `cd web && npm test`
Expected: all files pass.

- [ ] **Step 2: Production build (typecheck + bundle)**

Run: `cd web && npm run build`
Expected: `tsc --noEmit` clean, Vite writes `dist/`. No errors.

- [ ] **Step 3: Manual smoke (documented, run by the operator)**

Build the server and run it, then in the browser:
```bash
cd /Users/jschnur/dev/antsim2 && source "$HOME/.cargo/env" && cargo build --release -p server
./target/release/server --web web/dist
```
Confirm at http://127.0.0.1:8080:
1. Click an ant → the Explorer pane shows its detail with the NN and an `⤢` button.
2. Click `⤢` → the fullscreen brain modal opens: large network, a scrollable list of all 60 inputs (grouped) + 8 outputs, each with a value, color chip, and `ⓘ`.
3. Pause, click `⏭ step` → the network activations and the list values advance exactly one tick each click.
4. Hover a network node → popover names it and shows its computation. Hover/click an `ⓘ` in the list → the formula appears.
5. Esc / ✕ / backdrop-click all close the modal.
6. Across the left rail: playback, layer toggles, and every tuning slider show `ⓘ` dots; the Stats tab's chart titles show dots; clicking a stat dot does not open the graph.

- [ ] **Step 4: Commit (only if Step 3 surfaced doc-worthy notes)**

No code commit expected. If the smoke revealed a bug, return to the owning task rather than patching here.

---

## Self-Review

**Spec coverage:**
- Central registry + `infoDot` → Task 1. ✓
- Per-input meaning+computation → Task 2 (`inputInfo`, `nodeInfo`). ✓
- Fullscreen NN modal with step-through + input list → Task 3. ✓
- Expand button + systematic sweep (controls, stats, panels, sliders) → Tasks 4–5. ✓
- No sim/protocol/wire change → Global Constraints; every task is web-only. ✓
- Whisker grid gets header/direction copy, not 40 dots → covered by channel copy in Task 2 + fullscreen list; the grid itself keeps its existing `title` attributes. ✓
- Guards preserved/added → Task 2 keeps `INPUT_GROUPS`/`OUTPUT_LABELS` checks and adds the `inputInfo` coverage guard. ✓

**Type consistency:** `openNN(store, net)`, `infoDot(key)`, `inputInfo(i): {label, desc}`, `nodeInfo(): {label, desc?}` are named identically across the tasks that produce and consume them. `mountExplorer`/`renderAntDetail` gain a `net` parameter in Task 4, threaded from `main.ts`.

**Placeholder scan:** the one placeholder risk is the stray `infoDot("")` line inside `addRow` in Task 3's code — Step 3's note explicitly says to remove it and use `infoDotFor(desc)` at call sites. No TBDs elsewhere; all copy is written out.
