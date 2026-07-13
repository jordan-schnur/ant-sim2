/**
 * The wire format, mirrored by hand from `crates/server/src/protocol.rs`.
 *
 * Nothing in either language can see the other, so a field reordered on one
 * side is silent: it renders as ants in a diagonal line, not as an error.
 * `tests/protocol.test.ts` decodes byte fixtures emitted by the Rust encoder
 * and asserts field-for-field. That test is the only thing holding these two
 * files together — if you change a layout here, change it there, and expect
 * the fixture test to fail until you regenerate.
 *
 * Everything is little-endian. `DataView` defaults to big-endian, so every
 * read below passes `true` explicitly.
 */

export const TAG_HELLO = 0x01;
export const TAG_ANTS = 0x02;
export const TAG_PHERO = 0x03;
export const TAG_STATS = 0x04;
export const TAG_ANT_DETAIL = 0x05;
export const TAG_ANT_GENOME = 0x06;
export const TAG_CONFIG = 0x07;
export const TAG_TERRAIN = 0x08;
export const TAG_COLONY_META = 0x09;
export const TAG_CHRONICLE = 0x0a;

export const CMD_SET_PAUSED = 0x01;
export const CMD_SET_SPEED = 0x02;
export const CMD_STEP = 0x03;
export const CMD_SELECT_AT = 0x04;
export const CMD_CLEAR_SELECTION = 0x05;
export const CMD_SET_CONFIG = 0x06;
export const CMD_SET_PHERO_RES = 0x07;
export const CMD_SAVE = 0x08;
export const CMD_LOAD = 0x09;
export const CMD_RESET = 0x0a;

export const BYTES_PER_ANT = 8;
export const BYTES_PER_COLONY = 46;
export const ANT_DETAIL_LEN = 421;

export const N_INPUTS = 44;
export const N_HIDDEN1 = 16;
export const N_HIDDEN2 = 16;
export const N_OUTPUTS = 8;
export const N_PARAMS = 1128;

export const FLAG_CARRYING = 1 << 0;
export const FLAG_ATTACKING = 1 << 1;

/** Positions are fixed-point 9.7. The grid is 512 wide, so this is exact. */
export const POS_SCALE = 128;
/** `TRAIT_RANGES` caps max_size at 3.0, so the size byte cannot clip. */
export const MAX_ENCODABLE_SIZE = 3.0;
/** Alpha value meaning "no colony owns this cell's scent". */
export const NO_OWNER = 255;
/** Blue value in the terrain texture meaning "not a nest tile". */
export const NO_NEST = 255;

/** Tunable Config fields, in field-id order. Mirrors `CONFIG_FIELDS` in Rust. */
export const CONFIG_FIELDS = [
  "food_evaporation",
  "alarm_evaporation",
  "scent_evaporation",
  "food_diffusion",
  "alarm_diffusion",
  "scent_diffusion",
  "tax_speed",
  "tax_vision",
  "mutation_rate",
  "mutation_sigma",
  "birth_cost",
  "harvest_rate",
  "refuel_rate",
  "growth_threshold",
  "food_regrow",
  "attack_damage",
  "harvest_weight",
] as const;

export const TRAIT_NAMES = [
  "max_speed",
  "strength",
  "armor",
  "vision",
  "carry_capacity",
  "max_size",
  "metabolic_efficiency",
  "lifespan",
] as const;

export interface Hello {
  kind: "hello";
  width: number;
  height: number;
  numColonies: number;
  pheroResLog2: number;
  tick: number;
}

export interface Ants {
  kind: "ants";
  tick: number;
  count: number;
  /** The raw 8-byte-per-ant block, uploaded straight into a vertex buffer. */
  raw: Uint8Array;
}

export interface Phero {
  kind: "phero";
  tick: number;
  w: number;
  h: number;
  factor: number;
  /** RGBA8. R food, G alarm, B scent, A owning colony (255 = none). */
  rgba: Uint8Array;
}

/**
 * The map: stone, standing food, nest tiles. Without this the client draws an
 * empty void with pheromone smears on it -- the pheromone frame carries trails,
 * not the food they lead to, and knows nothing about the rock.
 */
export interface Terrain {
  kind: "terrain";
  tick: number;
  w: number;
  h: number;
  factor: number;
  /** RGBA8. R food (normalised), G stone coverage, B nest colony (255 none). */
  rgba: Uint8Array;
}

export interface ColonyStat {
  id: number;
  population: number;
  store: number;
  births: number;
  deaths: number;
  floorSpawns: number;
  meanSize: number;
  /** Mean lineage depth. This is what the project calls a "generation". */
  meanLineage: number;
  /** Monotonic. The only curve that answers "is this evolving". */
  deliveredTotal: number;
}

export interface Stats {
  kind: "stats";
  tick: number;
  colonies: ColonyStat[];
}

export interface AntDetail {
  kind: "detail";
  id: number;
  colony: number;
  alive: boolean;
  x: number;
  y: number;
  heading: number;
  energy: number;
  maxEnergy: number;
  size: number;
  carrying: number;
  foodDelivered: number;
  age: number;
  lineage: number;
  traits: Float32Array;
  inputs: Float32Array;
  h1: Float32Array;
  h2: Float32Array;
  outputs: Float32Array;
  name: string;
}

export interface AntGenome {
  kind: "genome";
  id: number;
  params: Float32Array;
}

export interface ConfigFrame {
  kind: "config";
  values: Map<number, number>;
}

export interface ColonyMeta {
  kind: "colonyMeta";
  colonies: { id: number; name: string }[];
}

export interface ChronicleEvent {
  tick: number;
  colony: number;
  eventKind: number;
  antId: number | null;
  antName: string | null;
  text: string;
}
export interface Chronicle {
  kind: "chronicle";
  events: ChronicleEvent[];
}

export type Frame =
  | Hello
  | Ants
  | Phero
  | Terrain
  | Stats
  | AntDetail
  | AntGenome
  | ConfigFrame
  | ColonyMeta
  | Chronicle;

/**
 * `id` and `tick` are u64 on the wire. JS numbers hold integers exactly to
 * 2^53, and the sim would need ~285,000 years at 1000 ticks/sec to exceed that,
 * so reading them as Number rather than BigInt is safe and keeps them usable as
 * Map keys and in arithmetic.
 */
function u64(v: DataView, off: number): number {
  return Number(v.getBigUint64(off, true));
}

/** Read a u8-length-prefixed UTF-8 string, advancing the cursor. */
function readStrU8(v: DataView, o: { p: number }): string {
  const n = v.getUint8(o.p);
  o.p += 1;
  const bytes = new Uint8Array(v.buffer, v.byteOffset + o.p, n);
  o.p += n;
  return new TextDecoder().decode(bytes);
}

export function decode(buf: ArrayBuffer): Frame | null {
  const v = new DataView(buf);
  if (buf.byteLength < 1) return null;

  switch (v.getUint8(0)) {
    case TAG_HELLO:
      return {
        kind: "hello",
        width: v.getUint16(1, true),
        height: v.getUint16(3, true),
        numColonies: v.getUint8(5),
        pheroResLog2: v.getUint8(6),
        tick: u64(v, 7),
      };

    case TAG_ANTS: {
      const count = v.getUint32(9, true);
      return {
        kind: "ants",
        tick: u64(v, 1),
        count,
        raw: new Uint8Array(buf, 13, count * BYTES_PER_ANT),
      };
    }

    case TAG_PHERO: {
      const w = v.getUint16(9, true);
      const h = v.getUint16(11, true);
      return {
        kind: "phero",
        tick: u64(v, 1),
        w,
        h,
        factor: v.getUint8(13),
        rgba: new Uint8Array(buf, 14, w * h * 4),
      };
    }

    case TAG_TERRAIN: {
      const w = v.getUint16(9, true);
      const h = v.getUint16(11, true);
      return {
        kind: "terrain",
        tick: u64(v, 1),
        w,
        h,
        factor: v.getUint8(13),
        rgba: new Uint8Array(buf, 14, w * h * 4),
      };
    }

    case TAG_STATS: {
      const n = v.getUint8(9);
      const colonies: ColonyStat[] = [];
      for (let i = 0; i < n; i++) {
        const o = 10 + i * BYTES_PER_COLONY;
        colonies.push({
          id: v.getUint8(o),
          // o + 1 is a pad byte
          population: v.getUint32(o + 2, true),
          store: v.getFloat32(o + 6, true),
          births: u64(v, o + 10),
          deaths: u64(v, o + 18),
          floorSpawns: u64(v, o + 26),
          meanSize: v.getFloat32(o + 34, true),
          meanLineage: v.getFloat32(o + 38, true),
          deliveredTotal: v.getFloat32(o + 42, true),
        });
      }
      return { kind: "stats", tick: u64(v, 1), colonies };
    }

    case TAG_ANT_DETAIL: {
      const f = (o: number) => v.getFloat32(o, true);
      const floats = (o: number, n: number) => {
        const a = new Float32Array(n);
        for (let i = 0; i < n; i++) a[i] = v.getFloat32(o + i * 4, true);
        return a;
      };
      // The fixed body ends at ANT_DETAIL_LEN; a length-prefixed name may
      // follow. An old server that sends exactly the fixed body yields "".
      const name =
        v.byteLength > ANT_DETAIL_LEN ? readStrU8(v, { p: ANT_DETAIL_LEN }) : "";
      return {
        kind: "detail",
        id: u64(v, 1),
        colony: v.getUint8(9),
        alive: v.getUint8(10) !== 0,
        x: f(13),
        y: f(17),
        heading: f(21),
        energy: f(25),
        maxEnergy: f(29),
        size: f(33),
        carrying: f(37),
        foodDelivered: f(41),
        age: v.getUint32(45, true),
        lineage: v.getUint32(49, true),
        traits: floats(53, 8),
        inputs: floats(85, N_INPUTS),
        h1: floats(261, N_HIDDEN1),
        h2: floats(325, N_HIDDEN2),
        outputs: floats(389, N_OUTPUTS),
        name,
      };
    }

    case TAG_ANT_GENOME: {
      const params = new Float32Array(N_PARAMS);
      for (let i = 0; i < N_PARAMS; i++) params[i] = v.getFloat32(9 + i * 4, true);
      return { kind: "genome", id: u64(v, 1), params };
    }

    case TAG_CONFIG: {
      const n = v.getUint8(1);
      const values = new Map<number, number>();
      for (let i = 0; i < n; i++) {
        const o = 2 + i * 5;
        values.set(v.getUint8(o), v.getFloat32(o + 1, true));
      }
      return { kind: "config", values };
    }

    case TAG_COLONY_META: {
      const count = v.getUint8(1);
      const colonies: { id: number; name: string }[] = [];
      const o = { p: 2 };
      for (let i = 0; i < count; i++) {
        const id = v.getUint8(o.p);
        o.p += 1;
        colonies.push({ id, name: readStrU8(v, o) });
      }
      return { kind: "colonyMeta", colonies };
    }

    case TAG_CHRONICLE: {
      const count = v.getUint16(1, true);
      const events: ChronicleEvent[] = [];
      const o = { p: 3 };
      for (let i = 0; i < count; i++) {
        const tick = u64(v, o.p);
        o.p += 8;
        const colony = v.getUint8(o.p);
        o.p += 1;
        const eventKind = v.getUint8(o.p);
        o.p += 1;
        const hasAnt = v.getUint8(o.p) !== 0;
        o.p += 1;
        const rawId = u64(v, o.p);
        o.p += 8;
        const antName = readStrU8(v, o);
        const textLen = v.getUint16(o.p, true);
        o.p += 2;
        const text = new TextDecoder().decode(
          new Uint8Array(v.buffer, v.byteOffset + o.p, textLen),
        );
        o.p += textLen;
        events.push({
          tick,
          colony,
          eventKind,
          antId: hasAnt ? rawId : null,
          antName: hasAnt ? antName : null,
          text,
        });
      }
      return { kind: "chronicle", events };
    }

    default:
      return null;
  }
}

// --- Commands ------------------------------------------------------------

const one = (tag: number) => new Uint8Array([tag]);

export const cmdStep = () => one(CMD_STEP);
export const cmdClearSelection = () => one(CMD_CLEAR_SELECTION);
export const cmdSave = () => one(CMD_SAVE);
export const cmdLoad = () => one(CMD_LOAD);

export const cmdSetPaused = (p: boolean) => new Uint8Array([CMD_SET_PAUSED, p ? 1 : 0]);
export const cmdSetSpeed = (s: number) => new Uint8Array([CMD_SET_SPEED, s]);
export const cmdSetPheroRes = (log2: number) => new Uint8Array([CMD_SET_PHERO_RES, log2]);

export function cmdSelectAt(x: number, y: number): Uint8Array {
  const b = new Uint8Array(9);
  const v = new DataView(b.buffer);
  v.setUint8(0, CMD_SELECT_AT);
  v.setFloat32(1, x, true);
  v.setFloat32(5, y, true);
  return b;
}

export function cmdSetConfig(field: number, value: number): Uint8Array {
  const b = new Uint8Array(6);
  const v = new DataView(b.buffer);
  v.setUint8(0, CMD_SET_CONFIG);
  v.setUint8(1, field);
  v.setFloat32(2, value, true);
  return b;
}

export function cmdReset(seed: number): Uint8Array {
  const b = new Uint8Array(9);
  const v = new DataView(b.buffer);
  v.setUint8(0, CMD_RESET);
  v.setBigUint64(1, BigInt(seed), true);
  return b;
}

// --- Ant record accessors ------------------------------------------------

/** Unpacks one ant from the raw block. The renderer does not use this — it
 *  hands `raw` straight to the GPU — but the inspector and tests do. */
export function antAt(raw: Uint8Array, i: number) {
  const v = new DataView(raw.buffer, raw.byteOffset + i * BYTES_PER_ANT, BYTES_PER_ANT);
  const flags = v.getUint8(6);
  return {
    x: v.getUint16(0, true) / POS_SCALE,
    y: v.getUint16(2, true) / POS_SCALE,
    colony: v.getUint8(4),
    size: (v.getUint8(5) / 255) * MAX_ENCODABLE_SIZE,
    carrying: (flags & FLAG_CARRYING) !== 0,
    attacking: (flags & FLAG_ATTACKING) !== 0,
  };
}
