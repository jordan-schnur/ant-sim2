/**
 * In-world ant popover: a small card anchored on the selected ant that follows
 * it as it moves. A glance-level readout (identity, energy, carrying,
 * delivered, and the fitness headline); the full stats and the network live in
 * the Explorer tab. Text-only on purpose — a live NN chasing a moving ant is
 * unreadable.
 */

import type { Camera } from "../render/camera.js";
import type { Store } from "../state.js";
import { projectToCss } from "./labels.js";
import { fitness, DEFAULT_HARVEST_WEIGHT, HARVEST_WEIGHT_FIELD } from "../fitness.js";

export class AntPopover {
  private root: HTMLElement;
  private nameEl!: HTMLElement;
  private rows = new Map<string, HTMLElement>();
  private built = false;

  constructor(parent: HTMLElement) {
    this.root = document.createElement("div");
    this.root.className = "colony-popover ant-popover";
    this.root.style.display = "none";
    parent.append(this.root);
  }

  update(camera: Camera, viewW: number, viewH: number, dpr: number, store: Store): void {
    const d = store.state.detail;
    const isAnt = store.state.selection?.kind === "ant";
    if (!isAnt || !d || !d.alive) {
      this.root.style.display = "none";
      return;
    }
    if (!this.built) {
      this.build();
      this.built = true;
    }
    this.root.style.display = "";

    const weight = store.state.config.get(HARVEST_WEIGHT_FIELD) ?? DEFAULT_HARVEST_WEIGHT;
    this.nameEl.textContent = d.name || `#${d.id}`;
    this.rows.get("energy")!.textContent = `${d.energy.toFixed(0)} / ${d.maxEnergy.toFixed(0)}`;
    this.rows.get("carrying")!.textContent = d.carrying.toFixed(1);
    this.rows.get("delivered")!.textContent = d.foodDelivered.toFixed(0);
    this.rows.get("fitness")!.textContent = fitness(d.foodDelivered, d.foodHarvested, weight).toFixed(0);

    const p = projectToCss(camera, d.x, d.y, viewW, viewH, dpr);
    this.root.style.left = `${p.left}px`;
    this.root.style.top = `${p.top}px`;
  }

  private build(): void {
    this.root.textContent = "";
    const title = document.createElement("div");
    title.className = "title";
    this.nameEl = document.createElement("span");
    title.append(this.nameEl);

    const kv = document.createElement("div");
    kv.className = "kv";
    for (const key of ["energy", "carrying", "delivered", "fitness"]) {
      const l = document.createElement("span");
      l.textContent = key;
      const v = document.createElement("b");
      kv.append(l, v);
      this.rows.set(key, v);
    }
    this.root.append(title, kv);
  }
}
