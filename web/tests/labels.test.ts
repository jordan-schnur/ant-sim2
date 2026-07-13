/**
 * The map-label projection is the picker's inverse. If it drifts, on-map labels
 * detach from the nests/food/ants they name — and worse on retina, where a
 * stray `dpr` in the world coordinate scales every position by `dpr`.
 */

import { describe, expect, it } from "vitest";
import { Camera } from "../src/render/camera.js";
import { projectToCss } from "../src/ui/labels.js";

// Device-pixel viewport; the camera works in device px, CSS is viewW/dpr wide.
const VIEW_W = 1600;
const VIEW_H = 1200;

describe("map label projection", () => {
  it("is independent of dpr: a label sits at the same CSS spot on any display", () => {
    // Same physical display, viewed at dpr 1 and dpr 2. Physically `zoom` is
    // device-px-per-cell, so on the 2x display the device viewport AND the zoom
    // both double; projectToCss divides dpr back out, so the CSS position must
    // match. The old `wx * dpr` bug broke exactly this — it doubled the world
    // coordinate on retina, so the two paths diverged.
    const cssW = 800;
    const cssH = 600;
    const lo = new Camera(512, 512);
    lo.zoom = 4;
    lo.cx = 200;
    lo.cy = 150;
    const hi = new Camera(512, 512);
    hi.zoom = 8; // 2x device pixels per cell
    hi.cx = 200;
    hi.cy = 150;
    const at1 = projectToCss(lo, 260, 190, cssW, cssH, 1);
    const at2 = projectToCss(hi, 260, 190, cssW * 2, cssH * 2, 2);
    expect(at2.left).toBeCloseTo(at1.left, 4);
    expect(at2.top).toBeCloseTo(at1.top, 4);
  });

  it("inverts the click picker so a label lands on the cell it names", () => {
    // The picker maps a CSS mouse point to a world cell via
    // screenToWorld(cssX * dpr). Projecting that cell back must return the CSS
    // point. This is the invariant that keeps labels glued to their anchors.
    const c = new Camera(512, 512);
    c.zoom = 3.5;
    c.cx = 111;
    c.cy = 222;
    const dpr = 2;
    for (const [cssX, cssY] of [[0, 0], [640, 480], [800, 600]]) {
      const world = c.screenToWorld(cssX * dpr, cssY * dpr, VIEW_W, VIEW_H);
      const back = projectToCss(c, world.x, world.y, VIEW_W, VIEW_H, dpr);
      expect(back.left).toBeCloseTo(cssX, 3);
      expect(back.top).toBeCloseTo(cssY, 3);
    }
  });

  it("centres a world point in the middle of the CSS viewport", () => {
    const c = new Camera(512, 512);
    c.cx = 256;
    c.cy = 256;
    const dpr = 2;
    const p = projectToCss(c, 256, 256, VIEW_W, VIEW_H, dpr);
    expect(p.left).toBeCloseTo(VIEW_W / 2 / dpr, 4);
    expect(p.top).toBeCloseTo(VIEW_H / 2 / dpr, 4);
  });
});
