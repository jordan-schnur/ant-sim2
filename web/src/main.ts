/**
 * Bootstrap. Waits for `hello` before building the renderer, because the world
 * size comes off the wire rather than being assumed.
 */

import { Net, socketUrl } from "./net.js";
import { cmdClearSelection, cmdSelectAt } from "./protocol.js";
import { Store } from "./state.js";
import { WorldRenderer } from "./render/world.js";
import { mountChronicle } from "./ui/chronicle.js";
import { mountColonies } from "./ui/colony.js";
import { mountControls } from "./ui/controls.js";
import { mountInspector } from "./ui/inspector.js";

/** A drag beyond this many pixels is a pan, not a click on an ant. */
const CLICK_SLOP_PX = 4;

const store = new Store();
const net = new Net(socketUrl(), store);

const canvas = document.getElementById("world") as HTMLCanvasElement;
const overlay = document.getElementById("overlay") as HTMLDivElement;
const leftRail = document.getElementById("left-rail") as HTMLElement;
const rightRail = document.getElementById("right-rail") as HTMLElement;

mountControls(leftRail, store, net);

// Explicit containers, in order. `mountColonies` creates its cards lazily when
// the first stats frame lands, so appending both panels straight onto the rail
// would leave the colonies underneath the inspector.
const coloniesEl = document.createElement("div");
const chronicleEl = document.createElement("div");
const inspectorEl = document.createElement("div");
rightRail.append(coloniesEl, chronicleEl, inspectorEl);
mountColonies(coloniesEl, store);
mountChronicle(chronicleEl, store);
mountInspector(inspectorEl, store);

document.getElementById("collapse-left")!.addEventListener("click", () => {
  leftRail.classList.toggle("collapsed");
});
document.getElementById("collapse-right")!.addEventListener("click", () => {
  rightRail.classList.toggle("collapsed");
});

let renderer: WorldRenderer | null = null;
let worldKey = "";

function ensureRenderer(): WorldRenderer | null {
  const h = store.state.hello;
  if (!h) return null;
  const key = `${h.width}x${h.height}`;
  if (renderer && worldKey === key) return renderer;

  try {
    renderer = new WorldRenderer(canvas, store, h.width, h.height);
  } catch (e) {
    overlay.textContent = e instanceof Error ? e.message : String(e);
    return null;
  }
  worldKey = key;
  renderer.resize();
  renderer.camera.fit(renderer.viewW, renderer.viewH);
  attachPointer(renderer);
  return renderer;
}

let pointerAttached = false;

function attachPointer(r: WorldRenderer): void {
  if (pointerAttached) return;
  pointerAttached = true;

  let dragging = false;
  let moved = 0;
  let lastX = 0;
  let lastY = 0;

  canvas.addEventListener("pointerdown", (e) => {
    dragging = true;
    moved = 0;
    lastX = e.clientX;
    lastY = e.clientY;
    canvas.setPointerCapture(e.pointerId);
  });

  canvas.addEventListener("pointermove", (e) => {
    if (!dragging) return;
    const dx = e.clientX - lastX;
    const dy = e.clientY - lastY;
    lastX = e.clientX;
    lastY = e.clientY;
    moved += Math.abs(dx) + Math.abs(dy);
    if (moved > CLICK_SLOP_PX) {
      canvas.classList.add("panning");
      r.camera.panByPixels(dx * r.dpr, dy * r.dpr);
    }
  });

  canvas.addEventListener("pointerup", (e) => {
    dragging = false;
    canvas.classList.remove("panning");
    canvas.releasePointerCapture(e.pointerId);
    if (moved > CLICK_SLOP_PX) return; // that was a pan

    // Selection is resolved server-side: the ant frame carries no ids, so we
    // send a world coordinate and the server replies with the nearest ant.
    const rect = canvas.getBoundingClientRect();
    const px = (e.clientX - rect.left) * r.dpr;
    const py = (e.clientY - rect.top) * r.dpr;
    const w = r.camera.screenToWorld(px, py, r.viewW, r.viewH);

    const h = store.state.hello;
    if (h && (w.x < 0 || w.y < 0 || w.x >= h.width || w.y >= h.height)) {
      store.clearSelection();
      net.send(cmdClearSelection());
      return;
    }
    net.send(cmdSelectAt(w.x, w.y));
  });

  canvas.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();
      const rect = canvas.getBoundingClientRect();
      const px = (e.clientX - rect.left) * r.dpr;
      const py = (e.clientY - rect.top) * r.dpr;
      // Exponential in the wheel delta so a trackpad and a mouse both feel sane.
      const factor = Math.exp(-e.deltaY * 0.002);
      r.camera.zoomAt(px, py, factor, r.viewW, r.viewH);
    },
    { passive: false },
  );

  window.addEventListener("keydown", (e) => {
    if (e.target instanceof HTMLInputElement) return;
    if (e.code === "Space") {
      e.preventDefault();
      (document.querySelector("#left-rail button") as HTMLButtonElement | null)?.click();
    }
    if (e.key === "f") r.camera.fit(r.viewW, r.viewH);
    if (e.key === "Escape") {
      store.clearSelection();
      net.send(cmdClearSelection());
    }
  });
}

function frame(): void {
  const r = ensureRenderer();
  if (r) {
    r.draw();
    const st = store.state;
    const ants = st.ants?.count ?? 0;
    overlay.textContent = st.connected
      ? `tick ${st.tick.toLocaleString()}  ·  ${ants.toLocaleString()} ants  ·  ${r.camera.zoom.toFixed(1)}x`
      : "disconnected — retrying";
  }
  requestAnimationFrame(frame);
}

net.connect();
requestAnimationFrame(frame);
