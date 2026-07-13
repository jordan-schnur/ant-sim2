/**
 * The selected ant: identity, energy economy, a fitness headline, its traits
 * and outputs, and its network drawn live. Rendered into a caller-supplied
 * container + canvas so the Explorer tab can host it.
 */

import { TRAIT_NAMES } from "../protocol.js";
import type { Store } from "../state.js";
import { draw as drawNet } from "./nnview.js";
import { fitness, DEFAULT_HARVEST_WEIGHT, HARVEST_WEIGHT_FIELD } from "../fitness.js";
import { tipLabel } from "./tooltips.js";

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
