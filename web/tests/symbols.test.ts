import { describe, it, expect } from "vitest";
import { symbolFor, SHAPES } from "../src/symbols.js";

describe("colony symbols", () => {
  it("maps each of 8 colonies to a distinct shape", () => {
    const shapes = new Set(Array.from({ length: 8 }, (_, i) => symbolFor(i)));
    expect(shapes.size).toBe(8);
  });
  it("wraps past the shape count without throwing", () => {
    expect(symbolFor(9)).toBe(SHAPES[9 % SHAPES.length]);
  });
});
