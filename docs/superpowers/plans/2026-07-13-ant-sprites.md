# Ant Sprites Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the flat circle ants with procedural sprites that show colony identity and live state (facing, carrying, fighting) at a glance, using a level-of-detail swap in the existing instanced ant shader.

**Architecture:** The per-ant wire record stays 8 bytes; its unused pad byte becomes a u8 heading. The ant fragment shader draws the colony's glyph (from a startup-generated mask atlas) when the ant is small on screen and a heading-oriented three-segment silhouette when it is large, cross-fading between them. Colony color, carrying, and fighting overlays ride on top at both levels of detail.

**Tech Stack:** Rust (sim/server, `cargo test`), TypeScript + WebGL2 GLSL ES 3.00 (web client, `vitest` for unit tests, `tsc`/`vite` for the build).

## Global Constraints

- Work in the worktree at `/Users/jschnur/dev/antsim2-ant-sprites` on branch `feat/ant-sprites`. All commands and paths are relative to that worktree root.
- The per-ant wire record MUST stay exactly 8 bytes, GPU-aligned. No new instance attributes; reuse the pad byte only.
- The ant pass MUST remain a single `gl.drawArraysInstanced` call with `raw` uploaded to the vertex buffer without CPU-side unpacking.
- Heading encoding is canonical and shared between Rust and GLSL: `byte = round((angle + PI) / (2*PI) * 255)`, decode `angle = byte / 255 * 2*PI - PI`, with `angle ∈ [-PI, PI)`.
- There are 8 colony glyphs; the atlas has 8 cells; colony→cell is `colony % 8` (mirrors `symbolFor` in `web/src/symbols.ts`).
- No hand-drawn art assets. Shapes are drawn from code (`drawSymbol`) or evaluated analytically in the shader.
- Commit messages end with the repo's co-author trailer:
  `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`

---

### Task 1: Encode heading into the ant wire record (Rust)

**Files:**
- Modify: `crates/server/src/protocol.rs` (`encode_ants`, ~lines 277-306; test module near line 640)

**Interfaces:**
- Consumes: `crates/sim/src/apply.rs::wrap_angle(f32) -> f32` (already `pub`); `w.ants.heading: Vec<f32>` (already wrapped to `[-PI, PI)`).
- Produces: byte 7 of each 8-byte ant record now carries heading as u8 per the canonical encoding. Frame layout unchanged otherwise: 13-byte header (`TAG_ANTS` u8 + tick u64 + count u32), then 8 bytes per live ant; field byte for ant `i` = `13 + 8*i + offset` (qx=0, qy=2, colony=4, size=5, flags=6, heading=7).

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `crates/server/src/protocol.rs`, after `carrying_and_attacking_set_independent_flag_bits`:

```rust
    #[test]
    fn heading_is_encoded_in_the_record_pad_byte() {
        use std::f32::consts::PI;
        let mut w = World::new(&small(), 1);
        // A heading of 0 (facing +x) sits at the middle of the u8 range.
        w.ants.heading[0] = 0.0;
        // -PI is the low end of the wrapped range -> byte 0.
        w.ants.heading[1] = -PI;
        // Just under +PI is the high end -> byte 255.
        w.ants.heading[2] = PI - 0.0001;

        let mut b = Vec::new();
        encode_ants(&mut b, &w);
        let heading_byte = |i: usize| b[13 + 8 * i + 7];

        assert_eq!(heading_byte(0), 128, "0 rad maps to mid-range");
        assert_eq!(heading_byte(1), 0, "-PI maps to 0");
        assert_eq!(heading_byte(2), 255, "just under +PI maps to 255");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p server heading_is_encoded_in_the_record_pad_byte`
Expected: FAIL — the pad byte is currently `0`, so `heading_byte(0)` is `0`, not `128`.

- [ ] **Step 3: Write the minimal implementation**

In `encode_ants`, replace the trailing pad write. Change:

```rust
        put_u8(out, flags);
        put_u8(out, 0); // pad, keeps the record 8-byte aligned for the GPU
```

to:

```rust
        put_u8(out, flags);
        // Heading rides in what was the pad byte: the record stays 8 bytes and
        // GPU-aligned. Canonical mapping (shared with the ant shader):
        // byte = (angle + PI) / (2*PI) * 255, angle in [-PI, PI).
        let a = sim::apply::wrap_angle(w.ants.heading[i]);
        let h = ((a + std::f32::consts::PI) / (2.0 * std::f32::consts::PI) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8;
        put_u8(out, h);
```

If `sim::apply::wrap_angle` is not resolvable from that path, add `use sim::apply::wrap_angle;` at the top of the file and call `wrap_angle(...)`; confirm the module path with `grep -n "pub fn wrap_angle" crates/sim/src/apply.rs` and `grep -n "pub mod apply" crates/sim/src/lib.rs`.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p server heading_is_encoded_in_the_record_pad_byte`
Expected: PASS.

- [ ] **Step 5: Run the full protocol test module to confirm nothing regressed**

Run: `cargo test -p server`
Expected: PASS (existing `carrying_and_attacking_...`, `position_survives_...`, etc. still green).

- [ ] **Step 6: Commit**

```bash
git add crates/server/src/protocol.rs
git commit -m "feat(server): encode ant heading in the wire pad byte

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Pure sprite helpers — atlas layout + heading mapping (TypeScript)

**Files:**
- Create: `web/src/render/sprites.ts`
- Create: `web/src/render/sprites.test.ts`

**Interfaces:**
- Consumes: `SHAPES` from `web/src/symbols.ts` (the 8-shape tuple).
- Produces:
  - `GLYPH_ATLAS_COLS: number` — `8`, the cell count.
  - `glyphCellRect(index: number, cell: number): { x: number; y: number; w: number; h: number }` — the pixel rect of atlas cell `index` given a square `cell` size; cells laid out left-to-right in one row.
  - `headingByteToRadians(b: number): number` — canonical decode, the exact inverse of the Rust encode; used as the documented-of-record formula the shader mirrors.
  - `radiansToHeadingByte(a: number): number` — canonical encode in TS, for round-trip testing against the decode.

- [ ] **Step 1: Write the failing test**

Create `web/src/render/sprites.test.ts`:

```typescript
import { describe, expect, it } from "vitest";
import { SHAPES } from "../symbols.js";
import {
  GLYPH_ATLAS_COLS,
  glyphCellRect,
  headingByteToRadians,
  radiansToHeadingByte,
} from "./sprites.js";

describe("glyph atlas layout", () => {
  it("has one cell per colony shape", () => {
    expect(GLYPH_ATLAS_COLS).toBe(SHAPES.length);
    expect(GLYPH_ATLAS_COLS).toBe(8);
  });

  it("lays cells out left to right in a single row", () => {
    expect(glyphCellRect(0, 32)).toEqual({ x: 0, y: 0, w: 32, h: 32 });
    expect(glyphCellRect(3, 32)).toEqual({ x: 96, y: 0, w: 32, h: 32 });
  });
});

describe("heading mapping", () => {
  it("decodes the range endpoints to a wrapped angle", () => {
    expect(headingByteToRadians(0)).toBeCloseTo(-Math.PI, 5);
    expect(headingByteToRadians(128)).toBeCloseTo(Math.PI / 255, 2); // ~0
  });

  it("round-trips a byte through encode(decode(b))", () => {
    for (const b of [0, 1, 64, 128, 200, 255]) {
      expect(radiansToHeadingByte(headingByteToRadians(b))).toBe(b);
    }
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd web && npm test -- sprites`
Expected: FAIL — `./sprites.js` does not exist yet (module resolution error).

- [ ] **Step 3: Write the minimal implementation**

Create `web/src/render/sprites.ts`:

```typescript
/**
 * Pure geometry/encoding helpers for the ant sprite pass. Kept free of WebGL
 * and canvas so they are unit-testable in node; the canvas atlas builder that
 * consumes `glyphCellRect` lives in `world.ts` where a real 2D context exists.
 */
import { SHAPES } from "../symbols.js";

/** One atlas cell per colony glyph; colony -> cell is `colony % GLYPH_ATLAS_COLS`. */
export const GLYPH_ATLAS_COLS = SHAPES.length;

/** Pixel rect of atlas cell `index` for a square `cell` size, one row. */
export function glyphCellRect(
  index: number,
  cell: number,
): { x: number; y: number; w: number; h: number } {
  return { x: index * cell, y: 0, w: cell, h: cell };
}

const TWO_PI = Math.PI * 2;

/**
 * Canonical heading decode — the exact inverse of the Rust encoder in
 * `crates/server/src/protocol.rs::encode_ants`. The ant vertex shader MUST use
 * this same formula; this function is the documented source of record for it.
 */
export function headingByteToRadians(b: number): number {
  return (b / 255) * TWO_PI - Math.PI;
}

/** Canonical heading encode in TS, mirroring the Rust side. */
export function radiansToHeadingByte(a: number): number {
  // Wrap into [-PI, PI) the same way `wrap_angle` does, then quantize.
  let w = a;
  while (w >= Math.PI) w -= TWO_PI;
  while (w < -Math.PI) w += TWO_PI;
  return Math.round(((w + Math.PI) / TWO_PI) * 255);
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd web && npm test -- sprites`
Expected: PASS (5 assertions across 4 tests).

- [ ] **Step 5: Typecheck**

Run: `cd web && npm run typecheck`
Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add web/src/render/sprites.ts web/src/render/sprites.test.ts
git commit -m "feat(web): pure glyph-atlas layout and heading-mapping helpers

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Build and bind the glyph mask atlas in the renderer

**Files:**
- Modify: `web/src/render/world.ts` (`WorldRenderer` — add atlas build in constructor, a texture field, and a bind + uniforms in the ant pass of `draw`, ~lines 214-239)
- Modify: `web/src/render/shaders.ts` (add the atlas sampler + glyph-cols uniforms and heading decode as a varying; keep the fragment shader drawing circles for now)

**Interfaces:**
- Consumes: `GLYPH_ATLAS_COLS`, `glyphCellRect` from `web/src/render/sprites.ts`; `SHAPES`, `drawSymbol` from `web/src/symbols.ts`.
- Produces: `WorldRenderer` binds a `uGlyphAtlas` texture (unit 2) and sets `uGlyphCols` in the ant pass; `ANT_VS` decodes `aMeta.w` into a `vHeading` varying and emits `vLod`, `flat vColony`, `flat vFlags`. Circles still render (no visible change yet), so this task is independently verifiable.

- [ ] **Step 1: Add the atlas builder method and texture field to `WorldRenderer`**

At the top of `web/src/render/world.ts`, add to the existing imports:

```typescript
import { GLYPH_ATLAS_COLS, glyphCellRect } from "./sprites.js";
import { SHAPES, drawSymbol } from "../symbols.js";
```

Add a field beside the other textures (near line 24):

```typescript
  private glyphTex: WebGLTexture;
```

Add this private method (place it after `makeTexture`):

```typescript
  /**
   * A one-row mask atlas of the 8 colony glyphs, drawn from the same
   * `drawSymbol` the cards and labels use, so shape is defined once. White on
   * transparent; the shader multiplies by colony color. 64px cells give crisp
   * edges when an ant is only a few pixels on screen.
   */
  private makeGlyphAtlas(): WebGLTexture {
    const cell = 64;
    const canvas = document.createElement("canvas");
    canvas.width = cell * GLYPH_ATLAS_COLS;
    canvas.height = cell;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("cannot get 2d context for the glyph atlas");
    for (let i = 0; i < GLYPH_ATLAS_COLS; i++) {
      const r = glyphCellRect(i, cell);
      // Inset a little so the shape does not touch the cell edge and bleed
      // into its neighbour when sampled.
      drawSymbol(ctx, SHAPES[i], r.x + cell / 2, r.y + cell / 2, cell * 0.38, "#ffffff");
    }

    const gl = this.gl;
    const t = gl.createTexture();
    if (!t) throw new Error("cannot create glyph atlas texture");
    gl.bindTexture(gl.TEXTURE_2D, t);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, gl.RGBA, gl.UNSIGNED_BYTE, canvas);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    return t;
  }
```

In the constructor, after `this.terrainTex = this.makeTexture();`, add:

```typescript
    this.glyphTex = this.makeGlyphAtlas();
```

- [ ] **Step 2: Bind the atlas and set its uniforms in the ant pass**

In `draw`, inside the `if (st.ants && st.ants.count > 0)` block, after `gl.useProgram(this.antProg);` and `gl.bindVertexArray(this.antVao);`, add the bind:

```typescript
      gl.activeTexture(gl.TEXTURE2);
      gl.bindTexture(gl.TEXTURE_2D, this.glyphTex);
```

Then, alongside the other `au(...)` uniform sets, add:

```typescript
      gl.uniform1i(au("uGlyphAtlas"), 2);
      gl.uniform1i(au("uGlyphCols"), GLYPH_ATLAS_COLS);
```

- [ ] **Step 3: Add the uniforms and heading varying to the vertex shader**

In `web/src/render/shaders.ts`, in `ANT_VS`, add these uniforms after `uniform bool uHasSelection;`:

```glsl
uniform sampler2D uGlyphAtlas;   // bound but sampled in the fragment shader
uniform int uGlyphCols;
```

Add these outputs after `out float vRing;`:

```glsl
out float vLod;        // 0 = far (glyph), 1 = near (silhouette)
out float vHeading;    // radians, decoded from aMeta.w
flat out int vColony;
flat out int vFlags;
```

Just before `vec2 corner = world + vLocal * radiusCells;`, add:

```glsl
  // Pixel radius drives the level-of-detail swap: uZoom is device pixels per
  // world cell, so radiusCells * uZoom is the ant's on-screen radius.
  float pxRadius = radiusCells * uZoom;
  vLod = smoothstep(3.0, 5.0, pxRadius);   // ~6px..~10px diameter blend zone

  // Canonical decode, mirroring headingByteToRadians in sprites.ts and the
  // Rust encoder: angle = byte/255 * 2PI - PI.
  vHeading = float(aMeta.w) / 255.0 * 6.2831853 - 3.14159265;
  vColony = colony;
  vFlags = int(flags);
```

- [ ] **Step 4: Add matching inputs to the fragment shader (still drawing circles)**

In `ANT_FS`, add after `in float vRing;`:

```glsl
in float vLod;
in float vHeading;
flat in int vColony;
flat in int vFlags;
uniform sampler2D uGlyphAtlas;
uniform int uGlyphCols;
```

Leave the `main()` body unchanged for now (still the circle). Referencing the new inputs is not required for it to compile; unused varyings/uniforms are legal.

- [ ] **Step 5: Build to verify the shaders still compile and link**

Run: `cd web && npm run build`
Expected: `tsc --noEmit` passes and `vite build` succeeds. (Shader compile/link happens at runtime, so also confirm no TypeScript errors from the new imports/fields.)

- [ ] **Step 6: Commit**

```bash
git add web/src/render/world.ts web/src/render/shaders.ts
git commit -m "feat(web): build the glyph mask atlas and plumb sprite uniforms

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: LOD fragment shader — colony glyph, oriented silhouette, state overlays

**Files:**
- Modify: `web/src/render/shaders.ts` (`ANT_FS` `main()` body; small `ANT_VS` size-bump for attacking)

**Interfaces:**
- Consumes: `vColor`, `vLocal`, `vRing`, `vLod`, `vHeading`, `vColony`, `vFlags`, `uGlyphAtlas`, `uGlyphCols` (from Task 3).
- Produces: final ant appearance — far LOD samples the colony glyph, near LOD draws a heading-oriented three-segment silhouette, cross-faded by `vLod`; carrying draws a load dot (near) or keeps the existing yellow tint (already folded into `vColor`); the selection halo is preserved.

- [ ] **Step 1: Add the attacking size bump in the vertex shader**

In `ANT_VS`, immediately after the line `float radiusCells = max(0.5 * size, 1.2 / uZoom);`, add:

```glsl
  if (attacking) radiusCells *= 1.15;   // a fighting ant reads slightly larger
```

- [ ] **Step 2: Replace the fragment shader `main()` with the LOD body**

In `ANT_FS`, replace the entire `void main() { ... }` with:

```glsl
// Union of three circles along the +y (heading-forward) axis: abdomen, thorax,
// head. Returns soft coverage in [0,1]. `p` is in the rotated unit square.
float antSilhouette(vec2 p) {
  float d = length(p - vec2(0.0, -0.45)) - 0.50; // abdomen
  d = min(d, length(p - vec2(0.0,  0.05)) - 0.32); // thorax
  d = min(d, length(p - vec2(0.0,  0.50)) - 0.26); // head
  return smoothstep(0.06, -0.06, d);
}

void main() {
  // Glyph (far): sample the colony's cell from the atlas. vLocal is [-1,1].
  vec2 cellUv = vLocal * 0.5 + 0.5;
  float cell = float(vColony - (vColony / uGlyphCols) * uGlyphCols); // colony % cols
  float u = (cell + cellUv.x) / float(uGlyphCols);
  float glyphA = texture(uGlyphAtlas, vec2(u, cellUv.y)).a;

  // Silhouette (near): rotate local coords by -heading so +y points along it.
  float s = sin(-vHeading), c = cos(-vHeading);
  vec2 rp = mat2(c, -s, s, c) * vLocal;
  float silA = antSilhouette(rp);

  float alpha = mix(glyphA, silA, vLod);
  if (alpha < 0.5 && vRing < 0.5) discard;

  vec3 col = vColor;

  // Carrying load dot at the head, only meaningful near; fade in with vLod.
  bool carrying = (vFlags & 1) != 0;
  if (carrying) {
    float dot = smoothstep(0.20, 0.10, length(rp - vec2(0.0, 0.5)));
    col = mix(col, vec3(1.0, 0.95, 0.55), dot * vLod);
  }

  // Selection halo (unchanged behaviour): a white ring on the outer edge.
  float r = length(vLocal);
  if (vRing > 0.5 && r > 0.62) {
    fragColor = vec4(1.0, 1.0, 1.0, 1.0);
    return;
  }

  // Edge darkening keeps overlapping ants countable, as before.
  fragColor = vec4(col * (1.0 - 0.35 * r * r), 1.0);
}
```

- [ ] **Step 3: Build to verify it compiles and typechecks**

Run: `cd web && npm run build`
Expected: passes.

- [ ] **Step 4: Re-run the unit suites to confirm no regression**

Run: `cd web && npm test` then `cargo test -p server`
Expected: both PASS (Tasks 1-2 tests still green).

- [ ] **Step 5: Manual visual verification (via the `/run` skill)**

Launch the app, then confirm each item. Record pass/fail in the commit body if any needs a threshold tweak (thresholds live in `ANT_VS`: `smoothstep(3.0, 5.0, pxRadius)`).

1. **Zoom out on a swarm** → ants render as their colony's glyph (triangle/diamond/star/…), visibly distinct between colonies even squinting/in grayscale.
2. **Zoom in** → ants become three-segment silhouettes pointing along their direction of travel; a moving column reads as directional flow.
3. **A foraging ant carrying food** → a bright load dot at its head.
4. **A border skirmish** → fighting ants flush red and read slightly larger.
5. **Slowly zoom across the threshold** → glyph↔silhouette cross-fades with no hard pop; selection halo still works on a clicked ant.

- [ ] **Step 6: Commit**

```bash
git add web/src/render/shaders.ts
git commit -m "feat(web): LOD ant sprites — colony glyphs and oriented silhouettes

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Notes for the implementer

- **Threshold tuning is expected.** The `smoothstep(3.0, 5.0, pxRadius)` far/near band and the silhouette segment radii are starting values from the spec's "open tuning parameters"; adjust them during Task 4 Step 5 and note any change in the commit. Do not treat a needed tweak as a failure.
- **If shader linking fails at runtime** with an unused-uniform or varying-mismatch error, confirm every `out` in `ANT_VS` has a matching `in` in `ANT_FS` with the identical type and `flat` qualifier — WebGL2 is strict about this.
- **Integer modulo in GLSL ES 3.00** is available (`%`), but the plan uses the `x - (x/cols)*cols` form in Step 2 of Task 4 to avoid any driver quirks with `%` on signed ints; either is acceptable if the other is preferred.
```
