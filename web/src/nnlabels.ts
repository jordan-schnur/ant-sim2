/**
 * Human-readable names for the brain's inputs and outputs, mirroring the sim's
 * input layout in `crates/sim/src/sense.rs` and output constants in
 * `crates/sim/src/brain.rs`. These two crates share no code with the web
 * client, so this file is a hand-kept copy of that contract — reorder a field
 * in `sense.rs` and the labels here silently go wrong, the same hazard the wire
 * format carries.
 */

import { N_INPUTS, N_OUTPUTS } from "./protocol.js";

// --- Whiskers: 5 directions x 7 channels = the first 35 inputs. ---
// Angles in sense.rs are [-1.2, -0.6, 0, 0.6, 1.2] rad, relative to heading.
export const WHISKER_DIRS = ["far left", "left", "ahead", "right", "far right"] as const;
/** One channel name per whisker slot, in sense.rs's CH_* order. `food trail`
 *  (index 1) is the food pheromone; `own trail` (index 7) is the colony
 *  recent-path signal, a distinct field from `home trail` (index 6), the
 *  shared ownerless exploration trail. */
export const CHANNELS = [
  "food",
  "food trail",
  "alarm",
  "own scent",
  "foe scent",
  "wall",
  "home trail",
  "own trail",
] as const;
/** Short forms for the compact whisker grid header. */
export const CHANNEL_ABBR = [
  "food",
  "ftrl",
  "alarm",
  "mine",
  "foe",
  "wall",
  "home",
  "trail",
] as const;

const WHISKERS = WHISKER_DIRS.length; // 5
const CH = CHANNELS.length; // 8
const WHISKER_INPUTS = WHISKERS * CH; // 40

/** A contiguous run of inputs that means one thing, for grouped display. */
export interface InputGroup {
  name: string;
  start: number;
  len: number;
}

// Offsets derived from sense.rs's layout: after the whisker block come
// IN_UNDERFOOT (4), IN_COUNTS (2), IN_PROPRIO (4), IN_BIAS (1), IN_MEMORY (4),
// IN_HOME (3), IN_HEADING (2). Deriving from WHISKER_INPUTS keeps them correct
// as CH grows.
const UNDERFOOT_START = WHISKER_INPUTS; // 40
const CROWD_START = UNDERFOOT_START + 4; // 44
const BODY_START = CROWD_START + 2; // 46
const BIAS_START = BODY_START + 4; // 50
const MEMORY_START = BIAS_START + 1; // 51
const HOME_START = MEMORY_START + 4; // 55
const FACING_START = HOME_START + 3; // 58

export const INPUT_GROUPS: InputGroup[] = [
  { name: "whiskers", start: 0, len: WHISKER_INPUTS },
  { name: "underfoot", start: UNDERFOOT_START, len: 4 },
  { name: "crowd", start: CROWD_START, len: 2 },
  { name: "body", start: BODY_START, len: 4 },
  { name: "bias", start: BIAS_START, len: 1 },
  { name: "memory", start: MEMORY_START, len: 4 },
  { name: "home", start: HOME_START, len: 3 }, // world-frame unit x, unit y, distance
  { name: "facing", start: FACING_START, len: 2 }, // sin, cos of the ant's heading
];

// Non-whisker input names, index-aligned to their group starts.
const UNDERFOOT = ["underfoot food", "underfoot trail", "underfoot alarm", "underfoot home trail"];
const CROWD = ["friends near", "foes near"];
const BODY = ["energy", "size", "carrying", "age"];
const HOME = ["home vector x", "home vector y", "home distance"];

/** Full human label for input index `i` (0..N_INPUTS). */
export function inputLabel(i: number): string {
  if (i < WHISKER_INPUTS) {
    const dir = WHISKER_DIRS[Math.floor(i / CH)];
    const ch = CHANNELS[i % CH];
    return `whisker ${dir} · ${ch}`;
  }
  if (i < CROWD_START) return UNDERFOOT[i - UNDERFOOT_START];
  if (i < BODY_START) return CROWD[i - CROWD_START];
  if (i < BIAS_START) return BODY[i - BODY_START];
  if (i === BIAS_START) return "bias";
  if (i < HOME_START) return `memory ${i - MEMORY_START}`;
  if (i < FACING_START) return HOME[i - HOME_START];
  return i === FACING_START ? "facing (sin)" : "facing (cos)";
}

export const OUTPUT_LABELS = [
  "vel x",
  "vel y",
  "attack",
  "grab",
  "mem 0",
  "mem 1",
  "mem 2",
  "mem 3",
] as const;

/** What each output does, for the tooltip and hover popover. */
export const OUTPUT_DESC = [
  "x of the desired world-frame velocity; with vel y it sets the direction the ant steers toward and how fast it moves",
  "y of the desired world-frame velocity; with vel x it sets the direction the ant steers toward and how fast it moves",
  "bite the foe ahead when this clears the attack threshold",
  "positive grabs food; strongly negative drops it or deposits at the nest",
  "recurrent memory — written back as an input on the next tick",
  "recurrent memory — written back as an input on the next tick",
  "recurrent memory — written back as an input on the next tick",
  "recurrent memory — written back as an input on the next tick",
] as const;

/** How each whisker channel is computed. Indexed by CH_* order (see sense.rs). */
const CHANNEL_DESC = [
  "Food seen along this antenna: grid food on the sampled cell ÷ food_patch_max, capped at 1. The cell is `vision` steps out at the whisker's angle from your heading.",
  "Food-trail pheromone along this antenna, log-squashed: ln(1 + value) ÷ phero_log_div, capped at 1.",
  "Alarm pheromone along this antenna, log-squashed the same way (spikes where ants were attacked).",
  "Your own colony's territory scent along this antenna, log-squashed.",
  "Rival colonies' scent along this antenna, log-squashed — enemy territory.",
  "1 if the sampled cell is stone or off the map, else 0.",
  "Shared home/exploration trail along this antenna, log-squashed — the trail every ant lays and reads.",
  "Your own colony's fast-fading recent-path trail along this antenna, log-squashed.",
] as const;

/** How each non-whisker input is computed, keyed by its inputLabel(). */
const INPUT_DESC: Record<string, string> = {
  "underfoot food": "Food on the cell you stand on ÷ food_patch_max, capped at 1.",
  "underfoot trail": "Food-trail pheromone on your cell, log-squashed.",
  "underfoot alarm": "Alarm pheromone on your cell, log-squashed.",
  "underfoot home trail": "Shared home/exploration trail on your cell, log-squashed.",
  "friends near": "Same-colony ants within 2 cells, not counting you, ÷ 8, capped at 1.",
  "foes near": "Other-colony ants within 2 cells ÷ 8, capped at 1.",
  "energy": "Fuel fraction: energy ÷ max_energy, clamped 0–1.",
  "size": "Body size ÷ your max_size trait, clamped 0–1.",
  "carrying": "Food in hand ÷ your carry_capacity trait, clamped 0–1.",
  "age": "Ticks alive ÷ your lifespan trait, clamped 0–1.",
  "bias": "Constant 1. A fixed input the network can weight as a learnable offset.",
  "memory 0": "Recurrent memory: whatever the brain wrote to memory output 0 last tick.",
  "memory 1": "Recurrent memory: whatever the brain wrote to memory output 1 last tick.",
  "memory 2": "Recurrent memory: whatever the brain wrote to memory output 2 last tick.",
  "memory 3": "Recurrent memory: whatever the brain wrote to memory output 3 last tick.",
  "home vector x": "World-frame unit vector toward your nest, X component: (nest_x − x) ÷ distance. Zero on the nest.",
  "home vector y": "World-frame unit vector toward your nest, Y component: (nest_y − y) ÷ distance. Zero on the nest.",
  "home distance": "Distance to your nest ÷ the map diagonal, capped at 1.",
  "facing (sin)": "sin of your heading — lets the network read its own facing without the ±π wrap a raw angle jumps at.",
  "facing (cos)": "cos of your heading — the other half of the facing signal.",
};

/** Meaning + computation for input index `i` (0..N_INPUTS). */
export function inputInfo(i: number): { label: string; desc: string } {
  const label = inputLabel(i);
  if (i < CH * WHISKER_DIRS.length) {
    return { label, desc: CHANNEL_DESC[i % CH] };
  }
  return { label, desc: INPUT_DESC[label] ?? "" };
}

// Fail loudly if the sim's vector sizes drift from these hand-kept labels.
if (INPUT_GROUPS.reduce((n, g) => n + g.len, 0) !== N_INPUTS) {
  throw new Error(`nnlabels: input groups cover != ${N_INPUTS} inputs`);
}
if (OUTPUT_LABELS.length !== N_OUTPUTS || OUTPUT_DESC.length !== N_OUTPUTS) {
  throw new Error(`nnlabels: output labels != ${N_OUTPUTS}`);
}
// Fail loudly if any input lacks a computation string.
for (let i = 0; i < N_INPUTS; i++) {
  if (!inputInfo(i).desc) throw new Error(`nnlabels: input ${i} has no computation copy`);
}

/** Name (and optional description) for a graph node, for the hover popover. */
export function nodeInfo(layer: number, index: number): { label: string; desc?: string } {
  if (layer === 0) return inputInfo(index);
  if (layer === 3) return { label: `output · ${OUTPUT_LABELS[index]}`, desc: OUTPUT_DESC[index] };
  return { label: `hidden ${layer} · neuron ${index}` };
}
