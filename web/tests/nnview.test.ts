import { describe, expect, it } from "vitest";
import { N_HIDDEN1, N_HIDDEN2, N_INPUTS, N_OUTPUTS, N_PARAMS } from "../src/protocol.js";
import {
  B3_OFF,
  LAYER_SIZES,
  W1_OFF,
  W2_OFF,
  W3_OFF,
  activationColor,
  layout,
  weightStyle,
} from "../src/ui/nnview.js";

describe("nnview layout", () => {
  it("places 86 nodes in four columns", () => {
    const { nodes, layerStart } = layout(400, 300);
    expect(nodes.length).toBe(N_INPUTS + N_HIDDEN1 + N_HIDDEN2 + N_OUTPUTS);
    expect(nodes.length).toBe(86);
    expect(layerStart).toEqual([0, 46, 62, 78]);
  });

  it("gives each layer its own x and orders columns left to right", () => {
    const { nodes, layerStart } = layout(400, 300);
    const xs = layerStart.map((s) => nodes[s].x);
    for (let i = 1; i < xs.length; i++) expect(xs[i]).toBeGreaterThan(xs[i - 1]);

    // Every node in a layer shares that layer's x.
    for (let l = 0; l < LAYER_SIZES.length; l++) {
      for (let i = 0; i < LAYER_SIZES[l]; i++) {
        expect(nodes[layerStart[l] + i].x).toBeCloseTo(xs[l], 6);
      }
    }
  });

  it("keeps every node inside the canvas", () => {
    const { nodes } = layout(400, 300, 18);
    for (const n of nodes) {
      expect(n.x).toBeGreaterThanOrEqual(0);
      expect(n.x).toBeLessThanOrEqual(400);
      expect(n.y).toBeGreaterThanOrEqual(0);
      expect(n.y).toBeLessThanOrEqual(300);
    }
  });

  it("does not divide by zero on a one-neuron layer", () => {
    // LAYER_SIZES has no 1 today, but the centring branch must not produce NaN
    // if the topology ever changes.
    const { nodes } = layout(100, 100);
    for (const n of nodes) {
      expect(Number.isFinite(n.x)).toBe(true);
      expect(Number.isFinite(n.y)).toBe(true);
    }
  });

  it("degenerate canvas sizes still yield finite coordinates", () => {
    for (const [w, h] of [[0, 0], [1, 1], [10, 5]]) {
      const { nodes } = layout(w, h);
      for (const n of nodes) {
        expect(Number.isFinite(n.x)).toBe(true);
        expect(Number.isFinite(n.y)).toBe(true);
      }
    }
  });
});

describe("parameter block offsets", () => {
  it("match the Rust genome layout and account for every parameter", () => {
    // If these drift, the inspector draws edges from the wrong weights and the
    // picture is a confident lie. W1[44x16] B1[16] W2[16x16] B2[16] W3[16x8] B3[8]
    expect(W1_OFF).toBe(0);
    expect(W2_OFF).toBe(N_INPUTS * N_HIDDEN1 + N_HIDDEN1);
    expect(W3_OFF).toBe(W2_OFF + N_HIDDEN1 * N_HIDDEN2 + N_HIDDEN2);
    expect(B3_OFF + N_OUTPUTS).toBe(N_PARAMS);
    expect(N_PARAMS).toBe(1160);
  });
});

describe("colour mapping", () => {
  it("is a signed diverging scale", () => {
    const pos = activationColor(1);
    const neg = activationColor(-1);
    expect(pos).not.toBe(neg);
    // Positive is red-dominant, negative is blue-dominant.
    const rgb = (s: string) => s.match(/\d+/g)!.map(Number);
    expect(rgb(pos)[0]).toBeGreaterThan(rgb(pos)[2]);
    expect(rgb(neg)[2]).toBeGreaterThan(rgb(neg)[0]);
  });

  it("saturates rather than overflowing past |1|", () => {
    // tanh cannot exceed 1, but a NaN or a protocol bug could. Clamp, do not
    // emit rgb(500, ...).
    const rgb = (s: string) => s.match(/\d+/g)!.map(Number);
    for (const c of [activationColor(5), activationColor(-5)]) {
      for (const ch of rgb(c)) {
        expect(ch).toBeGreaterThanOrEqual(0);
        expect(ch).toBeLessThanOrEqual(255);
      }
    }
  });

  it("weight alpha grows with magnitude and is capped", () => {
    const alpha = (s: string) => Number(s.slice(s.lastIndexOf(",") + 1, -1));
    expect(alpha(weightStyle(0.2))).toBeLessThan(alpha(weightStyle(0.9)));
    expect(alpha(weightStyle(100))).toBeLessThanOrEqual(0.85);
  });

  it("weight hue encodes the sign", () => {
    expect(weightStyle(0.5)).toContain("255, 90, 60");
    expect(weightStyle(-0.5)).toContain("70, 150, 255");
  });
});
