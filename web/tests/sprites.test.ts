import { describe, expect, it } from "vitest";
import { SHAPES } from "../src/symbols.js";
import {
  GLYPH_ATLAS_COLS,
  glyphCellRect,
  headingByteToRadians,
  radiansToHeadingByte,
} from "../src/render/sprites.js";

describe("glyph atlas layout", () => {
  it("has one cell per colony shape", () => {
    expect(GLYPH_ATLAS_COLS).toBe(SHAPES.length);
    expect(GLYPH_ATLAS_COLS).toBe(8);
  });

  it("lays cells out left to right in a single row", () => {
    expect(glyphCellRect(0, 32)).toEqual({ x: 0, y: 0, w: 32, h: 32 });
    expect(glyphCellRect(3, 32)).toEqual({ x: 96, y: 0, w: 32, h: 32 });
  });
});

describe("heading mapping", () => {
  it("decodes the range endpoints to a wrapped angle", () => {
    expect(headingByteToRadians(0)).toBeCloseTo(-Math.PI, 5);
    expect(headingByteToRadians(128)).toBeCloseTo(Math.PI / 255, 2); // ~0
  });

  it("round-trips a byte through encode(decode(b))", () => {
    // 255 is excluded: it decodes to exactly +PI, the seam wrap_angle folds
    // back to -PI (byte 0). Every non-seam byte is a fixed point.
    for (const b of [0, 1, 64, 128, 200, 254]) {
      expect(radiansToHeadingByte(headingByteToRadians(b))).toBe(b);
    }
  });
});
