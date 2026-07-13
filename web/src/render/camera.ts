/**
 * Pan and zoom, as a view matrix uniform. The camera costs nothing: no geometry
 * is touched, only a 3x3 upload per frame.
 *
 * World space is cells: (0,0) at the top-left corner of the grid, (w,h) at the
 * bottom-right. Clip space is the usual [-1,1], y up. The flip lives here so
 * that everything else — ant positions, pheromone texels, mouse picks — can
 * agree that y increases downward, as the grid's row-major indexing does.
 */

export const MIN_ZOOM = 0.5;
export const MAX_ZOOM = 64;

export class Camera {
  /** Pixels per cell. */
  zoom = 1;
  /** World-space cell at the centre of the viewport. */
  cx = 0;
  cy = 0;

  constructor(
    public worldW: number,
    public worldH: number,
  ) {
    this.cx = worldW / 2;
    this.cy = worldH / 2;
  }

  /** Fit the whole world into a viewport, with a little margin. */
  fit(viewW: number, viewH: number): void {
    this.zoom = Math.min(viewW / this.worldW, viewH / this.worldH) * 0.95;
    this.cx = this.worldW / 2;
    this.cy = this.worldH / 2;
  }

  /** Zoom about a fixed screen point, so the cell under the cursor stays put. */
  zoomAt(screenX: number, screenY: number, factor: number, viewW: number, viewH: number): void {
    const before = this.screenToWorld(screenX, screenY, viewW, viewH);
    this.zoom = clamp(this.zoom * factor, MIN_ZOOM, MAX_ZOOM);
    const after = this.screenToWorld(screenX, screenY, viewW, viewH);
    // Shift the centre by however far the anchor cell drifted.
    this.cx += before.x - after.x;
    this.cy += before.y - after.y;
  }

  panByPixels(dxPx: number, dyPx: number): void {
    this.cx -= dxPx / this.zoom;
    this.cy -= dyPx / this.zoom;
  }

  /** Put a world cell at the centre of the viewport. */
  centerOn(x: number, y: number): void {
    this.cx = x;
    this.cy = y;
  }

  screenToWorld(sx: number, sy: number, viewW: number, viewH: number): { x: number; y: number } {
    return {
      x: this.cx + (sx - viewW / 2) / this.zoom,
      y: this.cy + (sy - viewH / 2) / this.zoom,
    };
  }

  worldToScreen(wx: number, wy: number, viewW: number, viewH: number): { x: number; y: number } {
    return {
      x: (wx - this.cx) * this.zoom + viewW / 2,
      y: (wy - this.cy) * this.zoom + viewH / 2,
    };
  }

  /**
   * Column-major 3x3 for `uniformMatrix3fv`, mapping world cells to clip space.
   * The y row is negated: world y grows downward, clip y grows upward.
   */
  matrix(viewW: number, viewH: number): Float32Array {
    const sx = (2 * this.zoom) / viewW;
    const sy = (2 * this.zoom) / viewH;
    return new Float32Array([
      sx, 0, 0,
      0, -sy, 0,
      -this.cx * sx, this.cy * sy, 1,
    ]);
  }
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}
