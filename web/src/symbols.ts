/**
 * Per-colony glyph shapes. Colour alone fails colourblind viewers, so every
 * colony also gets a distinct shape (the shapez-2 trick). Eight shapes for the
 * eight colonies; the mapping wraps if there are ever more.
 */

export const SHAPES = [
  "circle", "triangle", "square", "diamond", "plus", "star", "hexagon", "cross",
] as const;
export type SymbolShape = (typeof SHAPES)[number];

export function symbolFor(colonyId: number): SymbolShape {
  return SHAPES[colonyId % SHAPES.length];
}

/** Draw a filled glyph centered at (x, y). Canvas — used on cards and labels. */
export function drawSymbol(
  ctx: CanvasRenderingContext2D,
  shape: SymbolShape,
  x: number,
  y: number,
  r: number,
  color: string,
): void {
  ctx.fillStyle = color;
  ctx.strokeStyle = color;
  ctx.lineWidth = Math.max(1, r * 0.35);
  ctx.beginPath();
  switch (shape) {
    case "circle":
      ctx.arc(x, y, r, 0, Math.PI * 2);
      ctx.fill();
      break;
    case "square":
      ctx.fillRect(x - r, y - r, 2 * r, 2 * r);
      break;
    case "triangle":
      ctx.moveTo(x, y - r);
      ctx.lineTo(x + r, y + r);
      ctx.lineTo(x - r, y + r);
      ctx.closePath();
      ctx.fill();
      break;
    case "diamond":
      ctx.moveTo(x, y - r);
      ctx.lineTo(x + r, y);
      ctx.lineTo(x, y + r);
      ctx.lineTo(x - r, y);
      ctx.closePath();
      ctx.fill();
      break;
    case "plus":
      ctx.fillRect(x - r * 0.35, y - r, r * 0.7, 2 * r);
      ctx.fillRect(x - r, y - r * 0.35, 2 * r, r * 0.7);
      break;
    case "cross":
      ctx.moveTo(x - r, y - r);
      ctx.lineTo(x + r, y + r);
      ctx.moveTo(x + r, y - r);
      ctx.lineTo(x - r, y + r);
      ctx.stroke();
      break;
    case "hexagon":
      for (let k = 0; k < 6; k++) {
        const a = (Math.PI / 3) * k - Math.PI / 6;
        const px = x + r * Math.cos(a);
        const py = y + r * Math.sin(a);
        k === 0 ? ctx.moveTo(px, py) : ctx.lineTo(px, py);
      }
      ctx.closePath();
      ctx.fill();
      break;
    case "star":
      for (let k = 0; k < 10; k++) {
        const rr = k % 2 === 0 ? r : r * 0.45;
        const a = (Math.PI / 5) * k - Math.PI / 2;
        const px = x + rr * Math.cos(a);
        const py = y + rr * Math.sin(a);
        k === 0 ? ctx.moveTo(px, py) : ctx.lineTo(px, py);
      }
      ctx.closePath();
      ctx.fill();
      break;
  }
}
