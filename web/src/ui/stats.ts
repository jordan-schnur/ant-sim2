/**
 * The Stats tab: every colony's evolution over time in one place. One stacked
 * chart per metric, each drawing a line per colony (plus a world total where a
 * sum is meaningful) against a shared tick axis.
 *
 * All series come from `store.state.history`, which `applyStats` appends to on
 * every stats frame — so all eight colonies' arrays advance in lockstep and
 * index `i` is the same tick across colonies. That lets the world-total line be
 * a plain elementwise sum.
 */

import { colonyCss } from "../colors.js";
import type { ColonyHistory, Store } from "../state.js";
import { symbolFor, drawSymbol } from "../symbols.js";
import { openGraph } from "./graphmodal.js";
import { infoDot } from "./explain.js";

/** One metric to chart: how to pull its per-colony series, its units, and how a
 *  single "world" line aggregates the colonies — a sum for extensive quantities
 *  (food, ants, refounds) and a mean for intensive ones (generation depth). */
export interface Metric {
  key: string;
  label: string;
  unit: string;
  pick: (h: ColonyHistory) => number[];
  agg: "sum" | "mean";
}

export const METRICS: Metric[] = [
  { key: "delivered", label: "Food delivered", unit: "food", pick: (h) => h.deliveredTotal, agg: "sum" },
  { key: "population", label: "Population", unit: "ants", pick: (h) => h.population, agg: "sum" },
  { key: "generation", label: "Generation (mean lineage depth)", unit: "depth", pick: (h) => h.generation, agg: "mean" },
  { key: "distinct", label: "Generations alive (distinct depths)", unit: "count", pick: (h) => h.distinctGenerations, agg: "mean" },
  { key: "refounds", label: "Refounds (collapse thrash)", unit: "count", pick: (h) => h.refounds, agg: "sum" },
  { key: "store", label: "Food store", unit: "food", pick: (h) => h.store, agg: "sum" },
];

export const WORLD_CSS = "rgba(235, 235, 245, 0.9)";

export function mountStats(root: HTMLElement, store: Store): void {
  const wrap = document.createElement("div");
  wrap.className = "stats-pane";

  const legend = document.createElement("div");
  legend.className = "stats-legend";
  wrap.append(legend);

  const charts = METRICS.map((m) => {
    const block = document.createElement("div");
    block.className = "stats-chart";
    block.title = "click to open a full graph";
    const title = document.createElement("div");
    title.className = "stats-chart-title";
    title.textContent = m.label;
    title.append(infoDot(`stat.${m.key}`));
    const canvas = document.createElement("canvas");
    canvas.className = "stats-canvas";
    block.append(title, canvas);
    // Click anywhere on the compact chart opens the interactive uPlot modal,
    // focused on this metric.
    block.addEventListener("click", () => openGraph(store, m.key));
    wrap.append(block);
    return { m, canvas, title };
  });

  root.append(wrap);

  const renderLegend = () => {
    // Rebuild only when the colony set changes, which is never after connect —
    // but cheap enough to just do once the first time stats arrive.
    if (legend.childElementCount > 0) return;
    const ids = [...store.state.history.keys()].sort((a, b) => a - b);
    if (ids.length === 0) return;
    for (const id of ids) {
      const item = document.createElement("span");
      item.className = "stats-legend-item";
      const swatch = document.createElement("canvas");
      swatch.width = 16;
      swatch.height = 16;
      swatch.className = "stats-legend-swatch";
      const sctx = swatch.getContext("2d");
      if (sctx) drawSymbol(sctx, symbolFor(id), 8, 8, 6, colonyCss(id));
      const name = document.createElement("span");
      name.textContent = store.colonyName(id);
      item.append(swatch, name);
      legend.append(item);
    }
    const world = document.createElement("span");
    world.className = "stats-legend-item";
    world.innerHTML = `<span class="stats-legend-line" style="background:${WORLD_CSS}"></span>world (sum / mean)`;
    legend.append(world);
  };

  const render = () => {
    // Cheap to skip when the tab is hidden — the rail sets display:none.
    if (store.state.activeTab !== "stats") return;
    renderLegend();
    for (const c of charts) drawChart(c.canvas, store, c.m);
  };

  store.subscribe(render);
  render();
}

function drawChart(canvas: HTMLCanvasElement, store: Store, m: Metric): void {
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  const w = Math.max(1, Math.round(canvas.clientWidth * dpr));
  const h = Math.max(1, Math.round(canvas.clientHeight * dpr));
  if (canvas.width !== w || canvas.height !== h) {
    canvas.width = w;
    canvas.height = h;
  }
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.clearRect(0, 0, w, h);
  ctx.fillStyle = "#0a0a0c";
  ctx.fillRect(0, 0, w, h);

  const hist = store.state.history;
  const ids = [...hist.keys()].sort((a, b) => a - b);
  if (ids.length === 0) return;

  // All colonies advance together, so any colony's length is the sample count.
  const len = Math.max(...ids.map((id) => m.pick(hist.get(id)!).length));
  if (len < 2) return;

  // A summed world line for extensive metrics; a mean line for intensive ones.
  const world = new Array<number>(len).fill(0);
  const counts = new Array<number>(len).fill(0);
  let max = 0;
  for (const id of ids) {
    const s = m.pick(hist.get(id)!);
    for (let i = 0; i < s.length; i++) {
      max = Math.max(max, s[i]);
      world[i + (len - s.length)] += s[i];
      counts[i + (len - s.length)] += 1;
    }
  }
  if (m.agg === "mean") for (let i = 0; i < len; i++) world[i] /= counts[i] || 1;
  for (const v of world) max = Math.max(max, v);
  // Flat-zero metric (nothing delivered yet) draws a baseline rather than
  // dividing by zero and vanishing.
  const pad = 3 * dpr;
  const scale = max > 0 ? (h - 2 * pad) / max : 0;

  const line = (s: number[], color: string, width: number) => {
    const off = len - s.length; // right-align shorter series (all equal here)
    ctx.strokeStyle = color;
    ctx.lineWidth = width * dpr;
    ctx.beginPath();
    for (let i = 0; i < s.length; i++) {
      const x = ((i + off) / (len - 1)) * (w - 1);
      const y = h - pad - s[i] * scale;
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.stroke();
  };

  for (const id of ids) line(m.pick(hist.get(id)!), colonyCss(id, 0.9), 1.25);
  line(world, WORLD_CSS, 1.75);

  // Peak label, top-left, so the eye gets a magnitude without axes.
  if (max > 0) {
    ctx.fillStyle = "rgba(200,200,210,0.6)";
    ctx.font = `${10 * dpr}px system-ui, sans-serif`;
    ctx.textBaseline = "top";
    const peak = max >= 100 ? Math.round(max).toString() : max.toFixed(1);
    ctx.fillText(`peak ${peak}`, 4 * dpr, 3 * dpr);
  }
}
