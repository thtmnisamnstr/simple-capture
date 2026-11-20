struct U {
    t: f32,
    vx: f32,
    vy: f32,
    vw: f32,
    vh: f32,
    p0: f32,
    p1: f32,
    p2: f32,
};

@group(0) @binding(0) var<uniform> u: U;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(i & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = (p.xy - vec2<f32>(u.vx, u.vy)) / vec2<f32>(u.vw, u.vh);
    if (uv.x < 0.5) {
        return vec4<f32>(1.0, 0.0, 1.0, 1.0);
    }
    let bar = fract(u.t * 0.5);
    let x = (uv.x - 0.5) * 2.0;
    if (abs(x - bar) < 0.03) {
        return vec4<f32>(1.0, 1.0, 1.0, 1.0);
    }
    return vec4<f32>(0.1, 0.3 + 0.2 * uv.y, 0.4, 1.0);
}
