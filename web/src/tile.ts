/**
 * Read the terrain + pheromone values under a world cell. Both frames are
 * downsampled by `factor`, so a world cell maps to one texel; the Explorer's
 * tile view is entirely derived here, with no server round-trip.
 */

import type { Phero, Terrain } from "./protocol.js";

export interface TileReadout {
  x: number;
  y: number;
  food: number;
  stone: number;
  nest: number | null;
  phFood: number;
  phAlarm: number;
  phScent: number;
  phHome: number;
  phOwner: number | null;
}

/** 255 is the "no owner / no nest" sentinel in both frames. */
const NONE = 255;

export function tileReadout(
  terrain: Terrain,
  phero: Phero,
  x: number,
  y: number,
): TileReadout | null {
  const cx = Math.floor(x);
  const cy = Math.floor(y);
  const worldW = terrain.w * terrain.factor;
  const worldH = terrain.h * terrain.factor;
  if (cx < 0 || cy < 0 || cx >= worldW || cy >= worldH) return null;

  const tx = Math.min(terrain.w - 1, Math.floor(cx / terrain.factor));
  const ty = Math.min(terrain.h - 1, Math.floor(cy / terrain.factor));
  const ti = (ty * terrain.w + tx) * 4;

  const px = Math.min(phero.w - 1, Math.floor(cx / phero.factor));
  const py = Math.min(phero.h - 1, Math.floor(cy / phero.factor));
  const pi = (py * phero.w + px) * 4;
  // The home trail is a separate single-channel plane, one byte per texel.
  const hi = py * phero.w + px;

  const nest = terrain.rgba[ti + 2];
  const phOwner = phero.rgba[pi + 3];
  return {
    x: cx,
    y: cy,
    food: terrain.rgba[ti],
    stone: terrain.rgba[ti + 1],
    nest: nest === NONE ? null : nest,
    phFood: phero.rgba[pi],
    phAlarm: phero.rgba[pi + 1],
    phScent: phero.rgba[pi + 2],
    phHome: phero.home[hi],
    phOwner: phOwner === NONE ? null : phOwner,
  };
}
