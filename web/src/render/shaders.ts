/**
 * GLSL ES 3.00 sources, plus the small amount of boilerplate needed to compile
 * them. Two draw calls total: a fullscreen quad for the field textures, and one
 * instanced quad for every ant.
 */

export const NUM_COLONY_COLORS = 8;

/** Shared: a unit quad in [0,1]^2, expanded per-instance in the vertex shader. */
export const QUAD_VERTS = new Float32Array([0, 0, 1, 0, 0, 1, 1, 1]);

// --- Field pass (terrain + pheromones) -----------------------------------

export const FIELD_VS = `#version 300 es
precision highp float;
layout(location = 0) in vec2 aQuad;

uniform mat3 uView;
uniform vec2 uWorldSize;

out vec2 vUv;

void main() {
  // The quad spans the whole world in cell coordinates.
  vec2 world = aQuad * uWorldSize;
  vUv = aQuad;
  vec3 clip = uView * vec3(world, 1.0);
  gl_Position = vec4(clip.xy, 0.0, 1.0);
}`;

/**
 * Terrain underneath, pheromones on top.
 *
 * Brightness of a pheromone channel is the value the ant's sensor reads: the
 * server already ran it through `squash_phero`. What you see is what it senses.
 *
 * Nearest-neighbour sampling everywhere. A smoothed pheromone field lies about
 * where the gradient is, and the gradient is the entire subject of this project.
 */
export const FIELD_FS = `#version 300 es
precision highp float;
precision highp sampler2D;

in vec2 vUv;
out vec4 fragColor;

uniform sampler2D uPhero;
uniform sampler2D uTerrain;
uniform vec3 uColonyColors[${NUM_COLONY_COLORS}];
uniform bool uShowFood;
uniform bool uShowAlarm;
uniform bool uShowScent;

const vec3 DIRT       = vec3(0.13, 0.11, 0.09);
const vec3 STONE      = vec3(0.38, 0.37, 0.36);
const vec3 FOOD       = vec3(0.45, 0.85, 0.35);
const vec3 FOOD_TRAIL = vec3(0.95, 0.90, 0.35);
const vec3 ALARM      = vec3(1.00, 0.20, 0.15);

vec3 colonyColor(float encoded) {
  int id = int(encoded * 255.0 + 0.5);
  // 255 means "unowned". Guard the array read: an out-of-range index in GLSL
  // is undefined behaviour, not a clamp.
  if (id < 0 || id >= ${NUM_COLONY_COLORS}) return vec3(0.0);
  return uColonyColors[id];
}

void main() {
  vec4 t = texture(uTerrain, vUv);
  vec4 p = texture(uPhero, vUv);

  // Terrain: dirt, then standing food, then stone over the top.
  vec3 col = mix(DIRT, FOOD, t.r);
  col = mix(col, STONE, t.g);

  // Nest tiles glow in their colony's colour so the operator can find home.
  if (t.b < 0.999) {
    col = mix(col, colonyColor(t.b), 0.55);
  }

  // Feature borders. Fill colour alone — dark food-green against grey stone
  // against dim dirt — reads as mud at a glance. A dark rim wherever a food,
  // stone, or nest patch abuts a different terrain type turns each patch into a
  // distinct object, the way a legend separates regions on a map. Sampling is
  // NEAREST and CLAMP_TO_EDGE, so neighbour reads are exact and the world's
  // outer border never self-outlines.
  vec2 tstep = 1.0 / vec2(textureSize(uTerrain, 0));
  vec2 offs[4] = vec2[4](vec2(1.0, 0.0), vec2(-1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, -1.0));
  float sFood  = step(0.12, t.r);
  float sStone = step(0.5, t.g);
  float sNest  = t.b < 0.999 ? 1.0 : 0.0;
  float edge = 0.0;
  for (int k = 0; k < 4; k++) {
    vec4 tn = texture(uTerrain, vUv + offs[k] * tstep);
    edge = max(edge, abs(sFood  - step(0.12, tn.r)));
    edge = max(edge, abs(sStone - step(0.5, tn.g)));
    edge = max(edge, abs(sNest  - (tn.b < 0.999 ? 1.0 : 0.0)));
  }
  col = mix(col, col * 0.28, edge);

  // Pheromones are additive: overlapping fields should read as overlapping,
  // not as whichever happened to be drawn last.
  if (uShowScent) col += colonyColor(p.a) * p.b * 0.55;
  if (uShowFood)  col += FOOD_TRAIL * p.r * 0.8;
  if (uShowAlarm) col += ALARM * p.g * 0.9;

  fragColor = vec4(min(col, vec3(1.0)), 1.0);
}`;

// --- Ant pass -------------------------------------------------------------

/**
 * One instanced quad per ant. The 8-byte wire record is the instance attribute
 * block, uploaded with no CPU-side unpacking:
 *   aPos   -> vec2 of u16, fixed-point 9.7
 *   aMeta  -> colony, size, flags, pad (4x u8)
 */
export const ANT_VS = `#version 300 es
precision highp float;
layout(location = 0) in vec2 aQuad;
layout(location = 1) in uvec2 aPos;
layout(location = 2) in uvec4 aMeta;

uniform mat3 uView;
uniform float uZoom;
uniform vec3 uColonyColors[${NUM_COLONY_COLORS}];
uniform uint uSelectedColony;
uniform vec2 uSelectedPos;
uniform bool uHasSelection;
uniform sampler2D uGlyphAtlas;   // bound here, sampled in the fragment shader
uniform int uGlyphCols;

out vec3 vColor;
out vec2 vLocal;
out float vRing;
out float vLod;        // 0 = far (glyph), 1 = near (silhouette)
out float vHeading;    // radians, decoded from aMeta.w
flat out int vColony;
flat out int vFlags;

void main() {
  vec2 world = vec2(aPos) / 128.0;

  int colony = int(aMeta.x);
  float size = float(aMeta.y) / 255.0 * 3.0;
  uint flags = aMeta.z;
  bool carrying = (flags & 1u) != 0u;
  bool attacking = (flags & 2u) != 0u;

  vec3 col = (colony >= 0 && colony < ${NUM_COLONY_COLORS})
    ? uColonyColors[colony]
    : vec3(0.7);

  // A carrying ant is visibly loaded; an attacking one flashes hot. Both are
  // states the operator is specifically looking for.
  if (carrying) col = mix(col, vec3(0.95, 0.95, 0.4), 0.55);
  if (attacking) col = mix(col, vec3(1.0, 0.25, 0.1), 0.75);
  vColor = col;

  // Never smaller than ~2 physical pixels: a zoomed-out world of 10k ants must
  // still show where the ants are, not a field of invisible sub-pixel dots.
  float radiusCells = max(0.5 * size, 1.2 / uZoom);
  if (attacking) radiusCells *= 1.15;   // a fighting ant reads slightly larger

  vLocal = aQuad * 2.0 - 1.0;
  vRing = (uHasSelection && distance(world, uSelectedPos) < 0.01) ? 1.0 : 0.0;
  if (vRing > 0.5) radiusCells *= 2.0;

  // Pixel radius drives the level-of-detail swap: uZoom is device pixels per
  // world cell, so radiusCells * uZoom is the ant's on-screen radius.
  float pxRadius = radiusCells * uZoom;
  vLod = smoothstep(3.0, 5.0, pxRadius);   // ~6px..~10px diameter blend zone

  // Canonical decode, mirroring headingByteToRadians in sprites.ts and the
  // Rust encoder: angle = byte/255 * 2PI - PI.
  vHeading = float(aMeta.w) / 255.0 * 6.2831853 - 3.14159265;
  vColony = colony;
  vFlags = int(flags);

  vec2 corner = world + vLocal * radiusCells;
  vec3 clip = uView * vec3(corner, 1.0);
  gl_Position = vec4(clip.xy, 0.0, 1.0);
}`;

export const ANT_FS = `#version 300 es
precision highp float;

in vec3 vColor;
in vec2 vLocal;
in float vRing;
in float vLod;
in float vHeading;
flat in int vColony;
flat in int vFlags;
uniform sampler2D uGlyphAtlas;
uniform int uGlyphCols;
out vec4 fragColor;

// Union of three circles along the +y (heading-forward) axis: abdomen, thorax,
// head. Returns soft coverage in [0,1]. p is in the rotated unit square.
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

  // A touch of edge darkening so overlapping ants remain countable.
  fragColor = vec4(col * (1.0 - 0.35 * r * r), 1.0);
}`;

// --- Compilation ----------------------------------------------------------

export function compile(gl: WebGL2RenderingContext, vs: string, fs: string): WebGLProgram {
  const program = gl.createProgram();
  if (!program) throw new Error("cannot create program");

  for (const [type, src] of [
    [gl.VERTEX_SHADER, vs],
    [gl.FRAGMENT_SHADER, fs],
  ] as const) {
    const sh = gl.createShader(type);
    if (!sh) throw new Error("cannot create shader");
    gl.shaderSource(sh, src);
    gl.compileShader(sh);
    if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
      const log = gl.getShaderInfoLog(sh);
      gl.deleteShader(sh);
      throw new Error(`shader compile failed: ${log}`);
    }
    gl.attachShader(program, sh);
    gl.deleteShader(sh);
  }

  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    const log = gl.getProgramInfoLog(program);
    gl.deleteProgram(program);
    throw new Error(`program link failed: ${log}`);
  }
  return program;
}
