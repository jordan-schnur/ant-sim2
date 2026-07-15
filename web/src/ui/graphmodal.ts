/**
 * The expanded, interactive graph — a uPlot chart in a modal over the world.
 *
 * uPlot gives us, for ~45KB and zero dependencies, everything the hand-rolled
 * canvas charts can't cheaply do: labeled axes, a click-to-toggle legend (our
 * colony filter), and drag-to-zoom on the tick axis (our timeline). We drive it
 * live from `store.state.history`, preserving the operator's zoom while data
 * streams in.
 *
 * Two modes:
 *  - one metric selected  -> a line per colony + a world aggregate, real Y axis.
 *  - several selected     -> each metric's world aggregate, normalised to its
 *    own peak, so metrics on wildly different scales (food vs generation) can be
 *    overlaid to compare *shape* over time.
 */

import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import { colonyCss } from "../colors.js";
import type { Store } from "../state.js";
import { METRICS, WORLD_CSS, type Metric } from "./stats.js";

/** Distinct strokes for the overlay (multi-metric) mode, by selection order. */
const METRIC_PALETTE = [
  "#6ea8fe",
  "#ffd166",
  "#06d6a0",
  "#ef476f",
  "#c77dff",
  "#f78c6b",
];

let close: (() => void) | null = null;

/**
 * A cursor tooltip: on every cursor move uPlot gives us the focused data index;
 * we read each visible series' value there and render a small floating box. The
 * legend already shows values, but a box at the point is what the operator asked
 * for when reading an exact tick off the curve.
 */
function tooltipPlugin(tip: HTMLElement): uPlot.Plugin {
  return {
    hooks: {
      setCursor: (u: uPlot) => {
        const { idx, left, top } = u.cursor;
        if (idx == null || left == null || top == null || left < 0) {
          tip.style.display = "none";
          return;
        }
        const tick = u.data[0][idx];
        if (tick == null) {
          tip.style.display = "none";
          return;
        }
        // Collect visible rows, then sort by value descending so the highest
        // line reads at the top — the way you scan a chart for its peak.
        const entries: { v: number; html: string }[] = [];
        for (let s = 1; s < u.series.length; s++) {
          const ser = u.series[s];
          if (ser.show === false) continue;
          const v = u.data[s][idx];
          if (v == null || Number.isNaN(v)) continue;
          const stroke = typeof ser.stroke === "function" ? ser.stroke(u, s) : ser.stroke;
          entries.push({
            v: v as number,
            html:
              `<div class="graph-tip-row"><span class="graph-tip-swatch" style="background:${stroke ?? "#9aa"}"></span>` +
              `${ser.label}: <b>${(v as number) >= 100 ? Math.round(v as number) : (v as number).toFixed(2)}</b></div>`,
          });
        }
        entries.sort((a, b) => b.v - a.v);
        let rows = `<div class="graph-tip-x">tick ${Math.round(tick as number)}</div>`;
        for (const e of entries) rows += e.html;
        tip.innerHTML = rows;
        tip.style.display = "";
        // Offset from the cursor; flip left near the right edge so it stays in view.
        const pad = 12;
        const flip = left > u.over.clientWidth - 160;
        tip.style.left = `${flip ? left - tip.offsetWidth - pad : left + pad}px`;
        tip.style.top = `${top + pad}px`;
      },
    },
  };
}

export function openGraph(store: Store, initialKey: string): void {
  // Singleton: a second open replaces the first.
  if (close) close();

  const active = new Set<string>([initialKey]);
  let followLive = true; // false once the operator zooms, so we don't yank it.

  const backdrop = document.createElement("div");
  backdrop.className = "graph-backdrop";
  const panel = document.createElement("div");
  panel.className = "graph-panel";
  backdrop.append(panel);

  const header = document.createElement("div");
  header.className = "graph-header";

  const metricBar = document.createElement("div");
  metricBar.className = "graph-metrics";
  const metricBtns = new Map<string, HTMLButtonElement>();
  for (const m of METRICS) {
    const b = document.createElement("button");
    b.className = "graph-metric-btn";
    b.textContent = m.label;
    b.addEventListener("click", () => {
      // Toggle, but never leave zero metrics selected.
      if (active.has(m.key)) {
        if (active.size > 1) active.delete(m.key);
      } else {
        active.add(m.key);
      }
      syncMetricBtns();
      rebuild();
    });
    metricBar.append(b);
    metricBtns.set(m.key, b);
  }
  const syncMetricBtns = () => {
    for (const [k, b] of metricBtns) b.classList.toggle("on", active.has(k));
  };
  syncMetricBtns();

  const actions = document.createElement("div");
  actions.className = "graph-actions";
  const resetBtn = document.createElement("button");
  resetBtn.textContent = "reset zoom";
  resetBtn.addEventListener("click", () => {
    followLive = true;
    rebuild();
  });
  const closeBtn = document.createElement("button");
  closeBtn.textContent = "✕";
  closeBtn.title = "close (Esc)";
  closeBtn.addEventListener("click", () => close?.());
  actions.append(resetBtn, closeBtn);

  header.append(metricBar, actions);

  const note = document.createElement("div");
  note.className = "graph-note";

  const plotHost = document.createElement("div");
  plotHost.className = "graph-plot";

  const tip = document.createElement("div");
  tip.className = "graph-tip";
  tip.style.display = "none";
  plotHost.append(tip);

  panel.append(header, note, plotHost);
  document.body.append(backdrop);

  let plot: uPlot | null = null;

  const plotSize = () => ({
    width: Math.max(320, plotHost.clientWidth),
    height: Math.max(240, plotHost.clientHeight),
  });

  /** Build the (data, series, axis label) for the current metric selection. */
  const build = (): { data: uPlot.AlignedData; opts: uPlot.Options } => {
    const hist = store.state.history;
    const ids = [...hist.keys()].sort((a, b) => a - b);
    // Longest tick array is the x axis; colonies advance together so they match.
    let xs: number[] = [];
    for (const id of ids) {
      const t = hist.get(id)!.tick;
      if (t.length > xs.length) xs = t;
    }

    const aggregate = (m: Metric): number[] => {
      const out = new Array<number>(xs.length).fill(0);
      const cnt = new Array<number>(xs.length).fill(0);
      for (const id of ids) {
        const s = m.pick(hist.get(id)!);
        const off = xs.length - s.length;
        for (let i = 0; i < s.length; i++) {
          out[i + off] += s[i];
          cnt[i + off] += 1;
        }
      }
      if (m.agg === "mean") for (let i = 0; i < out.length; i++) out[i] /= cnt[i] || 1;
      return out;
    };

    const selected = METRICS.filter((m) => active.has(m.key));
    const series: uPlot.Series[] = [{ label: "tick" }];
    const data: (number[] | Float64Array)[] = [xs];

    let yLabel: string;
    if (selected.length === 1) {
      const m = selected[0];
      note.textContent =
        "Drag to zoom the tick axis · double-click to reset · click a colony in the legend to hide it.";
      for (const id of ids) {
        series.push({
          label: store.colonyName(id),
          stroke: colonyCss(id, 0.95),
          width: 1.4,
        });
        const s = m.pick(hist.get(id)!);
        const off = xs.length - s.length;
        data.push(off > 0 ? new Array<number>(off).fill(NaN).concat(s) : s);
      }
      series.push({ label: `world (${m.agg})`, stroke: WORLD_CSS, width: 2 });
      data.push(aggregate(m));
      yLabel = `${m.label} (${m.unit})`;
    } else {
      note.textContent =
        "Overlay: each line is a metric's world " +
        "aggregate normalised to its own peak, to compare shape over time.";
      selected.forEach((m, i) => {
        const agg = aggregate(m);
        let max = 0;
        for (const v of agg) max = Math.max(max, v);
        const norm = max > 0 ? agg.map((v) => v / max) : agg;
        series.push({
          label: `${m.label} (÷ ${max >= 100 ? Math.round(max) : max.toFixed(1)})`,
          stroke: METRIC_PALETTE[i % METRIC_PALETTE.length],
          width: 1.75,
        });
        data.push(norm);
      });
      yLabel = "normalised (each ÷ its own peak)";
    }

    const { width, height } = plotSize();
    const opts: uPlot.Options = {
      width,
      height,
      // Ticks are plain integers, not timestamps.
      scales: { x: { time: false } },
      cursor: { drag: { x: true, y: false } },
      legend: { live: true },
      axes: [
        {
          label: "tick",
          stroke: "#9aa",
          grid: { stroke: "rgba(255,255,255,0.05)" },
          ticks: { stroke: "rgba(255,255,255,0.15)" },
        },
        {
          label: yLabel,
          stroke: "#9aa",
          grid: { stroke: "rgba(255,255,255,0.05)" },
          ticks: { stroke: "rgba(255,255,255,0.15)" },
        },
      ],
      series,
      plugins: [tooltipPlugin(tip)],
      hooks: {
        // A drag-zoom (setSelect) means the operator picked a window; stop
        // auto-following the live tail until they reset.
        setSelect: [
          (u: uPlot) => {
            if (u.select.width > 0) followLive = false;
          },
        ],
      },
    };
    return { data: data as uPlot.AlignedData, opts };
  };

  const rebuild = () => {
    if (plot) {
      plot.destroy();
      plot = null;
    }
    const { data, opts } = build();
    plot = new uPlot(opts, data, plotHost);
  };

  // Live streaming: keep the same plot, just push new data. Preserve the zoom
  // window unless the operator is following the live tail.
  const render = () => {
    if (!plot) return;
    const { data } = build();
    plot.setData(data, followLive);
  };

  rebuild();

  const onResize = () => {
    if (plot) plot.setSize(plotSize());
  };
  window.addEventListener("resize", onResize);

  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") close?.();
  };
  window.addEventListener("keydown", onKey);

  backdrop.addEventListener("click", (e) => {
    if (e.target === backdrop) close?.();
  });

  const unsub = store.subscribe(render);

  close = () => {
    unsub();
    window.removeEventListener("resize", onResize);
    window.removeEventListener("keydown", onKey);
    if (plot) plot.destroy();
    backdrop.remove();
    close = null;
  };
}
