// Synthetic 1920x1080 "editor composite": moving screen content, a camera
// bubble, and two bit strips (frame id, producer wall-clock ms low 32 bits)
// that survive downscaling so the webview can verify what it displayed.

struct Uniforms {
    frame_id: u32,
    ts_lo: u32,
    t: f32,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var src_tex: texture_2d<f32>;
@group(0) @binding(2) var src_samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) i: u32) -> VsOut {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    var o: VsOut;
    o.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    o.uv = vec2<f32>(x, y);
    return o;
}

const W: f32 = 1920.0;
const H: f32 = 1080.0;
const BLOCK: f32 = 40.0;

fn hsv(h: f32, s: f32, v: f32) -> vec3<f32> {
    let k = vec3<f32>(1.0, 2.0 / 3.0, 1.0 / 3.0);
    let p = abs(fract(vec3<f32>(h) + k) * 6.0 - vec3<f32>(3.0));
    return v * mix(vec3<f32>(1.0), clamp(p - vec3<f32>(1.0), vec3<f32>(0.0), vec3<f32>(1.0)), s);
}

fn bit_strip(p: vec2<f32>, row: f32, nbits: u32, value: u32) -> f32 {
    // returns -1 when outside the strip, else 0/1
    if (p.y < row * BLOCK || p.y >= (row + 1.0) * BLOCK) { return -1.0; }
    let k = u32(floor(p.x / BLOCK));
    if (k >= nbits) { return -1.0; }
    let bit = (value >> (nbits - 1u - k)) & 1u;
    return f32(bit);
}

@fragment
fn fs_composite(in: VsOut) -> @location(0) vec4<f32> {
    let p = in.uv * vec2<f32>(W, H);
    let t = u.t;

    // "screen": hue gradient + scrolling checker + a sweeping bar
    let g = hsv(fract(in.uv.x * 0.6 + in.uv.y * 0.3 + t * 0.07), 0.55, 0.85);
    let cp = floor((p + vec2<f32>(t * 120.0, t * 45.0)) / 60.0);
    let checker = f32((i32(cp.x) + i32(cp.y)) & 1);
    var col = mix(g, g * 0.6, checker);
    let bar_x = fract(t * 0.25) * W;
    if (abs(p.x - bar_x) < 6.0) { col = vec3<f32>(1.0, 1.0, 1.0); }

    // "camera bubble": circle orbiting near bottom-right
    let c = vec2<f32>(1600.0 + 60.0 * cos(t * 1.3), 800.0 + 60.0 * sin(t * 1.3));
    let r = 170.0;
    let d = length(p - c);
    let shadow = smoothstep(r + 30.0, r, d) * 0.45;
    col = col * (1.0 - shadow);
    if (d < r + 6.0) { col = vec3<f32>(1.0); }
    if (d < r) {
        let a = atan2(p.y - c.y, p.x - c.x);
        let stripes = 0.5 + 0.5 * sin(a * 8.0 + t * 3.0 + d * 0.05);
        col = mix(vec3<f32>(0.95, 0.55, 0.25), vec3<f32>(0.2, 0.3, 0.7), stripes);
    }

    // metadata strips (top-left), black/white blocks of 40 px
    let b0 = bit_strip(p, 0.0, 24u, u.frame_id & 0xffffffu);
    if (b0 >= 0.0) { col = vec3<f32>(b0); }
    let b1 = bit_strip(p, 1.0, 32u, u.ts_lo);
    if (b1 >= 0.0) { col = vec3<f32>(b1); }

    return vec4<f32>(col, 1.0);
}

@fragment
fn fs_blit(in: VsOut) -> @location(0) vec4<f32> {
    return textureSampleLevel(src_tex, src_samp, in.uv, 0.0);
}
