/**
 * Terrain-derived geometry the UI needs in world-cell coordinates. Kept out of
 * the renderer and the label overlay so the camera snap, the in-world colony
 * popover, and the labels all read one definition of "where is a colony's
 * nest" rather than three that can drift apart.
 */

import type { Terrain } from "./protocol.js";

/**
 * Mean position of each colony's nest texels, in world cells. The terrain B
 * channel holds the owning colony per texel; 255 means "no nest". A texel at
 * (tx, ty) covers a `factor`-sized cell block, so the centroid is offset by
 * half a texel to land at the block's centre.
 */
export function nestCentroids(t: Terrain): Map<number, { x: number; y: number }> {
  const { w, h, factor, rgba } = t;
  const sums = new Map<number, { sx: number; sy: number; n: number }>();
  for (let ty = 0; ty < h; ty++) {
    for (let tx = 0; tx < w; tx++) {
      const nest = rgba[(ty * w + tx) * 4 + 2];
      if (nest === 255) continue;
      const s = sums.get(nest) ?? { sx: 0, sy: 0, n: 0 };
      s.sx += tx;
      s.sy += ty;
      s.n += 1;
      sums.set(nest, s);
    }
  }
  const out = new Map<number, { x: number; y: number }>();
  for (const [colony, s] of sums) {
    out.set(colony, { x: (s.sx / s.n + 0.5) * factor, y: (s.sy / s.n + 0.5) * factor });
  }
  return out;
}
