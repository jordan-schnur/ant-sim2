/**
 * The selected ant: identity, energy economy, a fitness headline, its traits
 * and outputs, and its network drawn live. Rendered into a caller-supplied
 * container + canvas so the Explorer tab can host it.
 */

import { TRAIT_NAMES } from "../protocol.js";
import type { Store } from "../state.js";
import { draw as drawNet, hitTest, activationColor } from "./nnview.js";
import { fitness, DEFAULT_HARVEST_WEIGHT, HARVEST_WEIGHT_FIELD } from "../fitness.js";
import { tipLabel, tipText } from "./tooltips.js";
import {
  CHANNEL_ABBR,
  CHANNELS,
  OUTPUT_DESC,
  OUTPUT_LABELS,
  WHISKER_DIRS,
  inputLabel,
  nodeInfo,
} from "../nnlabels.js";

// The panel rebuilds its whole DOM on every frame (see explorer.ts), so a fresh
// <details> would snap shut the instant the next frame lands. Remember the
// operator's open/closed choice out here, where it survives the rebuild.
let explainerOpen = false;

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
    body.append(canvas);
    paint(canvas, null, null);
    return;
  }
  if (!d.alive) {
    body.append(muted(`ant #${d.id} died`));
    body.append(canvas);
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
  // `weight` arrives as an f32 widened to f64, so a bare `${weight}` prints
  // 0.019999999552965164. Trim the float noise without hard-coding 2 decimals,
  // since harvest_weight is tunable to other small values.
  const weightText = String(Number(weight.toFixed(4)));
  brk.textContent =
    `= delivered ${d.foodDelivered.toFixed(0)} + ${weightText} × harvested ${d.foodHarvested.toFixed(0)}`;
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

  body.append(inputsSection(d.inputs));

  body.append(heading("Outputs"));
  body.append(caption("what the brain decides each tick"));
  const okv = document.createElement("div");
  okv.className = "kv";
  OUTPUT_LABELS.forEach((name, i) => {
    okv.append(tipText(name, OUTPUT_DESC[i]));
    const b = document.createElement("b");
    b.textContent = signed(d.outputs[i]);
    okv.append(b);
  });
  body.append(okv);

  body.append(evolutionExplainer());
  // Attach before painting: `paint` sizes the canvas from `canvas.clientWidth`,
  // which is 0 (and forces a 1x1 backing store) until the canvas is in the DOM.
  body.append(canvas);
  paint(canvas, d, store.state.genome?.params ?? null);
}

/** Signed, 2-decimal formatting for activation values. */
function signed(v: number): string {
  return `${v >= 0 ? "+" : ""}${v.toFixed(2)}`;
}

/** The Inputs block: a whisker grid plus the interpretable body/memory rows. */
function inputsSection(inputs: Float32Array): HTMLElement {
  const wrap = document.createElement("div");
  wrap.append(heading("Inputs"));
  wrap.append(caption("what the ant senses — hover the network for any single value"));
  wrap.append(whiskerGrid(inputs));

  // The 14 non-whisker inputs read cleanly as label -> value rows.
  const kv = document.createElement("div");
  kv.className = "kv";
  for (let i = 30; i < inputs.length; i++) {
    const s = document.createElement("span");
    s.textContent = inputLabel(i);
    const b = document.createElement("b");
    b.textContent = inputs[i].toFixed(2);
    kv.append(s, b);
  }
  wrap.append(kv);
  return wrap;
}

/** 5 whisker directions x 6 channels, each cell tinted by its activation. */
function whiskerGrid(inputs: Float32Array): HTMLElement {
  const g = document.createElement("div");
  g.className = "whisker-grid";
  const cell = (text: string, cls: string): HTMLElement => {
    const c = document.createElement("div");
    c.className = cls;
    c.textContent = text;
    return c;
  };

  g.append(cell("", "wg-head")); // empty corner
  CHANNEL_ABBR.forEach((abbr, ci) => {
    const h = cell(abbr, "wg-head");
    h.title = CHANNELS[ci];
    g.append(h);
  });
  WHISKER_DIRS.forEach((dir, w) => {
    g.append(cell(dir, "wg-dir"));
    for (let ch = 0; ch < CHANNEL_ABBR.length; ch++) {
      const v = inputs[w * CHANNEL_ABBR.length + ch];
      const c = cell(v.toFixed(1), "wg-val");
      c.style.background = activationColor(v);
      c.title = `whisker ${dir} · ${CHANNELS[ch]}: ${v.toFixed(2)}`;
      g.append(c);
    }
  });
  return g;
}

/**
 * A hover popover over the network canvas: naming the input or output under the
 * cursor and its live value is what turns the coloured graph into something you
 * can read a single wire off of. Attached once (the canvas is persistent);
 * reads the current ant from the store on each move.
 */
export function attachNNPopover(canvas: HTMLCanvasElement, store: Store): void {
  const pop = document.createElement("div");
  pop.className = "nn-pop";
  pop.style.display = "none";
  document.body.append(pop);

  const hide = () => {
    pop.style.display = "none";
  };

  canvas.addEventListener("mousemove", (e) => {
    const d = store.state.detail;
    if (!d || !d.alive) return hide();
    const rect = canvas.getBoundingClientRect();
    const node = hitTest(e.clientX - rect.left, e.clientY - rect.top, rect.width, rect.height);
    if (!node) return hide();

    const info = nodeInfo(node.layer, node.index);
    const val = [d.inputs, d.h1, d.h2, d.outputs][node.layer][node.index] ?? 0;
    pop.innerHTML = "";
    const name = document.createElement("div");
    name.className = "nn-pop-name";
    name.textContent = info.label;
    const v = document.createElement("div");
    v.className = "nn-pop-val";
    v.textContent = signed(val);
    pop.append(name, v);
    if (info.desc) {
      const desc = document.createElement("div");
      desc.className = "nn-pop-desc";
      desc.textContent = info.desc;
      pop.append(desc);
    }
    // Output nodes sit at the canvas's right edge, so a popover placed to the
    // right of the cursor clips off-screen exactly where it is most wanted.
    // Measure it, then flip left/up whenever the default corner would overflow.
    pop.style.display = "block";
    pop.style.left = "0";
    pop.style.top = "0";
    const w = pop.offsetWidth;
    const h = pop.offsetHeight;
    const left = e.clientX + 14 + w > window.innerWidth ? e.clientX - 14 - w : e.clientX + 14;
    const top = e.clientY + 14 + h > window.innerHeight ? e.clientY - 14 - h : e.clientY + 14;
    pop.style.left = `${Math.max(4, left)}px`;
    pop.style.top = `${Math.max(4, top)}px`;
  });
  canvas.addEventListener("mouseleave", hide);
}

function caption(text: string): HTMLElement {
  const p = document.createElement("div");
  p.className = "caption";
  p.textContent = text;
  return p;
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
  d.open = explainerOpen;
  d.addEventListener("toggle", () => {
    explainerOpen = d.open;
  });
  const s = document.createElement("summary");
  s.textContent = "How evolution works";
  const p = document.createElement("p");
  p.textContent =
    "An ant's brain is fixed for life — it never learns. Adaptation happens " +
    "across generations, per colony. An ant's success is its fitness (food " +
    "carried home, plus a small credit for all food it has ever picked up). " +
    "When a colony can afford a birth, it picks a parent in proportion to fitness, so " +
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
