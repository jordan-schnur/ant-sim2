/**
 * On-map labels, Prison-Architect style: a nest label (glyph + colony name) per
 * colony, a "Food" label per standing-food cluster, and the selected ant's name.
 *
 * Positions are recomputed from the world each frame and projected through the
 * camera; the heavy work (finding nest centroids and food clusters) runs only
 * when a new terrain frame arrives, not every frame. Labels nudge apart rather
 * than overlap, and fade out when zoomed too far to be legible.
 */

import type { Camera } from "../render/camera.js";
import { colonyCss } from "../colors.js";
import type { Store } from "../state.js";
import { drawSymbol, symbolFor } from "../symbols.js";

/** Below this zoom the map is too small for labels to be readable. */
const LABEL_MIN_ZOOM = 1.2;
/** A terrain food texel this bright (0..255) anchors a food cluster. */
const FOOD_THRESHOLD = 40;
/** Don't drown the map: cap food labels to the biggest clusters. */
const MAX_FOOD_LABELS = 12;

interface Anchor {
  key: string;
  wx: number;
  wy: number;
  text: string;
  colony: number | null;
}

/**
 * World cell -> CSS pixel, matching the picker's inverse. The camera pans and
 * zooms in *device* pixels (mouse coords are multiplied by `dpr` before
 * `screenToWorld`), but world coordinates are plain cells: `worldToScreen`
 * takes cells and returns device px. The overlay is positioned in CSS px, so
 * the only `dpr` in the whole chain is the final divide. Feeding `dpr` into the
 * world coordinate — as this once did — scales every label's position by `dpr`
 * on retina, scattering them across the map.
 */
export function projectToCss(
  camera: Camera,
  wx: number,
  wy: number,
  viewW: number,
  viewH: number,
  dpr: number,
): { left: number; top: number } {
  const s = camera.worldToScreen(wx, wy, viewW, viewH);
  return { left: s.x / dpr, top: s.y / dpr };
}

export class LabelOverlay {
  private root: HTMLElement;
  private nodes = new Map<string, HTMLElement>();
  private foodCentroids: { x: number; y: number }[] = [];
  private terrainTick = -1;

  constructor(parent: HTMLElement) {
    this.root = document.createElement("div");
    this.root.className = "labels";
    parent.append(this.root);
  }

  setVisible(v: boolean): void {
    this.root.style.display = v ? "" : "none";
  }

  update(camera: Camera, viewW: number, viewH: number, dpr: number, store: Store): void {
    if (camera.zoom < LABEL_MIN_ZOOM) {
      this.setVisible(false);
      return;
    }
    this.setVisible(true);

    const t = store.state.terrain;
    if (t && t.tick !== this.terrainTick) {
      this.terrainTick = t.tick;
      this.recomputeTerrainAnchors(store);
    }

    const anchors: Anchor[] = [];
    for (const [colony, n] of store.state.nestCentroids) {
      // The open popover already names this nest; a label under it just doubles
      // the text and fights the popover's pointer.
      if (colony === store.selectedColony()) continue;
      anchors.push({
        key: `nest-${colony}`,
        wx: n.x,
        wy: n.y,
        text: store.colonyName(colony),
        colony,
      });
    }
    for (let i = 0; i < this.foodCentroids.length; i++) {
      const f = this.foodCentroids[i];
      anchors.push({ key: `food-${i}`, wx: f.x, wy: f.y, text: "Food", colony: null });
    }
    const d = store.state.detail;
    if (d && d.alive) {
      anchors.push({
        key: "selected-ant",
        wx: d.x,
        wy: d.y,
        text: d.name || `#${d.id}`,
        colony: d.colony,
      });
    }

    this.placeLabels(anchors, camera, viewW, viewH, dpr);
  }

  /**
   * Food clusters from the terrain R channel. Nest centroids come from the
   * store (see `Store.nestCentroids`), shared with the camera snap and popover.
   */
  private recomputeTerrainAnchors(store: Store): void {
    const t = store.state.terrain;
    if (!t) return;
    const { w, h, factor, rgba } = t;

    // --- Food clusters: connected components over bright food texels. ---
    const seen = new Uint8Array(w * h);
    const clusters: { x: number; y: number; mass: number }[] = [];
    const stack: number[] = [];
    for (let start = 0; start < w * h; start++) {
      if (seen[start] || rgba[start * 4] < FOOD_THRESHOLD) continue;
      let sx = 0;
      let sy = 0;
      let n = 0;
      stack.push(start);
      seen[start] = 1;
      while (stack.length) {
        const c = stack.pop()!;
        const cx = c % w;
        const cy = (c / w) | 0;
        sx += cx;
        sy += cy;
        n += 1;
        for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1]] as const) {
          const nx = cx + dx;
          const ny = cy + dy;
          if (nx < 0 || ny < 0 || nx >= w || ny >= h) continue;
          const ni = ny * w + nx;
          if (!seen[ni] && rgba[ni * 4] >= FOOD_THRESHOLD) {
            seen[ni] = 1;
            stack.push(ni);
          }
        }
      }
      clusters.push({ x: (sx / n + 0.5) * factor, y: (sy / n + 0.5) * factor, mass: n });
    }
    clusters.sort((a, b) => b.mass - a.mass);
    this.foodCentroids = clusters.slice(0, MAX_FOOD_LABELS).map((c) => ({ x: c.x, y: c.y }));
  }

  /** Project anchors to screen, nudge apart, and reuse DOM nodes by key. */
  private placeLabels(
    anchors: Anchor[],
    camera: Camera,
    viewW: number,
    viewH: number,
    dpr: number,
  ): void {
    const live = new Set<string>();
    const placed: { l: number; t: number; r: number; b: number }[] = [];

    for (const a of anchors) {
      const p = projectToCss(camera, a.wx, a.wy, viewW, viewH, dpr);
      let left = p.left;
      let top = p.top;
      if (left < -80 || top < -40 || left > viewW / dpr + 80 || top > viewH / dpr + 40) {
        continue; // off-screen: skip, but keep the node for reuse next frame
      }

      const el = this.nodeFor(a);
      live.add(a.key);
      // Measure once mounted so collision uses the real box.
      const wdt = el.offsetWidth || 60;
      const hgt = el.offsetHeight || 16;

      // Left-to-right, then down: shift until the box clears everything placed.
      for (let guard = 0; guard < 40; guard++) {
        const box = { l: left, t: top, r: left + wdt, b: top + hgt };
        const hit = placed.find(
          (p) => box.l < p.r && box.r > p.l && box.t < p.b && box.b > p.t,
        );
        if (!hit) {
          placed.push(box);
          break;
        }
        if (box.r + wdt < viewW / dpr) left = hit.r + 2;
        else {
          left = a.colony !== null ? p.left : left;
          top = hit.b + 2;
        }
      }

      el.style.left = `${left}px`;
      el.style.top = `${top}px`;
    }

    // Drop nodes whose anchor vanished (a colony reset, a deselected ant).
    for (const [key, el] of this.nodes) {
      if (!live.has(key)) {
        el.remove();
        this.nodes.delete(key);
      }
    }
  }

  private nodeFor(a: Anchor): HTMLElement {
    let el = this.nodes.get(a.key);
    if (!el) {
      el = document.createElement("div");
      el.className = "map-label";
      this.nodes.set(a.key, el);
      this.root.append(el);
    }
    // Rebuild content (name can change via rename; text is cheap).
    el.textContent = "";
    if (a.colony !== null) {
      const glyph = document.createElement("canvas");
      glyph.width = 16;
      glyph.height = 16;
      glyph.className = "map-label-glyph";
      const ctx = glyph.getContext("2d");
      if (ctx) drawSymbol(ctx, symbolFor(a.colony), 8, 8, 6, colonyCss(a.colony));
      el.append(glyph);
    }
    const span = document.createElement("span");
    span.textContent = a.text;
    el.append(span);
    return el;
  }
}
