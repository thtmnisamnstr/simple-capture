// Wire format shared with app/src/live.rs: a 16-byte header (id, width, height, format as
// little-endian u32), then premultiplied RGBA (format 0) or I420 planes in BT.709 limited range
// (format 1). Format 2 has no pixels: a native layer under the page draws the frame.

const HEADER = 16;
const RGBA = 0;
const I420 = 1;
const NATIVE = 2;

const VS = `#version 300 es
out vec2 uv;
void main() {
  vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
  uv = vec2(p.x, 1.0 - p.y);
  gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}`;

const FS = `#version 300 es
precision highp float;
in vec2 uv;
uniform int yuv;
uniform sampler2D t0;
uniform sampler2D t1;
uniform sampler2D t2;
out vec4 color;
void main() {
  if (yuv == 0) {
    color = texture(t0, uv);
    return;
  }
  float y = (texture(t0, uv).r - 16.0 / 255.0) * (255.0 / 219.0);
  float cb = (texture(t1, uv).r - 128.0 / 255.0) * (255.0 / 224.0);
  float cr = (texture(t2, uv).r - 128.0 / 255.0) * (255.0 / 224.0);
  vec3 rgb = vec3(y + 1.5748 * cr, y - 0.187324 * cb - 0.468124 * cr, y + 1.8556 * cb);
  color = vec4(clamp(rgb, 0.0, 1.0), 1.0);
}`;

export function liveUrl(name: string, after: number): string {
  const base = /Windows|Android/.test(navigator.userAgent) ? 'http://sc.localhost/live' : 'sc://live';
  return `${base}/${name}?after=${after}`;
}

export function framePlanes(format: number, w: number, h: number): [number, number, number][] | null {
  if (format === RGBA) return [[w, h, 4]];
  if (format === I420 && w % 2 === 0 && h % 2 === 0) {
    return [
      [w, h, 1],
      [w / 2, h / 2, 1],
      [w / 2, h / 2, 1],
    ];
  }
  return null;
}

export class LiveView {
  private gl: WebGL2RenderingContext | null;
  private size = { w: 0, h: 0, format: -1 };
  private yuv: WebGLUniformLocation | null = null;
  private running = true;
  private canvas: HTMLCanvasElement;
  private name: string;
  private onFrame: (w: number, h: number, native: boolean) => void;

  constructor(canvas: HTMLCanvasElement, name: string, onFrame: (w: number, h: number, native: boolean) => void) {
    this.canvas = canvas;
    this.name = name;
    this.onFrame = onFrame;
    this.gl = canvas.getContext('webgl2', { premultipliedAlpha: true, alpha: true, antialias: false });
    if (this.gl) this.setup(this.gl);
    void this.pump();
  }

  stop() {
    this.running = false;
  }

  private setup(gl: WebGL2RenderingContext) {
    const shader = (type: number, src: string) => {
      const s = gl.createShader(type)!;
      gl.shaderSource(s, src);
      gl.compileShader(s);
      return s;
    };
    const prog = gl.createProgram()!;
    gl.attachShader(prog, shader(gl.VERTEX_SHADER, VS));
    gl.attachShader(prog, shader(gl.FRAGMENT_SHADER, FS));
    gl.linkProgram(prog);
    gl.useProgram(prog);
    for (let i = 0; i < 3; i++) {
      gl.activeTexture(gl.TEXTURE0 + i);
      gl.bindTexture(gl.TEXTURE_2D, gl.createTexture());
      const filter = i === 0 ? gl.NEAREST : gl.LINEAR;
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, filter);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, filter);
      for (const p of [gl.TEXTURE_WRAP_S, gl.TEXTURE_WRAP_T]) gl.texParameteri(gl.TEXTURE_2D, p, gl.CLAMP_TO_EDGE);
      gl.uniform1i(gl.getUniformLocation(prog, `t${i}`), i);
    }
    this.yuv = gl.getUniformLocation(prog, 'yuv');
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
  }

  private draw(buf: ArrayBuffer) {
    const gl = this.gl;
    if (!gl || buf.byteLength < HEADER) return;
    const head = new DataView(buf, 0, HEADER);
    const w = head.getUint32(4, true);
    const h = head.getUint32(8, true);
    const format = head.getUint32(12, true);
    if (format === NATIVE) {
      this.onFrame(w, h, true);
      return;
    }
    const planes = framePlanes(format, w, h);
    if (!planes || buf.byteLength < HEADER + planes.reduce((n, [pw, ph, b]) => n + pw * ph * b, 0)) return;
    const fresh = w !== this.size.w || h !== this.size.h || format !== this.size.format;
    if (fresh) {
      this.size = { w, h, format };
      this.canvas.width = w;
      this.canvas.height = h;
      gl.viewport(0, 0, w, h);
      gl.uniform1i(this.yuv, format === I420 ? 1 : 0);
    }
    const [internal, layout] = format === RGBA ? [gl.RGBA, gl.RGBA] : [gl.R8, gl.RED];
    let at = HEADER;
    planes.forEach(([pw, ph, b], i) => {
      const px = new Uint8Array(buf, at, pw * ph * b);
      at += px.length;
      gl.activeTexture(gl.TEXTURE0 + i);
      if (fresh) gl.texImage2D(gl.TEXTURE_2D, 0, internal, pw, ph, 0, layout, gl.UNSIGNED_BYTE, px);
      else gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, pw, ph, layout, gl.UNSIGNED_BYTE, px);
    });
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    this.onFrame(w, h, false);
  }

  private async pump() {
    let after = 0;
    while (this.running) {
      try {
        const r = await fetch(liveUrl(this.name, after), { cache: 'no-store' });
        if (r.status === 200) {
          const buf = await r.arrayBuffer();
          after = new DataView(buf, 0, 4).getUint32(0, true);
          this.draw(buf);
        }
      } catch {
        await new Promise((res) => setTimeout(res, 250));
      }
    }
  }
}
