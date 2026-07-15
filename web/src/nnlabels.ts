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
/** One channel name per whisker slot, in sense.rs's CH_* order. */
export const CHANNELS = [
  "food",
  "trail",
  "alarm",
  "own scent",
  "foe scent",
  "wall",
  "home trail",
] as const;
/** Short forms for the compact whisker grid header. */
export const CHANNEL_ABBR = ["food", "trail", "alarm", "mine", "foe", "wall", "home"] as const;

const WHISKERS = WHISKER_DIRS.length; // 5
const CH = CHANNELS.length; // 7

/** A contiguous run of inputs that means one thing, for grouped display. */
export interface InputGroup {
  name: string;
  start: number;
  len: number;
}

// Offsets copied verbatim from sense.rs. IN_WHISKERS=0, IN_UNDERFOOT=35,
// IN_COUNTS=39, IN_PROPRIO=41, IN_BIAS=45, IN_MEMORY=46, IN_HOME=50,
// IN_HEADING=53.
export const INPUT_GROUPS: InputGroup[] = [
  { name: "whiskers", start: 0, len: WHISKERS * CH }, // 0..35
  { name: "underfoot", start: 35, len: 4 },
  { name: "crowd", start: 39, len: 2 },
  { name: "body", start: 41, len: 4 },
  { name: "bias", start: 45, len: 1 },
  { name: "memory", start: 46, len: 4 },
  { name: "home", start: 50, len: 3 }, // world-frame unit x, unit y, distance
  { name: "facing", start: 53, len: 2 }, // sin, cos of the ant's own heading
];

// Non-whisker input names, index-aligned to their group starts.
const UNDERFOOT = ["underfoot food", "underfoot trail", "underfoot alarm", "underfoot home trail"];
const CROWD = ["friends near", "foes near"];
const BODY = ["energy", "size", "carrying", "age"];
const HOME = ["home vector x", "home vector y", "home distance"];

/** Full human label for input index `i` (0..N_INPUTS). */
export function inputLabel(i: number): string {
  const whiskersEnd = WHISKERS * CH; // 35
  if (i < whiskersEnd) {
    const dir = WHISKER_DIRS[Math.floor(i / CH)];
    const ch = CHANNELS[i % CH];
    return `whisker ${dir} · ${ch}`;
  }
  if (i < 39) return UNDERFOOT[i - 35];
  if (i < 41) return CROWD[i - 39];
  if (i < 45) return BODY[i - 41];
  if (i === 45) return "bias";
  if (i < 50) return `memory ${i - 46}`;
  if (i < 53) return HOME[i - 50];
  return i === 53 ? "facing (sin)" : "facing (cos)";
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

// Fail loudly if the sim's vector sizes drift from these hand-kept labels.
if (INPUT_GROUPS.reduce((n, g) => n + g.len, 0) !== N_INPUTS) {
  throw new Error(`nnlabels: input groups cover != ${N_INPUTS} inputs`);
}
if (OUTPUT_LABELS.length !== N_OUTPUTS || OUTPUT_DESC.length !== N_OUTPUTS) {
  throw new Error(`nnlabels: output labels != ${N_OUTPUTS}`);
}

/** Name (and optional description) for a graph node, for the hover popover. */
export function nodeInfo(layer: number, index: number): { label: string; desc?: string } {
  if (layer === 0) return { label: inputLabel(index) };
  if (layer === 3) return { label: `output · ${OUTPUT_LABELS[index]}`, desc: OUTPUT_DESC[index] };
  return { label: `hidden ${layer} · neuron ${index}` };
}
