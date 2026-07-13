/**
 * The live neural network, canvas2d rather than WebGL: 84 nodes and ~1,088
 * edges redrawn at 4 fps is nothing, and 2D text and lines are far easier to
 * get right than a graph shader.
 *
 * Pair this with single-step. Pause, step one tick, watch the activations move.
 * That is the difference between a pretty picture and a debugging tool.
 */

import { N_HIDDEN1, N_HIDDEN2, N_INPUTS, N_OUTPUTS } from "../protocol.js";
import { INPUT_GROUPS, OUTPUT_LABELS } from "../nnlabels.js";

/**
 * Weights below this are not drawn. With all 1,088 edges visible the picture is
 * a uniform grey mat and no structure is legible; culling the near-zero ones is
 * what makes a specialised sub-network visible at all.
 */
export const WEIGHT_CULL = 0.15;

export const LAYER_SIZES = [N_INPUTS, N_HIDDEN1, N_HIDDEN2, N_OUTPUTS] as const;

/** Parameter block offsets, mirroring `impl Brain for Genome`. */
export const W1_OFF = 0;
export const B1_OFF = W1_OFF + N_INPUTS * N_HIDDEN1;
export const W2_OFF = B1_OFF + N_HIDDEN1;
export const B2_OFF = W2_OFF + N_HIDDEN1 * N_HIDDEN2;
export const W3_OFF = B2_OFF + N_HIDDEN2;
export const B3_OFF = W3_OFF + N_HIDDEN2 * N_OUTPUTS;

export interface Node {
  x: number;
  y: number;
  layer: number;
  index: number;
}

/**
 * Four columns, each vertically centred. Returns nodes in layer-major order so
 * `nodes[layerStart[l] + i]` is neuron `i` of layer `l`.
 */
export function layout(
  width: number,
  height: number,
  pad = 18,
  padTop = pad,
  padLeft = pad,
  padRight = pad,
): { nodes: Node[]; layerStart: number[] } {
  const nodes: Node[] = [];
  const layerStart: number[] = [];
  const usableW = Math.max(1, width - padLeft - padRight);
  const usableH = Math.max(1, height - padTop - pad);

  LAYER_SIZES.forEach((n, layer) => {
    layerStart.push(nodes.length);
    const x = padLeft + (usableW * layer) / (LAYER_SIZES.length - 1);
    // A single-neuron layer would divide by zero; centre it instead.
    const step = n > 1 ? usableH / (n - 1) : 0;
    const y0 = n > 1 ? padTop : padTop + usableH / 2;
    for (let i = 0; i < n; i++) {
      nodes.push({ x, y: y0 + step * i, layer, index: i });
    }
  });

  return { nodes, layerStart };
}

/** Padding used by `draw`, shared with `hitTest` so hover math matches pixels. */
const PAD = 18;
const PAD_TOP = 30; // room for the column headers
const PAD_LEFT = 52; // room for the input-group labels
const PAD_RIGHT = 18;

/**
 * Nearest node to a CSS-pixel point, or null if none is within `maxDist`. The
 * caller passes the canvas's CSS size (not its backing-store size) so the hit
 * math lines up with mouse coordinates. Hidden nodes are skipped — they carry
 * no operator-facing meaning, so a hover on one should fall through.
 */
export function hitTest(
  x: number,
  y: number,
  width: number,
  height: number,
  maxDist = 10,
): Node | null {
  const { nodes } = layout(width, height, PAD, PAD_TOP, PAD_LEFT, PAD_RIGHT);
  let best: Node | null = null;
  let bestD = maxDist * maxDist;
  for (const n of nodes) {
    if (n.layer === 1 || n.layer === 2) continue;
    const dx = n.x - x;
    const dy = n.y - y;
    const d = dx * dx + dy * dy;
    if (d < bestD) {
      bestD = d;
      best = n;
    }
  }
  return best;
}

/** Signed diverging fill: blue negative, near-black at zero, red positive. */
export function activationColor(v: number): string {
  const t = Math.min(1, Math.abs(v));
  if (v >= 0) {
    return `rgb(${Math.round(40 + 215 * t)}, ${Math.round(40 + 30 * t)}, ${Math.round(45 - 20 * t)})`;
  }
  return `rgb(${Math.round(40 - 20 * t)}, ${Math.round(40 + 90 * t)}, ${Math.round(45 + 210 * t)})`;
}

/** Edge colour: sign picks the hue, magnitude picks the alpha. */
export function weightStyle(w: number): string {
  const a = Math.min(0.85, Math.abs(w) * 0.5);
  return w >= 0 ? `rgba(255, 90, 60, ${a})` : `rgba(70, 150, 255, ${a})`;
}

export interface NNFrame {
  inputs: Float32Array;
  h1: Float32Array;
  h2: Float32Array;
  outputs: Float32Array;
}

export function draw(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  act: NNFrame | null,
  params: Float32Array | null,
): void {
  ctx.clearRect(0, 0, width, height);
  ctx.fillStyle = "#0a0a0c";
  ctx.fillRect(0, 0, width, height);

  if (!act) {
    ctx.fillStyle = "#8a8a96";
    ctx.font = "12px ui-monospace, monospace";
    ctx.fillText("click an ant", 12, 22);
    return;
  }

  const { nodes, layerStart } = layout(width, height, PAD, PAD_TOP, PAD_LEFT, PAD_RIGHT);
  const acts = [act.inputs, act.h1, act.h2, act.outputs];

  // Edges first, so nodes sit on top of them.
  if (params) {
    const blocks = [
      { off: W1_OFF, from: 0, cols: N_HIDDEN1 },
      { off: W2_OFF, from: 1, cols: N_HIDDEN2 },
      { off: W3_OFF, from: 2, cols: N_OUTPUTS },
    ];
    ctx.lineWidth = 1;
    for (const { off, from, cols } of blocks) {
      const rows = LAYER_SIZES[from];
      for (let i = 0; i < rows; i++) {
        const a = nodes[layerStart[from] + i];
        for (let j = 0; j < cols; j++) {
          // Row-major: weight (i -> j) at off + i * cols + j.
          const w = params[off + i * cols + j];
          if (Math.abs(w) < WEIGHT_CULL) continue;
          const b = nodes[layerStart[from + 1] + j];
          ctx.strokeStyle = weightStyle(w);
          ctx.beginPath();
          ctx.moveTo(a.x, a.y);
          ctx.lineTo(b.x, b.y);
          ctx.stroke();
        }
      }
    }
  }

  for (let l = 0; l < LAYER_SIZES.length; l++) {
    const r = l === 0 ? 2.4 : 4.2;
    for (let i = 0; i < LAYER_SIZES[l]; i++) {
      const n = nodes[layerStart[l] + i];
      ctx.fillStyle = activationColor(acts[l][i] ?? 0);
      ctx.beginPath();
      ctx.arc(n.x, n.y, r, 0, Math.PI * 2);
      ctx.fill();
      ctx.strokeStyle = "rgba(255,255,255,0.18)";
      ctx.lineWidth = 0.6;
      ctx.stroke();
    }
  }

  drawLabels(ctx, nodes, layerStart);
}

/** Column headers, input-group labels down the left, and the 8 output names. */
function drawLabels(ctx: CanvasRenderingContext2D, nodes: Node[], layerStart: number[]): void {
  ctx.fillStyle = "#8a8a96";
  ctx.font = "10px ui-monospace, monospace";

  // Column headers, centred over each column.
  const headers = ["inputs", "hidden", "hidden", "outputs"];
  ctx.textAlign = "center";
  ctx.textBaseline = "alphabetic";
  headers.forEach((h, l) => {
    ctx.fillText(h, nodes[layerStart[l]].x, 12);
  });

  // Input-group labels, right-aligned in the left margin, at each run's centre.
  ctx.textAlign = "right";
  ctx.textBaseline = "middle";
  for (const g of INPUT_GROUPS) {
    const first = nodes[layerStart[0] + g.start];
    const last = nodes[layerStart[0] + g.start + g.len - 1];
    const cy = (first.y + last.y) / 2;
    ctx.fillText(g.name, first.x - 6, cy);
  }

  // Output names, right-aligned just left of each output node.
  ctx.fillStyle = "#c8c8d0";
  const outStart = layerStart[3];
  OUTPUT_LABELS.forEach((name, i) => {
    const n = nodes[outStart + i];
    ctx.fillText(name, n.x - 8, n.y);
  });
  ctx.textAlign = "left";
}
