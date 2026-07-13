/**
 * The story log: the chronicle of firsts and rolling titles, newest first.
 *
 * The sim streams a full capped snapshot at the stats cadence and the client
 * replaces its list wholesale, so there is no incremental state to reconcile —
 * the panel simply re-renders whatever the latest chronicle frame holds.
 */

import { colonyCss } from "../colors.js";
import type { Store } from "../state.js";

export function mountChronicle(root: HTMLElement, store: Store): void {
  const h = document.createElement("h2");
  h.textContent = "Chronicle";
  const panel = document.createElement("div");
  panel.className = "chronicle";
  root.append(h, panel);

  const render = () => {
    const evs = store.state.chronicle?.events ?? [];
    panel.innerHTML = "";
    if (evs.length === 0) {
      const empty = document.createElement("div");
      empty.className = "chron-empty";
      empty.textContent = "no firsts yet";
      panel.append(empty);
      return;
    }
    // Newest first; cap what we draw so a long session cannot grow the DOM
    // without bound (the sim already caps the log itself).
    for (const e of [...evs].reverse().slice(0, 60)) {
      const row = document.createElement("div");
      row.className = "chron-row";
      row.style.borderLeft = `3px solid ${colonyCss(e.colony)}`;
      const t = document.createElement("span");
      t.className = "chron-tick";
      t.textContent = `t${e.tick.toLocaleString()}`;
      const txt = document.createElement("span");
      txt.className = "chron-text";
      txt.textContent = e.antName ? `${e.text} — ${e.antName}` : e.text;
      row.append(t, txt);
      panel.append(row);
    }
  };
  store.subscribe(render);
  render();
}
