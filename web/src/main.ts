/**
 * Bootstrap. Waits for `hello` before building the renderer, because the world
 * size comes off the wire rather than being assumed.
 */

import { Net, socketUrl } from "./net.js";
import {
  cmdAddToStore,
  cmdClearSelection,
  cmdRenameColony,
  cmdSelectAt,
  cmdSetFood,
  cmdSetStone,
  cmdSpawnAnt,
} from "./protocol.js";
import { Store } from "./state.js";
import { WorldRenderer } from "./render/world.js";
import { mountChronicle } from "./ui/chronicle.js";
import { mountColonies } from "./ui/colony.js";
import { mountControls } from "./ui/controls.js";
import { openContextMenu, type MenuItem } from "./ui/contextmenu.js";
import { mountInspector } from "./ui/inspector.js";
import { LabelOverlay } from "./ui/labels.js";
import { ColonyPanel } from "./ui/colonypanel.js";

/** A drag beyond this many pixels is a pan, not a click on an ant. */
const CLICK_SLOP_PX = 4;

/** Snapping to a colony frames it at least this close, never zooming out. */
const FOCUS_ZOOM = 8;

const store = new Store();
const net = new Net(socketUrl(), store);

const canvas = document.getElementById("world") as HTMLCanvasElement;
const overlay = document.getElementById("overlay") as HTMLDivElement;
const worldWrap = document.getElementById("world-wrap") as HTMLElement;
const labels = new LabelOverlay(worldWrap);
const colonyPanel = new ColonyPanel(worldWrap);
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
mountColonies(coloniesEl, store, focusColony);
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

    // A click on a nest tile opens that colony's in-world stats popover instead
    // of selecting the nearest ant — on a nest, colony info is what you want.
    const colony = nestColonyAt(w.x, w.y);
    if (colony !== null) {
      store.selectColony(colony);
      return;
    }
    store.clearColony(); // clicking elsewhere dismisses the popover
    net.send(cmdSelectAt(w.x, w.y));
  });

  canvas.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    const rect = canvas.getBoundingClientRect();
    const px = (e.clientX - rect.left) * r.dpr;
    const py = (e.clientY - rect.top) * r.dpr;
    const wp = r.camera.screenToWorld(px, py, r.viewW, r.viewH);
    const h = store.state.hello;
    if (h && (wp.x < 0 || wp.y < 0 || wp.x >= h.width || wp.y >= h.height)) return;
    openContextMenu(e.clientX, e.clientY, menuItemsFor(wp.x, wp.y));
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

/**
 * The nest colony under a world cell, from the latest terrain frame's B
 * channel (255 = no nest), or null. Lets the menu default colony-scoped edits
 * to the colony you right-clicked on.
 */
function nestColonyAt(x: number, y: number): number | null {
  const t = store.state.terrain;
  if (!t) return null;
  const tx = Math.floor(x / t.factor);
  const ty = Math.floor(y / t.factor);
  if (tx < 0 || ty < 0 || tx >= t.w || ty >= t.h) return null;
  const nest = t.rgba[(ty * t.w + tx) * 4 + 2];
  return nest === 255 ? null : nest;
}

/**
 * Frame a colony's nest: recentre on its centroid and, if zoomed far out, zoom
 * in to a legible level. Never zooms *out* — clicking a colony you are already
 * close to should not throw the view backward.
 */
function focusColony(colonyId: number): void {
  const r = renderer;
  const c = store.state.nestCentroids.get(colonyId);
  if (!r || !c) return;
  r.camera.centerOn(c.x, c.y);
  if (r.camera.zoom < FOCUS_ZOOM) r.camera.zoom = FOCUS_ZOOM;
}

/** The right-click menu for a world position, tailored to what is under it. */
function menuItemsFor(x: number, y: number): MenuItem[] {
  const colony = nestColonyAt(x, y);
  const items: MenuItem[] = [
    {
      label: "Set food here…",
      editor: {
        placeholder: "amount",
        initial: "100",
        apply: (v) => {
          const a = Number(v);
          if (Number.isFinite(a)) net.send(cmdSetFood(x, y, a));
        },
      },
    },
    { label: "Place stone", onClick: () => net.send(cmdSetStone(x, y, true)) },
    { label: "Clear stone", onClick: () => net.send(cmdSetStone(x, y, false)) },
    {
      label: "Spawn ant here…",
      editor: {
        placeholder: "colony id",
        initial: String(colony ?? 0),
        apply: (v) => {
          const c = Number(v);
          if (Number.isInteger(c) && c >= 0) net.send(cmdSpawnAnt(x, y, c));
        },
      },
    },
    { label: "Inspect ant here", onClick: () => net.send(cmdSelectAt(x, y)) },
  ];

  // Colony-scoped edits only make sense when a nest was clicked.
  if (colony !== null) {
    items.push(
      {
        label: `Add to ${store.colonyName(colony)}'s store…`,
        editor: {
          placeholder: "amount",
          initial: "40",
          apply: (v) => {
            const a = Number(v);
            if (Number.isFinite(a)) net.send(cmdAddToStore(colony, a));
          },
        },
      },
      {
        label: `Rename ${store.colonyName(colony)}…`,
        editor: {
          placeholder: "new name",
          initial: store.colonyName(colony),
          apply: (v) => {
            if (v.trim()) net.send(cmdRenameColony(colony, v.trim()));
          },
        },
      },
    );
  }
  return items;
}

function frame(): void {
  const r = ensureRenderer();
  if (r) {
    r.draw();
    const st = store.state;
    if (st.labels) labels.update(r.camera, r.viewW, r.viewH, r.dpr, store);
    else labels.setVisible(false);
    colonyPanel.update(r.camera, r.viewW, r.viewH, r.dpr, store);
    const ants = st.ants?.count ?? 0;
    overlay.textContent = st.connected
      ? `tick ${st.tick.toLocaleString()}  ·  ${ants.toLocaleString()} ants  ·  ${r.camera.zoom.toFixed(1)}x`
      : "disconnected — retrying";
  }
  requestAnimationFrame(frame);
}

net.connect();
requestAnimationFrame(frame);
