/**
 * Right rail, lower half: the selected ant, and its network drawn live.
 *
 * A selected ant can die between frames. The server says so with the `alive`
 * byte, and this says so too, rather than freezing on numbers that stopped
 * being true.
 */

import { TRAIT_NAMES } from "../protocol.js";
import type { Store } from "../state.js";
import { draw as drawNet } from "./nnview.js";

const OUTPUT_NAMES = ["turn", "throttle", "attack", "grab", "mem0", "mem1", "mem2", "mem3"];

export function mountInspector(root: HTMLElement, store: Store): void {
  const h = document.createElement("h2");
  h.textContent = "Ant";
  const body = document.createElement("div");
  body.id = "inspector";
  const canvas = document.createElement("canvas");
  canvas.id = "nn";
  root.append(h, body, canvas);

  const render = () => {
    const d = store.state.detail;
    body.innerHTML = "";

    if (!d) {
      const p = document.createElement("div");
      p.className = "muted";
      p.textContent = "click an ant to inspect it";
      body.append(p);
      paint(canvas, null, null);
      return;
    }

    if (!d.alive) {
      const p = document.createElement("div");
      p.className = "muted";
      p.textContent = `ant #${d.id} died`;
      body.append(p);
      paint(canvas, null, null);
      return;
    }

    const kv = document.createElement("div");
    kv.className = "kv";
    const row = (k: string, v: string) => {
      const a = document.createElement("span");
      a.textContent = k;
      const b = document.createElement("b");
      b.textContent = v;
      kv.append(a, b);
    };

    row("id", `#${d.id}`);
    row("colony", String(d.colony));
    row("energy", `${d.energy.toFixed(1)} / ${d.maxEnergy.toFixed(0)}`);
    row("size", d.size.toFixed(2));
    row("age", String(d.age));
    row("generation", String(d.lineage));
    row("carrying", d.carrying.toFixed(2));
    row("delivered", d.foodDelivered.toFixed(1));
    body.append(kv);

    const th = document.createElement("h2");
    th.textContent = "Traits";
    body.append(th);
    const tkv = document.createElement("div");
    tkv.className = "kv";
    TRAIT_NAMES.forEach((name, i) => {
      const a = document.createElement("span");
      a.textContent = name;
      const b = document.createElement("b");
      b.textContent = d.traits[i].toFixed(name === "lifespan" ? 0 : 2);
      tkv.append(a, b);
    });
    body.append(tkv);

    const oh = document.createElement("h2");
    oh.textContent = "Outputs";
    body.append(oh);
    const okv = document.createElement("div");
    okv.className = "kv";
    OUTPUT_NAMES.forEach((name, i) => {
      const a = document.createElement("span");
      a.textContent = name;
      const b = document.createElement("b");
      b.textContent = d.outputs[i].toFixed(3);
      okv.append(a, b);
    });
    body.append(okv);

    paint(canvas, d, store.state.genome?.params ?? null);
  };

  store.subscribe(render);
  render();
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
