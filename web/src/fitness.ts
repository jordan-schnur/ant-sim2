/**
 * An ant's selection fitness, mirroring `Config::fitness` in the sim:
 * delivered food plus a small credit for food still being carried. This is the
 * scalar reproduction is proportional to, so it is the honest "how successful
 * is this ant" number.
 */

/** Config field id for `harvest_weight` (see `apply_config_field`). */
export const HARVEST_WEIGHT_FIELD = 16;
/** Used until the config frame arrives. Matches `Config::default`. */
export const DEFAULT_HARVEST_WEIGHT = 0.02;

export function fitness(delivered: number, harvested: number, weight: number): number {
  return delivered + weight * harvested;
}
