/**
 * Nest centroids drive the camera snap and the in-world colony popover. If they
 * drift, "focus this colony" frames empty dirt and the popover points nowhere.
 */

import { describe, expect, it } from "vitest";
import { nestCentroids } from "../src/terrain.js";
import type { Terrain } from "../src/protocol.js";

/** Build a terrain frame with the given per-texel nest owners (255 = none). */
function terrain(w: number, h: number, factor: number, owners: number[]): Terrain {
  const rgba = new Uint8Array(w * h * 4);
  for (let i = 0; i < w * h; i++) rgba[i * 4 + 2] = owners[i];
  return { w, h, factor, rgba, tick: 0 } as Terrain;
}

describe("nestCentroids", () => {
  it("returns the half-texel-centred mean of each colony's nest texels", () => {
    // A single colony-0 texel at (1,1), factor 4: centre of that block is (6,6).
    const t = terrain(3, 3, 4, [255, 255, 255, 255, 0, 255, 255, 255, 255]);
    const c = nestCentroids(t);
    expect(c.size).toBe(1);
    expect(c.get(0)).toEqual({ x: 6, y: 6 });
  });

  it("separates colonies and averages multi-texel nests", () => {
    // Colony 0 spans texels (0,0)+(2,0) -> mean tx 1; colony 1 at (1,1).
    const t = terrain(3, 2, 2, [0, 255, 0, 255, 1, 255]);
    const c = nestCentroids(t);
    expect(c.get(0)).toEqual({ x: (1 + 0.5) * 2, y: (0 + 0.5) * 2 });
    expect(c.get(1)).toEqual({ x: (1 + 0.5) * 2, y: (1 + 0.5) * 2 });
  });

  it("is empty when no texel is owned", () => {
    const t = terrain(2, 2, 1, [255, 255, 255, 255]);
    expect(nestCentroids(t).size).toBe(0);
  });
});
