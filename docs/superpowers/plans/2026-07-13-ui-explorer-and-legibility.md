# UI Explorer, Ant Popover & Legibility — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the sim's numbers self-explanatory (hover tooltips + an ant "fitness/success" number + an evolution explainer) and turn the right rail into a tabbed, context-sensitive Explorer (ant / colony / tile / world), with a glance-level in-world ant popover.

**Architecture:** A single `selection` union in the store (`ant | colony | tile | none`) drives both the in-world ant popover and the Explorer pane. The right rail becomes two tabs (Colonies, Explorer). One additive wire field (`food_harvested` on the ant-detail frame) lets the client compute the exact fitness. Everything else is web-only, derived from frames already on the wire.

**Tech Stack:** Rust (`crates/sim`, `crates/server`), TypeScript + WebGL2/canvas2d (`web/`), Vitest, cargo test.

## Global Constraints

- No sim *behavior* change. The only Rust touch is serializing an
  already-tracked field (`Ants::food_harvested`) into the ant-detail frame.
  Determinism / golden master must stay green.
- Fitness is `delivered + harvest_weight * harvested` (`Config::fitness`),
  `harvest_weight` = config field id **16**, default **0.02**.
- Wire layout: `ANT_DETAIL_LEN` currently **421**; the fixed body ends right
  after the 8 output f32s (client offset 389..421). `food_harvested` is appended
  at offset **421**, making the new length **425**; the name tail follows at 425.
  All existing offsets (age@45, lineage@49, traits@53, inputs@85, h1@261,
  h2@325, outputs@389) are unchanged.
- Match existing file idioms: struct-of-DOM builders like `ColonyPanel` /
  `mountInspector`, comments explain *why*, no emoji.
- Run cargo with `source "$HOME/.cargo/env"` first. `cargo test` takes ONE
  positional filter. Web tests: `cd web && npx vitest run`.

---

## Task 1: Wire `food_harvested` through the ant-detail frame

**Files:**
- Modify: `crates/server/src/protocol.rs` (`ANT_DETAIL_LEN:33`, `AntDetail:437`, `encode_ant_detail:456`, three test constructors, guard test `778`)
- Modify: `crates/server/src/sim_thread.rs:376` and `:402` (the two `AntDetail` construction sites)
- Modify: `web/src/protocol.ts` (`AntDetail` interface `158`, decoder `313`)
- Test: `crates/server/src/protocol.rs` (guard test), `web/tests/protocol` (optional decode check — see step)

**Interfaces:**
- Produces (Rust): `AntDetail.food_harvested: f32`; `ANT_DETAIL_LEN == 425`.
- Produces (TS): `AntDetail.foodHarvested: number`, decoded at offset 421.

- [ ] **Step 1: Update the guard test to expect the new length and field first (red)**

In `crates/server/src/protocol.rs`, the guard test `an_ant_detail_frame_is_exactly_the_documented_length` already asserts `b.len() == ANT_DETAIL_LEN + 1`; that stays true once the constant changes. Add a field assertion and the new constructor field. Edit its `AntDetail { … }` to include `food_harvested: 9.0,` after `food_delivered: 0.0,`, and add after the existing offset asserts:

```rust
        // food_harvested is the last fixed f32, at offset 421 (just past outputs).
        assert_eq!(f32::from_le_bytes(b[421..425].try_into().unwrap()), 9.0);
```

- [ ] **Step 2: Run it — fails to compile (missing field / wrong length)**

Run: `source "$HOME/.cargo/env" && cargo test -p server an_ant_detail_frame_is_exactly_the_documented_length`
Expected: compile error — `AntDetail` has no field `food_harvested`.

- [ ] **Step 3: Add the field and bump the length constant**

`crates/server/src/protocol.rs`:
- Line 33: `pub const ANT_DETAIL_LEN: usize = 425;` (was 421)
- In `struct AntDetail`, after `pub food_delivered: f32,` add:

```rust
    pub food_harvested: f32,
```

- [ ] **Step 4: Encode it at the end of the fixed body**

In `encode_ant_detail`, after the `for v in d.act.outputs { put_f32(out, v); }` loop and **before** the `debug_assert_eq!(out.len(), ANT_DETAIL_LEN);` line, add:

```rust
    // Appended after the activations so every earlier offset is unchanged; the
    // client reads it at ANT_DETAIL_LEN - 4. Fitness = delivered + w*harvested,
    // and the inspector shows that number, so the client needs harvested too.
    put_f32(out, d.food_harvested);
```

- [ ] **Step 5: Fix the other two test constructors in protocol.rs**

The tests near lines 841 and 868 also build `AntDetail { … }`. Add `food_harvested: 0.0,` after their `food_delivered: 0.0,` so they compile.

- [ ] **Step 6: Set the field at both server construction sites**

`crates/server/src/sim_thread.rs`:
- The dead-ant detail near line 376 (where `food_delivered: 0.0,`): add `food_harvested: 0.0,`.
- The live detail near line 402 (where `food_delivered: w.ants.food_delivered[i],`): add:

```rust
                food_harvested: w.ants.food_harvested[i],
```

- [ ] **Step 7: Run the Rust guard + server tests (green)**

Run: `source "$HOME/.cargo/env" && cargo test -p server`
Expected: PASS, including the length/offset guard.

- [ ] **Step 8: Decode it on the client**

`web/src/protocol.ts`:
- In `interface AntDetail`, after `foodDelivered: number;` add `foodHarvested: number;`.
- In the `TAG_ANT_DETAIL` decode object, after `foodDelivered: f(41),` add:

```ts
        foodHarvested: f(ANT_DETAIL_LEN - 4),
```

(`ANT_DETAIL_LEN` is the client constant; ensure it is updated to 425 wherever it is defined in `web/src/protocol.ts` — grep `ANT_DETAIL_LEN` and set it to 425.)

- [ ] **Step 9: Typecheck the web bundle**

Run: `cd web && npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 10: Commit**

```bash
git add crates/server/src/protocol.rs crates/server/src/sim_thread.rs web/src/protocol.ts
git commit -m "feat(protocol): add food_harvested to the ant-detail frame"
```

---

## Task 2: Client fitness helper

**Files:**
- Create: `web/src/fitness.ts`
- Test: `web/tests/fitness.test.ts`

**Interfaces:**
- Produces: `fitness(delivered, harvested, weight): number`;
  `HARVEST_WEIGHT_FIELD = 16`; `DEFAULT_HARVEST_WEIGHT = 0.02`.

- [ ] **Step 1: Write the failing test**

`web/tests/fitness.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { fitness, DEFAULT_HARVEST_WEIGHT, HARVEST_WEIGHT_FIELD } from "../src/fitness.js";

describe("fitness", () => {
  it("is delivered plus weight times harvested", () => {
    expect(fitness(1240, 372, 0.02)).toBeCloseTo(1247.44, 5);
  });
  it("equals delivered when weight is zero", () => {
    expect(fitness(50, 999, 0)).toBe(50);
  });
  it("pins the harvest_weight config field id and default", () => {
    expect(HARVEST_WEIGHT_FIELD).toBe(16);
    expect(DEFAULT_HARVEST_WEIGHT).toBe(0.02);
  });
});
```

- [ ] **Step 2: Run it — fails (module not found)**

Run: `cd web && npx vitest run fitness`
Expected: FAIL — cannot find `../src/fitness.js`.

- [ ] **Step 3: Implement**

`web/src/fitness.ts`:

```ts
/**
 * An ant's selection fitness, mirroring `Config::fitness` in the sim:
 * delivered food plus a small credit for food still being carried. This is the
 * scalar reproduction is proportional to, so it is the honest "how successful
 * is this ant" number.
 */

/** Config field id for `harvest_weight` (see `apply_config_field`). */
export const HARVEST_WEIGHT_FIELD = 16;
/** Used until the config frame arrives. Matches `Config::default`. */
export const DEFAULT_HARVEST_WEIGHT = 0.02;

export function fitness(delivered: number, harvested: number, weight: number): number {
  return delivered + weight * harvested;
}
```

- [ ] **Step 4: Run it (green)**

Run: `cd web && npx vitest run fitness`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/fitness.ts web/tests/fitness.test.ts
git commit -m "feat(web): ant fitness helper mirroring Config::fitness"
```

---

## Task 3: Tile readout helper

**Files:**
- Create: `web/src/tile.ts`
- Test: `web/tests/tile.test.ts`

**Interfaces:**
- Consumes: `Terrain` and `Phero` frames (`web/src/protocol.ts`). Terrain rgba =
  R food(norm), G stone, B nest owner (255 none). Phero rgba = R food, G alarm,
  B scent, A owner (255 none). Both are downsampled by `factor`.
- Produces: `tileReadout(terrain, phero, x, y): TileReadout | null` where x,y are
  world cells. Returns null if out of bounds.

```ts
export interface TileReadout {
  x: number; y: number;               // the world cell asked for (ints)
  food: number;                       // terrain R, 0..255 (normalised food)
  stone: number;                      // terrain G, 0..255
  nest: number | null;                // terrain B owner, null if 255
  phFood: number; phAlarm: number; phScent: number;  // phero R/G/B, 0..255
  phOwner: number | null;             // phero A owner, null if 255
}
```

- [ ] **Step 1: Write the failing test**

`web/tests/tile.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { tileReadout } from "../src/tile.js";

// A 2x2 downsampled map at factor 4 => 8x8 world. Cell (5,1) maps to texel (1,0).
function frames() {
  const terrain = new Uint8Array(2 * 2 * 4);
  const phero = new Uint8Array(2 * 2 * 4);
  const t = (tx: number, ty: number) => (ty * 2 + tx) * 4;
  terrain[t(1, 0) + 0] = 200; // food
  terrain[t(1, 0) + 1] = 10;  // stone
  terrain[t(1, 0) + 2] = 3;   // nest owner 3
  phero[t(1, 0) + 0] = 40;    // food trail
  phero[t(1, 0) + 1] = 5;     // alarm
  phero[t(1, 0) + 2] = 90;    // scent
  phero[t(1, 0) + 3] = 255;   // no owner
  const T = { kind: "terrain", tick: 1, w: 2, h: 2, factor: 4, rgba: terrain } as const;
  const P = { kind: "phero", tick: 1, w: 2, h: 2, factor: 4, rgba: phero } as const;
  return { T, P };
}

describe("tileReadout", () => {
  it("reads the texel a world cell falls in", () => {
    const { T, P } = frames();
    const r = tileReadout(T as never, P as never, 5, 1)!;
    expect(r).not.toBeNull();
    expect(r.food).toBe(200);
    expect(r.stone).toBe(10);
    expect(r.nest).toBe(3);
    expect(r.phScent).toBe(90);
    expect(r.phOwner).toBeNull(); // 255 sentinel
  });
  it("returns null out of bounds", () => {
    const { T, P } = frames();
    expect(tileReadout(T as never, P as never, -1, 0)).toBeNull();
    expect(tileReadout(T as never, P as never, 8, 0)).toBeNull();
  });
  it("treats a 255 owner/nest byte as none", () => {
    const { T, P } = frames();
    // Mark texel (0,0)'s nest owner and phero owner as the 255 sentinel.
    T.rgba[0 * 4 + 2] = 255;
    P.rgba[0 * 4 + 3] = 255;
    const r = tileReadout(T as never, P as never, 0, 0)!;
    expect(r.nest).toBeNull();
    expect(r.phOwner).toBeNull();
  });
});
```

- [ ] **Step 2: Run it — fails (module not found)**

Run: `cd web && npx vitest run tile`
Expected: FAIL.

- [ ] **Step 3: Implement**

`web/src/tile.ts`:

```ts
/**
 * Read the terrain + pheromone values under a world cell. Both frames are
 * downsampled by `factor`, so a world cell maps to one texel; the Explorer's
 * tile view is entirely derived here, with no server round-trip.
 */

import type { Phero, Terrain } from "./protocol.js";

export interface TileReadout {
  x: number;
  y: number;
  food: number;
  stone: number;
  nest: number | null;
  phFood: number;
  phAlarm: number;
  phScent: number;
  phOwner: number | null;
}

/** 255 is the "no owner / no nest" sentinel in both frames. */
const NONE = 255;

export function tileReadout(
  terrain: Terrain,
  phero: Phero,
  x: number,
  y: number,
): TileReadout | null {
  const cx = Math.floor(x);
  const cy = Math.floor(y);
  const worldW = terrain.w * terrain.factor;
  const worldH = terrain.h * terrain.factor;
  if (cx < 0 || cy < 0 || cx >= worldW || cy >= worldH) return null;

  const tx = Math.min(terrain.w - 1, Math.floor(cx / terrain.factor));
  const ty = Math.min(terrain.h - 1, Math.floor(cy / terrain.factor));
  const ti = (ty * terrain.w + tx) * 4;

  const px = Math.min(phero.w - 1, Math.floor(cx / phero.factor));
  const py = Math.min(phero.h - 1, Math.floor(cy / phero.factor));
  const pi = (py * phero.w + px) * 4;

  const nest = terrain.rgba[ti + 2];
  const phOwner = phero.rgba[pi + 3];
  return {
    x: cx,
    y: cy,
    food: terrain.rgba[ti],
    stone: terrain.rgba[ti + 1],
    nest: nest === NONE ? null : nest,
    phFood: phero.rgba[pi],
    phAlarm: phero.rgba[pi + 1],
    phScent: phero.rgba[pi + 2],
    phOwner: phOwner === NONE ? null : phOwner,
  };
}
```

- [ ] **Step 4: Run it (green)**

Run: `cd web && npx vitest run tile`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/tile.ts web/tests/tile.test.ts
git commit -m "feat(web): tile readout helper from terrain + pheromone frames"
```

---

## Task 4: Unified selection + world summary in the store

**Files:**
- Modify: `web/src/state.ts` (State, Store)
- Modify: `web/src/ui/colonypanel.ts:32` and `web/src/ui/labels.ts:85` (readers of `selectedColony`)
- Test: `web/tests/state.test.ts`

**Interfaces:**
- Produces:
  - `type Selection = { kind: "ant" } | { kind: "colony"; id: number } | { kind: "tile"; x: number; y: number } | null;`
  - `state.selection: Selection`, `state.activeTab: "colonies" | "explorer"`.
  - `Store.selectAnt()`, `Store.selectColony(id)`, `Store.selectTile(x, y)`,
    `Store.clearSelection()` (already exists — extend it), `Store.setTab(tab)`,
    `Store.selectedColony(): number | null`, `worldSummary(stats)`.

- [ ] **Step 1: Write failing tests**

Append to `web/tests/state.test.ts` a new describe:

```ts
import { worldSummary } from "../src/state.js";

describe("selection + tabs", () => {
  it("selecting an entity records it and switches to the explorer tab", () => {
    const s = new Store();
    s.selectTile(3, 4);
    expect(s.state.selection).toEqual({ kind: "tile", x: 3, y: 4 });
    expect(s.state.activeTab).toBe("explorer");
  });
  it("selectedColony() reflects a colony selection and nothing else", () => {
    const s = new Store();
    s.selectColony(2);
    expect(s.selectedColony()).toBe(2);
    s.selectTile(0, 0);
    expect(s.selectedColony()).toBeNull();
  });
  it("clearSelection wipes selection, detail, genome", () => {
    const s = new Store();
    s.selectAnt();
    s.clearSelection();
    expect(s.state.selection).toBeNull();
  });
  it("worldSummary sums pop, store, delivered", () => {
    const sum = worldSummary([
      { id: 0, population: 3, store: 10, deliveredTotal: 100 },
      { id: 1, population: 4, store: 5, deliveredTotal: 250 },
    ] as never);
    expect(sum).toEqual({ pop: 7, store: 15, delivered: 350 });
  });
});
```

- [ ] **Step 2: Run — fails**

Run: `cd web && npx vitest run state`
Expected: FAIL (missing `selection`/`activeTab`/methods/`worldSummary`).

- [ ] **Step 3: Extend State**

In `web/src/state.ts`, add near the top-level types:

```ts
export type Selection =
  | { kind: "ant" }
  | { kind: "colony"; id: number }
  | { kind: "tile"; x: number; y: number }
  | null;
```

In `interface State`, replace `selectedColony: number | null;` with:

```ts
  /** What the Explorer and in-world popover are focused on. */
  selection: Selection;
  /** Which right-rail tab is shown. Selecting anything flips it to explorer. */
  activeTab: "colonies" | "explorer";
```

In the `state` initializer, replace `selectedColony: null,` with:

```ts
    selection: null,
    activeTab: "colonies",
```

- [ ] **Step 4: Replace the selection methods**

Replace the existing `selectColony`/`clearColony` and extend `clearSelection`:

```ts
  selectAnt(): void {
    this.state.selection = { kind: "ant" };
    this.state.activeTab = "explorer";
    this.notify();
  }

  selectColony(id: number): void {
    this.state.selection = { kind: "colony", id };
    this.state.activeTab = "explorer";
    this.notify();
  }

  selectTile(x: number, y: number): void {
    this.state.selection = { kind: "tile", x, y };
    this.state.activeTab = "explorer";
    this.notify();
  }

  /** The colony id iff a colony is selected — for the in-world colony popover. */
  selectedColony(): number | null {
    return this.state.selection?.kind === "colony" ? this.state.selection.id : null;
  }

  setTab(tab: State["activeTab"]): void {
    this.state.activeTab = tab;
    this.notify();
  }

  clearSelection(): void {
    this.state.detail = null;
    this.state.genome = null;
    this.state.selection = null;
    this.notify();
  }
```

Remove the old `clearColony()` (its callers are updated in Task 9) and the old
`selectColony`/`clearSelection` bodies referencing `selectedColony`.

- [ ] **Step 5: Add the world summary helper**

At the bottom of `state.ts` (near `push`):

```ts
export interface WorldSummary {
  pop: number;
  store: number;
  delivered: number;
}

/** All-colony totals, for the Explorer's default (nothing-selected) view. */
export function worldSummary(stats: ColonyStat[]): WorldSummary {
  let pop = 0;
  let store = 0;
  let delivered = 0;
  for (const c of stats) {
    pop += c.population;
    store += c.store;
    delivered += c.deliveredTotal;
  }
  return { pop, store, delivered };
}
```

(Ensure `ColonyStat` is imported in `state.ts` — it already imports from `protocol.js`.)

- [ ] **Step 6: Update the two `selectedColony` field readers**

- `web/src/ui/colonypanel.ts:32`: change `const id = store.state.selectedColony;`
  to `const id = store.selectedColony();`.
- `web/src/ui/labels.ts:85`: change
  `if (colony === store.state.selectedColony) continue;` to
  `if (colony === store.selectedColony()) continue;`.

- [ ] **Step 7: Run state + existing tests (green)**

Run: `cd web && npx vitest run state labels && npx tsc --noEmit`
Expected: PASS; typecheck clean. (labels.test.ts still green — the popover
suppression path is behavior-preserving.)

- [ ] **Step 8: Commit**

```bash
git add web/src/state.ts web/src/ui/colonypanel.ts web/src/ui/labels.ts web/tests/state.test.ts
git commit -m "feat(web): unified selection union + world summary in the store"
```

---

## Task 5: Tooltip infrastructure and copy

**Files:**
- Create: `web/src/ui/tooltips.ts`
- Modify: `web/index.html` (tooltip styles)
- Test: `web/tests/tooltips.test.ts`

**Interfaces:**
- Produces: `TOOLTIPS: Record<string, string>`; `tipLabel(text, key): HTMLElement`
  (a `<span>` label carrying a hover tooltip via a `data-tip` attribute).

- [ ] **Step 1: Write the failing test**

`web/tests/tooltips.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { TOOLTIPS } from "../src/ui/tooltips.js";

const REQUIRED = [
  "store", "delivered", "energy", "generation", "carrying", "pop",
  "fitness", "harvested", "phFood", "phAlarm", "phScent", "phOwner",
];

describe("tooltip copy", () => {
  it("defines non-empty copy for every stat key the panels use", () => {
    for (const k of REQUIRED) {
      expect(TOOLTIPS[k], k).toBeTruthy();
      expect(TOOLTIPS[k].length, k).toBeGreaterThan(8);
    }
  });
});
```

- [ ] **Step 2: Run — fails**

Run: `cd web && npx vitest run tooltips`
Expected: FAIL (module missing).

- [ ] **Step 3: Implement the copy + label helper**

`web/src/ui/tooltips.ts`:

```ts
/**
 * One-line, plain-English definitions for every number the panels show, in one
 * place. `tipLabel` builds a label span that reveals its definition on hover
 * (styled via the `[data-tip]` CSS in index.html, not the native title).
 */

export const TOOLTIPS: Record<string, string> = {
  pop: "Ants alive right now.",
  store:
    "Spendable food fund. Births and refueling draw from it. A colony at 0 " +
    "survives on ant energy reserves plus the extinction floor.",
  delivered:
    "Lifetime food carried home. An odometer — never spent down. This is the " +
    "colony's fitness signal.",
  energy: "The ant's personal fuel. Spent moving; refilled only at its own nest.",
  generation: "Lineage depth — how many births deep this line is.",
  carrying: "Food the ant is holding, not yet banked at a nest.",
  fitness:
    "This ant's success: food carried home (delivered) plus a small 2% credit " +
    "for food it is still holding. Fitter ants are chosen as parents more often.",
  harvested: "Lifetime food this ant has picked up (banked or not).",
  size: "Body size. Bigger ants cost more upkeep but hit harder.",
  "paid births": "Births paid for from the store (birth_cost each).",
  free: "Share of this colony's ants that were free extinction-floor spawns.",
  phFood: "Food-trail pheromone here: laid by laden ants, leads to food.",
  phAlarm: "Alarm pheromone here: spikes where ants were attacked.",
  phScent: "Territory scent here: the owning colony's claim on this cell.",
  phOwner: "Colony that owns the scent on this cell (none if unclaimed).",
  nest: "Colony whose nest tile this is (none if open ground).",
  stone: "Stone coverage here (impassable).",
  food: "Standing food on this cell.",
};

/** A label span that shows its tooltip on hover. `key` selects the copy. */
export function tipLabel(text: string, key: string): HTMLSpanElement {
  const el = document.createElement("span");
  el.textContent = text;
  const tip = TOOLTIPS[key];
  if (tip) {
    el.className = "tip";
    el.setAttribute("data-tip", tip);
  }
  return el;
}
```

- [ ] **Step 4: Add tooltip styles to index.html**

In the `<style>` block of `web/index.html`, add:

```css
      .tip { position: relative; cursor: help; border-bottom: 1px dotted var(--dim); }
      .tip:hover::after {
        content: attr(data-tip);
        position: absolute; left: 0; top: 140%; z-index: 20;
        width: max-content; max-width: 220px;
        background: #06070a; color: var(--text);
        border: 1px solid var(--line); border-radius: 5px;
        padding: 5px 7px; font-size: 11px; line-height: 1.35;
        box-shadow: 0 6px 18px rgba(0,0,0,0.6); white-space: normal;
        pointer-events: none;
      }
```

- [ ] **Step 5: Run test (green)**

Run: `cd web && npx vitest run tooltips`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/ui/tooltips.ts web/index.html web/tests/tooltips.test.ts
git commit -m "feat(web): stat tooltip copy + hover-label helper"
```

---

## Task 6: Right-rail tab strip

**Files:**
- Create: `web/src/ui/rail.ts`
- Modify: `web/index.html` (tab styles)

**Interfaces:**
- Consumes: `Store` (`state.activeTab`, `setTab`).
- Produces: `mountRail(root, store): { coloniesPane: HTMLElement; explorerPane: HTMLElement }`.

- [ ] **Step 1: Implement the tab strip**

`web/src/ui/rail.ts`:

```ts
/**
 * The right rail is two tabs: Colonies (overview) and Explorer (whatever is
 * selected). The tab strip reflects `store.state.activeTab`, which selecting an
 * entity flips to "explorer", so a click in the world brings its inspector up
 * without the operator hunting for the tab.
 */

import type { Store } from "../state.js";

export function mountRail(
  root: HTMLElement,
  store: Store,
): { coloniesPane: HTMLElement; explorerPane: HTMLElement } {
  const strip = document.createElement("div");
  strip.className = "tabstrip";

  const coloniesPane = document.createElement("div");
  const explorerPane = document.createElement("div");
  coloniesPane.className = "tabpane";
  explorerPane.className = "tabpane";

  const tabs: { key: "colonies" | "explorer"; label: string; pane: HTMLElement }[] = [
    { key: "colonies", label: "Colonies", pane: coloniesPane },
    { key: "explorer", label: "Explorer", pane: explorerPane },
  ];
  const btns = tabs.map((t) => {
    const b = document.createElement("button");
    b.className = "tab";
    b.textContent = t.label;
    b.addEventListener("click", () => store.setTab(t.key));
    strip.append(b);
    return b;
  });

  root.append(strip, coloniesPane, explorerPane);

  const sync = () => {
    const active = store.state.activeTab;
    tabs.forEach((t, i) => {
      const on = t.key === active;
      btns[i].classList.toggle("on", on);
      t.pane.style.display = on ? "" : "none";
    });
  };
  store.subscribe(sync);
  sync();

  return { coloniesPane, explorerPane };
}
```

- [ ] **Step 2: Add tab styles to index.html**

```css
      .tabstrip { display: flex; gap: 4px; margin-bottom: 10px; }
      .tab { flex: 1; }
      .tabpane { min-width: 0; }
```

- [ ] **Step 3: Typecheck**

Run: `cd web && npx tsc --noEmit`
Expected: clean (rail is not yet wired; that happens in Task 9).

- [ ] **Step 4: Commit**

```bash
git add web/src/ui/rail.ts web/index.html
git commit -m "feat(web): right-rail tab strip bound to activeTab"
```

---

## Task 7: Ant-detail renderer refactor + Explorer pane

**Files:**
- Modify: `web/src/ui/inspector.ts` — export a reusable renderer, add fitness row, tooltips, evolution explainer.
- Create: `web/src/ui/explorer.ts`
- Modify: `web/src/ui/colony.ts` — export `sparkline` for reuse.

**Interfaces:**
- Consumes: `Store`, `tipLabel`/`TOOLTIPS`, `fitness`, `tileReadout`,
  `worldSummary`, `sparkline`, `draw as drawNet`.
- Produces: `renderAntDetail(container: HTMLElement, canvas: HTMLCanvasElement, store: Store): void` (from inspector.ts);
  `mountExplorer(pane: HTMLElement, store: Store): void`.

- [ ] **Step 1: Export the sparkline from colony.ts**

In `web/src/ui/colony.ts`, change `function sparkline(` to `export function sparkline(`. No other change.

- [ ] **Step 2: Refactor inspector.ts into a reusable renderer with fitness + tooltips + explainer**

Replace the body of `web/src/ui/inspector.ts` with a version that (a) exports
`renderAntDetail`, (b) adds the fitness row using `fitness()` and the
config-derived weight, (c) uses `tipLabel` for the labels that have tooltips,
and (d) appends a collapsible evolution explainer. `mountInspector` is kept as a
thin wrapper for backward compatibility but is no longer mounted by `main.ts`.

```ts
/**
 * The selected ant: identity, energy economy, a fitness headline, its traits
 * and outputs, and its network drawn live. Rendered into a caller-supplied
 * container + canvas so the Explorer tab can host it.
 */

import { TRAIT_NAMES } from "../protocol.js";
import type { Store } from "../state.js";
import { draw as drawNet } from "./nnview.js";
import { fitness, DEFAULT_HARVEST_WEIGHT, HARVEST_WEIGHT_FIELD } from "../fitness.js";
import { tipLabel, TOOLTIPS } from "./tooltips.js";

const OUTPUT_NAMES = ["turn", "throttle", "attack", "grab", "mem0", "mem1", "mem2", "mem3"];

/** Render the selected ant into `body`, painting its NN into `canvas`. */
export function renderAntDetail(
  body: HTMLElement,
  canvas: HTMLCanvasElement,
  store: Store,
): void {
  const d = store.state.detail;
  body.innerHTML = "";

  if (!d) {
    body.append(muted("click an ant to inspect it"));
    paint(canvas, null, null);
    return;
  }
  if (!d.alive) {
    body.append(muted(`ant #${d.id} died`));
    paint(canvas, null, null);
    return;
  }

  const weight = store.state.config.get(HARVEST_WEIGHT_FIELD) ?? DEFAULT_HARVEST_WEIGHT;
  const fit = fitness(d.foodDelivered, d.foodHarvested, weight);

  // Fitness headline: the one number that answers "how successful is this ant".
  const head = document.createElement("div");
  head.className = "fitness";
  const fl = tipLabel("fitness", "fitness");
  const fv = document.createElement("b");
  fv.textContent = fit.toFixed(1);
  head.append(fl, fv);
  const brk = document.createElement("div");
  brk.className = "muted fitness-brk";
  brk.textContent =
    `= delivered ${d.foodDelivered.toFixed(0)} + ${weight} × harvested ${d.foodHarvested.toFixed(0)}`;
  body.append(head, brk);

  const kv = document.createElement("div");
  kv.className = "kv";
  const row = (label: string, key: string, value: string) => {
    kv.append(tipLabel(label, key));
    const b = document.createElement("b");
    b.textContent = value;
    kv.append(b);
  };
  row("name", "", d.name || `#${d.id}`);
  row("id", "", `#${d.id}`);
  row("colony", "", store.colonyName(d.colony));
  row("energy", "energy", `${d.energy.toFixed(1)} / ${d.maxEnergy.toFixed(0)}`);
  row("size", "size", d.size.toFixed(2));
  row("age", "", String(d.age));
  row("generation", "generation", String(d.lineage));
  row("carrying", "carrying", d.carrying.toFixed(2));
  row("delivered", "delivered", d.foodDelivered.toFixed(1));
  row("harvested", "harvested", d.foodHarvested.toFixed(1));
  body.append(kv);

  body.append(heading("Traits"));
  const tkv = document.createElement("div");
  tkv.className = "kv";
  TRAIT_NAMES.forEach((name, i) => {
    tkv.append(tipLabel(name, name));
    const b = document.createElement("b");
    b.textContent = d.traits[i].toFixed(name === "lifespan" ? 0 : 2);
    tkv.append(b);
  });
  body.append(tkv);

  body.append(heading("Outputs"));
  const okv = document.createElement("div");
  okv.className = "kv";
  OUTPUT_NAMES.forEach((name, i) => {
    const s = document.createElement("span");
    s.textContent = name;
    const b = document.createElement("b");
    b.textContent = d.outputs[i].toFixed(3);
    okv.append(s, b);
  });
  body.append(okv);

  body.append(evolutionExplainer());
  paint(canvas, d, store.state.genome?.params ?? null);
}

/** Kept so any remaining caller still works; main.ts now uses the Explorer. */
export function mountInspector(root: HTMLElement, store: Store): void {
  const h = heading("Ant");
  const body = document.createElement("div");
  body.id = "inspector";
  const canvas = document.createElement("canvas");
  canvas.id = "nn";
  root.append(h, body, canvas);
  const render = () => renderAntDetail(body, canvas, store);
  store.subscribe(render);
  render();
}

function muted(text: string): HTMLElement {
  const p = document.createElement("div");
  p.className = "muted";
  p.textContent = text;
  return p;
}

function heading(text: string): HTMLElement {
  const h = document.createElement("h2");
  h.textContent = text;
  return h;
}

/** A collapsed <details> explaining the evolutionary loop the fitness feeds. */
function evolutionExplainer(): HTMLElement {
  const d = document.createElement("details");
  d.className = "explainer";
  const s = document.createElement("summary");
  s.textContent = "How evolution works";
  const p = document.createElement("p");
  p.textContent =
    "An ant's brain is fixed for life — it never learns. Adaptation happens " +
    "across generations, per colony. An ant's success is its fitness (food " +
    "carried home, plus a small credit for food it's still holding). When a " +
    "colony can afford a birth, it picks a parent in proportion to fitness, so " +
    "the best foragers have the most offspring; each child is a mutated copy of " +
    "the parent's brain. A per-colony hall of fame keeps the fittest genomes, so " +
    "a colony that nearly dies re-seeds from its best-ever ants. You can see it " +
    "working when delivered rises while generation climbs.";
  d.append(s, p);
  return d;
}

function paint(
  canvas: HTMLCanvasElement,
  act: { inputs: Float32Array; h1: Float32Array; h2: Float32Array; outputs: Float32Array } | null,
  params: Float32Array | null,
): void {
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  const w = Math.max(1, Math.round(canvas.clientWidth * dpr));
  const h = Math.max(1, Math.round(canvas.clientHeight * dpr));
  if (canvas.width !== w || canvas.height !== h) {
    canvas.width = w;
    canvas.height = h;
  }
  const ctx = canvas.getContext("2d");
  if (ctx) drawNet(ctx, w, h, act, params);
}
```

- [ ] **Step 3: Implement the Explorer pane**

`web/src/ui/explorer.ts`:

```ts
/**
 * The context-sensitive right pane: renders whatever `store.state.selection`
 * points at — an ant (full detail + NN), a colony (stats + its chart), a tile
 * (terrain + pheromone here), or, with nothing selected, world totals. One
 * panel replaces the old fixed stack, the way a game engine's inspector shows
 * the current object.
 */

import type { Store } from "../state.js";
import { worldSummary } from "../state.js";
import { renderAntDetail } from "./inspector.js";
import { sparkline } from "./colony.js";
import { tileReadout } from "../tile.js";
import { tipLabel } from "./tooltips.js";
import { colonyCss } from "../colors.js";
import { drawSymbol, symbolFor } from "../symbols.js";

export function mountExplorer(pane: HTMLElement, store: Store): void {
  const body = document.createElement("div");
  body.id = "explorer";
  // The NN canvas is persistent (reused across renders) so it is created once.
  const nn = document.createElement("canvas");
  nn.id = "nn";
  const chart = document.createElement("canvas");
  chart.className = "explorer-chart";
  pane.append(body);

  const kvRow = (kv: HTMLElement, label: string, key: string, value: string) => {
    kv.append(tipLabel(label, key));
    const b = document.createElement("b");
    b.textContent = value;
    kv.append(b);
  };

  const render = () => {
    const sel = store.state.selection;
    body.innerHTML = "";

    if (sel?.kind === "ant") {
      renderAntDetail(body, nn, store);
      body.append(nn);
      return;
    }

    if (sel?.kind === "colony") {
      const c = store.state.stats.find((s) => s.id === sel.id);
      const h = heading(store.colonyName(sel.id), sel.id);
      body.append(h);
      if (!c) {
        body.append(muted("waiting for stats…"));
        return;
      }
      const kv = document.createElement("div");
      kv.className = "kv";
      kvRow(kv, "pop", "pop", String(c.population));
      kvRow(kv, "store", "store", c.store.toFixed(0));
      kvRow(kv, "delivered", "delivered", c.deliveredTotal.toFixed(0));
      kvRow(kv, "generation", "generation", c.meanLineage.toFixed(1));
      kvRow(kv, "paid births", "paid births", String(c.births));
      kvRow(kv, "deaths", "", String(c.deaths));
      kvRow(kv, "free spawns", "free", String(c.floorSpawns));
      kvRow(kv, "mean size", "size", c.meanSize.toFixed(2));
      body.append(kv);
      const hist = store.state.history.get(sel.id);
      if (hist) {
        body.append(chart);
        sparkline(chart, hist, sel.id);
      }
      return;
    }

    if (sel?.kind === "tile") {
      const t = store.state.terrain;
      const p = store.state.phero;
      body.append(plainHeading(`Tile ${sel.x}, ${sel.y}`));
      if (!t || !p) {
        body.append(muted("waiting for terrain…"));
        return;
      }
      const r = tileReadout(t, p, sel.x, sel.y);
      if (!r) {
        body.append(muted("out of bounds"));
        return;
      }
      const kv = document.createElement("div");
      kv.className = "kv";
      kvRow(kv, "food", "food", r.food.toFixed(0));
      kvRow(kv, "stone", "stone", r.stone.toFixed(0));
      kvRow(kv, "nest", "nest", r.nest === null ? "—" : store.colonyName(r.nest));
      kvRow(kv, "food trail", "phFood", r.phFood.toFixed(0));
      kvRow(kv, "alarm", "phAlarm", r.phAlarm.toFixed(0));
      kvRow(kv, "scent", "phScent", r.phScent.toFixed(0));
      kvRow(kv, "scent owner", "phOwner", r.phOwner === null ? "—" : store.colonyName(r.phOwner));
      body.append(kv);
      return;
    }

    // Nothing selected: world totals so the pane is never blank.
    body.append(plainHeading("World"));
    const sum = worldSummary(store.state.stats);
    const kv = document.createElement("div");
    kv.className = "kv";
    kvRow(kv, "tick", "", store.state.tick.toLocaleString());
    kvRow(kv, "ants", "pop", String(store.state.ants?.count ?? 0));
    kvRow(kv, "total store", "store", sum.store.toFixed(0));
    kvRow(kv, "total delivered", "delivered", sum.delivered.toFixed(0));
    body.append(kv);
    body.append(muted("click an ant, a nest, or Alt-click a tile to inspect it."));
  };

  store.subscribe(render);
  render();
}

function muted(text: string): HTMLElement {
  const p = document.createElement("div");
  p.className = "muted";
  p.textContent = text;
  return p;
}

function plainHeading(text: string): HTMLElement {
  const h = document.createElement("h2");
  h.textContent = text;
  return h;
}

/** Colony heading with the same glyph the cards and labels use. */
function heading(name: string, id: number): HTMLElement {
  const h = document.createElement("h2");
  h.style.display = "flex";
  h.style.alignItems = "center";
  h.style.gap = "6px";
  const g = document.createElement("canvas");
  g.width = 16;
  g.height = 16;
  g.className = "swatch-glyph";
  const ctx = g.getContext("2d");
  if (ctx) drawSymbol(ctx, symbolFor(id), 8, 8, 6, colonyCss(id));
  const s = document.createElement("span");
  s.textContent = name;
  h.append(g, s);
  return h;
}
```

- [ ] **Step 4: Add explorer/fitness/explainer styles to index.html**

```css
      .fitness { display: flex; justify-content: space-between; align-items: baseline; margin-top: 4px; }
      .fitness b { font-size: 16px; color: var(--accent); }
      .fitness-brk { font-size: 11px; margin-bottom: 8px; }
      .explorer-chart { width: 100%; height: 60px; display: block; margin-top: 8px; }
      .explainer { margin-top: 12px; }
      .explainer summary { cursor: pointer; color: var(--dim); font-size: 11px; text-transform: uppercase; letter-spacing: 0.09em; }
      .explainer p { color: var(--dim); font-size: 12px; line-height: 1.5; margin: 8px 0 0; }
```

- [ ] **Step 5: Typecheck + run all web tests**

Run: `cd web && npx tsc --noEmit && npx vitest run`
Expected: clean typecheck; all tests pass.

- [ ] **Step 6: Commit**

```bash
git add web/src/ui/inspector.ts web/src/ui/explorer.ts web/src/ui/colony.ts web/index.html
git commit -m "feat(web): context-sensitive Explorer pane + ant fitness & evolution explainer"
```

---

## Task 8: In-world ant popover

**Files:**
- Create: `web/src/ui/antpopover.ts`
- Modify: `web/index.html` (reuse `.colony-popover`; add `.ant-popover` tweaks if needed)

**Interfaces:**
- Consumes: `Store` (`state.selection`, `state.detail`, `state.config`),
  `Camera`, `projectToCss`, `fitness`.
- Produces: `class AntPopover { constructor(parent); update(camera, viewW, viewH, dpr, store): void }`.

- [ ] **Step 1: Implement the popover (mirrors ColonyPanel)**

`web/src/ui/antpopover.ts`:

```ts
/**
 * In-world ant popover: a small card anchored on the selected ant that follows
 * it as it moves. A glance-level readout (identity, energy, carrying,
 * delivered, and the fitness headline); the full stats and the network live in
 * the Explorer tab. Text-only on purpose — a live NN chasing a moving ant is
 * unreadable.
 */

import type { Camera } from "../render/camera.js";
import type { Store } from "../state.js";
import { projectToCss } from "./labels.js";
import { fitness, DEFAULT_HARVEST_WEIGHT, HARVEST_WEIGHT_FIELD } from "../fitness.js";

export class AntPopover {
  private root: HTMLElement;
  private nameEl!: HTMLElement;
  private rows = new Map<string, HTMLElement>();
  private built = false;

  constructor(parent: HTMLElement) {
    this.root = document.createElement("div");
    this.root.className = "colony-popover ant-popover";
    this.root.style.display = "none";
    parent.append(this.root);
  }

  update(camera: Camera, viewW: number, viewH: number, dpr: number, store: Store): void {
    const d = store.state.detail;
    const isAnt = store.state.selection?.kind === "ant";
    if (!isAnt || !d || !d.alive) {
      this.root.style.display = "none";
      return;
    }
    if (!this.built) {
      this.build();
      this.built = true;
    }
    this.root.style.display = "";

    const weight = store.state.config.get(HARVEST_WEIGHT_FIELD) ?? DEFAULT_HARVEST_WEIGHT;
    this.nameEl.textContent = d.name || `#${d.id}`;
    this.rows.get("energy")!.textContent = `${d.energy.toFixed(0)} / ${d.maxEnergy.toFixed(0)}`;
    this.rows.get("carrying")!.textContent = d.carrying.toFixed(1);
    this.rows.get("delivered")!.textContent = d.foodDelivered.toFixed(0);
    this.rows.get("fitness")!.textContent = fitness(d.foodDelivered, d.foodHarvested, weight).toFixed(0);

    const p = projectToCss(camera, d.x, d.y, viewW, viewH, dpr);
    this.root.style.left = `${p.left}px`;
    this.root.style.top = `${p.top}px`;
  }

  private build(): void {
    this.root.textContent = "";
    const title = document.createElement("div");
    title.className = "title";
    this.nameEl = document.createElement("span");
    title.append(this.nameEl);

    const kv = document.createElement("div");
    kv.className = "kv";
    for (const key of ["energy", "carrying", "delivered", "fitness"]) {
      const l = document.createElement("span");
      l.textContent = key;
      const v = document.createElement("b");
      kv.append(l, v);
      this.rows.set(key, v);
    }
    this.root.append(title, kv);
  }
}
```

- [ ] **Step 2: Typecheck**

Run: `cd web && npx tsc --noEmit`
Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add web/src/ui/antpopover.ts
git commit -m "feat(web): in-world ant popover with fitness headline"
```

---

## Task 9: Wire it all in main.ts (tabs, popover, tile selection, hotkeys)

**Files:**
- Modify: `web/src/main.ts`
- Modify: `web/index.html` (hotkeys line styles)

**Interfaces:**
- Consumes: `mountRail`, `mountExplorer`, `AntPopover`, `store.selectAnt/selectColony/selectTile/clearSelection`.

- [ ] **Step 1: Restructure the right-rail mounting**

In `web/src/main.ts`, replace the import of `mountInspector` and the
`coloniesEl/chronicleEl/inspectorEl` block with the tabbed layout:

Change imports:

```ts
import { mountColonies } from "./ui/colony.js";
import { mountChronicle } from "./ui/chronicle.js";
import { mountRail } from "./ui/rail.js";
import { mountExplorer } from "./ui/explorer.js";
import { AntPopover } from "./ui/antpopover.js";
```

(remove the `mountInspector` import.)

Replace the right-rail mounting (lines ~45–54) with:

```ts
const { coloniesPane, explorerPane } = mountRail(rightRail, store);
// Colonies tab: the cards (with camera-focus on click) then the chronicle.
mountColonies(coloniesPane, store, focusColony);
mountChronicle(coloniesPane, store);
// Explorer tab: the context-sensitive inspector.
mountExplorer(explorerPane, store);
```

Add the popover next to the colony popover:

```ts
const antPopover = new AntPopover(worldWrap);
```

- [ ] **Step 2: Select an ant (not just server-inspect) on a plain click**

In the `pointerup` handler, replace the nest/else block
(`const colony = nestColonyAt(...)` … `net.send(cmdSelectAt(w.x, w.y));`) with:

```ts
    // Alt/Option-click inspects the bare tile under the cursor.
    if (e.altKey) {
      store.selectTile(Math.floor(w.x), Math.floor(w.y));
      return;
    }
    // A nest tile opens that colony (popover + Explorer).
    const colony = nestColonyAt(w.x, w.y);
    if (colony !== null) {
      store.selectColony(colony);
      return;
    }
    // Otherwise select the nearest ant: mark the selection optimistically and
    // ask the server for the ant payload.
    store.selectAnt();
    net.send(cmdSelectAt(w.x, w.y));
```

(The `store.clearColony()` call is gone — `clearColony` was removed in Task 4.)

- [ ] **Step 3: Add "Inspect tile here" to the right-click menu**

In `menuItemsFor`, add as the first item (before "Set food here…"):

```ts
    { label: "Inspect tile here", onClick: () => store.selectTile(Math.floor(x), Math.floor(y)) },
```

- [ ] **Step 4: Update the popover each frame**

In `frame()`, after the `colonyPanel.update(...)` line add:

```ts
    antPopover.update(r.camera, r.viewW, r.viewH, r.dpr, store);
```

- [ ] **Step 5: Add a hotkeys legend line**

In `web/index.html`, inside `#world-wrap`, add after the overlay div:

```html
        <div class="hotkeys" id="hotkeys">Alt-click: tile · F: fit · Space: pause · Esc: deselect</div>
```

And style it:

```css
      .hotkeys {
        position: absolute; left: 8px; bottom: 8px;
        background: rgba(0,0,0,0.55); color: var(--dim);
        padding: 3px 8px; border-radius: 4px; font-size: 11px;
        pointer-events: none;
      }
```

- [ ] **Step 6: Typecheck + full web test run**

Run: `cd web && npx tsc --noEmit && npx vitest run`
Expected: clean typecheck; all tests pass.

- [ ] **Step 7: Build the bundle**

Run: `cd web && npx vite build`
Expected: builds into `web/dist` with no errors.

- [ ] **Step 8: Commit**

```bash
git add web/src/main.ts web/index.html
git commit -m "feat(web): tabbed rail, ant popover, Alt-click tile inspect, hotkeys line"
```

---

## Task 10: Rebuild the server and verify live

**Files:** none (verification only).

- [ ] **Step 1: Rebuild the release server (picks up the wire change)**

Run: `source "$HOME/.cargo/env" && cargo build --release -p server`
Expected: builds clean.

- [ ] **Step 2: Run the full Rust suite (determinism intact)**

Run: `source "$HOME/.cargo/env" && cargo test`
Expected: all pass, including the golden master / known-good (unchanged — no sim
behavior touched).

- [ ] **Step 3: Restart the server and smoke-test**

Restart `./target/release/server --web web/dist`, reload the browser, and check:
- Right rail shows two tabs; Colonies lists cards; clicking a card frames the nest.
- Clicking an ant: the in-world popover appears and follows it; the Explorer tab
  fills with full stats, a `fitness` headline (`= delivered … + 0.02 × harvested …`),
  and the "How evolution works" explainer.
- Clicking a nest: colony popover + Explorer colony view with its chart.
- Alt-clicking open ground: Explorer tile view with food/stone/pheromone.
- Hovering a labelled number shows its tooltip; the hotkeys line is visible.
- No panics in the server log across a few thousand ticks at high speed with an
  ant selected.

- [ ] **Step 4: Final commit if any tweaks were needed**

```bash
git add -A && git commit -m "chore: rebuild web bundle for the UI pass"
```

## Self-Review notes

- **Spec coverage:** tabs (§2) → T6/T9; unified selection (§1) → T4; ant popover
  (§3) → T8/T9; tile + hotkeys (§4) → T3/T9; tooltips (§5) → T5 + T7 usage;
  fitness (§6) → T1/T2/T7/T8; evolution explainer (§7) → T7.
- **Type consistency:** `foodHarvested` (TS) / `food_harvested` (Rust) added in
  T1 and consumed in T2/T7/T8. `sparkline` exported in T7 step 1, consumed in
  T7 step 3. `selectedColony()` method (T4) consumed by the two readers same task.
- **Untested-by-design:** the DOM-heavy `rail.ts`/`explorer.ts`/`antpopover.ts`
  follow the project's existing pattern of not unit-testing GL/DOM builders
  (`colony.ts`, `inspector.ts`, `colonypanel.ts` have no tests); their logic is
  pushed into the tested pure helpers (`fitness`, `tile`, `worldSummary`,
  `tooltips`).
