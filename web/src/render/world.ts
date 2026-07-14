/**
 * Two draw calls: a fullscreen quad for the field textures, and one instanced
 * quad for every ant. Ten thousand instances is trivial for any modern GPU.
 *
 * This file holds no simulation state. It draws whatever the last frame said.
 */

import { colonyPalette } from "../colors.js";
import type { Store } from "../state.js";
import { SHAPES, drawSymbol } from "../symbols.js";
import { Camera } from "./camera.js";
import { GLYPH_ATLAS_COLS, glyphCellRect } from "./sprites.js";
import { ANT_FS, ANT_VS, FIELD_FS, FIELD_VS, QUAD_VERTS, compile } from "./shaders.js";

export class WorldRenderer {
  private gl: WebGL2RenderingContext;
  private fieldProg: WebGLProgram;
  private antProg: WebGLProgram;

  private quadBuf: WebGLBuffer;
  private antBuf: WebGLBuffer;
  private fieldVao: WebGLVertexArrayObject;
  private antVao: WebGLVertexArrayObject;

  private pheroTex: WebGLTexture;
  private pheroTrailTex: WebGLTexture;
  private terrainTex: WebGLTexture;
  private glyphTex: WebGLTexture;
  private pheroSize = { w: 0, h: 0 };
  private pheroTrailSize = { w: 0, h: 0 };
  private terrainSize = { w: 0, h: 0 };

  /** The tick of the last uploaded texture, so a paused sim uploads nothing. */
  private pheroTick = -1;
  private terrainTick = -1;
  private antCount = 0;

  readonly camera: Camera;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly store: Store,
    worldW: number,
    worldH: number,
  ) {
    const gl = canvas.getContext("webgl2", {
      alpha: false,
      antialias: false,
      preserveDrawingBuffer: false,
    });
    if (!gl) throw new Error("WebGL2 is required and this browser does not have it");
    this.gl = gl;
    this.camera = new Camera(worldW, worldH);

    this.fieldProg = compile(gl, FIELD_VS, FIELD_FS);
    this.antProg = compile(gl, ANT_VS, ANT_FS);

    const quad = gl.createBuffer();
    const ants = gl.createBuffer();
    if (!quad || !ants) throw new Error("cannot create buffers");
    this.quadBuf = quad;
    this.antBuf = ants;

    gl.bindBuffer(gl.ARRAY_BUFFER, this.quadBuf);
    gl.bufferData(gl.ARRAY_BUFFER, QUAD_VERTS, gl.STATIC_DRAW);

    this.fieldVao = this.makeFieldVao();
    this.antVao = this.makeAntVao();
    this.pheroTex = this.makeTexture();
    this.pheroTrailTex = this.makeTexture();
    this.terrainTex = this.makeTexture();
    this.glyphTex = this.makeGlyphAtlas();
  }

  /**
   * A one-row mask atlas of the 8 colony glyphs, drawn from the same
   * `drawSymbol` the cards and labels use, so shape is defined once. White on
   * transparent; the shader multiplies by colony colour. 64px cells give crisp
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
      // Inset (radius < cell/2) so the shape never touches the cell edge and
      // bleeds into its neighbour when the atlas is sampled with LINEAR.
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

  private makeTexture(): WebGLTexture {
    const gl = this.gl;
    const t = gl.createTexture();
    if (!t) throw new Error("cannot create texture");
    gl.bindTexture(gl.TEXTURE_2D, t);
    // NEAREST: a smoothed pheromone field lies about where its gradient is.
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    return t;
  }

  private makeFieldVao(): WebGLVertexArrayObject {
    const gl = this.gl;
    const vao = gl.createVertexArray();
    if (!vao) throw new Error("cannot create vao");
    gl.bindVertexArray(vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.quadBuf);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
    gl.bindVertexArray(null);
    return vao;
  }

  /**
   * The 8-byte wire record maps straight onto two instance attributes. No CPU
   * unpacking: `raw` goes into the vertex buffer exactly as it came off the
   * socket.
   */
  private makeAntVao(): WebGLVertexArrayObject {
    const gl = this.gl;
    const vao = gl.createVertexArray();
    if (!vao) throw new Error("cannot create vao");
    gl.bindVertexArray(vao);

    gl.bindBuffer(gl.ARRAY_BUFFER, this.quadBuf);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);

    gl.bindBuffer(gl.ARRAY_BUFFER, this.antBuf);
    // aPos: 2 x u16 at offset 0. Integer attribute, not normalised — the
    // shader divides by 128 itself.
    gl.enableVertexAttribArray(1);
    gl.vertexAttribIPointer(1, 2, gl.UNSIGNED_SHORT, 8, 0);
    gl.vertexAttribDivisor(1, 1);
    // aMeta: colony, size, flags, pad at offset 4.
    gl.enableVertexAttribArray(2);
    gl.vertexAttribIPointer(2, 4, gl.UNSIGNED_BYTE, 8, 4);
    gl.vertexAttribDivisor(2, 1);

    gl.bindVertexArray(null);
    return vao;
  }

  /** Match the drawing buffer to the CSS size and the device pixel ratio. */
  resize(): void {
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const w = Math.max(1, Math.round(this.canvas.clientWidth * dpr));
    const h = Math.max(1, Math.round(this.canvas.clientHeight * dpr));
    if (this.canvas.width !== w || this.canvas.height !== h) {
      this.canvas.width = w;
      this.canvas.height = h;
    }
  }

  get viewW(): number {
    return this.canvas.width;
  }
  get viewH(): number {
    return this.canvas.height;
  }

  /** Device pixels per CSS pixel, so mouse coords can be scaled to the buffer. */
  get dpr(): number {
    return this.canvas.width / Math.max(1, this.canvas.clientWidth);
  }

  private uploadField(
    tex: WebGLTexture,
    size: { w: number; h: number },
    w: number,
    h: number,
    rgba: Uint8Array,
  ): void {
    const gl = this.gl;
    gl.bindTexture(gl.TEXTURE_2D, tex);
    if (size.w !== w || size.h !== h) {
      // A resolution toggle or a reset changes the texture's shape.
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, rgba);
      size.w = w;
      size.h = h;
    } else {
      gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, rgba);
    }
  }

  draw(): void {
    const gl = this.gl;
    const st = this.store.state;
    this.resize();

    gl.viewport(0, 0, this.viewW, this.viewH);
    gl.clearColor(0.05, 0.05, 0.06, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);

    const view = this.camera.matrix(this.viewW, this.viewH);
    const palette = colonyPalette();

    // Uploading a texture the sim has not changed is pure waste, and while
    // paused it is every frame.
    if (st.phero && st.phero.tick !== this.pheroTick) {
      this.uploadField(this.pheroTex, this.pheroSize, st.phero.w, st.phero.h, st.phero.rgba);
      this.uploadField(
        this.pheroTrailTex,
        this.pheroTrailSize,
        st.phero.w,
        st.phero.h,
        st.phero.trail,
      );
      this.pheroTick = st.phero.tick;
    }
    if (st.terrain && st.terrain.tick !== this.terrainTick) {
      this.uploadField(
        this.terrainTex,
        this.terrainSize,
        st.terrain.w,
        st.terrain.h,
        st.terrain.rgba,
      );
      this.terrainTick = st.terrain.tick;
    }
    if (!this.pheroSize.w || !this.terrainSize.w) return; // nothing to draw yet

    // --- field pass ---
    gl.useProgram(this.fieldProg);
    gl.bindVertexArray(this.fieldVao);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.pheroTex);
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, this.terrainTex);
    gl.activeTexture(gl.TEXTURE2);
    gl.bindTexture(gl.TEXTURE_2D, this.pheroTrailTex);

    const fu = (n: string) => gl.getUniformLocation(this.fieldProg, n);
    gl.uniformMatrix3fv(fu("uView"), false, view);
    gl.uniform2f(fu("uWorldSize"), this.camera.worldW, this.camera.worldH);
    gl.uniform1i(fu("uPhero"), 0);
    gl.uniform1i(fu("uTerrain"), 1);
    gl.uniform1i(fu("uPheroTrail"), 2);
    gl.uniform3fv(fu("uColonyColors"), palette);
    gl.uniform1i(fu("uShowFood"), st.layers.food ? 1 : 0);
    gl.uniform1i(fu("uShowAlarm"), st.layers.alarm ? 1 : 0);
    gl.uniform1i(fu("uShowScent"), st.layers.scent ? 1 : 0);
    gl.uniform1i(fu("uShowTrail"), st.layers.trail ? 1 : 0);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);

    // --- ant pass ---
    if (st.ants && st.ants.count > 0) {
      gl.bindBuffer(gl.ARRAY_BUFFER, this.antBuf);
      // Orphan the buffer when it grows so the driver does not stall waiting
      // for the previous draw to finish reading it.
      if (st.ants.count > this.antCount) {
        gl.bufferData(gl.ARRAY_BUFFER, st.ants.raw, gl.STREAM_DRAW);
      } else {
        gl.bufferSubData(gl.ARRAY_BUFFER, 0, st.ants.raw);
      }
      this.antCount = Math.max(this.antCount, st.ants.count);

      gl.useProgram(this.antProg);
      gl.bindVertexArray(this.antVao);

      gl.activeTexture(gl.TEXTURE2);
      gl.bindTexture(gl.TEXTURE_2D, this.glyphTex);

      const au = (n: string) => gl.getUniformLocation(this.antProg, n);
      gl.uniformMatrix3fv(au("uView"), false, view);
      gl.uniform1f(au("uZoom"), this.camera.zoom);
      gl.uniform3fv(au("uColonyColors"), palette);
      gl.uniform1i(au("uGlyphAtlas"), 2);
      gl.uniform1i(au("uGlyphCols"), GLYPH_ATLAS_COLS);

      const sel = st.detail;
      gl.uniform1i(au("uHasSelection"), sel && sel.alive ? 1 : 0);
      gl.uniform2f(au("uSelectedPos"), sel?.x ?? 0, sel?.y ?? 0);

      gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, st.ants.count);
    }

    gl.bindVertexArray(null);
  }
}
