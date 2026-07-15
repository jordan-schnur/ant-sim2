/**
 * The live-tuning sliders. Field ids mirror `CONFIG_FIELDS` in `protocol.rs`.
 *
 * Ids 10..13 (birth_cost, harvest_rate, refuel_rate, growth_threshold) are not
 * in the spec's slider list. The first 500k-tick run showed 97.7% of ants are
 * born free from the extinction floor rather than paid for out of a colony's
 * food store, and named exactly those four as the reason. The spec's list was
 * written before that run existed.
 * See `docs/superpowers/notes/2026-07-09-first-500k-tick-run.md`.
 */

export interface Tunable {
  id: number;
  label: string;
  min: number;
  max: number;
  /**
   * Evaporation lives in the last decimal — 0.995 and 0.999 behave completely
   * differently, and a linear slider from 0.9 to 1.0 spends 90% of its travel
   * in the region nobody cares about. Interpolate geometrically on `1 - v`,
   * which spans decades, then invert.
   */
  scale: "linear" | "decay";
  hint?: string;
}

export const TUNABLES: Tunable[] = [
  { id: 0, label: "food evap", min: 0.9, max: 0.9999, scale: "decay" },
  { id: 1, label: "alarm evap", min: 0.9, max: 0.9999, scale: "decay" },
  { id: 2, label: "scent evap", min: 0.9, max: 0.9999, scale: "decay" },
  { id: 3, label: "food diffuse", min: 0, max: 0.4, scale: "linear" },
  { id: 4, label: "alarm diffuse", min: 0, max: 0.4, scale: "linear" },
  { id: 5, label: "scent diffuse", min: 0, max: 0.4, scale: "linear" },
  { id: 6, label: "tax speed", min: 0, max: 0.05, scale: "linear" },
  { id: 7, label: "tax vision", min: 0, max: 0.05, scale: "linear", hint: "x8: vision ranges to 8" },
  { id: 8, label: "mutation rate", min: 0, max: 0.5, scale: "linear" },
  { id: 9, label: "mutation sigma", min: 0, max: 0.5, scale: "linear" },
  { id: 10, label: "birth cost", min: 1, max: 100, scale: "linear", hint: "a trip yields ~10" },
  { id: 11, label: "harvest rate", min: 0.1, max: 10, scale: "linear" },
  { id: 12, label: "refuel rate", min: 0, max: 10, scale: "linear", hint: "loiterers drain the store" },
  { id: 13, label: "growth threshold", min: 0.01, max: 1, scale: "linear", hint: "growing costs a forager" },
  { id: 14, label: "food regrow", min: 0, max: 0.02, scale: "linear" },
  { id: 15, label: "attack damage", min: 0, max: 20, scale: "linear" },
  { id: 16, label: "harvest weight", min: 0, max: 0.2, scale: "linear", hint: "0 = deliver-only; nudge toward foraging" },
  { id: 17, label: "homing weight", min: 0, max: 1, scale: "linear", hint: "reward carrying food home; helps bootstrap" },
  { id: 18, label: "trail emission", min: 0, max: 5, scale: "linear", hint: "colony recent-path signal ants lay each tick" },
  { id: 19, label: "trail evap", min: 0.9, max: 0.9999, scale: "decay", hint: "fast decay = trail means recent" },
  { id: 20, label: "trail diffuse", min: 0, max: 0.4, scale: "linear" },
  { id: 21, label: "productivity weight", min: 0, max: 1, scale: "linear", hint: "reward recent harvest/deliver/kills; 0 = off (cumulative only)" },
  { id: 22, label: "productivity decay", min: 0.9, max: 0.9999, scale: "decay", hint: "how fast 'recent' fades; 0.99 = ~69-tick half-life" },
];

/** Slider position [0,1] -> config value. */
export function toValue(t: Tunable, pos: number): number {
  const p = clamp01(pos);
  if (t.scale === "linear") return t.min + (t.max - t.min) * p;
  // Interpolate on (1 - v), which spans decades, then invert.
  const lo = 1 - t.max;
  const hi = 1 - t.min;
  return 1 - hi * Math.pow(lo / hi, p);
}

/** Config value -> slider position [0,1]. Inverse of `toValue`. */
export function toPosition(t: Tunable, value: number): number {
  if (t.scale === "linear") {
    if (t.max === t.min) return 0;
    return clamp01((value - t.min) / (t.max - t.min));
  }
  const lo = 1 - t.max;
  const hi = 1 - t.min;
  const v = clamp(value, t.min, t.max);
  return clamp01(Math.log((1 - v) / hi) / Math.log(lo / hi));
}

export function formatValue(t: Tunable, v: number): string {
  if (t.scale === "decay") return v.toFixed(4);
  if (t.max <= 0.06) return v.toFixed(4);
  if (t.max <= 1) return v.toFixed(3);
  if (t.max <= 20) return v.toFixed(2);
  return v.toFixed(1);
}

function clamp01(v: number): number {
  return Math.min(1, Math.max(0, v));
}
function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

/** Slider `step` is 1/DIVISIONS; the value mapping happens in `toValue`. */
export const DIVISIONS = 1000;
