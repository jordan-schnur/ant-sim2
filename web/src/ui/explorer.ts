/**
 * The context-sensitive right pane: renders whatever `store.state.selection`
 * points at — an ant (full detail + NN), a colony (stats + its chart), a tile
 * (terrain + pheromone here), or, with nothing selected, world totals. One
 * panel replaces the old fixed stack, the way a game engine's inspector shows
 * the current object.
 */

import type { Store } from "../state.js";
import { worldSummary } from "../state.js";
import { renderAntDetail, attachNNPopover } from "./inspector.js";
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
  attachNNPopover(nn, store);
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
      kvRow(kv, "refounds", "refounds", String(c.refounds));
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
