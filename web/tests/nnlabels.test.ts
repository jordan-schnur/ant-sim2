import { describe, expect, it } from "vitest";
import { N_INPUTS, N_OUTPUTS } from "../src/protocol.js";
import {
  INPUT_GROUPS,
  OUTPUT_DESC,
  OUTPUT_LABELS,
  inputLabel,
  nodeInfo,
} from "../src/nnlabels.js";
import { hitTest, layout } from "../src/ui/nnview.js";

describe("input labels mirror the sim's sense.rs layout", () => {
  it("groups cover exactly N_INPUTS with no gaps or overlap", () => {
    let cursor = 0;
    for (const g of INPUT_GROUPS) {
      expect(g.start).toBe(cursor);
      cursor += g.len;
    }
    expect(cursor).toBe(N_INPUTS);
  });

  it("names the whisker channels by direction and channel", () => {
    // Index 0 is whisker 0 (far left), channel 0 (food).
    expect(inputLabel(0)).toBe("whisker far left · food");
    // Index 16 is whisker 2 (ahead), channel 2 (alarm): 2*7 + 2.
    expect(inputLabel(16)).toBe("whisker ahead · alarm");
    // Index 33 is whisker 4 (far right), channel 5 (wall): 4*7 + 5.
    expect(inputLabel(33)).toBe("whisker far right · wall");
    // Index 34 is the new 7th channel: whisker 4, home trail: 4*7 + 6.
    expect(inputLabel(34)).toBe("whisker far right · home trail");
  });

  it("names the non-whisker inputs", () => {
    expect(inputLabel(35)).toBe("underfoot food");
    expect(inputLabel(38)).toBe("underfoot home trail");
    expect(inputLabel(41)).toBe("energy");
    expect(inputLabel(45)).toBe("bias");
    expect(inputLabel(46)).toBe("memory 0");
    expect(inputLabel(49)).toBe("memory 3");
    expect(inputLabel(50)).toBe("home vector x");
    expect(inputLabel(52)).toBe("home distance");
    expect(inputLabel(53)).toBe("facing (sin)");
    expect(inputLabel(N_INPUTS - 1)).toBe("facing (cos)");
  });
});

describe("output labels", () => {
  it("has one label and one description per output", () => {
    expect(OUTPUT_LABELS).toHaveLength(N_OUTPUTS);
    expect(OUTPUT_DESC).toHaveLength(N_OUTPUTS);
  });
});

describe("nodeInfo", () => {
  it("labels inputs, outputs (with a description), and hidden neurons", () => {
    expect(nodeInfo(0, 35).label).toBe("underfoot food");
    const out = nodeInfo(3, 1);
    expect(out.label).toBe("output · vel y");
    expect(out.desc).toBeTruthy();
    expect(nodeInfo(1, 5).label).toBe("hidden 1 · neuron 5");
    expect(nodeInfo(1, 5).desc).toBeUndefined();
  });
});

describe("hitTest", () => {
  it("returns the node nearest a point and null when nothing is close", () => {
    const { nodes } = layout(300, 300, 18, 30, 52, 18);
    const target = nodes.find((n) => n.layer === 3 && n.index === 2)!;
    const hit = hitTest(target.x, target.y, 300, 300);
    expect(hit).not.toBeNull();
    expect(hit!.layer).toBe(3);
    expect(hit!.index).toBe(2);
  });

  it("never returns a hidden-layer node", () => {
    const { nodes } = layout(300, 300, 18, 30, 52, 18);
    const hidden = nodes.find((n) => n.layer === 1)!;
    // Directly on a hidden node, the nearest reportable node must not be hidden.
    const hit = hitTest(hidden.x, hidden.y, 300, 300, 4);
    expect(hit).toBeNull();
  });
});
