import { describe, expect, it } from "vitest";
import { TOOLTIPS } from "../src/ui/tooltips.js";

const REQUIRED = [
  "store", "delivered", "energy", "generation", "carrying", "pop",
  "fitness", "harvested", "phFood", "phAlarm", "phScent", "phOwner",
];

describe("tooltip copy", () => {
  it("defines non-empty copy for every stat key the panels use", () => {
    for (const k of REQUIRED) {
      expect(TOOLTIPS[k], k).toBeTruthy();
      expect(TOOLTIPS[k].length, k).toBeGreaterThan(8);
    }
  });
});
