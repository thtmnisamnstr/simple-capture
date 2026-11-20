// Live frames from `sc://live/<name>` (app/src/live.rs): a 16-byte header (id, width, height, 0
// as little-endian u32) then premultiplied RGBA, drawn with WebGL2 into a canvas sized to the
// frame. Spike 2 mode B: pull the next frame as soon as one is drawn.

const HEADER = 16;

const VS = `#version 300 es
out vec2 uv;
void main() {
  vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
  uv = vec2(p.x, 1.0 - p.y);
  gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}`;

const FS = `#version 300 es
precision mediump float;
in vec2 uv;
uniform sampler2D tex;
out vec4 color;
void main() { color = texture(tex, uv); }`;

export function liveUrl(name: string, after: number): string {
  const base = /Windows|Android/.test(navigator.userAgent) ? 'http://sc.localhost/live' : 'sc://live';
  return `${base}/${name}?after=${after}`;
}

export class LiveView {
  private gl: WebGL2RenderingContext | null;
  private size = { w: 0, h: 0 };
  private running = true;

  constructor(
    private canvas: HTMLCanvasElement,
    private name: string,
    private onFrame: (w: number, h: number) => void,
  ) {
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
    gl.bindTexture(gl.TEXTURE_2D, gl.createTexture());
    for (const p of [gl.TEXTURE_MIN_FILTER, gl.TEXTURE_MAG_FILTER]) gl.texParameteri(gl.TEXTURE_2D, p, gl.NEAREST);
    for (const p of [gl.TEXTURE_WRAP_S, gl.TEXTURE_WRAP_T]) gl.texParameteri(gl.TEXTURE_2D, p, gl.CLAMP_TO_EDGE);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
  }

  private draw(buf: ArrayBuffer) {
    const gl = this.gl;
    if (!gl || buf.byteLength < HEADER) return;
    const head = new DataView(buf, 0, HEADER);
    const w = head.getUint32(4, true);
    const h = head.getUint32(8, true);
    if (buf.byteLength < HEADER + w * h * 4) return;
    const px = new Uint8Array(buf, HEADER, w * h * 4);
    if (w !== this.size.w || h !== this.size.h) {
      this.size = { w, h };
      this.canvas.width = w;
      this.canvas.height = h;
      gl.viewport(0, 0, w, h);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, px);
    } else {
      gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, px);
    }
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    this.onFrame(w, h);
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
