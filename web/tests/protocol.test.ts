/**
 * Decodes byte fixtures produced by the Rust encoder and asserts TypeScript
 * agrees with it, field for field.
 *
 * This is the joint between the two halves of the protocol. Neither language
 * can see the other's layout, and a mismatch produces no error — just garbled
 * rendering, weeks later, in a way that looks like a shader bug.
 *
 * Regenerate the fixtures after an intentional format change:
 *     cargo test -p server --test fixtures
 */

import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  ANT_DETAIL_LEN,
  CONFIG_FIELDS,
  FLAG_ATTACKING,
  FLAG_CARRYING,
  N_PARAMS,
  POS_SCALE,
  TAG_ANTS,
  TAG_ANT_DETAIL,
  TAG_ANT_GENOME,
  TAG_CONFIG,
  TAG_HELLO,
  TAG_PHERO,
  TAG_STATS,
  antAt,
  cmdReset,
  cmdSelectAt,
  cmdSetConfig,
  cmdSetPaused,
  decode,
} from "../src/protocol.js";

const here = dirname(fileURLToPath(import.meta.url));
const fixtures = join(here, "../../crates/server/tests/fixtures");

function load(name: string): ArrayBuffer {
  const b = readFileSync(join(fixtures, name));
  // Copy out of Node's pooled Buffer: its byteOffset is rarely 0, and a
  // DataView over the pool would read a neighbouring fixture's bytes.
  return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength) as ArrayBuffer;
}

const expected = JSON.parse(readFileSync(join(fixtures, "expected.json"), "utf8"));

describe("hello", () => {
  it("agrees with the Rust encoder", () => {
    const f = decode(load("hello.bin"));
    expect(f?.kind).toBe("hello");
    if (f?.kind !== "hello") return;
    expect(f.width).toBe(expected.hello.width);
    expect(f.height).toBe(expected.hello.height);
    expect(f.numColonies).toBe(expected.hello.numColonies);
    expect(f.pheroResLog2).toBe(expected.hello.pheroResLog2);
    expect(f.tick).toBe(expected.hello.tick);
  });

  it("is exactly fifteen bytes", () => {
    expect(load("hello.bin").byteLength).toBe(15);
  });
});

describe("ants", () => {
  it("agrees on the count and the header", () => {
    const f = decode(load("ants.bin"));
    expect(f?.kind).toBe("ants");
    if (f?.kind !== "ants") return;
    expect(f.tick).toBe(expected.ants.tick);
    expect(f.count).toBe(expected.ants.count);
    expect(f.raw.byteLength).toBe(f.count * 8);
  });

  it("unpacks the first ant exactly as Rust packed it", () => {
    const f = decode(load("ants.bin"));
    if (f?.kind !== "ants") throw new Error("not an ant frame");
    const a = antAt(f.raw, 0);
    const e = expected.ants.first;

    // Rust wrote the raw fixed-point integer; we must land on the same one.
    expect(Math.round(a.x * POS_SCALE)).toBe(e.x);
    expect(Math.round(a.y * POS_SCALE)).toBe(e.y);
    expect(a.colony).toBe(e.colony);
    expect(a.carrying).toBe((e.flags & FLAG_CARRYING) !== 0);
    expect(a.attacking).toBe((e.flags & FLAG_ATTACKING) !== 0);
  });

  it("round-trips a 9.7 fixed-point position without drift", () => {
    const f = decode(load("ants.bin"));
    if (f?.kind !== "ants") throw new Error("not an ant frame");
    // 17.5 and 4.25 are exactly representable, so this must be exact, not close.
    expect(antAt(f.raw, 0).x).toBe(17.5);
    expect(antAt(f.raw, 0).y).toBe(4.25);
  });

  it("reads every ant in the block without running past the end", () => {
    const f = decode(load("ants.bin"));
    if (f?.kind !== "ants") throw new Error("not an ant frame");
    for (let i = 0; i < f.count; i++) {
      const a = antAt(f.raw, i);
      expect(a.x).toBeGreaterThanOrEqual(0);
      expect(a.x).toBeLessThan(512);
      expect(a.colony).toBeLessThan(expected.hello.numColonies);
    }
  });
});

describe("pheromones", () => {
  it("agrees on dimensions and downsample factor", () => {
    const f = decode(load("phero.bin"));
    expect(f?.kind).toBe("phero");
    if (f?.kind !== "phero") return;
    expect(f.w).toBe(expected.phero.w);
    expect(f.h).toBe(expected.phero.h);
    expect(f.factor).toBe(expected.phero.factor);
    expect(f.rgba.byteLength).toBe(f.w * f.h * 4);
  });

  it("reads the first texel byte-for-byte", () => {
    const f = decode(load("phero.bin"));
    if (f?.kind !== "phero") throw new Error("not a phero frame");
    expect([...f.rgba.slice(0, 4)]).toEqual(expected.phero.firstTexel);
  });

  it("finds the same brightest scent texel Rust did", () => {
    // A spot check on an empty corner would pass against a broken encoder.
    // This one carries real signal and a real owner.
    const f = decode(load("phero.bin"));
    if (f?.kind !== "phero") throw new Error("not a phero frame");
    const e = expected.phero.brightestScent;
    expect(f.rgba[e.texel * 4 + 2]).toBe(e.value);
    expect(f.rgba[e.texel * 4 + 3]).toBe(e.owner);
    expect(e.value).toBeGreaterThan(32);
  });
});

describe("stats", () => {
  it("agrees on every field of the first colony", () => {
    const f = decode(load("stats.bin"));
    expect(f?.kind).toBe("stats");
    if (f?.kind !== "stats") return;
    expect(f.colonies.length).toBe(expected.stats.count);

    const c = f.colonies[0];
    const e = expected.stats.first;
    expect(c.id).toBe(e.id);
    expect(c.population).toBe(e.population);
    expect(c.store).toBeCloseTo(e.store, 5);
    expect(c.births).toBe(e.births);
    expect(c.deaths).toBe(e.deaths);
    expect(c.floorSpawns).toBe(e.floorSpawns);
    expect(c.meanSize).toBeCloseTo(e.meanSize, 5);
    expect(c.meanLineage).toBeCloseTo(e.meanLineage, 5);
    expect(c.deliveredTotal).toBeCloseTo(e.deliveredTotal, 5);
  });

  it("checks fields that are actually non-zero", () => {
    // Guard the guard. If the fixture's store is 0.0, then reading `store` at
    // the wrong offset also yields 0.0 and the assertion above proves nothing.
    // A mutation test (shift `store` one byte) must fail; it only does if the
    // fixture carries signal in every field.
    const e = expected.stats.first;
    for (const k of ["store", "births", "deaths", "floorSpawns", "meanSize", "meanLineage", "deliveredTotal"]) {
      expect(e[k], `stats fixture field ${k} is zero and proves nothing`).not.toBe(0);
    }
    for (const k of ["age", "lineage", "trait0"]) {
      expect(expected.detail[k], `detail fixture ${k} is zero`).not.toBe(0);
    }
  });

  it("is a header plus forty-six bytes per colony", () => {
    const f = decode(load("stats.bin"));
    if (f?.kind !== "stats") throw new Error("not a stats frame");
    expect(load("stats.bin").byteLength).toBe(10 + 46 * f.colonies.length);
  });
});

describe("ant detail", () => {
  it("is exactly the documented length", () => {
    expect(load("detail.bin").byteLength).toBe(ANT_DETAIL_LEN);
  });

  it("agrees on scalars, traits, and activations", () => {
    const f = decode(load("detail.bin"));
    expect(f?.kind).toBe("detail");
    if (f?.kind !== "detail") return;
    const e = expected.detail;

    expect(f.id).toBe(e.id);
    expect(f.colony).toBe(e.colony);
    expect(f.alive).toBe(true);
    expect(f.x).toBeCloseTo(e.x, 5);
    expect(f.y).toBeCloseTo(e.y, 5);
    expect(f.age).toBe(e.age);
    expect(f.lineage).toBe(e.lineage);
    expect(f.traits[0]).toBeCloseTo(e.trait0, 6);
    expect(f.traits[7]).toBeCloseTo(e.trait7, 3);
    expect(f.inputs[0]).toBeCloseTo(e.input0, 6);
    expect(f.outputs[0]).toBeCloseTo(e.output0, 6);
  });

  it("pins the head and tail of every activation layer", () => {
    // Checking only inputs[0] and outputs[0] is not enough: a shifted `h2`
    // reads into `h1`, and since both are tanh outputs in (-1, 1) the wrong
    // values look entirely plausible. Only the exact endpoints catch it.
    const f = decode(load("detail.bin"));
    if (f?.kind !== "detail") throw new Error("not a detail frame");
    const e = expected.detail;
    expect(f.inputs[0]).toBeCloseTo(e.input0, 6);
    expect(f.inputs[43]).toBeCloseTo(e.input43, 6);
    expect(f.h1[0]).toBeCloseTo(e.h1_0, 6);
    expect(f.h1[15]).toBeCloseTo(e.h1_15, 6);
    expect(f.h2[0]).toBeCloseTo(e.h2_0, 6);
    expect(f.h2[15]).toBeCloseTo(e.h2_15, 6);
    expect(f.outputs[0]).toBeCloseTo(e.output0, 6);
    expect(f.outputs[7]).toBeCloseTo(e.output7, 6);

    // And the layers really are distinguishable, or the checks above are luck.
    expect(e.h1_0).not.toBe(e.h2_0);
    expect(e.h1_15).not.toBe(e.h2_15);
  });

  it("carries the full activation vector for every layer", () => {
    const f = decode(load("detail.bin"));
    if (f?.kind !== "detail") throw new Error("not a detail frame");
    expect(f.inputs.length).toBe(44);
    expect(f.h1.length).toBe(16);
    expect(f.h2.length).toBe(16);
    expect(f.outputs.length).toBe(8);
    // tanh layers: every hidden and output activation is inside (-1, 1).
    for (const a of [f.h1, f.h2, f.outputs]) {
      for (const v of a) expect(Math.abs(v)).toBeLessThanOrEqual(1);
    }
  });
});

describe("genome", () => {
  it("carries every parameter and the ant id", () => {
    const f = decode(load("genome.bin"));
    expect(f?.kind).toBe("genome");
    if (f?.kind !== "genome") return;
    expect(f.id).toBe(expected.genome.id);
    expect(f.params.length).toBe(N_PARAMS);
    expect(expected.genome.nParams).toBe(N_PARAMS);
    expect(f.params[0]).toBeCloseTo(expected.genome.param0, 6);
  });

  it("is nine bytes of header plus 1128 floats", () => {
    expect(load("genome.bin").byteLength).toBe(9 + N_PARAMS * 4);
  });
});

describe("config", () => {
  it("covers every tunable field", () => {
    const f = decode(load("config.bin"));
    expect(f?.kind).toBe("config");
    if (f?.kind !== "config") return;
    expect(f.values.size).toBe(expected.config.count);
    expect(f.values.size).toBe(CONFIG_FIELDS.length);
    expect(f.values.get(0)).toBeCloseTo(expected.config.field0, 6);
  });

  it("has a name in TypeScript for every id Rust sends", () => {
    const f = decode(load("config.bin"));
    if (f?.kind !== "config") throw new Error("not a config frame");
    for (const id of f.values.keys()) {
      expect(CONFIG_FIELDS[id], `no name for field id ${id}`).toBeDefined();
    }
  });
});

describe("unknown frames", () => {
  it("decode returns null rather than throwing", () => {
    expect(decode(new Uint8Array([0xff, 1, 2, 3]).buffer)).toBeNull();
    expect(decode(new ArrayBuffer(0))).toBeNull();
  });
});

describe("commands", () => {
  it("encodes set_paused the way Rust decodes it", () => {
    expect([...cmdSetPaused(true)]).toEqual([0x01, 1]);
    expect([...cmdSetPaused(false)]).toEqual([0x01, 0]);
  });

  it("encodes select_at as little-endian f32 pairs", () => {
    const b = cmdSelectAt(3.5, 7.25);
    const v = new DataView(b.buffer);
    expect(v.getUint8(0)).toBe(0x04);
    expect(v.getFloat32(1, true)).toBe(3.5);
    expect(v.getFloat32(5, true)).toBe(7.25);
    expect(b.byteLength).toBe(9);
  });

  it("encodes set_config with the field id before the value", () => {
    const b = cmdSetConfig(10, 12.5);
    const v = new DataView(b.buffer);
    expect(v.getUint8(0)).toBe(0x06);
    expect(v.getUint8(1)).toBe(10);
    expect(v.getFloat32(2, true)).toBe(12.5);
  });

  it("encodes reset as a little-endian u64 seed", () => {
    const b = cmdReset(99);
    const v = new DataView(b.buffer);
    expect(v.getUint8(0)).toBe(0x0a);
    expect(v.getBigUint64(1, true)).toBe(99n);
    expect(b.byteLength).toBe(9);
  });
});

describe("tags", () => {
  it("match the Rust constants", () => {
    expect([TAG_HELLO, TAG_ANTS, TAG_PHERO, TAG_STATS, TAG_ANT_DETAIL, TAG_ANT_GENOME, TAG_CONFIG])
      .toEqual([1, 2, 3, 4, 5, 6, 7]);
  });

  it("each fixture leads with its own tag", () => {
    const pairs: [string, number][] = [
      ["hello.bin", TAG_HELLO],
      ["ants.bin", TAG_ANTS],
      ["phero.bin", TAG_PHERO],
      ["stats.bin", TAG_STATS],
      ["detail.bin", TAG_ANT_DETAIL],
      ["genome.bin", TAG_ANT_GENOME],
      ["config.bin", TAG_CONFIG],
    ];
    for (const [name, tag] of pairs) {
      expect(new Uint8Array(load(name))[0], name).toBe(tag);
    }
  });
});
