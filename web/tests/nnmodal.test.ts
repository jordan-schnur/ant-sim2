// @vitest-environment jsdom
import { describe, expect, it, beforeEach, vi } from "vitest";
import { openNN } from "../src/ui/nnmodal.js";
import { N_INPUTS } from "../src/protocol.js";

function fakeStore(detail: unknown) {
  const subs: Array<() => void> = [];
  return {
    state: { detail, genome: { params: new Float32Array(1) }, paused: true, tick: 42 },
    subscribe(fn: () => void) { subs.push(fn); return () => {}; },
    _emit() { for (const f of subs) f(); },
  } as any;
}
function fakeDetail() {
  return {
    alive: true, id: 1,
    inputs: new Float32Array(N_INPUTS).fill(0.5),
    h1: new Float32Array(16), h2: new Float32Array(16),
    outputs: new Float32Array(8),
  };
}

describe("openNN", () => {
  beforeEach(() => { document.body.innerHTML = ""; });

  it("mounts a backdrop with a panel and closes on Escape", () => {
    const net = { send: vi.fn() } as any;
    openNN(fakeStore(fakeDetail()), net);
    expect(document.querySelector(".nn-backdrop")).not.toBeNull();
    expect(document.querySelector(".nn-modal-panel")).not.toBeNull();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(document.querySelector(".nn-backdrop")).toBeNull();
  });

  it("lists a row per input", () => {
    openNN(fakeStore(fakeDetail()), { send: vi.fn() } as any);
    expect(document.querySelectorAll(".nnm-row").length).toBe(N_INPUTS + 8);
  });

  it("step button sends cmdStep and pauses", () => {
    const net = { send: vi.fn() } as any;
    openNN(fakeStore(fakeDetail()), net);
    (document.querySelector(".nnm-step") as HTMLElement).click();
    // cmdSetPaused(true) + cmdStep() → two sends.
    expect(net.send).toHaveBeenCalled();
  });

  it("is safe with no ant selected", () => {
    openNN(fakeStore(null), { send: vi.fn() } as any);
    expect(document.querySelector(".nn-backdrop")).not.toBeNull();
    expect(document.body.textContent).toContain("select an ant");
  });
});
