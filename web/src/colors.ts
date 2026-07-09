/**
 * Colony colors are a client concern: the server ships colony ids and the
 * shader looks the color up. Eight hues, evenly spaced and chosen to stay
 * distinguishable against the dark terrain and against each other when two
 * colonies' scent fields meet.
 */

export const COLONY_COLORS: readonly [number, number, number][] = [
  [0.95, 0.35, 0.30], // red
  [0.35, 0.70, 0.95], // blue
  [0.55, 0.85, 0.40], // green
  [0.98, 0.75, 0.25], // amber
  [0.80, 0.45, 0.95], // violet
  [0.30, 0.88, 0.80], // teal
  [0.98, 0.55, 0.75], // pink
  [0.70, 0.70, 0.45], // olive
];

export function colonyColor(id: number): [number, number, number] {
  return COLONY_COLORS[id % COLONY_COLORS.length];
}

export function colonyCss(id: number, alpha = 1): string {
  const [r, g, b] = colonyColor(id);
  const q = (v: number) => Math.round(v * 255);
  return `rgba(${q(r)}, ${q(g)}, ${q(b)}, ${alpha})`;
}

/** Flattened RGB triples for a shader uniform array. */
export function colonyPalette(): Float32Array {
  const a = new Float32Array(COLONY_COLORS.length * 3);
  COLONY_COLORS.forEach(([r, g, b], i) => {
    a[i * 3] = r;
    a[i * 3 + 1] = g;
    a[i * 3 + 2] = b;
  });
  return a;
}
