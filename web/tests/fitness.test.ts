import { describe, expect, it } from "vitest";
import { fitness, DEFAULT_HARVEST_WEIGHT, HARVEST_WEIGHT_FIELD } from "../src/fitness.js";

describe("fitness", () => {
  it("is delivered plus weight times harvested", () => {
    expect(fitness(1240, 372, 0.02)).toBeCloseTo(1247.44, 5);
  });
  it("equals delivered when weight is zero", () => {
    expect(fitness(50, 999, 0)).toBe(50);
  });
  it("pins the harvest_weight config field id and default", () => {
    expect(HARVEST_WEIGHT_FIELD).toBe(16);
    expect(DEFAULT_HARVEST_WEIGHT).toBe(0.02);
  });
});
