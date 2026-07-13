/**
 * The camera is pure math, so it is tested without a GL context. Getting
 * screen<->world wrong is how click-to-select ends up picking the wrong ant,
 * and that failure looks exactly like a broken `nearest_ant` on the server.
 */

import { describe, expect, it } from "vitest";
import { Camera, MAX_ZOOM, MIN_ZOOM } from "../src/render/camera.js";

const VIEW_W = 800;
const VIEW_H = 600;

describe("camera", () => {
  it("starts centred on the world", () => {
    const c = new Camera(512, 512);
    expect(c.cx).toBe(256);
    expect(c.cy).toBe(256);
  });

  it("maps the viewport centre to the camera centre", () => {
    const c = new Camera(512, 512);
    const w = c.screenToWorld(VIEW_W / 2, VIEW_H / 2, VIEW_W, VIEW_H);
    expect(w.x).toBeCloseTo(256, 6);
    expect(w.y).toBeCloseTo(256, 6);
  });

  it("round-trips screen -> world -> screen", () => {
    const c = new Camera(512, 512);
    c.zoom = 3.7;
    c.cx = 100.25;
    c.cy = 33.5;
    for (const [sx, sy] of [[0, 0], [123, 456], [VIEW_W, VIEW_H]]) {
      const w = c.screenToWorld(sx, sy, VIEW_W, VIEW_H);
      const s = c.worldToScreen(w.x, w.y, VIEW_W, VIEW_H);
      expect(s.x).toBeCloseTo(sx, 4);
      expect(s.y).toBeCloseTo(sy, 4);
    }
  });

  it("fit shows the whole world", () => {
    const c = new Camera(512, 512);
    c.fit(VIEW_W, VIEW_H);
    // The limiting dimension is height: 600/512 with a 5% margin.
    expect(c.zoom).toBeCloseTo((600 / 512) * 0.95, 6);
    const tl = c.worldToScreen(0, 0, VIEW_W, VIEW_H);
    const br = c.worldToScreen(512, 512, VIEW_W, VIEW_H);
    expect(tl.x).toBeGreaterThan(0);
    expect(tl.y).toBeGreaterThan(0);
    expect(br.x).toBeLessThan(VIEW_W);
    expect(br.y).toBeLessThan(VIEW_H);
  });

  it("zooming keeps the cell under the cursor pinned", () => {
    // The whole point of zoomAt. If this drifts, zooming feels like the world
    // is sliding away from the pointer.
    const c = new Camera(512, 512);
    c.fit(VIEW_W, VIEW_H);
    const anchor = { x: 300, y: 200 };
    const before = c.screenToWorld(anchor.x, anchor.y, VIEW_W, VIEW_H);
    c.zoomAt(anchor.x, anchor.y, 2.5, VIEW_W, VIEW_H);
    const after = c.screenToWorld(anchor.x, anchor.y, VIEW_W, VIEW_H);
    expect(after.x).toBeCloseTo(before.x, 4);
    expect(after.y).toBeCloseTo(before.y, 4);
  });

  it("zoom is clamped at both ends", () => {
    const c = new Camera(512, 512);
    for (let i = 0; i < 50; i++) c.zoomAt(0, 0, 2, VIEW_W, VIEW_H);
    expect(c.zoom).toBe(MAX_ZOOM);
    for (let i = 0; i < 100; i++) c.zoomAt(0, 0, 0.5, VIEW_W, VIEW_H);
    expect(c.zoom).toBe(MIN_ZOOM);
  });

  it("panning by pixels moves the world by pixels", () => {
    const c = new Camera(512, 512);
    c.zoom = 4;
    const before = c.worldToScreen(256, 256, VIEW_W, VIEW_H);
    c.panByPixels(40, -20);
    const after = c.worldToScreen(256, 256, VIEW_W, VIEW_H);
    expect(after.x - before.x).toBeCloseTo(40, 4);
    expect(after.y - before.y).toBeCloseTo(-20, 4);
  });

  it("the matrix agrees with worldToScreen", () => {
    // A matrix that disagrees with the picker means you click one ant and
    // select another. Check the two paths against each other directly.
    const c = new Camera(512, 512);
    c.zoom = 2.25;
    c.cx = 111;
    c.cy = 222;
    const m = c.matrix(VIEW_W, VIEW_H);

    const toClip = (wx: number, wy: number) => ({
      x: m[0] * wx + m[3] * wy + m[6],
      y: m[1] * wx + m[4] * wy + m[7],
    });

    for (const [wx, wy] of [[0, 0], [111, 222], [512, 512], [50, 400]]) {
      const clip = toClip(wx, wy);
      const screen = c.worldToScreen(wx, wy, VIEW_W, VIEW_H);
      // clip [-1,1] -> screen [0,W], with y flipped.
      expect(((clip.x + 1) / 2) * VIEW_W).toBeCloseTo(screen.x, 3);
      expect(((1 - clip.y) / 2) * VIEW_H).toBeCloseTo(screen.y, 3);
    }
  });

  it("centerOn puts a world cell at the viewport centre", () => {
    const c = new Camera(512, 512);
    c.zoom = 5;
    c.centerOn(400, 90);
    const s = c.worldToScreen(400, 90, VIEW_W, VIEW_H);
    expect(s.x).toBeCloseTo(VIEW_W / 2, 4);
    expect(s.y).toBeCloseTo(VIEW_H / 2, 4);
  });

  it("world y grows downward on screen", () => {
    // The grid is row-major from y=0 at the top. If this inverts, the map is
    // drawn upside down and every pheromone gradient reads backwards.
    const c = new Camera(512, 512);
    const top = c.worldToScreen(256, 0, VIEW_W, VIEW_H);
    const bottom = c.worldToScreen(256, 512, VIEW_W, VIEW_H);
    expect(top.y).toBeLessThan(bottom.y);
  });
});
