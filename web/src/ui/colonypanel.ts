/**
 * In-world colony popover. Clicking a nest opens a small card that sits on that
 * nest and follows the camera, showing the colony's live stats — population and,
 * the number the operator asked for, how much food is stored. Dismissed by
 * clicking empty ground or pressing Escape (both clear `selectedColony`).
 *
 * Positioned like the map labels: the nest centroid is projected through the
 * camera every frame via `projectToCss`, so the popover stays glued to the nest
 * as you pan and zoom.
 */

import { colonyCss } from "../colors.js";
import type { Camera } from "../render/camera.js";
import type { Store } from "../state.js";
import { drawSymbol, symbolFor } from "../symbols.js";
import { projectToCss } from "./labels.js";

export class ColonyPanel {
  private root: HTMLElement;
  private shownColony: number | null = null;
  private nameEl!: HTMLElement;
  private rows = new Map<string, HTMLElement>();

  constructor(parent: HTMLElement) {
    this.root = document.createElement("div");
    this.root.className = "colony-popover";
    this.root.style.display = "none";
    parent.append(this.root);
  }

  update(camera: Camera, viewW: number, viewH: number, dpr: number, store: Store): void {
    const id = store.selectedColony();
    const centroid = id === null ? undefined : store.state.nestCentroids.get(id);
    if (id === null || !centroid) {
      this.root.style.display = "none";
      this.shownColony = null;
      return;
    }

    if (this.shownColony !== id) {
      this.build(id);
      this.shownColony = id;
    }
    this.root.style.display = "";

    this.nameEl.textContent = store.colonyName(id);
    const s = store.state.stats.find((c) => c.id === id);
    // Stats arrive on their own frame; before the first one lands, show dashes
    // rather than an empty card so the popover never looks broken.
    this.rows.get("pop")!.textContent = s ? String(s.population) : "—";
    this.rows.get("store")!.textContent = s ? s.store.toFixed(0) : "—";
    this.rows.get("delivered")!.textContent = s ? s.deliveredTotal.toFixed(0) : "—";
    this.rows.get("gen")!.textContent = s ? s.meanLineage.toFixed(1) : "—";

    const p = projectToCss(camera, centroid.x, centroid.y, viewW, viewH, dpr);
    this.root.style.left = `${p.left}px`;
    this.root.style.top = `${p.top}px`;
  }

  /** Rebuild the card for a newly selected colony (glyph colour and rows). */
  private build(id: number): void {
    this.root.textContent = "";
    this.rows.clear();

    const title = document.createElement("div");
    title.className = "title";
    const glyph = document.createElement("canvas");
    glyph.width = 18;
    glyph.height = 18;
    glyph.className = "swatch-glyph";
    const ctx = glyph.getContext("2d");
    if (ctx) drawSymbol(ctx, symbolFor(id), 9, 9, 7, colonyCss(id));
    this.nameEl = document.createElement("span");
    title.append(glyph, this.nameEl);

    const kv = document.createElement("div");
    kv.className = "kv";
    for (const key of ["pop", "store", "delivered", "gen"]) {
      const l = document.createElement("span");
      l.textContent = key;
      const v = document.createElement("b");
      kv.append(l, v);
      this.rows.set(key, v);
    }

    this.root.append(title, kv);
  }
}
