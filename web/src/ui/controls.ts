/**
 * Left rail: playback, pheromone layer toggles, and the live tuning sliders.
 *
 * Sliders write straight into the running simulation. Tuning these by editing
 * Rust and recompiling would make the project miserable; after the pause button
 * this is the highest-leverage thing in the UI.
 */

import type { Net } from "../net.js";
import {
  cmdClearSelection,
  cmdLoad,
  cmdReset,
  cmdSave,
  cmdSetConfig,
  cmdSetPaused,
  cmdSetPheroRes,
  cmdSetSpeed,
  cmdStep,
} from "../protocol.js";
import type { Speed, Store } from "../state.js";
import { DIVISIONS, TUNABLES, formatValue, toPosition, toValue } from "./tunables.js";

/** Slider drags fire faster than the sim needs; ~20 Hz is plenty. */
const SEND_THROTTLE_MS = 50;

export function mountControls(root: HTMLElement, store: Store, net: Net): void {
  root.innerHTML = "";

  root.append(section("Playback"));
  const playRow = div("row");
  const btnPause = button("▶", () => {
    const next = !store.state.paused;
    store.setPaused(next);
    net.send(cmdSetPaused(next));
  });
  const btnStep = button("⏭", () => {
    store.setPaused(true);
    net.send(cmdStep());
  });
  playRow.append(btnPause, btnStep);
  root.append(playRow);

  const speedRow = div("row");
  const speedBtns: HTMLButtonElement[] = [];
  (["1x", "10x", "100x"] as const).forEach((label, i) => {
    const b = button(label, () => {
      store.setSpeed(i as Speed);
      net.send(cmdSetSpeed(i));
      net.send(cmdSetPaused(false));
    });
    speedBtns.push(b);
    speedRow.append(b);
  });
  root.append(speedRow);

  root.append(section("Layers"));
  const layerBoxes: Record<string, HTMLInputElement> = {};
  for (const key of ["food", "alarm", "scent"] as const) {
    const { label, input } = checkbox(key, store.state.layers[key], () => store.toggleLayer(key));
    layerBoxes[key] = input;
    root.append(label);
  }
  const labelsBox = checkbox("labels", store.state.labels, () => store.toggleLabels());
  root.append(labelsBox.label);

  const resRow = div("row");
  const btnRes = button("phero 256²", () => {
    const next = store.state.pheroResLog2 === 8 ? 9 : 8;
    store.state.pheroResLog2 = next;
    net.send(cmdSetPheroRes(next));
    store.notify();
  });
  resRow.append(btnRes);
  root.append(resRow);

  // The tuning sliders are long and rarely touched, so they hide behind a
  // collapsible header — the save/load/reset row was being pushed off-screen.
  const tuningHead = document.createElement("button");
  tuningHead.className = "section-toggle";
  tuningHead.textContent = "Tuning ▸";
  const tuningBody = div("tuning-body collapsed");
  tuningHead.addEventListener("click", () => {
    const open = tuningBody.classList.toggle("collapsed") === false;
    tuningHead.textContent = open ? "Tuning ▾" : "Tuning ▸";
  });
  root.append(tuningHead);
  const sliders = TUNABLES.map((t) => {
    const wrap = div("slider");
    const head = div("head");
    const name = document.createElement("span");
    name.textContent = t.label;
    const val = document.createElement("b");
    head.append(name, val);
    if (t.hint) head.title = t.hint;

    const input = document.createElement("input");
    input.type = "range";
    input.min = "0";
    input.max = String(DIVISIONS);
    input.step = "1";
    if (t.hint) input.title = t.hint;

    let last = 0;
    let pending: number | null = null;
    let timer: number | null = null;

    const flush = () => {
      timer = null;
      if (pending === null) return;
      net.send(cmdSetConfig(t.id, pending));
      pending = null;
    };

    input.addEventListener("input", () => {
      const v = toValue(t, Number(input.value) / DIVISIONS);
      val.textContent = formatValue(t, v);
      pending = v;
      const now = performance.now();
      if (now - last >= SEND_THROTTLE_MS) {
        last = now;
        flush();
      } else if (timer === null) {
        timer = window.setTimeout(flush, SEND_THROTTLE_MS);
      }
    });

    wrap.append(head, input);
    return { t, input, val, wrap };
  });
  sliders.forEach((s) => tuningBody.append(s.wrap));
  root.append(tuningBody);

  root.append(section("World"));
  const worldRow = div("row");
  worldRow.append(
    button("save", () => net.send(cmdSave())),
    button("load", () => net.send(cmdLoad())),
  );
  root.append(worldRow);

  const resetRow = div("row");
  const seedInput = document.createElement("input");
  seedInput.type = "number";
  seedInput.value = "1";
  seedInput.min = "0";
  seedInput.style.cssText =
    "flex:1;min-width:0;background:var(--panel-2);color:var(--text);border:1px solid var(--line);border-radius:4px;padding:4px 6px;font:inherit";
  resetRow.append(
    seedInput,
    button("reset", () => {
      const seed = Math.max(0, Math.floor(Number(seedInput.value) || 0));
      store.clearSelection();
      net.send(cmdClearSelection());
      net.send(cmdReset(seed));
    }),
  );
  root.append(resetRow);

  const status = div("status");
  root.append(status);

  const render = () => {
    const st = store.state;
    btnPause.textContent = st.paused ? "▶ run" : "❚❚ pause";
    btnPause.classList.toggle("on", !st.paused);
    btnStep.textContent = "⏭ step";
    speedBtns.forEach((b, i) => b.classList.toggle("on", st.speed === i && !st.paused));

    for (const key of ["food", "alarm", "scent"] as const) {
      layerBoxes[key].checked = st.layers[key];
    }
    labelsBox.input.checked = st.labels;
    btnRes.textContent = st.pheroResLog2 === 9 ? "phero 512²" : "phero 256²";

    // Slider positions come from the server's config frame, not from our own
    // guess at the defaults. Skip the one being dragged: writing to `value`
    // mid-drag fights the pointer.
    for (const s of sliders) {
      const v = st.config.get(s.t.id);
      if (v === undefined) continue;
      s.val.textContent = formatValue(s.t, v);
      if (document.activeElement !== s.input) {
        s.input.value = String(Math.round(toPosition(s.t, v) * DIVISIONS));
      }
    }

    status.textContent = st.connected ? `tick ${st.tick.toLocaleString()}` : "disconnected";
    status.classList.toggle("bad", !st.connected);
  };

  store.subscribe(render);
  render();
}

function section(title: string): HTMLElement {
  const h = document.createElement("h2");
  h.textContent = title;
  return h;
}

function div(cls: string): HTMLDivElement {
  const d = document.createElement("div");
  d.className = cls;
  return d;
}

function button(label: string, onClick: () => void): HTMLButtonElement {
  const b = document.createElement("button");
  b.textContent = label;
  b.addEventListener("click", onClick);
  return b;
}

function checkbox(
  label: string,
  checked: boolean,
  onChange: () => void,
): { label: HTMLLabelElement; input: HTMLInputElement } {
  const l = document.createElement("label");
  l.className = "check";
  const input = document.createElement("input");
  input.type = "checkbox";
  input.checked = checked;
  input.addEventListener("change", onChange);
  const span = document.createElement("span");
  span.textContent = label;
  l.append(input, span);
  return { label: l, input };
}
