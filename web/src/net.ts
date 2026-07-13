/**
 * The WebSocket. Decodes binary frames into the store and sends commands back.
 *
 * The server drops frames for a slow client rather than queueing them, so
 * anything that arrives here is the newest thing there is. Nothing in this file
 * buffers or interpolates: the renderer draws whatever the last frame said.
 */

import { decode } from "./protocol.js";
import type { Store } from "./state.js";

/** Backoff on reconnect. A dead server should not be hammered at 60 Hz. */
const RECONNECT_MIN_MS = 250;
const RECONNECT_MAX_MS = 5000;

export class Net {
  private ws: WebSocket | null = null;
  private backoff = RECONNECT_MIN_MS;
  private closed = false;

  constructor(
    private readonly url: string,
    private readonly store: Store,
  ) {}

  connect(): void {
    this.closed = false;
    const ws = new WebSocket(this.url);
    ws.binaryType = "arraybuffer";
    this.ws = ws;

    ws.onopen = () => {
      this.backoff = RECONNECT_MIN_MS;
      this.store.setConnected(true);
    };

    ws.onmessage = (ev) => {
      if (!(ev.data instanceof ArrayBuffer)) return;
      this.dispatch(ev.data);
    };

    ws.onclose = () => {
      this.store.setConnected(false);
      if (!this.closed) this.scheduleReconnect();
    };

    // `onerror` is always followed by `onclose`, so reconnect is handled there.
    ws.onerror = () => ws.close();
  }

  close(): void {
    this.closed = true;
    this.ws?.close();
  }

  send(bytes: Uint8Array): void {
    if (this.ws?.readyState !== WebSocket.OPEN) return;
    // Copy rather than pass `bytes.buffer`: that would send the whole backing
    // buffer, which is only ever the right length by accident. Commands are at
    // most 9 bytes, so the copy costs nothing and cannot silently send padding.
    this.ws.send(bytes.slice().buffer as ArrayBuffer);
  }

  /** Exposed for tests: routes one frame into the store. */
  dispatch(buf: ArrayBuffer): void {
    const f = decode(buf);
    if (!f) return; // unknown tag: the server is newer than we are. Ignore it.

    switch (f.kind) {
      case "hello":
        this.store.applyHello(f);
        break;
      case "ants":
        // Deliberately does not notify: the renderer polls on rAF, and waking
        // every DOM subscriber 20 times a second to redraw text is waste.
        this.store.applyAnts(f);
        break;
      case "phero":
        this.store.applyPhero(f);
        break;
      case "terrain":
        this.store.applyTerrain(f);
        break;
      case "stats":
        this.store.applyStats(f.tick, f.colonies);
        break;
      case "detail":
        this.store.applyDetail(f);
        break;
      case "genome":
        this.store.applyGenome(f);
        break;
      case "config":
        this.store.applyConfig(f);
        break;
      case "colonyMeta":
        this.store.applyColonyMeta(f);
        break;
      case "chronicle":
        this.store.applyChronicle(f);
        break;
    }
  }

  private scheduleReconnect(): void {
    const wait = this.backoff;
    this.backoff = Math.min(this.backoff * 2, RECONNECT_MAX_MS);
    setTimeout(() => {
      if (!this.closed) this.connect();
    }, wait);
  }
}

export function socketUrl(): string {
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${location.host}/ws`;
}
