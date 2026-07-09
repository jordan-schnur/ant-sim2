import { describe, expect, it } from "vitest";
import { CONFIG_FIELDS } from "../src/protocol.js";
import { TUNABLES, formatValue, toPosition, toValue } from "../src/ui/tunables.js";

describe("tunables", () => {
  it("has one slider per protocol config field, in id order", () => {
    // A slider pointing at the wrong field id silently retunes something else.
    expect(TUNABLES.length).toBe(CONFIG_FIELDS.length);
    TUNABLES.forEach((t, i) => expect(t.id).toBe(i));
  });

  it("exposes the four knobs the 500k-tick run implicated", () => {
    const byId = new Map(TUNABLES.map((t) => [t.id, t.label]));
    expect(byId.get(10)).toContain("birth");
    expect(byId.get(11)).toContain("harvest");
    expect(byId.get(12)).toContain("refuel");
    expect(byId.get(13)).toContain("growth");
  });

  it("round-trips value -> position -> value on every slider", () => {
    for (const t of TUNABLES) {
      for (const p of [0, 0.25, 0.5, 0.75, 1]) {
        const v = toValue(t, p);
        expect(toPosition(t, v)).toBeCloseTo(p, 5);
      }
    }
  });

  it("hits both endpoints exactly", () => {
    for (const t of TUNABLES) {
      expect(toValue(t, 0)).toBeCloseTo(t.min, 6);
      expect(toValue(t, 1)).toBeCloseTo(t.max, 6);
    }
  });

  it("keeps every produced value inside the declared range", () => {
    for (const t of TUNABLES) {
      for (let i = 0; i <= 100; i++) {
        const v = toValue(t, i / 100);
        expect(v).toBeGreaterThanOrEqual(t.min - 1e-9);
        expect(v).toBeLessThanOrEqual(t.max + 1e-9);
      }
    }
  });

  it("clamps out-of-range slider positions rather than extrapolating", () => {
    const t = TUNABLES[10];
    expect(toValue(t, -5)).toBeCloseTo(t.min, 6);
    expect(toValue(t, 5)).toBeCloseTo(t.max, 6);
  });

  it("spends most of the decay slider's travel near 1.0", () => {
    // The whole reason for the non-linear scale. 0.995 and 0.999 behave
    // completely differently; a linear slider would give the interesting
    // region a sliver of travel and make it untunable by hand.
    const evap = TUNABLES[0];
    const mid = toValue(evap, 0.5);
    const linearMid = (evap.min + evap.max) / 2;
    expect(mid).toBeGreaterThan(linearMid);

    // The default 0.995 must sit somewhere usable, not pinned at an end.
    const p = toPosition(evap, 0.995);
    expect(p).toBeGreaterThan(0.2);
    expect(p).toBeLessThan(0.9);
  });

  it("resolves the default evaporation values to distinct positions", () => {
    // 0.995 (food) and 0.999 (scent) must be visibly different on the slider,
    // or the operator cannot tell them apart while dragging.
    const food = toPosition(TUNABLES[0], 0.995);
    const scent = toPosition(TUNABLES[2], 0.999);
    expect(Math.abs(food - scent)).toBeGreaterThan(0.1);
  });

  it("formats without losing the digit that matters", () => {
    expect(formatValue(TUNABLES[0], 0.995)).toBe("0.9950");
    expect(formatValue(TUNABLES[10], 40)).toBe("40.0");
    expect(formatValue(TUNABLES[6], 0.02)).toBe("0.0200");
  });
});
