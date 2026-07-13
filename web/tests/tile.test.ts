import { describe, expect, it } from "vitest";
import { tileReadout } from "../src/tile.js";

// A 2x2 downsampled map at factor 4 => 8x8 world. Cell (5,1) maps to texel (1,0).
function frames() {
  const terrain = new Uint8Array(2 * 2 * 4);
  const phero = new Uint8Array(2 * 2 * 4);
  const t = (tx: number, ty: number) => (ty * 2 + tx) * 4;
  terrain[t(1, 0) + 0] = 200; // food
  terrain[t(1, 0) + 1] = 10;  // stone
  terrain[t(1, 0) + 2] = 3;   // nest owner 3
  phero[t(1, 0) + 0] = 40;    // food trail
  phero[t(1, 0) + 1] = 5;     // alarm
  phero[t(1, 0) + 2] = 90;    // scent
  phero[t(1, 0) + 3] = 255;   // no owner
  const T = { kind: "terrain", tick: 1, w: 2, h: 2, factor: 4, rgba: terrain } as const;
  const P = { kind: "phero", tick: 1, w: 2, h: 2, factor: 4, rgba: phero } as const;
  return { T, P };
}

describe("tileReadout", () => {
  it("reads the texel a world cell falls in", () => {
    const { T, P } = frames();
    const r = tileReadout(T as never, P as never, 5, 1)!;
    expect(r).not.toBeNull();
    expect(r.food).toBe(200);
    expect(r.stone).toBe(10);
    expect(r.nest).toBe(3);
    expect(r.phScent).toBe(90);
    expect(r.phOwner).toBeNull(); // 255 sentinel
  });
  it("returns null out of bounds", () => {
    const { T, P } = frames();
    expect(tileReadout(T as never, P as never, -1, 0)).toBeNull();
    expect(tileReadout(T as never, P as never, 8, 0)).toBeNull();
  });
  it("treats a 255 owner/nest byte as none", () => {
    const { T, P } = frames();
    // Mark texel (0,0)'s nest owner and phero owner as the 255 sentinel.
    T.rgba[0 * 4 + 2] = 255;
    P.rgba[0 * 4 + 3] = 255;
    const r = tileReadout(T as never, P as never, 0, 0)!;
    expect(r.nest).toBeNull();
    expect(r.phOwner).toBeNull();
  });
});
