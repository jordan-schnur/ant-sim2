/**
 * Right rail: per-colony stats and a sparkline each.
 *
 * The chart is the point. The first 500k-tick run showed the delivery rate does
 * not bend upward until roughly tick 100,000 — at tick 5,000 every colony looks
 * dead. Without a curve the operator reads a single number and concludes the
 * wrong thing. `delivered_total` is monotonic and is the one to watch.
 */

import { colonyCss } from "../colors.js";
import type { ColonyHistory, Store } from "../state.js";

export function mountColonies(root: HTMLElement, store: Store): void {
  const heading = document.createElement("h2");
  heading.textContent = "Colonies";
  root.append(heading);
  const cards = new Map<number, ReturnType<typeof card>>();

  const render = () => {
    const st = store.state;

    for (const c of st.stats) {
      let k = cards.get(c.id);
      if (!k) {
        k = card(c.id);
        cards.set(c.id, k);
        root.append(k.el);
      }
      k.pop.textContent = String(c.population);
      k.store.textContent = c.store.toFixed(0);
      k.gen.textContent = c.meanLineage.toFixed(1);
      k.delivered.textContent = c.deliveredTotal.toFixed(0);
      k.births.textContent = String(c.births);

      // Free floor spawns against paid births. When this ratio runs away, the
      // safety net -- not the food economy -- is reproducing the colony, which
      // is exactly what the 500k-tick run found.
      const total = c.births + c.floorSpawns;
      const freeFrac = total > 0 ? c.floorSpawns / total : 0;
      k.freeBar.style.width = `${(freeFrac * 100).toFixed(1)}%`;
      k.free.textContent = `${(freeFrac * 100).toFixed(0)}%`;
      k.free.title = `${c.floorSpawns} free floor spawns vs ${c.births} paid births`;

      const h = st.history.get(c.id);
      if (h) sparkline(k.canvas, h, c.id);
    }

    // A reset can reduce the colony count; drop cards that no longer exist.
    for (const [id, k] of cards) {
      if (!st.stats.some((c) => c.id === id)) {
        k.el.remove();
        cards.delete(id);
      }
    }
  };

  store.subscribe(render);
  render();
}

function card(id: number) {
  const el = document.createElement("div");
  el.className = "colony";

  const title = document.createElement("div");
  title.className = "title";
  const sw = document.createElement("span");
  sw.className = "swatch";
  sw.style.background = colonyCss(id);
  const name = document.createElement("span");
  name.textContent = `colony ${id}`;
  title.append(sw, name);

  const kv = document.createElement("div");
  kv.className = "kv";
  const mk = (label: string) => {
    const l = document.createElement("span");
    l.textContent = label;
    const v = document.createElement("b");
    kv.append(l, v);
    return v;
  };
  const pop = mk("pop");
  const store_ = mk("store");
  const gen = mk("gen");
  const delivered = mk("delivered");
  const births = mk("paid births");
  const free = mk("free");

  const bar = document.createElement("div");
  bar.className = "bar";
  const freeBar = document.createElement("div");
  bar.append(freeBar);

  const canvas = document.createElement("canvas");
  canvas.title = "delivered_total over time";

  el.append(title, kv, bar, canvas);
  return { el, pop, store: store_, gen, delivered, births, free, freeBar, canvas };
}

/**
 * `delivered_total` against tick. Autoscaled to the window, because absolute
 * magnitudes differ by orders of magnitude between a foraging colony and one
 * that never found food.
 */
function sparkline(canvas: HTMLCanvasElement, h: ColonyHistory, id: number): void {
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  const w = Math.max(1, Math.round(canvas.clientWidth * dpr));
  const ht = Math.max(1, Math.round(canvas.clientHeight * dpr));
  if (canvas.width !== w || canvas.height !== ht) {
    canvas.width = w;
    canvas.height = ht;
  }

  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.clearRect(0, 0, w, ht);
  ctx.fillStyle = "#0a0a0c";
  ctx.fillRect(0, 0, w, ht);

  const n = h.deliveredTotal.length;
  if (n < 2) return;

  let max = 0;
  for (const v of h.deliveredTotal) max = Math.max(max, v);
  // A colony that has delivered nothing draws a flat line at the bottom rather
  // than dividing by zero and vanishing.
  const scale = max > 0 ? (ht - 2) / max : 0;

  ctx.strokeStyle = colonyCss(id, 0.95);
  ctx.lineWidth = Math.max(1, dpr);
  ctx.beginPath();
  for (let i = 0; i < n; i++) {
    const x = (i / (n - 1)) * (w - 1);
    const y = ht - 1 - h.deliveredTotal[i] * scale;
    if (i === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.stroke();
}
