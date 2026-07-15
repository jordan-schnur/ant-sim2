/**
 * An ant's selection fitness, mirroring `Config::fitness` in the sim:
 * delivered food plus a small credit for all food ever picked up (harvested),
 * plus a decaying credit for recent productivity. (The sim's fitness also
 * includes a `homing_weight` term over sensed food-homing signal, which isn't
 * on the wire, so it's omitted here.) This is the scalar reproduction is
 * proportional to, so it is the honest "how successful is this ant" number.
 */

/** Config field id for `harvest_weight` (see `apply_config_field`). */
export const HARVEST_WEIGHT_FIELD = 16;
/** Used until the config frame arrives. Matches `Config::default`. */
export const DEFAULT_HARVEST_WEIGHT = 0.02;

/** Config field id for `productivity_weight` (see `apply_config_field`). */
export const PRODUCTIVITY_WEIGHT_FIELD = 21;
/** Used until the config frame arrives. Matches `Config::default`. */
export const DEFAULT_PRODUCTIVITY_WEIGHT = 0.1;

export function fitness(
  delivered: number,
  harvested: number,
  harvestWeight: number,
  recentProductivity: number,
  productivityWeight: number,
): number {
  return delivered + harvestWeight * harvested + productivityWeight * recentProductivity;
}
