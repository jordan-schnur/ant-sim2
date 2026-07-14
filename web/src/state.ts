/**
 * The client's whole memory. It holds the latest of each frame and nothing
 * derived from the simulation: the browser never computes anything about an ant.
 *
 * Charts are the one exception, and deliberately so — a time series cannot be
 * reconstructed from a single frame, and the 500k-tick run showed the delivery
 * curve does not bend upward until roughly tick 100,000. Without history the
 * operator is staring at a number and guessing.
 */

import type {
  AntDetail,
  AntGenome,
  Ants,
  Chronicle,
  ColonyMeta,
  ColonyStat,
  ConfigFrame,
  Hello,
  Phero,
  Terrain,
} from "./protocol.js";
import { nestCentroids } from "./terrain.js";

/** ~2.5 minutes of stats at 4 fps. Enough to see a trend, cheap to redraw. */
export const HISTORY_LEN = 600;

export type Speed = 0 | 1 | 2;

export type Selection =
  | { kind: "ant" }
  | { kind: "colony"; id: number }
  | { kind: "tile"; x: number; y: number }
  | null;

export interface ColonyHistory {
  tick: number[];
  population: number[];
  store: number[];
  deliveredTotal: number[];
}

export interface State {
  connected: boolean;
  hello: Hello | null;
  ants: Ants | null;
  phero: Phero | null;
  terrain: Terrain | null;
  stats: ColonyStat[];
  tick: number;
  detail: AntDetail | null;
  genome: AntGenome | null;
  /** Nest centroid per colony, in world cells. Recomputed on each terrain
   * frame; drives the camera snap and the in-world colony popover. */
  nestCentroids: Map<number, { x: number; y: number }>;
  /** What the Explorer and in-world popover are focused on. */
  selection: Selection;
  /** Which right-rail tab is shown. Selecting anything flips it to explorer. */
  activeTab: "colonies" | "explorer";
  config: Map<number, number>;
  history: Map<number, ColonyHistory>;
  colonyMeta: ColonyMeta | null;
  chronicle: Chronicle | null;

  // Playback state is client-side optimism: the server is authoritative, but
  // the button must light up the instant it is pressed.
  paused: boolean;
  speed: Speed;
  layers: { food: boolean; alarm: boolean; scent: boolean; trail: boolean };
  labels: boolean;
  pheroResLog2: number;
}

type Listener = () => void;

export class Store {
  readonly state: State = {
    connected: false,
    hello: null,
    ants: null,
    phero: null,
    terrain: null,
    stats: [],
    tick: 0,
    detail: null,
    genome: null,
    nestCentroids: new Map(),
    selection: null,
    activeTab: "colonies",
    config: new Map(),
    history: new Map(),
    colonyMeta: null,
    chronicle: null,
    paused: true,
    speed: 0,
    layers: { food: true, alarm: false, scent: true, trail: false },
    labels: true,
    pheroResLog2: 8,
  };

  private listeners = new Set<Listener>();

  subscribe(fn: Listener): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  notify(): void {
    for (const fn of this.listeners) fn();
  }

  setConnected(v: boolean): void {
    this.state.connected = v;
    this.notify();
  }

  applyHello(h: Hello): void {
    const prev = this.state.hello;
    this.state.hello = h;
    this.state.pheroResLog2 = h.pheroResLog2;

    // A reset or a load rebuilds the world. Old history describes a world that
    // no longer exists; keeping it would draw a chart that lies.
    if (prev && h.tick < prev.tick) this.state.history.clear();
    this.notify();
  }

  applyAnts(a: Ants): void {
    this.state.ants = a;
    this.state.tick = a.tick;
  }

  applyPhero(p: Phero): void {
    this.state.phero = p;
  }

  applyTerrain(t: Terrain): void {
    this.state.terrain = t;
    // Nests can move when the world is reset or reshaped, so recompute rather
    // than trusting a cache keyed on anything but the frame itself.
    this.state.nestCentroids = nestCentroids(t);
  }

  applyStats(tick: number, colonies: ColonyStat[]): void {
    this.state.stats = colonies;
    this.state.tick = tick;
    for (const c of colonies) {
      let h = this.state.history.get(c.id);
      if (!h) {
        h = { tick: [], population: [], store: [], deliveredTotal: [] };
        this.state.history.set(c.id, h);
      }
      push(h.tick, tick);
      push(h.population, c.population);
      push(h.store, c.store);
      push(h.deliveredTotal, c.deliveredTotal);
    }
    this.notify();
  }

  applyDetail(d: AntDetail): void {
    this.state.detail = d;
    // The genome belongs to whoever is selected. If the server has moved on to
    // a different ant, the old weights would be drawn against new activations.
    if (this.state.genome && this.state.genome.id !== d.id) this.state.genome = null;
    this.notify();
  }

  applyGenome(g: AntGenome): void {
    this.state.genome = g;
    this.notify();
  }

  applyConfig(c: ConfigFrame): void {
    this.state.config = c.values;
    this.notify();
  }

  applyColonyMeta(m: ColonyMeta): void {
    this.state.colonyMeta = m;
    this.notify();
  }

  applyChronicle(c: Chronicle): void {
    this.state.chronicle = c;
    this.notify();
  }

  /** The colony's generated name, or a stable fallback before meta arrives. */
  colonyName(id: number): string {
    return (
      this.state.colonyMeta?.colonies.find((c) => c.id === id)?.name ??
      `colony ${id}`
    );
  }

  selectAnt(): void {
    this.state.selection = { kind: "ant" };
    this.state.activeTab = "explorer";
    this.notify();
  }

  /** Open the in-world stats popover for a colony (clicking its nest). */
  selectColony(id: number): void {
    this.state.selection = { kind: "colony", id };
    this.state.activeTab = "explorer";
    this.notify();
  }

  selectTile(x: number, y: number): void {
    this.state.selection = { kind: "tile", x, y };
    this.state.activeTab = "explorer";
    this.notify();
  }

  /** The colony id iff a colony is selected — for the in-world colony popover. */
  selectedColony(): number | null {
    return this.state.selection?.kind === "colony" ? this.state.selection.id : null;
  }

  setTab(tab: State["activeTab"]): void {
    this.state.activeTab = tab;
    this.notify();
  }

  clearSelection(): void {
    this.state.detail = null;
    this.state.genome = null;
    this.state.selection = null;
    this.notify();
  }

  setPaused(p: boolean): void {
    this.state.paused = p;
    this.notify();
  }

  setSpeed(s: Speed): void {
    this.state.speed = s;
    this.state.paused = false;
    this.notify();
  }

  toggleLayer(k: keyof State["layers"]): void {
    this.state.layers[k] = !this.state.layers[k];
    this.notify();
  }

  toggleLabels(): void {
    this.state.labels = !this.state.labels;
    this.notify();
  }
}

function push(a: number[], v: number): void {
  a.push(v);
  if (a.length > HISTORY_LEN) a.shift();
}

export interface WorldSummary {
  pop: number;
  store: number;
  delivered: number;
}

/** All-colony totals, for the Explorer's default (nothing-selected) view. */
export function worldSummary(stats: ColonyStat[]): WorldSummary {
  let pop = 0;
  let store = 0;
  let delivered = 0;
  for (const c of stats) {
    pop += c.population;
    store += c.store;
    delivered += c.deliveredTotal;
  }
  return { pop, store, delivered };
}
