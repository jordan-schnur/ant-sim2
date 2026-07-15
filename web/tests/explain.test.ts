// @vitest-environment jsdom
import { describe, expect, it, beforeEach } from "vitest";
import { EXPLAIN, explainText, infoDot } from "../src/ui/explain.js";
import { TUNABLES } from "../src/ui/tunables.js";

// Keys that existing panels already rely on via TOOLTIPS; must survive the move.
const LEGACY_KEYS = [
  "pop", "store", "delivered", "energy", "generation", "carrying",
  "fitness", "harvested", "recentProductivity", "size", "paid births",
  "free", "phFood", "phAlarm", "phScent", "phOwner", "nest", "stone", "food",
];

describe("EXPLAIN registry", () => {
  it("keeps non-empty copy for every legacy tooltip key", () => {
    for (const k of LEGACY_KEYS) {
      expect(EXPLAIN[k], k).toBeTruthy();
      expect(EXPLAIN[k].length, k).toBeGreaterThan(8);
    }
  });

  it("explainText returns the copy or undefined for an unknown key", () => {
    expect(explainText("store")).toBe(EXPLAIN["store"]);
    expect(explainText("nope_not_a_key")).toBeUndefined();
  });
});

describe("infoDot", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("renders an ⓘ span carrying the copy for a known key", () => {
    const el = infoDot("store");
    expect(el.textContent).toContain("ⓘ");
    expect(el.getAttribute("data-info")).toBe(EXPLAIN["store"]);
  });

  it("shows a popover on click and removes it on Escape", () => {
    document.body.append(infoDot("store"));
    const dot = document.querySelector(".info-dot") as HTMLElement;
    dot.click();
    expect(document.querySelector(".info-pop")).not.toBeNull();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(document.querySelector(".info-pop")).toBeNull();
  });

  it("does not throw and shows nothing for an unknown key", () => {
    const el = infoDot("nope_not_a_key");
    document.body.append(el);
    el.click();
    expect(document.querySelector(".info-pop")).toBeNull();
  });
});

describe("slider copy", () => {
  it("has non-empty copy for every tunable id", () => {
    for (const t of TUNABLES) {
      const copy = EXPLAIN[`tune.${t.id}`];
      expect(copy, `tune.${t.id} (${t.label})`).toBeTruthy();
      expect(copy.length, `tune.${t.id}`).toBeGreaterThan(12);
    }
  });
});
