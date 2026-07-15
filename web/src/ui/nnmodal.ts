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
    // Optional-chained: the test's fake store exercises only subscribe/state,
    // and a missing setPaused should never block the network commands.
    store.setPaused?.(next);
    net.send(cmdSetPaused(next));
  });
  const stepBtn = document.createElement("button");
  stepBtn.className = "nnm-step";
  stepBtn.textContent = "⏭ step";
  stepBtn.addEventListener("click", () => {
    store.setPaused?.(true);
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
  document.addEventListener("keydown", onKey);
  backdrop.addEventListener("click", (e) => { if (e.target === backdrop) close?.(); });

  const unsub = store.subscribe(render);
  render();

  close = () => {
    unsub();
    window.removeEventListener("resize", onResize);
    document.removeEventListener("keydown", onKey);
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
