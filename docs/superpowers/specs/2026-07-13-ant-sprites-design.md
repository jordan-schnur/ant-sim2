# Ant sprites: colony identity and attributes at a glance

**Status:** approved design, ready for implementation plan
**Date:** 2026-07-13
**Branch:** `feat/ant-sprites`

## Goal

Replace the flat circle ants with procedural sprites that read, at a glance,
as *which colony an ant belongs to* and *what it is doing right now* — colony,
facing, carrying food, and fighting — without clicking the ant. Keep the
existing performance model: one instanced WebGL2 draw call for all ants,
`raw` uploaded straight into the vertex buffer with no CPU unpacking, scaling
to 10k+ ants.

## Non-goals

- Hand-drawn / illustrated / pixel-art assets. Shapes are drawn from code.
- A texture-atlas art pipeline (per-colony painted variants).
- Legs and antennae on the silhouette (sub-pixel noise at normal zoom).
- Shader-pixel snapshot tests (the repo does not use them; brittle).
- New per-ant attributes beyond what the sim already tracks.

## Approach

Procedural shapes drawn in the existing ant shader, with a level-of-detail
(LOD) swap driven by the ant's on-screen pixel size:

- **Far (dense swarm):** each ant is its colony's **glyph** — the same
  `triangle / square / diamond / plus / star / hexagon / cross / circle`
  shapes already used on cards and labels — colored by the colony palette.
  This keeps colony identity colorblind-safe when an ant is only a few pixels
  and a silhouette's detail is invisible.
- **Near (inspecting):** each ant is a **three-segment silhouette** (head +
  thorax + abdomen) oriented to its heading, colored by colony.
- **Blend zone:** cross-fade between the two so the swap does not visibly pop.

State overlays ride on top at both LODs: carrying, fighting, size, selection.

## Section 1 — Data flow & wire format

The only wire change: spend the currently-unused pad byte on heading. The
record stays 8 bytes, so bandwidth and GPU alignment are unchanged.

```
byte:  0    1    2    3    4        5      6        7
       qx (u16) qy (u16)  colony   size   flags    heading
                                                    (was pad = 0)
```

- The sim already tracks a per-ant heading (`Spawn.heading`; ants move along
  it; `wrap_angle` keeps it in `[-π, π)`), so no simulation change is needed —
  only the encoder reads a field it already has.
- **Encode** (`crates/server/src/protocol.rs`, `encode_ants`): replace the
  `put_u8(out, 0)` pad with
  `heading_u8 = ((wrap_angle(h) + PI) / (2*PI) * 255.0).round().clamp(0,255)`.
  256 directions ≈ 1.4° resolution, ample for a silhouette.
- **Client:** the VAO already reads bytes 4–7 as `aMeta` (`uvec4`), so
  `aMeta.w` is heading with **no JS or buffer-layout change**. Only the shader
  starts reading the 4th meta byte. Decode back to radians in the vertex
  shader: `angle = float(aMeta.w) / 255.0 * 2*PI - PI`.

Everything else in the pipeline is untouched: still one `drawArraysInstanced`
call, still no CPU-side unpacking of `raw`.

## Section 2 — Rendering: LOD in the ant shader

All new logic lives in `web/src/render/shaders.ts` (`ANT_VS` / `ANT_FS`) plus
the atlas wiring in `web/src/render/world.ts`.

- The vertex shader already computes an on-screen radius (`radiusCells`,
  `uZoom`). Derive an approximate pixel size from it and pass an LOD factor
  `0..1` (far→near) to the fragment shader as a varying.
- **Far (ant ≲ ~6 device px):** sample the colony glyph from the mask atlas
  (Section 3), multiply by colony color. No rotation.
- **Near (ant ≳ ~10 px):** evaluate the three-segment silhouette SDF oriented
  to heading, colored by colony.
- **Blend zone (~6–10 px):** `mix` the two contributions by the LOD factor.

Thresholds (6 px / 10 px) are starting values to tune during the visual pass;
they live as named constants, not magic numbers scattered through the shader.

Selection halo behavior is preserved as-is.

## Section 3 — Glyph shapes: startup-generated mask atlas

The 8 glyphs reach the shader as a **mask atlas generated once at renderer
construction from the existing `drawSymbol()`**, not as analytic GLSL SDFs.

- At construction, draw each `SHAPES[i]` (`web/src/symbols.ts`) as a
  white-on-transparent mask into one row of 8 cells on an offscreen canvas,
  and upload it as a single texture. The far-LOD path samples cell
  `colony % 8`.
- **Rationale:** one source of truth for shape — cards, labels, and the world
  all render from `drawSymbol()`. Changing a shape once changes it everywhere,
  and the star/hexagon are not reimplemented (and cannot drift) in GLSL. This
  still honors "no art assets": the atlas is a code-drawn mask, not a painted
  PNG.
- **Cost / tradeoff:** one extra texture, a ~50-line atlas builder, and one
  texture bind in the ant pass. A sampled mask has softer edges than an
  analytic SDF, but at 3–6 px that is invisible.

The **silhouette stays an analytic SDF** (three ellipses/capsules along the
heading axis), *not* atlased, because it must rotate per-ant by heading —
trivial to rotate `vLocal` in the shader, awkward to rotate atlas UVs cleanly.

Net: glyphs = generated mask atlas; silhouette = analytic SDF. Two small code
paths, each doing what it is good at.

## Section 4 — State overlays & size

- **Size** (`aMeta.y`): unchanged — scales `radiusCells`, so bigger ants read
  bigger at both LODs.
- **Carrying** (`flags & FLAG_CARRYING`): near LOD, a bright load dot at the
  head end of the silhouette; far LOD, fall back to today's yellow tint (a dot
  is sub-pixel there).
- **Fighting** (`flags & FLAG_ATTACKING`): body flushes hot red (reuse the
  current `mix(col, red, 0.75)`) plus a small size bump, at both LODs.
- **Precedence** when both bits are set: attacking wins the body color; the
  carrying dot still draws on top. Fighting is the state the operator most
  needs to catch.

## Section 5 — Testing

Match the repo's existing approach; do not invent a GPU test harness.

- **Rust unit** (`protocol.rs` test module): a known `heading` round-trips to
  the expected byte-7 value, and the per-ant record is still 8 bytes.
- **Client unit:** extract the pure helpers out of the shader/renderer —
  the `heading_u8 → radians` decode and the atlas cell count — and test the
  byte→radians mapping and that the atlas has 8 cells.
- **Manual visual checklist** (run via the `/run` skill once built):
  1. Zoom out on a swarm → colony-distinct glyphs, distinguishable in
     grayscale (colorblind check).
  2. Zoom in → silhouettes oriented along movement; a moving trail reads as
     directional.
  3. A foraging ant carrying food shows the load dot.
  4. A border skirmish shows red fighting ants.
  5. The far↔near swap cross-fades without a visible pop.

## Affected files

- `crates/server/src/protocol.rs` — `encode_ants` heading byte; new unit test.
- `web/src/render/shaders.ts` — LOD, glyph sampling, silhouette SDF, heading
  decode, state overlays.
- `web/src/render/world.ts` — build + upload the glyph mask atlas; bind it in
  the ant pass; new atlas/decode helpers (kept pure for testing).
- `web/src/symbols.ts` — reused as-is (source of truth for glyph shapes); no
  change expected unless the atlas builder needs a mask-drawing entry point.

## Open tuning parameters (decide during the visual pass, not blockers)

- Far/near pixel thresholds and blend-zone width.
- Silhouette proportions (head/thorax/abdomen sizes) and load-dot size.
- Attacking size-bump magnitude.
