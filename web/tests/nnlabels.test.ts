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
    // 8 channels per whisker now: food, food trail, alarm, own scent, foe
    // scent, wall, home trail, own trail.
    // Index 0 is whisker 0 (far left), channel 0 (food).
    expect(inputLabel(0)).toBe("whisker far left · food");
    // Index 18 is whisker 2 (ahead), channel 2 (alarm): 2*8 + 2.
    expect(inputLabel(18)).toBe("whisker ahead · alarm");
    // Index 37 is whisker 4 (far right), channel 5 (wall): 4*8 + 5.
    expect(inputLabel(37)).toBe("whisker far right · wall");
    // Index 38 is whisker 4, home trail: 4*8 + 6.
    expect(inputLabel(38)).toBe("whisker far right · home trail");
    // Index 39 is whisker 4, own trail: 4*8 + 7 — the last whisker input.
    expect(inputLabel(39)).toBe("whisker far right · own trail");
  });

  it("names the non-whisker inputs", () => {
    // Whisker block is now 40 wide (5x8).
    expect(inputLabel(40)).toBe("underfoot food");
    expect(inputLabel(43)).toBe("underfoot home trail");
    expect(inputLabel(46)).toBe("energy");
    expect(inputLabel(50)).toBe("bias");
    expect(inputLabel(51)).toBe("memory 0");
    expect(inputLabel(54)).toBe("memory 3");
    expect(inputLabel(55)).toBe("home vector x");
    expect(inputLabel(57)).toBe("home distance");
    expect(inputLabel(58)).toBe("facing (sin)");
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
    expect(nodeInfo(0, 40).label).toBe("underfoot food");
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
