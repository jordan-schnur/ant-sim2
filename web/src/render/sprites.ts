/**
 * Pure geometry/encoding helpers for the ant sprite pass. Kept free of WebGL
 * and canvas so they are unit-testable in node; the canvas atlas builder that
 * consumes `glyphCellRect` lives in `world.ts` where a real 2D context exists.
 */
import { SHAPES } from "../symbols.js";

/** One atlas cell per colony glyph; colony -> cell is `colony % GLYPH_ATLAS_COLS`. */
export const GLYPH_ATLAS_COLS = SHAPES.length;

/** Pixel rect of atlas cell `index` for a square `cell` size, laid out in one row. */
export function glyphCellRect(
  index: number,
  cell: number,
): { x: number; y: number; w: number; h: number } {
  return { x: index * cell, y: 0, w: cell, h: cell };
}

const TWO_PI = Math.PI * 2;

/**
 * Canonical heading decode — the exact inverse of the Rust encoder in
 * `crates/server/src/protocol.rs::encode_ants`. The ant vertex shader MUST use
 * this same formula; this function is the documented source of record for it.
 */
export function headingByteToRadians(b: number): number {
  return (b / 255) * TWO_PI - Math.PI;
}

/** Canonical heading encode in TS, mirroring the Rust side. */
export function radiansToHeadingByte(a: number): number {
  // Wrap into [-PI, PI) the same way `wrap_angle` does, then quantize.
  let w = a;
  while (w >= Math.PI) w -= TWO_PI;
  while (w < -Math.PI) w += TWO_PI;
  return Math.round(((w + Math.PI) / TWO_PI) * 255);
}
